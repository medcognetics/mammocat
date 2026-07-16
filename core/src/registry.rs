//! Canonical mammography metadata rules shared by readers, writers, and validation.

use dicom_core::{Tag, VR};
use dicom_dictionary_std::{tags, uids};

use crate::types::{Laterality, MammographyViewModifier, ViewPosition};

const RETIRED_SNOMED_CODING_SCHEMES: &[&str] = &["SRT", "SNM3", "99SDM"];
const LEGACY_BILATERAL_LATERALITY: &str = "BILATERAL";

pub(crate) fn is_retired_snomed_coding_scheme(value: &str) -> bool {
    RETIRED_SNOMED_CODING_SCHEMES
        .iter()
        .any(|scheme| value.eq_ignore_ascii_case(scheme))
}

pub(crate) fn parse_laterality_value(value: &str) -> Option<Laterality> {
    match value.trim().to_ascii_uppercase().as_str() {
        "L" => Some(Laterality::Left),
        "R" => Some(Laterality::Right),
        "B" | LEGACY_BILATERAL_LATERALITY => Some(Laterality::Bilateral),
        _ => None,
    }
}

pub(crate) fn retired_view_code_matches(definition: &ViewCodeDefinition, code: &str) -> bool {
    code.eq_ignore_ascii_case(definition.legacy_code_value)
        || matches!(
            (definition.view, code.to_ascii_uppercase().as_str()),
            (ViewPosition::Xccl, "Y-X1770") | (ViewPosition::Xccm, "Y-X1771")
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Heuristic,
    Structural,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataConsumer {
    Extraction,
    Classification,
    Selection,
    Validation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalParser {
    CodeTupleThenMeaning,
    CanonicalString,
    UnsignedShort,
    SopIdentity,
    SharedFrameAnatomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterRepresentation {
    Primitive,
    CodedSequence,
    NestedCodedSequence,
    SharedFunctionalGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SopApplicability {
    AllSupported,
    ClassicMammography,
    EnhancedMammography,
    PresentationStorage,
    ProcessingStorage,
    BreastTomosynthesis,
}

impl SopApplicability {
    pub fn applies(self, sop_class_uid: &str) -> bool {
        match self {
            Self::AllSupported => SUPPORTED_SOP_CLASSES.contains(&sop_class_uid),
            Self::ClassicMammography => matches!(
                sop_class_uid,
                uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION
                    | uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PROCESSING
            ),
            Self::EnhancedMammography => matches!(
                sop_class_uid,
                uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE
                    | uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION
                    | uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PROCESSING
            ),
            Self::PresentationStorage => matches!(
                sop_class_uid,
                uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION
                    | uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION
            ),
            Self::ProcessingStorage => matches!(
                sop_class_uid,
                uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PROCESSING
                    | uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PROCESSING
            ),
            Self::BreastTomosynthesis => sop_class_uid == uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalValue {
    Text(&'static str),
    UnsignedShort(u16),
    ContextGroup(&'static str),
    Inferred,
}

impl CanonicalValue {
    pub fn display(self) -> String {
        match self {
            Self::Text(value) | Self::ContextGroup(value) => value.to_string(),
            Self::UnsignedShort(value) => value.to_string(),
            Self::Inferred => "inferred".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalMetadataRule {
    pub keyword: &'static str,
    pub applicability: SopApplicability,
    pub path: &'static str,
    pub tag: Tag,
    pub vr: VR,
    pub vm: &'static str,
    pub canonical_value: CanonicalValue,
    pub parser: CanonicalParser,
    pub accepted_legacy_aliases: &'static [&'static str],
    pub inference_sources: &'static [&'static str],
    pub confidence: Confidence,
    pub writer_representation: WriterRepresentation,
    pub consumers: &'static [MetadataConsumer],
}

#[derive(Debug, Clone, Copy)]
pub struct ViewCodeDefinition {
    pub view: ViewPosition,
    pub code_value: &'static str,
    pub code_meaning: &'static str,
    pub legacy_code_value: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ViewModifierCodeDefinition {
    pub modifier: MammographyViewModifier,
    pub code_value: &'static str,
    pub code_meaning: &'static str,
    pub legacy_code_value: &'static str,
}

const EXTRACT_VALIDATE: &[MetadataConsumer] = &[
    MetadataConsumer::Extraction,
    MetadataConsumer::Classification,
    MetadataConsumer::Validation,
];
const EXTRACT_SELECT_VALIDATE: &[MetadataConsumer] = &[
    MetadataConsumer::Extraction,
    MetadataConsumer::Selection,
    MetadataConsumer::Validation,
];
const VALIDATE: &[MetadataConsumer] = &[MetadataConsumer::Validation];

pub const SUPPORTED_SOP_CLASSES: &[&str] = &[
    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
    uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE,
    uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    uids::BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
];

pub const VIEW_CODE_DEFINITIONS: &[ViewCodeDefinition] = &[
    ViewCodeDefinition {
        view: ViewPosition::Ml,
        code_value: "399260004",
        code_meaning: "medio-lateral",
        legacy_code_value: "R-10224",
    },
    ViewCodeDefinition {
        view: ViewPosition::Mlo,
        code_value: "399368009",
        code_meaning: "medio-lateral oblique",
        legacy_code_value: "R-10226",
    },
    ViewCodeDefinition {
        view: ViewPosition::Lm,
        code_value: "399352003",
        code_meaning: "latero-medial",
        legacy_code_value: "R-10228",
    },
    ViewCodeDefinition {
        view: ViewPosition::Lmo,
        code_value: "399099002",
        code_meaning: "latero-medial oblique",
        legacy_code_value: "R-10230",
    },
    ViewCodeDefinition {
        view: ViewPosition::Cc,
        code_value: "399162004",
        code_meaning: "cranio-caudal",
        legacy_code_value: "R-10242",
    },
    ViewCodeDefinition {
        view: ViewPosition::Fb,
        code_value: "399196006",
        code_meaning: "caudo-cranial (from below)",
        legacy_code_value: "R-10244",
    },
    ViewCodeDefinition {
        view: ViewPosition::Sio,
        code_value: "399188001",
        code_meaning: "superolateral to inferomedial oblique",
        legacy_code_value: "R-102D0",
    },
    ViewCodeDefinition {
        view: ViewPosition::Iso,
        code_value: "441555000",
        code_meaning: "inferomedial to superolateral oblique",
        legacy_code_value: "R-40AAA",
    },
    ViewCodeDefinition {
        view: ViewPosition::Xccl,
        code_value: "399192008",
        code_meaning: "cranio-caudal exaggerated laterally",
        legacy_code_value: "R-1024A",
    },
    ViewCodeDefinition {
        view: ViewPosition::Xccm,
        code_value: "399101009",
        code_meaning: "cranio-caudal exaggerated medially",
        legacy_code_value: "R-1024B",
    },
    ViewCodeDefinition {
        view: ViewPosition::Specimen,
        code_value: "127457009",
        code_meaning: "tissue specimen from breast",
        legacy_code_value: "G-8310",
    },
];

pub const VIEW_MODIFIER_CODE_DEFINITIONS: &[ViewModifierCodeDefinition] = &[
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::Cleavage,
        code_value: "399161006",
        code_meaning: "Cleavage",
        legacy_code_value: "R-102D2",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::AxillaryTail,
        code_value: "399011000",
        code_meaning: "Axillary Tail",
        legacy_code_value: "R-102D1",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::RolledLateral,
        code_value: "399197002",
        code_meaning: "Rolled Lateral",
        legacy_code_value: "R-102D3",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::RolledMedial,
        code_value: "399226006",
        code_meaning: "Rolled Medial",
        legacy_code_value: "R-102D4",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::RolledInferior,
        code_value: "414493004",
        code_meaning: "Rolled Inferior",
        legacy_code_value: "R-102CA",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::RolledSuperior,
        code_value: "415670009",
        code_meaning: "Rolled Superior",
        legacy_code_value: "R-102C9",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::ImplantDisplaced,
        code_value: "399209000",
        code_meaning: "Implant Displaced",
        legacy_code_value: "R-102D5",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::Magnification,
        code_value: "399163009",
        code_meaning: "Magnification",
        legacy_code_value: "R-102D6",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::SpotCompression,
        code_value: "399055006",
        code_meaning: "Spot Compression",
        legacy_code_value: "R-102D7",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::Tangential,
        code_value: "399110001",
        code_meaning: "Tangential",
        legacy_code_value: "R-102C2",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::NippleInProfile,
        code_value: "442581004",
        code_meaning: "Nipple in profile",
        legacy_code_value: "R-40AB3",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::AnteriorCompression,
        code_value: "441752004",
        code_meaning: "Anterior compression",
        legacy_code_value: "P2-00161",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::InfraMammaryFold,
        code_value: "442593008",
        code_meaning: "Infra-mammary fold",
        legacy_code_value: "R-40ABE",
    },
    ViewModifierCodeDefinition {
        modifier: MammographyViewModifier::AxillaryTissue,
        code_value: "442580003",
        code_meaning: "Axillary tissue",
        legacy_code_value: "R-40AB2",
    },
];

pub const CANONICAL_METADATA_RULES: &[CanonicalMetadataRule] = &[
    CanonicalMetadataRule {
        keyword: "SOPClassUID",
        applicability: SopApplicability::AllSupported,
        path: "SOPClassUID",
        tag: tags::SOP_CLASS_UID,
        vr: VR::UI,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::SopIdentity,
        accepted_legacy_aliases: &[],
        inference_sources: &["MediaStorageSOPClassUID"],
        confidence: Confidence::Exact,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "ImageLaterality",
        applicability: SopApplicability::ClassicMammography,
        path: "ImageLaterality",
        tag: tags::IMAGE_LATERALITY,
        vr: VR::CS,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::CanonicalString,
        accepted_legacy_aliases: &[LEGACY_BILATERAL_LATERALITY],
        inference_sources: &["ImageLaterality", "Laterality", "FrameLaterality"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: EXTRACT_VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "SOPInstanceUID",
        applicability: SopApplicability::AllSupported,
        path: "SOPInstanceUID",
        tag: tags::SOP_INSTANCE_UID,
        vr: VR::UI,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::SopIdentity,
        accepted_legacy_aliases: &[],
        inference_sources: &["MediaStorageSOPInstanceUID"],
        confidence: Confidence::Exact,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "FrameAnatomySequence",
        applicability: SopApplicability::EnhancedMammography,
        path: "SharedFunctionalGroupsSequence/FrameAnatomySequence",
        tag: tags::FRAME_ANATOMY_SEQUENCE,
        vr: VR::SQ,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::SharedFrameAnatomy,
        accepted_legacy_aliases: &[LEGACY_BILATERAL_LATERALITY],
        inference_sources: &["ImageLaterality", "Laterality", "FrameLaterality"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::SharedFunctionalGroup,
        consumers: EXTRACT_VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "ViewPosition",
        applicability: SopApplicability::AllSupported,
        path: "ViewPosition",
        tag: tags::VIEW_POSITION,
        vr: VR::CS,
        vm: "1",
        canonical_value: CanonicalValue::ContextGroup("CID 4014 ACR equivalent"),
        parser: CanonicalParser::CanonicalString,
        accepted_legacy_aliases: &["AT", "CV"],
        inference_sources: &["ViewPosition", "ViewCodeSequence", "descriptions"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: EXTRACT_SELECT_VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "ViewCodeSequence",
        applicability: SopApplicability::AllSupported,
        path: "ViewCodeSequence",
        tag: tags::VIEW_CODE_SEQUENCE,
        vr: VR::SQ,
        vm: "1",
        canonical_value: CanonicalValue::ContextGroup("CID 4014"),
        parser: CanonicalParser::CodeTupleThenMeaning,
        accepted_legacy_aliases: &[
            "SRT",
            "SNM3",
            "99SDM",
            "deprecated XCC codes Y-X1770 and Y-X1771",
        ],
        inference_sources: &["ViewCodeSequence", "ViewPosition", "descriptions"],
        confidence: Confidence::Exact,
        writer_representation: WriterRepresentation::CodedSequence,
        consumers: EXTRACT_SELECT_VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "ViewModifierCodeSequence",
        applicability: SopApplicability::AllSupported,
        path: "ViewCodeSequence/ViewModifierCodeSequence",
        tag: tags::VIEW_MODIFIER_CODE_SEQUENCE,
        vr: VR::SQ,
        vm: "0-n",
        canonical_value: CanonicalValue::ContextGroup("CID 4015"),
        parser: CanonicalParser::CodeTupleThenMeaning,
        accepted_legacy_aliases: &["SRT", "SNM3", "99SDM", "top-level sequence"],
        inference_sources: &[
            "nested/top-level code sequence",
            "ViewPosition",
            "PaddleDescription",
            "descriptions",
        ],
        confidence: Confidence::Exact,
        writer_representation: WriterRepresentation::NestedCodedSequence,
        consumers: EXTRACT_SELECT_VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "HighBit",
        applicability: SopApplicability::AllSupported,
        path: "HighBit",
        tag: tags::HIGH_BIT,
        vr: VR::US,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::UnsignedShort,
        accepted_legacy_aliases: &[],
        inference_sources: &["BitsStored - 1"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "PhotometricInterpretation",
        applicability: SopApplicability::AllSupported,
        path: "PhotometricInterpretation",
        tag: tags::PHOTOMETRIC_INTERPRETATION,
        vr: VR::CS,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::CanonicalString,
        accepted_legacy_aliases: &[],
        inference_sources: &["PresentationLUTShape"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    CanonicalMetadataRule {
        keyword: "PresentationLUTShape",
        applicability: SopApplicability::AllSupported,
        path: "PresentationLUTShape",
        tag: tags::PRESENTATION_LUT_SHAPE,
        vr: VR::CS,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::CanonicalString,
        accepted_legacy_aliases: &[],
        inference_sources: &["PhotometricInterpretation"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    fixed_rule(
        "Modality",
        tags::MODALITY,
        VR::CS,
        CanonicalValue::Text("MG"),
        SopApplicability::AllSupported,
        EXTRACT_VALIDATE,
    ),
    fixed_rule(
        "OrganExposed",
        tags::ORGAN_EXPOSED,
        VR::CS,
        CanonicalValue::Text("BREAST"),
        SopApplicability::AllSupported,
        VALIDATE,
    ),
    CanonicalMetadataRule {
        keyword: "PositionerType",
        applicability: SopApplicability::AllSupported,
        path: "PositionerType",
        tag: tags::POSITIONER_TYPE,
        vr: VR::CS,
        vm: "1",
        canonical_value: CanonicalValue::Inferred,
        parser: CanonicalParser::CanonicalString,
        accepted_legacy_aliases: &["MAMMOGRAPHIC", "NONE"],
        inference_sources: &["unambiguous positioning evidence"],
        confidence: Confidence::Structural,
        writer_representation: WriterRepresentation::Primitive,
        consumers: VALIDATE,
    },
    fixed_rule(
        "SamplesPerPixel",
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        CanonicalValue::UnsignedShort(1),
        SopApplicability::AllSupported,
        VALIDATE,
    ),
    fixed_rule(
        "PixelRepresentation",
        tags::PIXEL_REPRESENTATION,
        VR::US,
        CanonicalValue::UnsignedShort(0),
        SopApplicability::AllSupported,
        VALIDATE,
    ),
    fixed_rule(
        "BurnedInAnnotation",
        tags::BURNED_IN_ANNOTATION,
        VR::CS,
        CanonicalValue::Text("NO"),
        SopApplicability::AllSupported,
        VALIDATE,
    ),
    fixed_rule(
        "PresentationIntentType",
        tags::PRESENTATION_INTENT_TYPE,
        VR::CS,
        CanonicalValue::Text("FOR PRESENTATION"),
        SopApplicability::PresentationStorage,
        EXTRACT_VALIDATE,
    ),
    fixed_rule(
        "PresentationIntentType",
        tags::PRESENTATION_INTENT_TYPE,
        VR::CS,
        CanonicalValue::Text("FOR PROCESSING"),
        SopApplicability::ProcessingStorage,
        EXTRACT_VALIDATE,
    ),
    fixed_rule(
        "RescaleIntercept",
        tags::RESCALE_INTERCEPT,
        VR::DS,
        CanonicalValue::Text("0"),
        SopApplicability::ClassicMammography,
        VALIDATE,
    ),
    fixed_rule(
        "RescaleSlope",
        tags::RESCALE_SLOPE,
        VR::DS,
        CanonicalValue::Text("1"),
        SopApplicability::ClassicMammography,
        VALIDATE,
    ),
    fixed_rule(
        "RescaleType",
        tags::RESCALE_TYPE,
        VR::LO,
        CanonicalValue::Text("US"),
        SopApplicability::ClassicMammography,
        VALIDATE,
    ),
    fixed_rule(
        "PhotometricInterpretation",
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        CanonicalValue::Text("MONOCHROME2"),
        SopApplicability::BreastTomosynthesis,
        VALIDATE,
    ),
    fixed_rule(
        "PresentationLUTShape",
        tags::PRESENTATION_LUT_SHAPE,
        VR::CS,
        CanonicalValue::Text("IDENTITY"),
        SopApplicability::BreastTomosynthesis,
        VALIDATE,
    ),
];

const fn fixed_rule(
    keyword: &'static str,
    tag: Tag,
    vr: VR,
    canonical_value: CanonicalValue,
    applicability: SopApplicability,
    consumers: &'static [MetadataConsumer],
) -> CanonicalMetadataRule {
    CanonicalMetadataRule {
        keyword,
        applicability,
        path: keyword,
        tag,
        vr,
        vm: "1",
        canonical_value,
        parser: match canonical_value {
            CanonicalValue::UnsignedShort(_) => CanonicalParser::UnsignedShort,
            _ => CanonicalParser::CanonicalString,
        },
        accepted_legacy_aliases: &[],
        inference_sources: &["SOP/IOD"],
        confidence: Confidence::Exact,
        writer_representation: WriterRepresentation::Primitive,
        consumers,
    }
}

#[derive(Debug)]
pub struct CanonicalMetadataRegistry {
    pub rules: &'static [CanonicalMetadataRule],
    pub view_codes: &'static [ViewCodeDefinition],
    pub view_modifier_codes: &'static [ViewModifierCodeDefinition],
}

pub static CANONICAL_METADATA_REGISTRY: CanonicalMetadataRegistry = CanonicalMetadataRegistry {
    rules: CANONICAL_METADATA_RULES,
    view_codes: VIEW_CODE_DEFINITIONS,
    view_modifier_codes: VIEW_MODIFIER_CODE_DEFINITIONS,
};

pub fn view_code_definition(view: ViewPosition) -> Option<&'static ViewCodeDefinition> {
    VIEW_CODE_DEFINITIONS
        .iter()
        .find(|definition| definition.view == view)
}

/// Canonical ACR string for ViewPosition when CID 4014 defines one.
pub fn view_position_value(view: ViewPosition) -> Option<&'static str> {
    match view {
        ViewPosition::Unknown | ViewPosition::Specimen => None,
        ViewPosition::Ml => Some("ML"),
        ViewPosition::Mlo => Some("MLO"),
        ViewPosition::Lm => Some("LM"),
        ViewPosition::Lmo => Some("LMO"),
        ViewPosition::Cc => Some("CC"),
        ViewPosition::Fb => Some("FB"),
        ViewPosition::Sio => Some("SIO"),
        ViewPosition::Iso => Some("ISO"),
        ViewPosition::Xccl => Some("XCCL"),
        ViewPosition::Xccm => Some("XCCM"),
    }
}

pub fn view_modifier_code_definition(
    modifier: MammographyViewModifier,
) -> &'static ViewModifierCodeDefinition {
    VIEW_MODIFIER_CODE_DEFINITIONS
        .iter()
        .find(|definition| definition.modifier == modifier)
        .expect("every modifier has a CID 4015 definition")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_complete_context_groups() {
        assert_eq!(CANONICAL_METADATA_REGISTRY.view_codes.len(), 11);
        assert_eq!(CANONICAL_METADATA_REGISTRY.view_modifier_codes.len(), 14);
    }

    #[test]
    fn every_canonical_rule_has_a_reader_or_validator() {
        for rule in CANONICAL_METADATA_REGISTRY.rules {
            assert!(!rule.path.is_empty(), "{} has no DICOM path", rule.keyword);
            assert!(!rule.vm.is_empty(), "{} has no VM", rule.keyword);
            assert!(
                !rule.consumers.is_empty(),
                "{} has no registered consumer",
                rule.keyword
            );
            assert!(matches!(
                rule.writer_representation,
                WriterRepresentation::Primitive
                    | WriterRepresentation::CodedSequence
                    | WriterRepresentation::NestedCodedSequence
                    | WriterRepresentation::SharedFunctionalGroup
            ));
        }
    }
}
