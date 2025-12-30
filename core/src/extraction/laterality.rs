use crate::error::Result;
use crate::types::Laterality;
use dicom_object::InMemDicomObject;

use super::tags::{
    get_string_value, FRAME_ANATOMY_SEQUENCE, FRAME_LATERALITY, IMAGE_LATERALITY,
    LATERALITY as LATERALITY_TAG, SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
};

/// Extracts laterality from DICOM file
///
/// Implements the fallback hierarchy from Python types.py:382-432
///
/// # Algorithm
///
/// 1. Try ImageLaterality tag first
/// 2. Fall back to Laterality tag
/// 3. Fall back to FrameLaterality in SharedFunctionalGroupsSequence
/// 4. Parse: "l"→Left, "r"→Right, else→Unknown
pub fn extract_laterality(dcm: &InMemDicomObject) -> Result<Laterality> {
    // First try ImageLaterality
    if let Some(lat) = get_string_value(dcm, IMAGE_LATERALITY) {
        if !lat.is_empty() {
            return Ok(parse_laterality_string(&lat));
        }
    }

    // Next try Laterality
    if let Some(lat) = get_string_value(dcm, LATERALITY_TAG) {
        if !lat.is_empty() {
            return Ok(parse_laterality_string(&lat));
        }
    }

    // Fall back to FrameLaterality in SharedFunctionalGroupsSequence
    // This mirrors Python types.py:393-404
    if let Some(lat) = extract_frame_laterality(dcm) {
        if !lat.is_empty() {
            return Ok(parse_laterality_string(&lat));
        }
    }

    Ok(Laterality::Unknown)
}

/// Extracts FrameLaterality from SharedFunctionalGroupsSequence
///
/// Navigates: SharedFunctionalGroupsSequence[0] → FrameAnatomySequence[0] → FrameLaterality
/// This mirrors the Python implementation in types.py:393-404
///
/// # Arguments
///
/// * `dcm` - DICOM object to extract from
///
/// # Returns
///
/// `Some(String)` if FrameLaterality is found, `None` otherwise
fn extract_frame_laterality(dcm: &InMemDicomObject) -> Option<String> {
    // Try to navigate the sequence hierarchy
    // SharedFunctionalGroupsSequence[0] → FrameAnatomySequence[0] → FrameLaterality
    dcm.element(SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .ok()
        .and_then(|shared_seq| shared_seq.items())
        .and_then(|items| items.first())
        .and_then(|first_item| first_item.element(FRAME_ANATOMY_SEQUENCE).ok())
        .and_then(|frame_anatomy_seq| frame_anatomy_seq.items())
        .and_then(|items| items.first())
        .and_then(|first_item| get_string_value(first_item, FRAME_LATERALITY))
}

/// Parses laterality from a string value
///
/// Handles the standard DICOM laterality codes:
/// - "L" → Left
/// - "R" → Right
/// - Otherwise → Unknown
fn parse_laterality_string(s: &str) -> Laterality {
    let s_lower = s.trim().to_lowercase();
    if s_lower == "l" {
        Laterality::Left
    } else if s_lower == "r" {
        Laterality::Right
    } else {
        Laterality::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, VR};
    use dicom_object::InMemDicomObject;

    #[test]
    fn test_parse_laterality_string() {
        assert_eq!(parse_laterality_string("L"), Laterality::Left);
        assert_eq!(parse_laterality_string("R"), Laterality::Right);
        assert_eq!(parse_laterality_string("l"), Laterality::Left);
        assert_eq!(parse_laterality_string("r"), Laterality::Right);
        assert_eq!(parse_laterality_string(" L "), Laterality::Left);
        assert_eq!(parse_laterality_string(""), Laterality::Unknown);
        assert_eq!(parse_laterality_string("UNKNOWN"), Laterality::Unknown);
    }

    #[test]
    fn test_extract_frame_laterality_empty() {
        // Empty DICOM should return None
        let dcm = InMemDicomObject::new_empty();
        assert!(extract_frame_laterality(&dcm).is_none());
    }

    #[test]
    fn test_extract_frame_laterality_with_sequence() {
        // Create a DICOM object with FrameLaterality in SharedFunctionalGroupsSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Create FrameAnatomySequence item with FrameLaterality
        let frame_anatomy_item = InMemDicomObject::from_element_iter([DataElement::new(
            FRAME_LATERALITY,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("L"),
        )]);

        // Create FrameAnatomySequence
        let frame_anatomy_seq = DataElement::new(
            FRAME_ANATOMY_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_anatomy_item]),
        );

        // Create SharedFunctionalGroupsSequence item containing FrameAnatomySequence
        let shared_groups_item = InMemDicomObject::from_element_iter([frame_anatomy_seq]);

        // Create SharedFunctionalGroupsSequence
        let shared_groups_seq = DataElement::new(
            SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![shared_groups_item]),
        );

        dcm.put(shared_groups_seq);

        // Test extraction
        let result = extract_frame_laterality(&dcm);
        assert_eq!(result, Some("L".to_string()));
    }

    #[test]
    fn test_extract_laterality_fallback_to_frame_laterality() {
        // Create a DICOM object with only FrameLaterality (no ImageLaterality or Laterality)
        let mut dcm = InMemDicomObject::new_empty();

        // Create FrameAnatomySequence item with FrameLaterality
        let frame_anatomy_item = InMemDicomObject::from_element_iter([DataElement::new(
            FRAME_LATERALITY,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("R"),
        )]);

        // Create FrameAnatomySequence
        let frame_anatomy_seq = DataElement::new(
            FRAME_ANATOMY_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_anatomy_item]),
        );

        // Create SharedFunctionalGroupsSequence item
        let shared_groups_item = InMemDicomObject::from_element_iter([frame_anatomy_seq]);

        // Create SharedFunctionalGroupsSequence
        let shared_groups_seq = DataElement::new(
            SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![shared_groups_item]),
        );

        dcm.put(shared_groups_seq);

        // Test that extract_laterality falls back to FrameLaterality
        let result = extract_laterality(&dcm).unwrap();
        assert_eq!(result, Laterality::Right);
    }

    #[test]
    fn test_extract_laterality_priority() {
        // Create a DICOM object with all three laterality tags
        // Should prioritize ImageLaterality > Laterality > FrameLaterality
        let mut dcm = InMemDicomObject::new_empty();

        // Add ImageLaterality (should be used)
        dcm.put(DataElement::new(
            IMAGE_LATERALITY,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("L"),
        ));

        // Add Laterality (should be ignored because ImageLaterality is present)
        dcm.put(DataElement::new(
            LATERALITY_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("R"),
        ));

        let result = extract_laterality(&dcm).unwrap();
        assert_eq!(result, Laterality::Left); // Should use ImageLaterality
    }
}
