use std::fmt;

use dicom_object::InMemDicomObject;

use crate::registry::parse_laterality_value;
use crate::types::{Laterality, ViewPosition};

use super::laterality::{extract_frame_laterality, extract_laterality};
use super::tags::{
    get_string_value, IMAGE_LATERALITY, LATERALITY as LATERALITY_TAG, PATIENT_ORIENTATION,
};
use super::view_position::extract_view_descriptor;

/// Relationship between PatientOrientation and the conventional mammography orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConventionalOrientationStatus {
    Matches,
    RequiresFlip,
    Indeterminate,
    NotApplicable,
}

impl ConventionalOrientationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matches => "matches",
            Self::RequiresFlip => "requires_flip",
            Self::Indeterminate => "indeterminate",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl fmt::Display for ConventionalOrientationStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Assessment of PatientOrientation against the conventional CC or MLO orientation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConventionalOrientationAssessment {
    pub status: ConventionalOrientationStatus,
    pub expected_components: Option<Vec<String>>,
    pub observed_components: Option<Vec<String>>,
    pub horizontal_flip_required: Option<bool>,
    pub vertical_flip_required: Option<bool>,
}

impl Default for ConventionalOrientationAssessment {
    fn default() -> Self {
        Self::unresolved(ConventionalOrientationStatus::NotApplicable, None, None)
    }
}

impl ConventionalOrientationAssessment {
    fn unresolved(
        status: ConventionalOrientationStatus,
        expected_components: Option<Vec<String>>,
        observed_components: Option<Vec<String>>,
    ) -> Self {
        Self {
            status,
            expected_components,
            observed_components,
            horizontal_flip_required: None,
            vertical_flip_required: None,
        }
    }
}

/// Assess PatientOrientation without imposing SOP Class, modality, or mammogram-type gates.
pub fn assess_conventional_orientation(
    dcm: &InMemDicomObject,
) -> ConventionalOrientationAssessment {
    let observed_components = patient_orientation_components(dcm);
    let view_descriptor = extract_view_descriptor(dcm);
    let laterality = extract_laterality(dcm).unwrap_or(Laterality::Unknown);

    if !view_descriptor.conflicts.is_empty() || has_laterality_conflict(dcm) {
        return ConventionalOrientationAssessment::unresolved(
            ConventionalOrientationStatus::Indeterminate,
            None,
            observed_components,
        );
    }

    let Some(expected_components) =
        conventional_components(laterality, view_descriptor.view_position)
    else {
        return ConventionalOrientationAssessment::unresolved(
            ConventionalOrientationStatus::NotApplicable,
            None,
            observed_components,
        );
    };
    let expected_components = expected_components.map(str::to_string).to_vec();

    let Some(observed) = observed_components.as_ref() else {
        return ConventionalOrientationAssessment::unresolved(
            ConventionalOrientationStatus::Indeterminate,
            Some(expected_components),
            None,
        );
    };
    if observed.len() != expected_components.len() {
        return ConventionalOrientationAssessment::unresolved(
            ConventionalOrientationStatus::Indeterminate,
            Some(expected_components),
            observed_components,
        );
    }

    let horizontal_flip_required = component_flip_required(&expected_components[0], &observed[0]);
    let vertical_flip_required = component_flip_required(&expected_components[1], &observed[1]);
    let (Some(horizontal_flip_required), Some(vertical_flip_required)) =
        (horizontal_flip_required, vertical_flip_required)
    else {
        return ConventionalOrientationAssessment::unresolved(
            ConventionalOrientationStatus::Indeterminate,
            Some(expected_components),
            observed_components,
        );
    };

    ConventionalOrientationAssessment {
        status: if horizontal_flip_required || vertical_flip_required {
            ConventionalOrientationStatus::RequiresFlip
        } else {
            ConventionalOrientationStatus::Matches
        },
        expected_components: Some(expected_components),
        observed_components,
        horizontal_flip_required: Some(horizontal_flip_required),
        vertical_flip_required: Some(vertical_flip_required),
    }
}

fn patient_orientation_components(dcm: &InMemDicomObject) -> Option<Vec<String>> {
    let element = dcm.element(PATIENT_ORIENTATION).ok()?;
    let components = element.to_multi_str().ok()?;
    Some(
        components
            .iter()
            .map(|component| {
                component
                    .trim_matches(|character| character == ' ' || character == '\0')
                    .to_string()
            })
            .collect(),
    )
}

fn conventional_components(
    laterality: Laterality,
    view_position: ViewPosition,
) -> Option<[&'static str; 2]> {
    match (laterality, view_position) {
        (Laterality::Right, ViewPosition::Cc) => Some(["P", "L"]),
        (Laterality::Left, ViewPosition::Cc) => Some(["A", "R"]),
        (Laterality::Right, ViewPosition::Mlo) => Some(["P", "FL"]),
        (Laterality::Left, ViewPosition::Mlo) => Some(["A", "FR"]),
        _ => None,
    }
}

