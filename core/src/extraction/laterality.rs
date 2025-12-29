use crate::error::Result;
use crate::types::Laterality;
use dicom_object::InMemDicomObject;

use super::tags::{get_string_value, IMAGE_LATERALITY, LATERALITY as LATERALITY_TAG};

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

    // TODO: Fall back to FrameLaterality in SharedFunctionalGroupsSequence
    // This requires navigating nested sequences using dicom-rs
    // For MVP, we'll implement this in a future iteration

    Ok(Laterality::Unknown)
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

/// Extracts laterality from patient orientation
///
/// Patient orientation can indicate laterality:
/// - Contains "L" → Right (image shows left side of patient, so breast is on right of image)
/// - Contains "R" → Left (image shows right side of patient, so breast is on left of image)
///
/// This follows the DICOM convention where patient orientation describes
/// the direction from the first pixel row/column to the last.
#[allow(dead_code)]
fn from_patient_orientation(orientation: &[String]) -> Laterality {
    for item in orientation {
        let parts: Vec<char> = item.chars().collect();
        if parts.contains(&'L') {
            return Laterality::Right;
        } else if parts.contains(&'R') {
            return Laterality::Left;
        }
    }

    // Fallback: try from_str
    for item in orientation {
        let result = Laterality::from_str(item);
        if !result.is_unknown() {
            return result;
        }
    }

    Laterality::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_from_patient_orientation() {
        // Test the orientation parsing logic
        assert_eq!(
            from_patient_orientation(&["FL".to_string()]),
            Laterality::Right
        );
        assert_eq!(
            from_patient_orientation(&["FR".to_string()]),
            Laterality::Left
        );
        assert_eq!(
            from_patient_orientation(&["L".to_string()]),
            Laterality::Right
        );
        assert_eq!(
            from_patient_orientation(&["R".to_string()]),
            Laterality::Left
        );
    }
}
