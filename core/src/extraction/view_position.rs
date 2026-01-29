use crate::error::Result;
use crate::types::ViewPosition;
use dicom_object::InMemDicomObject;

use super::tags::{
    get_string_value, CODE_MEANING, VIEW_CODE_SEQUENCE, VIEW_MODIFIER_CODE_SEQUENCE,
    VIEW_POSITION as VIEW_POSITION_TAG,
};

// Pattern sets for view position matching
const CC_STRINGS: &[&str] = &["cranio-caudal", "caudal-cranial"];
const ML_STRINGS: &[&str] = &["medio-lateral", "medial-lateral"];
const LM_STRINGS: &[&str] = &["latero-medial", "lateral-medial"];
const AT_STRINGS: &[&str] = &["axillary tail"];
const CV_STRINGS: &[&str] = &["cleavage view", "valley-view"];

/// Extracts view position from DICOM file
///
/// Implements the extraction logic from Python types.py:586-594
///
/// # Algorithm
///
/// 1. Extract from ViewPosition tag with pattern matching
/// 2. Extract from all ViewCodeSequence → CodeMeaning entries
/// 3. Extract from all ViewModifierCodeSequence → CodeMeaning entries (including nested)
/// 4. Return the candidate with the highest enum value (Unknown < Xccl < ... < Cv)
///
/// This matches the Python behavior of combining all sources and selecting the maximum.
pub fn extract_view_position(dcm: &InMemDicomObject) -> Result<ViewPosition> {
    let mut candidates = Vec::new();

    // Extract from ViewPosition tag (loose mode)
    if let Some(vp) = get_string_value(dcm, VIEW_POSITION_TAG) {
        let result = from_str(&vp, false);
        if !result.is_unknown() {
            candidates.push(result);
        }
    }

    // Extract from ViewCodeSequence (strict mode)
    candidates.extend(extract_all_from_view_code_sequence(dcm));

    // Extract from ViewModifierCodeSequence (strict mode, including nested)
    candidates.extend(extract_all_from_view_modifier_code_sequence(dcm));

    // Return the maximum value (Python: sorted(candidates, key=lambda x: x.value)[-1])
    // ViewPosition derives Ord, so we can use max() directly
    Ok(candidates
        .into_iter()
        .max()
        .unwrap_or(ViewPosition::Unknown))
}

/// Extracts all view positions from ViewCodeSequence
///
/// Navigates: ViewCodeSequence items → CodeMeaning
/// This mirrors the Python implementation in types.py:591
///
/// # Arguments
///
/// * `dcm` - DICOM object to extract from
///
/// # Returns
///
/// Vector of all valid ViewPosition values found in ViewCodeSequence
fn extract_all_from_view_code_sequence(dcm: &InMemDicomObject) -> Vec<ViewPosition> {
    let mut results = Vec::new();

    // Try to get ViewCodeSequence
    if let Ok(seq_elem) = dcm.element(VIEW_CODE_SEQUENCE) {
        if let Some(items) = seq_elem.items() {
            for item in items {
                if let Some(code_meaning) = get_string_value(item, CODE_MEANING) {
                    let view_pos = from_str(&code_meaning, true); // strict mode
                    if !view_pos.is_unknown() {
                        results.push(view_pos);
                    }
                }
            }
        }
    }

    results
}

/// Extracts all view positions from ViewModifierCodeSequence
///
/// Navigates: ViewModifierCodeSequence items → CodeMeaning
/// Also recursively checks ViewModifierCodeSequence within ViewCodeSequence items
/// This mirrors the Python implementation in types.py:53-62 and types.py:592
///
/// # Arguments
///
/// * `dcm` - DICOM object to extract from
///
/// # Returns
///
/// Vector of all valid ViewPosition values found in ViewModifierCodeSequence
fn extract_all_from_view_modifier_code_sequence(dcm: &InMemDicomObject) -> Vec<ViewPosition> {
    let mut results = Vec::new();

    // Extract from top-level ViewModifierCodeSequence
    if let Ok(seq_elem) = dcm.element(VIEW_MODIFIER_CODE_SEQUENCE) {
        if let Some(items) = seq_elem.items() {
            for item in items {
                if let Some(code_meaning) = get_string_value(item, CODE_MEANING) {
                    let view_pos = from_str(&code_meaning, true); // strict mode
                    if !view_pos.is_unknown() {
                        results.push(view_pos);
                    }
                }
            }
        }
    }

    // Also check ViewModifierCodeSequence nested within ViewCodeSequence items
    if let Ok(seq_elem) = dcm.element(VIEW_CODE_SEQUENCE) {
        if let Some(items) = seq_elem.items() {
            for view_code_item in items {
                // Recursively extract from nested ViewModifierCodeSequence
                results.extend(extract_all_from_view_modifier_code_sequence(view_code_item));
            }
        }
    }

    results
}