fn component_flip_required(expected: &str, observed: &str) -> Option<bool> {
    if observed == expected {
        return Some(false);
    }

    let inverse: Option<String> = expected.chars().map(inverse_anatomical_direction).collect();
    (inverse.as_deref() == Some(observed)).then_some(true)
}

fn inverse_anatomical_direction(direction: char) -> Option<char> {
    match direction {
        'A' => Some('P'),
        'P' => Some('A'),
        'R' => Some('L'),
        'L' => Some('R'),
        'F' => Some('H'),
        'H' => Some('F'),
        _ => None,
    }
}

fn has_laterality_conflict(dcm: &InMemDicomObject) -> bool {
    let evidence = [
        get_string_value(dcm, IMAGE_LATERALITY),
        get_string_value(dcm, LATERALITY_TAG),
        extract_frame_laterality(dcm),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_empty())
    .map(|value| {
        parse_laterality_value(&value)
            .map(|laterality| format!("known:{laterality}"))
            .unwrap_or_else(|| format!("unsupported:{}", value.trim().to_ascii_uppercase()))
    });

    let mut first = None;
    for candidate in evidence {
        if first.as_ref().is_some_and(|first| first != &candidate) {
            return true;
        }
        first = Some(candidate);
    }
    false
}

#[cfg(test)]
mod tests {
    use dicom_core::value::{DataSetSequence, PrimitiveValue};
    use dicom_core::{DataElement, VR};

    use super::*;
    use crate::extraction::tags::{
        CODE_MEANING, CODE_VALUE, CODING_SCHEME_DESIGNATOR, FRAME_ANATOMY_SEQUENCE,
        FRAME_LATERALITY, SHARED_FUNCTIONAL_GROUPS_SEQUENCE, VIEW_CODE_SEQUENCE,
        VIEW_POSITION as VIEW_POSITION_TAG,
    };

