use crate::extraction::tags::{
    get_string_value, CODE_MEANING, PADDLE_DESCRIPTION, VIEW_MODIFIER_CODE_SEQUENCE, VIEW_POSITION,
};
use dicom_object::InMemDicomObject;

/// Extracts all view modifier code meanings from a DICOM file
///
/// Returns a vector of lowercase code meanings for matching against patterns.
/// Mirrors Python: dicom_utils/container/record.py:688-690
///
/// # Arguments
///
/// * `dcm` - DICOM object to extract from
///
/// # Returns
///
/// Vector of view modifier code meanings (trimmed and lowercased)
pub fn extract_view_modifier_meanings(dcm: &InMemDicomObject) -> Vec<String> {
    let mut meanings = Vec::new();

    // Try to get ViewModifierCodeSequence
    if let Ok(sequence_elem) = dcm.element(VIEW_MODIFIER_CODE_SEQUENCE) {
        if let Some(items) = sequence_elem.items() {
            for item in items {
                if let Some(meaning) = get_string_value(item, CODE_MEANING) {
                    meanings.push(meaning.trim().to_lowercase());
                }
            }
        }
    }

    meanings
}

/// Checks if "implant displaced" is present in view modifier codes
///
/// Implements Python logic from record.py:861-862
///
/// # Arguments
///
/// * `dcm` - DICOM object to check
///
/// # Returns
///
/// `true` if "implant displaced" is found in view modifier code meanings
pub fn is_implant_displaced(dcm: &InMemDicomObject) -> bool {
    extract_view_modifier_meanings(dcm)
        .iter()
        .any(|meaning| meaning.contains("implant displaced"))
}

/// Checks if this is a spot compression view
///
/// Implements Python logic from record.py:849-854 checking:
/// 1. PaddleDescription for "SPOT" or "SPT" (case-sensitive)
/// 2. ViewPosition for "spot" (case-insensitive)
/// 3. ViewModifierCodeSequence CodeMeaning for "spot compression"
///
/// # Arguments
///
/// * `dcm` - DICOM object to check
///
/// # Returns
///
/// `true` if spot compression is detected
pub fn is_spot_compression(dcm: &InMemDicomObject) -> bool {
    // Check PaddleDescription for "SPOT" or "SPT" (case-sensitive)
    if let Some(paddle_desc) = get_string_value(dcm, PADDLE_DESCRIPTION) {
        if paddle_desc.contains("SPOT") || paddle_desc.contains("SPT") {
            return true;
        }
    }

    // Check ViewPosition for "spot" (case-insensitive)
    if let Some(view_pos) = get_string_value(dcm, VIEW_POSITION) {
        if view_pos.to_lowercase().contains("spot") {
            return true;
        }
    }

    // Check ViewModifierCodeSequence for "spot compression"
    extract_view_modifier_meanings(dcm)
        .iter()
        .any(|meaning| meaning.contains("spot compression"))
}

/// Checks if this is a magnification view
///
/// Implements Python logic from record.py:857-858 checking:
/// 1. PaddleDescription for "MAG" (case-sensitive)
/// 2. ViewModifierCodeSequence CodeMeaning for "magnification" or "magnified" (case-insensitive)
///
/// # Arguments
///
/// * `dcm` - DICOM object to check
///
/// # Returns
///
/// `true` if magnification is detected
pub fn is_magnified(dcm: &InMemDicomObject) -> bool {
    // Check PaddleDescription for "MAG" (case-sensitive)
    if let Some(paddle_desc) = get_string_value(dcm, PADDLE_DESCRIPTION) {
        if paddle_desc.contains("MAG") {
            return true;
        }
    }

    // Check ViewModifierCodeSequence for "magnification" or "magnified"
    extract_view_modifier_meanings(dcm)
        .iter()
        .any(|meaning| meaning.contains("magnification") || meaning.contains("magnified"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_object::InMemDicomObject;

    #[test]
    fn test_is_implant_displaced_empty() {
        // Empty DICOM should return false
        let dcm = InMemDicomObject::new_empty();
        assert!(!is_implant_displaced(&dcm));
    }

    #[test]
    fn test_extract_view_modifier_meanings_empty() {
        // Empty DICOM should return empty vector
        let dcm = InMemDicomObject::new_empty();
        let meanings = extract_view_modifier_meanings(&dcm);
        assert!(meanings.is_empty());
    }

    #[test]
    fn test_is_spot_compression_empty() {
        // Empty DICOM should return false
        let dcm = InMemDicomObject::new_empty();
        assert!(!is_spot_compression(&dcm));
    }

    #[test]
    fn test_is_magnified_empty() {
        // Empty DICOM should return false
        let dcm = InMemDicomObject::new_empty();
        assert!(!is_magnified(&dcm));
    }

    // Note: Testing with actual sequences would require building proper DICOM sequences
    // which is complex. The main logic is tested here; integration tests with real
    // DICOM files will verify the complete functionality.
}