/// Parses view position from string
///
/// Supports both strict and loose matching modes:
/// - Strict: exact match with predefined patterns only
/// - Loose: also tries substring matching
#[allow(clippy::should_implement_trait)]
pub fn from_str(s: &str, strict: bool) -> ViewPosition {
    let s_lower = s.trim().to_lowercase();

    if let Some(pos) = match_strict_patterns(&s_lower) {
        return pos;
    }

    if !strict {
        if let Some(pos) = match_loose_patterns(&s_lower) {
            return pos;
        }
    }

    ViewPosition::Unknown
}

/// Matches exact patterns and descriptive names
fn match_strict_patterns(s: &str) -> Option<ViewPosition> {
    // CC - Cranio-caudal
    if CC_STRINGS.contains(&s) || s == "cc" {
        return Some(ViewPosition::Cc);
    }

    // LMO - check before MLO (both contain "lateral" and "oblique")
    if matches_lmo(s) {
        return Some(ViewPosition::Lmo);
    }

    // MLO - Medio-lateral oblique
    if matches_mlo(s) {
        return Some(ViewPosition::Mlo);
    }

    // LM - check before ML
    if LM_STRINGS.contains(&s) || s == "lm" {
        return Some(ViewPosition::Lm);
    }

    // ML - Medio-lateral
    if ML_STRINGS.contains(&s) || s == "ml" {
        return Some(ViewPosition::Ml);
    }

    // XCCL - CC exaggerated laterally
    if s.contains("exaggerated laterally") || s == "xccl" {
        return Some(ViewPosition::Xccl);
    }

    // XCCM - CC exaggerated medially
    if s.contains("exaggerated medially") || s == "xccm" {
        return Some(ViewPosition::Xccm);
    }

    // AT - Axillary tail
    if AT_STRINGS.iter().any(|&p| s.contains(p)) || s == "at" {
        return Some(ViewPosition::At);
    }

    // CV - Cleavage view
    if CV_STRINGS.iter().any(|&p| s.contains(p)) || s == "cv" {
        return Some(ViewPosition::Cv);
    }

    None
}

/// Checks if string matches LMO patterns
fn matches_lmo(s: &str) -> bool {
    s == "lmo"
        || s == "latero-medial oblique"
        || s == "lateral-medial oblique"
        || (s.contains("oblique") && s.contains("latero"))
}

/// Checks if string matches MLO patterns
fn matches_mlo(s: &str) -> bool {
    s == "mlo"
        || s == "medio-lateral oblique"
        || s == "medial-lateral oblique"
        || (s.contains("oblique") && s.contains("medio"))
        || (s.contains("oblique") && s.contains("medial") && !s.contains("latero"))
}

/// Matches view position abbreviations as substrings
fn match_loose_patterns(s: &str) -> Option<ViewPosition> {
    // Check more specific patterns first to avoid false matches
    // (e.g., "xccl" before "cc", "mlo" before "ml")
    if s.contains("xccl") {
        Some(ViewPosition::Xccl)
    } else if s.contains("xccm") {
        Some(ViewPosition::Xccm)
    } else if s.contains("mlo") {
        Some(ViewPosition::Mlo)
    } else if s.contains("lmo") {
        Some(ViewPosition::Lmo)
    } else if contains_token(s, "cc") {
        Some(ViewPosition::Cc)
    } else if contains_token(s, "ml") {
        Some(ViewPosition::Ml)
    } else if contains_token(s, "lm") {
        Some(ViewPosition::Lm)
    } else if contains_token(s, "at") {
        Some(ViewPosition::At)
    } else if contains_token(s, "cv") {
        Some(ViewPosition::Cv)
    } else {
        None
    }
}

