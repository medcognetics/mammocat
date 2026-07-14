use std::collections::BTreeSet;

use crate::error::Result;
use crate::registry::{is_retired_snomed_coding_scheme, retired_view_code_matches};
pub use crate::registry::{
    view_code_definition, view_modifier_code_definition, Confidence, ViewCodeDefinition,
    ViewModifierCodeDefinition, VIEW_CODE_DEFINITIONS, VIEW_MODIFIER_CODE_DEFINITIONS,
};
use crate::types::{MammographyViewModifier, ViewPosition};
use dicom_object::InMemDicomObject;

use super::tags::{
    get_string_value, CODE_MEANING, CODE_VALUE, CODING_SCHEME_DESIGNATOR, PADDLE_DESCRIPTION,
    PERFORMED_PROCEDURE_STEP_DESCRIPTION, SERIES_DESCRIPTION, STUDY_DESCRIPTION,
    VIEW_CODE_SEQUENCE, VIEW_MODIFIER_CODE_SEQUENCE, VIEW_POSITION as VIEW_POSITION_TAG,
};

const CURRENT_CODING_SCHEME: &str = "SCT";
#[cfg(test)]
const LEGACY_CODING_SCHEME: &str = "SRT";
const ROLLED_LATERAL_ABBREVIATION: &str = "rl";
const ROLLED_MEDIAL_ABBREVIATION: &str = "rm";
const TANGENTIAL_ABBREVIATION: &str = "tan";
const IMPLANT_DISPLACED_SUFFIX: &str = "id";
const MAGNIFICATION_SUFFIX: &str = "m";

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Evidence {
    pub source: String,
    pub value: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct MammographyViewDescriptor {
    pub view_position: ViewPosition,
    pub modifiers: BTreeSet<MammographyViewModifier>,
    pub evidence: Vec<Evidence>,
    pub conflicts: Vec<String>,
}

impl Default for MammographyViewDescriptor {
    fn default() -> Self {
        Self {
            view_position: ViewPosition::Unknown,
            modifiers: BTreeSet::new(),
            evidence: Vec::new(),
            conflicts: Vec::new(),
        }
    }
}

pub fn extract_view_descriptor(dcm: &InMemDicomObject) -> MammographyViewDescriptor {
    let mut descriptor = MammographyViewDescriptor::default();
    let mut base_candidates = Vec::new();

    if let Ok(element) = dcm.element(VIEW_CODE_SEQUENCE) {
        if let Some(items) = element.items() {
            for item in items {
                if let Some(candidate) = parse_view_code_item(item, &mut descriptor) {
                    base_candidates.push(candidate);
                }
                extract_modifier_sequence(
                    item,
                    "ViewCodeSequence/ViewModifierCodeSequence",
                    &mut descriptor,
                );
            }
        }
    }

    extract_modifier_sequence(dcm, "ViewModifierCodeSequence", &mut descriptor);

    if let Some(raw_view) = get_string_value(dcm, VIEW_POSITION_TAG) {
        let compact_alias = compact_view_position_alias(&raw_view);
        let strict_view = from_str(&raw_view, true);
        if !strict_view.is_unknown() {
            add_base_candidate(
                &mut base_candidates,
                &mut descriptor,
                strict_view,
                Confidence::Structural,
                "ViewPosition",
                &raw_view,
            );
        } else {
            let loose_view = compact_alias
                .and_then(|(view, _)| view)
                .unwrap_or_else(|| from_str(&raw_view, false));
            if !loose_view.is_unknown() {
                add_base_candidate(
                    &mut base_candidates,
                    &mut descriptor,
                    loose_view,
                    Confidence::Heuristic,
                    "ViewPosition",
                    &raw_view,
                );
            }
        }

        if let Some(modifier) = modifier_from_text(&raw_view, true) {
            add_modifier(
                &mut descriptor,
                modifier,
                Confidence::Structural,
                "ViewPosition",
                &raw_view,
            );
        } else if let Some((_, modifier)) = compact_alias {
            add_modifier(
                &mut descriptor,
                modifier,
                Confidence::Heuristic,
                "ViewPosition",
                &raw_view,
            );
        } else {
            for definition in VIEW_MODIFIER_CODE_DEFINITIONS {
                if description_contains_modifier(&raw_view, definition.modifier) {
                    add_modifier(
                        &mut descriptor,
                        definition.modifier,
                        Confidence::Heuristic,
                        "ViewPosition",
                        &raw_view,
                    );
                }
            }
        }
    }

    if let Some(paddle) = get_string_value(dcm, PADDLE_DESCRIPTION) {
        if paddle.contains("SPOT") || paddle.contains("SPT") {
            add_modifier(
                &mut descriptor,
                MammographyViewModifier::SpotCompression,
                Confidence::Heuristic,
                "PaddleDescription",
                &paddle,
            );
        }
        if paddle.contains("MAG") {
            add_modifier(
                &mut descriptor,
                MammographyViewModifier::Magnification,
                Confidence::Heuristic,
                "PaddleDescription",
                &paddle,
            );
        }
    }

    for (source, tag) in [
        ("SeriesDescription", SERIES_DESCRIPTION),
        ("StudyDescription", STUDY_DESCRIPTION),
        (
            "PerformedProcedureStepDescription",
            PERFORMED_PROCEDURE_STEP_DESCRIPTION,
        ),
    ] {
        if let Some(description) = get_string_value(dcm, tag) {
            let view = from_str(&description, false);
            if !view.is_unknown() {
                add_base_candidate(
                    &mut base_candidates,
                    &mut descriptor,
                    view,
                    Confidence::Heuristic,
                    source,
                    &description,
                );
            }
            for definition in VIEW_MODIFIER_CODE_DEFINITIONS {
                if description_contains_modifier(&description, definition.modifier) {
                    add_modifier(
                        &mut descriptor,
                        definition.modifier,
                        Confidence::Heuristic,
                        source,
                        &description,
                    );
                }
            }
        }
    }

    descriptor.view_position = resolve_base_view(&base_candidates, &mut descriptor.conflicts);
    descriptor
}

pub fn extract_view_position(dcm: &InMemDicomObject) -> Result<ViewPosition> {
    Ok(extract_view_descriptor(dcm).view_position)
}

pub fn extract_view_modifiers(dcm: &InMemDicomObject) -> BTreeSet<MammographyViewModifier> {
    extract_view_descriptor(dcm).modifiers
}

fn parse_view_code_item(
    item: &InMemDicomObject,
    descriptor: &mut MammographyViewDescriptor,
) -> Option<BaseCandidate> {
    let tuple_match = match_view_tuple(item);
    let meaning = get_string_value(item, CODE_MEANING);
    let meaning_match = meaning
        .as_deref()
        .map(|value| from_str(value, true))
        .filter(|view| !view.is_unknown());

    if let (Some((tuple_view, _)), Some(meaning_view)) = (tuple_match, meaning_match) {
        if tuple_view != meaning_view {
            descriptor.conflicts.push(format!(
                "ViewCodeSequence code resolves to {} but CodeMeaning resolves to {}",
                tuple_view, meaning_view
            ));
        }
    }

    if let Some((view, confidence)) = tuple_match {
        let value = get_string_value(item, CODE_VALUE).unwrap_or_default();
        descriptor.evidence.push(Evidence {
            source: "ViewCodeSequence".to_string(),
            value,
            confidence,
        });
        return Some(BaseCandidate {
            view,
            confidence,
            authoritative_code: true,
        });
    }

    let tuple_is_incomplete =
        element_is_empty(item, CODING_SCHEME_DESIGNATOR) || element_is_empty(item, CODE_VALUE);
    tuple_is_incomplete
        .then_some(meaning_match)
        .flatten()
        .map(|view| {
            let value = meaning.unwrap_or_default();
            descriptor.evidence.push(Evidence {
                source: "ViewCodeSequence/CodeMeaning".to_string(),
                value,
                confidence: Confidence::Structural,
            });
            BaseCandidate {
                view,
                confidence: Confidence::Structural,
                authoritative_code: false,
            }
        })
}

fn extract_modifier_sequence(
    object: &InMemDicomObject,
    source: &str,
    descriptor: &mut MammographyViewDescriptor,
) {
    let Ok(element) = object.element(VIEW_MODIFIER_CODE_SEQUENCE) else {
        return;
    };
    let Some(items) = element.items() else {
        return;
    };
    for item in items {
        let tuple_match = match_modifier_tuple(item);
        let meaning = get_string_value(item, CODE_MEANING);
        let meaning_match = meaning
            .as_deref()
            .and_then(|value| modifier_from_text(value, true));
        if let (Some((tuple_modifier, _)), Some(meaning_modifier)) = (tuple_match, meaning_match) {
            if tuple_modifier != meaning_modifier {
                descriptor.conflicts.push(format!(
                    "{source} code resolves to {tuple_modifier} but CodeMeaning resolves to {meaning_modifier}"
                ));
            }
        }
        if let Some((modifier, confidence)) = tuple_match {
            let value = get_string_value(item, CODE_VALUE).unwrap_or_default();
            add_modifier(descriptor, modifier, confidence, source, &value);
        } else if element_is_empty(item, CODING_SCHEME_DESIGNATOR)
            || element_is_empty(item, CODE_VALUE)
        {
            if let Some(modifier) = meaning_match {
                add_modifier(
                    descriptor,
                    modifier,
                    Confidence::Structural,
                    &format!("{source}/CodeMeaning"),
                    meaning.as_deref().unwrap_or_default(),
                );
            }
        }
    }
}

fn match_view_tuple(item: &InMemDicomObject) -> Option<(ViewPosition, Confidence)> {
    let scheme = get_string_value(item, CODING_SCHEME_DESIGNATOR)?;
    let code = get_string_value(item, CODE_VALUE)?;
    VIEW_CODE_DEFINITIONS.iter().find_map(|definition| {
        if scheme.eq_ignore_ascii_case(CURRENT_CODING_SCHEME) && code == definition.code_value {
            Some((definition.view, Confidence::Exact))
        } else if is_retired_snomed_coding_scheme(&scheme)
            && retired_view_code_matches(definition, &code)
        {
            Some((definition.view, Confidence::Structural))
        } else {
            None
        }
    })
}

fn match_modifier_tuple(item: &InMemDicomObject) -> Option<(MammographyViewModifier, Confidence)> {
    let scheme = get_string_value(item, CODING_SCHEME_DESIGNATOR)?;
    let code = get_string_value(item, CODE_VALUE)?;
    VIEW_MODIFIER_CODE_DEFINITIONS
        .iter()
        .find_map(|definition| {
            if scheme.eq_ignore_ascii_case(CURRENT_CODING_SCHEME) && code == definition.code_value {
                Some((definition.modifier, Confidence::Exact))
            } else if is_retired_snomed_coding_scheme(&scheme)
                && code.eq_ignore_ascii_case(definition.legacy_code_value)
            {
                Some((definition.modifier, Confidence::Structural))
            } else {
                None
            }
        })
}

#[derive(Debug, Clone, Copy)]
struct BaseCandidate {
    view: ViewPosition,
    confidence: Confidence,
    authoritative_code: bool,
}

fn add_base_candidate(
    candidates: &mut Vec<BaseCandidate>,
    descriptor: &mut MammographyViewDescriptor,
    view: ViewPosition,
    confidence: Confidence,
    source: &str,
    value: &str,
) {
    candidates.push(BaseCandidate {
        view,
        confidence,
        authoritative_code: false,
    });
    descriptor.evidence.push(Evidence {
        source: source.to_string(),
        value: value.to_string(),
        confidence,
    });
}

fn resolve_base_view(candidates: &[BaseCandidate], conflicts: &mut Vec<String>) -> ViewPosition {
    let Some(selected) = candidates
        .iter()
        .max_by_key(|candidate| (candidate.authoritative_code, candidate.confidence))
    else {
        return ViewPosition::Unknown;
    };
    for candidate in candidates {
        if candidate.view != selected.view {
            conflicts.push(format!(
                "view evidence disagrees: {} versus {}",
                selected.view, candidate.view
            ));
        }
    }
    selected.view
}

fn add_modifier(
    descriptor: &mut MammographyViewDescriptor,
    modifier: MammographyViewModifier,
    confidence: Confidence,
    source: &str,
    value: &str,
) {
    descriptor.modifiers.insert(modifier);
    descriptor.evidence.push(Evidence {
        source: source.to_string(),
        value: value.to_string(),
        confidence,
    });
}

#[allow(clippy::should_implement_trait)]
pub fn from_str(value: &str, strict: bool) -> ViewPosition {
    let normalized = normalize_text(value);
    let exact = VIEW_CODE_DEFINITIONS.iter().find_map(|definition| {
        let short = definition.view.short_str();
        let meaning = normalize_text(definition.code_meaning);
        (normalized == short || normalized == meaning).then_some(definition.view)
    });
    if exact.is_some() || strict {
        return exact.unwrap_or(ViewPosition::Unknown);
    }
    VIEW_CODE_DEFINITIONS
        .iter()
        .find_map(|definition| {
            contains_token(&normalized, definition.view.short_str()).then_some(definition.view)
        })
        .unwrap_or(ViewPosition::Unknown)
}

fn modifier_from_text(value: &str, strict: bool) -> Option<MammographyViewModifier> {
    let normalized = normalize_text(value);
    let exact = VIEW_MODIFIER_CODE_DEFINITIONS
        .iter()
        .find_map(|definition| {
            let meaning = normalize_text(definition.code_meaning);
            (normalized == meaning).then_some(definition.modifier)
        });
    if exact.is_some() {
        return exact;
    }
    match normalized.as_str() {
        "at" => Some(MammographyViewModifier::AxillaryTail),
        "cv" | "cleavage view" => Some(MammographyViewModifier::Cleavage),
        "magnified" => Some(MammographyViewModifier::Magnification),
        _ if !strict && normalized.contains("spot compression") => {
            Some(MammographyViewModifier::SpotCompression)
        }
        _ if !strict && normalized.contains("implant displaced") => {
            Some(MammographyViewModifier::ImplantDisplaced)
        }
        _ if !strict && normalized.contains("magnif") => {
            Some(MammographyViewModifier::Magnification)
        }
        _ => None,
    }
}

fn compact_view_position_alias(
    value: &str,
) -> Option<(Option<ViewPosition>, MammographyViewModifier)> {
    let compact = normalize_text(value).replace(' ', "");
    match compact.as_str() {
        ROLLED_LATERAL_ABBREVIATION => Some((None, MammographyViewModifier::RolledLateral)),
        ROLLED_MEDIAL_ABBREVIATION => Some((None, MammographyViewModifier::RolledMedial)),
        TANGENTIAL_ABBREVIATION => Some((None, MammographyViewModifier::Tangential)),
        _ => compact
            .strip_suffix(IMPLANT_DISPLACED_SUFFIX)
            .and_then(compact_base_view)
            .map(|view| (Some(view), MammographyViewModifier::ImplantDisplaced))
            .or_else(|| {
                compact
                    .strip_suffix(MAGNIFICATION_SUFFIX)
                    .and_then(compact_base_view)
                    .map(|view| (Some(view), MammographyViewModifier::Magnification))
            }),
    }
}

fn compact_base_view(value: &str) -> Option<ViewPosition> {
    let view = from_str(value, true);
    (!view.is_unknown()).then_some(view)
}

fn description_contains_modifier(value: &str, modifier: MammographyViewModifier) -> bool {
    let normalized = normalize_text(value);
    let definition = view_modifier_code_definition(modifier);
    normalized.contains(&normalize_text(definition.code_meaning))
        || match modifier {
            MammographyViewModifier::SpotCompression => {
                normalized.contains("spot") || contains_token(&normalized, "spt")
            }
            MammographyViewModifier::Magnification => {
                normalized.contains("magnif") || contains_token(&normalized, "mag")
            }
            MammographyViewModifier::ImplantDisplaced => {
                normalized.contains("implant displaced") || contains_token(&normalized, "id")
            }
            MammographyViewModifier::AxillaryTail => contains_token(&normalized, "at"),
            MammographyViewModifier::Cleavage => contains_token(&normalized, "cv"),
            _ => false,
        }
}

fn element_is_empty(item: &InMemDicomObject, tag: dicom_core::Tag) -> bool {
    get_string_value(item, tag).is_none_or(|value| value.is_empty())
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_token(value: &str, token: &str) -> bool {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| part == token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::value::{DataSetSequence, PrimitiveValue};
    use dicom_core::{DataElement, VR};

    fn coded_item(scheme: &str, code: &str, meaning: &str) -> InMemDicomObject {
        InMemDicomObject::from_element_iter([
            DataElement::new(
                CODING_SCHEME_DESIGNATOR,
                VR::SH,
                PrimitiveValue::from(scheme),
            ),
            DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from(code)),
            DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from(meaning)),
        ])
    }

    #[test]
    fn parses_every_cid_4014_code() {
        for definition in VIEW_CODE_DEFINITIONS {
            let item = coded_item(
                CURRENT_CODING_SCHEME,
                definition.code_value,
                definition.code_meaning,
            );
            let mut descriptor = MammographyViewDescriptor::default();
            let candidate = parse_view_code_item(&item, &mut descriptor).unwrap();
            assert_eq!(candidate.view, definition.view);
            assert_eq!(candidate.confidence, Confidence::Exact);
        }
    }

    #[test]
    fn parses_every_cid_4015_code() {
        for definition in VIEW_MODIFIER_CODE_DEFINITIONS {
            let modifier = match_modifier_tuple(&coded_item(
                CURRENT_CODING_SCHEME,
                definition.code_value,
                definition.code_meaning,
            ))
            .unwrap();
            assert_eq!(modifier, (definition.modifier, Confidence::Exact));
        }
    }

    #[test]
    fn parses_legacy_snomed_rt_aliases() {
        for definition in VIEW_CODE_DEFINITIONS {
            let item = coded_item(
                LEGACY_CODING_SCHEME,
                definition.legacy_code_value,
                definition.code_meaning,
            );
            assert_eq!(
                match_view_tuple(&item),
                Some((definition.view, Confidence::Structural))
            );
        }
        for definition in VIEW_MODIFIER_CODE_DEFINITIONS {
            let item = coded_item(
                LEGACY_CODING_SCHEME,
                definition.legacy_code_value,
                definition.code_meaning,
            );
            assert_eq!(
                match_modifier_tuple(&item),
                Some((definition.modifier, Confidence::Structural))
            );
        }
    }

    #[test]
    fn parses_all_retired_snomed_scheme_designators() {
        for scheme in ["SRT", "SNM3", "99SDM"] {
            let modifier = coded_item(scheme, "R-102D1", "Axillary Tail");
            let mut view = coded_item(scheme, "R-10226", "medio-lateral oblique");
            view.put(DataElement::new(
                VIEW_MODIFIER_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![modifier]),
            ));
            let mut dcm = InMemDicomObject::new_empty();
            dcm.put(DataElement::new(
                VIEW_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![view]),
            ));

            let descriptor = extract_view_descriptor(&dcm);

            assert_eq!(descriptor.view_position, ViewPosition::Mlo, "{scheme}");
            assert!(
                descriptor
                    .modifiers
                    .contains(&MammographyViewModifier::AxillaryTail),
                "{scheme}"
            );
        }
    }

    #[test]
    fn parses_deprecated_exaggerated_view_codes() {
        for (scheme, code, expected) in [
            ("SNM3", "Y-X1770", ViewPosition::Xccl),
            ("SNM3", "Y-X1771", ViewPosition::Xccm),
        ] {
            let item = coded_item(scheme, code, expected.short_str());

            assert_eq!(
                match_view_tuple(&item),
                Some((expected, Confidence::Structural)),
                "{scheme}:{code}"
            );
        }
    }

    #[test]
    fn meaning_only_fallback_requires_an_incomplete_tuple() {
        let meaning_only = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            PrimitiveValue::from("  CRANIO_CAUDAL  "),
        )]);
        let mut descriptor = MammographyViewDescriptor::default();
        assert_eq!(
            parse_view_code_item(&meaning_only, &mut descriptor)
                .unwrap()
                .view,
            ViewPosition::Cc
        );

        let private_tuple = coded_item("99VENDOR", "PRIVATE_CC", "cranio-caudal");
        let mut descriptor = MammographyViewDescriptor::default();
        assert!(parse_view_code_item(&private_tuple, &mut descriptor).is_none());
    }

    #[test]
    fn reads_nested_and_top_level_modifier_sequences() {
        let nested_modifier = coded_item("SCT", "399055006", "Spot Compression");
        let mut view = coded_item("SCT", "399162004", "cranio-caudal");
        view.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![nested_modifier]),
        ));
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view]),
        ));
        dcm.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_item("SCT", "399163009", "Magnification")]),
        ));

        let descriptor = extract_view_descriptor(&dcm);
        assert_eq!(descriptor.view_position, ViewPosition::Cc);
        assert!(descriptor
            .modifiers
            .contains(&MammographyViewModifier::SpotCompression));
        assert!(descriptor
            .modifiers
            .contains(&MammographyViewModifier::Magnification));
    }

    #[test]
    fn ignores_base_view_codes_in_nonstandard_modifier_sequences() {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_item("SNM3", "R-10242", "cranio-caudal")]),
        ));
        dcm.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_item("SNM3", "R-10242", "cranio-caudal")]),
        ));

        let descriptor = extract_view_descriptor(&dcm);

        assert_eq!(descriptor.view_position, ViewPosition::Cc);
        assert!(descriptor.modifiers.is_empty());
        assert!(descriptor.conflicts.is_empty());
    }

    #[test]
    fn modifier_does_not_replace_base_view() {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            PrimitiveValue::from("CC"),
        ));
        let modifier = coded_item("SCT", "399161006", "Cleavage");
        dcm.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier]),
        ));
        let descriptor = extract_view_descriptor(&dcm);
        assert_eq!(descriptor.view_position, ViewPosition::Cc);
        assert!(descriptor
            .modifiers
            .contains(&MammographyViewModifier::Cleavage));
    }

    #[test]
    fn canonical_code_is_authoritative_and_conflict_is_retained() {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            PrimitiveValue::from("CC"),
        ));
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_item(
                "SCT",
                "399368009",
                "medio-lateral oblique",
            )]),
        ));
        let descriptor = extract_view_descriptor(&dcm);
        assert_eq!(descriptor.view_position, ViewPosition::Mlo);
        assert!(!descriptor.conflicts.is_empty());
    }

    #[test]
    fn legacy_coded_base_is_authoritative_over_view_position() {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            PrimitiveValue::from("CC"),
        ));
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_item("SRT", "R-10226", "medio-lateral oblique")]),
        ));
        let descriptor = extract_view_descriptor(&dcm);
        assert_eq!(descriptor.view_position, ViewPosition::Mlo);
        assert!(!descriptor.conflicts.is_empty());
    }

    #[test]
    fn legacy_at_and_cv_are_modifiers() {
        for (value, modifier) in [
            ("AT", MammographyViewModifier::AxillaryTail),
            ("CV", MammographyViewModifier::Cleavage),
        ] {
            let mut dcm = InMemDicomObject::new_empty();
            dcm.put(DataElement::new(
                VIEW_POSITION_TAG,
                VR::CS,
                PrimitiveValue::from(value),
            ));
            let descriptor = extract_view_descriptor(&dcm);
            assert_eq!(descriptor.view_position, ViewPosition::Unknown);
            assert!(descriptor.modifiers.contains(&modifier));
        }
    }

    #[test]
    fn compound_view_position_preserves_base_and_modifier_evidence() {
        for (raw, expected_view, expected_modifier) in [
            (
                "CC SPOT",
                ViewPosition::Cc,
                MammographyViewModifier::SpotCompression,
            ),
            (
                "MLO MAG",
                ViewPosition::Mlo,
                MammographyViewModifier::Magnification,
            ),
            (
                "MLO ID",
                ViewPosition::Mlo,
                MammographyViewModifier::ImplantDisplaced,
            ),
            (
                "CCM",
                ViewPosition::Cc,
                MammographyViewModifier::Magnification,
            ),
            (
                "MLOM",
                ViewPosition::Mlo,
                MammographyViewModifier::Magnification,
            ),
            (
                "CCID",
                ViewPosition::Cc,
                MammographyViewModifier::ImplantDisplaced,
            ),
            (
                "MLOID",
                ViewPosition::Mlo,
                MammographyViewModifier::ImplantDisplaced,
            ),
            (
                "LMID",
                ViewPosition::Lm,
                MammographyViewModifier::ImplantDisplaced,
            ),
        ] {
            let mut dcm = InMemDicomObject::new_empty();
            dcm.put(DataElement::new(
                VIEW_POSITION_TAG,
                VR::CS,
                PrimitiveValue::from(raw),
            ));

            let descriptor = extract_view_descriptor(&dcm);

            assert_eq!(descriptor.view_position, expected_view, "{raw}");
            assert!(descriptor.modifiers.contains(&expected_modifier), "{raw}");
        }
    }

    #[test]
    fn modifier_abbreviations_from_real_files_are_heuristic() {
        for (raw, expected_modifier) in [
            ("RL", MammographyViewModifier::RolledLateral),
            ("RM", MammographyViewModifier::RolledMedial),
            ("TAN", MammographyViewModifier::Tangential),
        ] {
            let mut dcm = InMemDicomObject::new_empty();
            dcm.put(DataElement::new(
                VIEW_POSITION_TAG,
                VR::CS,
                PrimitiveValue::from(raw),
            ));

            let descriptor = extract_view_descriptor(&dcm);

            assert_eq!(descriptor.view_position, ViewPosition::Unknown, "{raw}");
            assert!(descriptor.modifiers.contains(&expected_modifier), "{raw}");
            assert!(descriptor.evidence.iter().any(|evidence| {
                evidence.source == "ViewPosition"
                    && evidence.value == raw
                    && evidence.confidence == Confidence::Heuristic
            }));
        }
    }
}
