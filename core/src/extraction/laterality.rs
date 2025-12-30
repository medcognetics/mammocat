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

    // NOTE: FrameLaterality in SharedFunctionalGroupsSequence is not yet implemented.
    // This requires dicom-rs sequence navigation. Most DICOM files have ImageLaterality
    // or Laterality tags, so this fallback is rarely needed in practice.

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
}