fn contains_token(s: &str, token: &str) -> bool {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .any(|part| part == token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, VR};
    use dicom_object::InMemDicomObject;

    #[test]
    fn test_from_str_basic() {
        assert_eq!(from_str("cc", false), ViewPosition::Cc);
        assert_eq!(from_str("CC", false), ViewPosition::Cc);
        assert_eq!(from_str("mlo", false), ViewPosition::Mlo);
        assert_eq!(from_str("MLO", false), ViewPosition::Mlo);
        assert_eq!(from_str("ml", false), ViewPosition::Ml);
        assert_eq!(from_str("lm", false), ViewPosition::Lm);
    }

    #[test]
    fn test_from_str_full_names() {
        assert_eq!(from_str("cranio-caudal", false), ViewPosition::Cc);
        assert_eq!(from_str("medio-lateral oblique", false), ViewPosition::Mlo);
        assert_eq!(from_str("medio-lateral", false), ViewPosition::Ml);
        assert_eq!(from_str("latero-medial", false), ViewPosition::Lm);
        assert_eq!(from_str("latero-medial oblique", false), ViewPosition::Lmo);
    }

    #[test]
    fn test_from_str_exaggerated() {
        assert_eq!(from_str("xccl", false), ViewPosition::Xccl);
        assert_eq!(from_str("xccm", false), ViewPosition::Xccm);
        assert_eq!(
            from_str("cranio-caudal exaggerated laterally", false),
            ViewPosition::Xccl
        );
        assert_eq!(
            from_str("cranio-caudal exaggerated medially", false),
            ViewPosition::Xccm
        );
    }

    #[test]
    fn test_from_str_special_views() {
        assert_eq!(from_str("axillary tail", false), ViewPosition::At);
        assert_eq!(from_str("at", false), ViewPosition::At);
        assert_eq!(from_str("cleavage view", false), ViewPosition::Cv);
        assert_eq!(from_str("cv", false), ViewPosition::Cv);
    }

    #[test]
    fn test_from_str_strict_mode() {
        assert_eq!(from_str("cc", true), ViewPosition::Cc);
        assert_eq!(from_str("mlo", true), ViewPosition::Mlo);

        // Loose patterns shouldn't match in strict mode
        assert_eq!(from_str("some cc view", true), ViewPosition::Unknown);
    }

    #[test]
    fn test_from_str_loose_mode() {
        assert_eq!(from_str("left cc view", false), ViewPosition::Cc);
        assert_eq!(from_str("right mlo projection", false), ViewPosition::Mlo);
    }

    #[test]
    fn test_from_str_loose_mode_avoids_false_positives() {
        assert_eq!(from_str("lateral", false), ViewPosition::Unknown);
        assert_eq!(from_str("accession", false), ViewPosition::Unknown);
    }

    #[test]
    fn test_from_str_unknown() {
        assert_eq!(from_str("", false), ViewPosition::Unknown);
        assert_eq!(from_str("invalid", false), ViewPosition::Unknown);
        assert_eq!(from_str("xyz", false), ViewPosition::Unknown);
    }

    #[test]
    fn test_extract_all_from_view_code_sequence_empty() {
        // Empty DICOM should return empty vector
        let dcm = InMemDicomObject::new_empty();
        assert!(extract_all_from_view_code_sequence(&dcm).is_empty());
    }

    #[test]
    fn test_extract_all_from_view_code_sequence_cranio_caudal() {
        // Test the user's example: CodeMeaning = "cranio-caudal" should parse to CC
        let mut dcm = InMemDicomObject::new_empty();

        // Create a ViewCodeSequence item with CodeMeaning "cranio-caudal"
        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cranio-caudal"),
        )]);

        // Create ViewCodeSequence
        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Test extraction
        let results = extract_all_from_view_code_sequence(&dcm);
        assert_eq!(results, vec![ViewPosition::Cc]);
    }

    #[test]
    fn test_extract_all_from_view_code_sequence_mlo() {
        // Test medio-lateral oblique
        let mut dcm = InMemDicomObject::new_empty();

        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("medio-lateral oblique"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        let results = extract_all_from_view_code_sequence(&dcm);
        assert_eq!(results, vec![ViewPosition::Mlo]);
    }

    #[test]
    fn test_extract_view_position_fallback_to_sequence() {
        // Test that extract_view_position falls back to ViewCodeSequence when ViewPosition is absent
        let mut dcm = InMemDicomObject::new_empty();

        // Only add ViewCodeSequence, no ViewPosition tag
        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cranio-caudal"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Test that extract_view_position uses the sequence fallback
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::Cc);
    }

    #[test]
    fn test_extract_view_position_priority() {
        // Test that ViewPosition tag takes priority over ViewCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Add ViewPosition tag with MLO
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("MLO"),
        ));

        // Add ViewCodeSequence with CC (should be ignored)
        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cranio-caudal"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Should use ViewPosition tag (MLO), not ViewCodeSequence (CC)
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::Mlo);
    }

    #[test]
    fn test_extract_all_from_view_code_sequence_multiple_items() {
        // Test with multiple items in sequence - should return all valid matches
        let mut dcm = InMemDicomObject::new_empty();

        // Create first item with invalid CodeMeaning
        let invalid_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("unknown view"),
        )]);

        // Create second item with valid CodeMeaning
        let mlo_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("medio-lateral oblique"),
        )]);

        // Create third item with another valid CodeMeaning
        let cc_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cranio-caudal"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![invalid_item, mlo_item, cc_item]),
        );

        dcm.put(view_code_seq);

        let results = extract_all_from_view_code_sequence(&dcm);
        assert_eq!(results, vec![ViewPosition::Mlo, ViewPosition::Cc]);
    }

    #[test]
    fn test_extract_all_from_view_modifier_code_sequence_empty() {
        // Empty DICOM should return empty vector
        let dcm = InMemDicomObject::new_empty();
        assert!(extract_all_from_view_modifier_code_sequence(&dcm).is_empty());
    }

    #[test]
    fn test_extract_all_from_view_modifier_code_sequence_top_level() {
        // Test extraction from top-level ViewModifierCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Create a ViewModifierCodeSequence item with CodeMeaning "axillary tail"
        let modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("axillary tail"),
        )]);

        let modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier_item]),
        );

        dcm.put(modifier_seq);

        let results = extract_all_from_view_modifier_code_sequence(&dcm);
        assert_eq!(results, vec![ViewPosition::At]);
    }

    #[test]
    fn test_extract_all_from_view_modifier_code_sequence_nested() {
        // Test extraction from ViewModifierCodeSequence nested within ViewCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Create a nested ViewModifierCodeSequence within a ViewCodeSequence item
        let modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cleavage view"),
        )]);

        let modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier_item]),
        );

        let mut view_code_item = InMemDicomObject::new_empty();
        view_code_item.put(modifier_seq);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        let results = extract_all_from_view_modifier_code_sequence(&dcm);
        assert_eq!(results, vec![ViewPosition::Cv]);
    }

    #[test]
    fn test_extract_all_from_view_modifier_code_sequence_both_levels() {
        // Test extraction from both top-level and nested ViewModifierCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Top-level ViewModifierCodeSequence with "axillary tail"
        let top_modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("axillary tail"),
        )]);

        let top_modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![top_modifier_item]),
        );

        dcm.put(top_modifier_seq);

        // Nested ViewModifierCodeSequence with "cleavage view"
        let nested_modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cleavage view"),
        )]);

        let nested_modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![nested_modifier_item]),
        );

        let mut view_code_item = InMemDicomObject::new_empty();
        view_code_item.put(nested_modifier_seq);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        let results = extract_all_from_view_modifier_code_sequence(&dcm);
        // Should get both: At from top-level and Cv from nested
        assert_eq!(results.len(), 2);
        assert!(results.contains(&ViewPosition::At));
        assert!(results.contains(&ViewPosition::Cv));
    }

    #[test]
    fn test_extract_view_position_selects_maximum() {
        // Test that extract_view_position selects the candidate with highest enum value
        // When we have both CC (value 3) and MLO (value 4), should return MLO
        let mut dcm = InMemDicomObject::new_empty();

        // Add ViewPosition tag with CC
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("CC"),
        ));

        // Add ViewCodeSequence with MLO (higher value than CC)
        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("medio-lateral oblique"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Should return MLO because it has higher enum value than CC
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::Mlo);
    }

    #[test]
    fn test_extract_view_position_with_view_modifier() {
        // Test extraction when ViewPosition comes from ViewModifierCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Add ViewPosition tag with CC
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("CC"),
        ));

        // Add ViewModifierCodeSequence with CV (cleavage view, highest value)
        let modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cleavage view"),
        )]);

        let modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier_item]),
        );

        dcm.put(modifier_seq);

        // Should return CV because it has highest enum value
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::Cv);
    }

    #[test]
    fn test_extract_view_position_all_three_sources() {
        // Test combining all three sources: ViewPosition, ViewCodeSequence, ViewModifierCodeSequence
        let mut dcm = InMemDicomObject::new_empty();

        // Add ViewPosition tag with CC (value 3)
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("CC"),
        ));

        // Add ViewCodeSequence with MLO (value 4)
        let view_code_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("medio-lateral oblique"),
        )]);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Add ViewModifierCodeSequence with AT (value 8, highest)
        let modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("axillary tail"),
        )]);

        let modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier_item]),
        );

        dcm.put(modifier_seq);

        // Should return AT because it has the highest enum value
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::At);
    }

    #[test]
    fn test_extract_view_position_nested_modifier_wins() {
        // Test that nested ViewModifierCodeSequence is included in selection
        let mut dcm = InMemDicomObject::new_empty();

        // Add ViewPosition tag with CC (value 3)
        dcm.put(DataElement::new(
            VIEW_POSITION_TAG,
            VR::CS,
            dicom_core::value::PrimitiveValue::from("CC"),
        ));

        // Add nested ViewModifierCodeSequence with CV (value 9, highest)
        let nested_modifier_item = InMemDicomObject::from_element_iter([DataElement::new(
            CODE_MEANING,
            VR::LO,
            dicom_core::value::PrimitiveValue::from("cleavage view"),
        )]);

        let nested_modifier_seq = DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![nested_modifier_item]),
        );

        let mut view_code_item = InMemDicomObject::new_empty();
        view_code_item.put(nested_modifier_seq);

        let view_code_seq = DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![view_code_item]),
        );

        dcm.put(view_code_seq);

        // Should return CV from nested ViewModifierCodeSequence
        let result = extract_view_position(&dcm).unwrap();
        assert_eq!(result, ViewPosition::Cv);
    }
}