    fn dicom(
        laterality: &str,
        view: &str,
        orientation: Option<PrimitiveValue>,
    ) -> InMemDicomObject {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            IMAGE_LATERALITY,
            VR::CS,
            PrimitiveValue::from(laterality),
        ));
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            PrimitiveValue::from(view),
        ));
        if let Some(orientation) = orientation {
            dcm.put(DataElement::new(PATIENT_ORIENTATION, VR::CS, orientation));
        }
        dcm
    }

    fn orientation(components: &[&str]) -> PrimitiveValue {
        PrimitiveValue::Strs(
            components
                .iter()
                .map(|component| (*component).to_string())
                .collect::<Vec<_>>()
                .into(),
        )
    }

    #[test]
    fn assesses_all_standard_views_and_axis_flip_combinations() {
        let cases = [
            ("R", "CC", ["P", "L"], ["A", "R"]),
            ("L", "CC", ["A", "R"], ["P", "L"]),
            ("R", "MLO", ["P", "FL"], ["A", "HR"]),
            ("L", "MLO", ["A", "FR"], ["P", "HL"]),
        ];

        for (laterality, view, expected, inverse) in cases {
            for (horizontal_flip, vertical_flip) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let observed = [
                    if horizontal_flip {
                        inverse[0]
                    } else {
                        expected[0]
                    },
                    if vertical_flip {
                        inverse[1]
                    } else {
                        expected[1]
                    },
                ];
                let assessment = assess_conventional_orientation(&dicom(
                    laterality,
                    view,
                    Some(orientation(&observed)),
                ));

                assert_eq!(
                    assessment.status,
                    if horizontal_flip || vertical_flip {
                        ConventionalOrientationStatus::RequiresFlip
                    } else {
                        ConventionalOrientationStatus::Matches
                    }
                );
                assert_eq!(
                    assessment.expected_components,
                    Some(expected.map(str::to_string).to_vec())
                );
                assert_eq!(
                    assessment.observed_components,
                    Some(observed.map(str::to_string).to_vec())
                );
                assert_eq!(assessment.horizontal_flip_required, Some(horizontal_flip));
                assert_eq!(assessment.vertical_flip_required, Some(vertical_flip));
            }
        }
    }

    #[test]
    fn reports_missing_and_zero_length_orientation_as_indeterminate() {
        let missing = assess_conventional_orientation(&dicom("R", "CC", None));
        assert_eq!(missing.status, ConventionalOrientationStatus::Indeterminate);
        assert_eq!(missing.observed_components, None);

        let empty = assess_conventional_orientation(&dicom("R", "CC", Some(PrimitiveValue::Empty)));
        assert_eq!(empty.status, ConventionalOrientationStatus::Indeterminate);
        assert_eq!(empty.observed_components, Some(Vec::new()));
    }

    #[test]
    fn trims_only_dicom_padding() {
        let padded =
            assess_conventional_orientation(&dicom("R", "MLO", Some(orientation(&[" P ", "FL "]))));
        assert_eq!(padded.status, ConventionalOrientationStatus::Matches);

        let tabbed =
            assess_conventional_orientation(&dicom("R", "CC", Some(orientation(&["\tP", "L"]))));
        assert_eq!(tabbed.status, ConventionalOrientationStatus::Indeterminate);
    }

    #[test]
    fn malformed_partial_reordered_and_unsupported_values_are_indeterminate() {
        for components in [
            vec!["P"],
            vec!["P", "L", "F"],
            vec!["", "L"],
            vec!["p", "L"],
            vec!["X", "L"],
            vec!["P", ""],
            vec!["L", "P"],
            vec!["P", "F"],
            vec!["P", "LF"],
        ] {
            let assessment =
                assess_conventional_orientation(&dicom("R", "MLO", Some(orientation(&components))));
            assert_eq!(
                assessment.status,
                ConventionalOrientationStatus::Indeterminate,
                "components: {components:?}"
            );
            assert_eq!(assessment.horizontal_flip_required, None);
            assert_eq!(assessment.vertical_flip_required, None);
        }
    }

    #[test]
    fn nonstandard_or_unsupported_view_context_is_not_applicable() {
        for (laterality, view) in [("R", "ML"), ("", "CC"), ("B", "CC"), ("X", "CC")] {
            let assessment = assess_conventional_orientation(&dicom(
                laterality,
                view,
                Some(orientation(&["P", "L"])),
            ));
            assert_eq!(
                assessment.status,
                ConventionalOrientationStatus::NotApplicable
            );
            assert_eq!(assessment.expected_components, None);
            assert_eq!(
                assessment.observed_components,
                Some(vec!["P".to_string(), "L".to_string()])
            );
        }
    }

    #[test]
    fn uses_laterality_fallbacks() {
        let mut dcm = dicom("", "CC", Some(orientation(&["A", "R"])));
        dcm.put(DataElement::new(
            LATERALITY_TAG,
            VR::CS,
            PrimitiveValue::from("L"),
        ));
        assert_eq!(
            assess_conventional_orientation(&dcm).status,
            ConventionalOrientationStatus::Matches
        );

        let mut dcm = dicom("", "CC", Some(orientation(&["P", "L"])));
        let frame_item = InMemDicomObject::from_element_iter([DataElement::new(
            FRAME_LATERALITY,
            VR::CS,
            PrimitiveValue::from("R"),
        )]);
        let frame_sequence = DataElement::new(
            FRAME_ANATOMY_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_item]),
        );
        let shared_item = InMemDicomObject::from_element_iter([frame_sequence]);
        dcm.put(DataElement::new(
            SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![shared_item]),
        ));
        assert_eq!(
            assess_conventional_orientation(&dcm).status,
            ConventionalOrientationStatus::Matches
        );
    }

    #[test]
    fn conflicting_laterality_is_indeterminate() {
        let mut dcm = dicom("L", "CC", Some(orientation(&["A", "R"])));
        dcm.put(DataElement::new(
            LATERALITY_TAG,
            VR::CS,
            PrimitiveValue::from("R"),
        ));

        let assessment = assess_conventional_orientation(&dcm);
        assert_eq!(
            assessment.status,
            ConventionalOrientationStatus::Indeterminate
        );
        assert_eq!(assessment.expected_components, None);

        let mut unsupported_and_known = dicom("X", "CC", Some(orientation(&["A", "R"])));
        unsupported_and_known.put(DataElement::new(
            LATERALITY_TAG,
            VR::CS,
            PrimitiveValue::from("L"),
        ));
        assert_eq!(
            assess_conventional_orientation(&unsupported_and_known).status,
            ConventionalOrientationStatus::Indeterminate
        );
    }

    #[test]
    fn conflicting_view_evidence_is_indeterminate() {
        let mut dcm = dicom("L", "CC", Some(orientation(&["A", "R"])));
        let coded_mlo = InMemDicomObject::from_element_iter([
            DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from("399368009")),
            DataElement::new(
                CODING_SCHEME_DESIGNATOR,
                VR::SH,
                PrimitiveValue::from("SCT"),
            ),
            DataElement::new(
                CODE_MEANING,
                VR::LO,
                PrimitiveValue::from("medio-lateral oblique"),
            ),
        ]);
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![coded_mlo]),
        ));

        let assessment = assess_conventional_orientation(&dcm);
        assert_eq!(
            assessment.status,
            ConventionalOrientationStatus::Indeterminate
        );
        assert_eq!(assessment.expected_components, None);
    }

    #[test]
    fn assessment_does_not_require_mammography_modality_or_sop_class() {
        let dcm = dicom("R", "CC", Some(orientation(&["A", "R"])));
        let assessment = assess_conventional_orientation(&dcm);
        assert_eq!(
            assessment.status,
            ConventionalOrientationStatus::RequiresFlip
        );
        assert_eq!(assessment.horizontal_flip_required, Some(true));
        assert_eq!(assessment.vertical_flip_required, Some(true));
    }
}
