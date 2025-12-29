use crate::error::Result;
use crate::types::ViewPosition;
use dicom_object::InMemDicomObject;

use super::tags::{get_string_value, VIEW_POSITION as VIEW_POSITION_TAG};

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
/// 2. Extract from ViewCodeSequence → CodeMeaning (TODO)
/// 3. Extract from ViewModifierCodeSequence → CodeMeaning (TODO)
/// 4. Return highest priority match
pub fn extract_view_position(dcm: &InMemDicomObject) -> Result<ViewPosition> {
    // Try ViewPosition tag
    if let Some(vp) = get_string_value(dcm, VIEW_POSITION_TAG) {
        let result = from_str(&vp, false);
        if !result.is_unknown() {
            return Ok(result);
        }
    }

    // TODO: Try ViewCodeSequence and ViewModifierCodeSequence
    // This requires sequence navigation with dicom-rs
    // For MVP, we'll implement this in a future iteration

    Ok(ViewPosition::Unknown)
}

/// Parses view position from string
///
/// Supports both strict and loose matching modes:
/// - Strict: exact match with predefined patterns
/// - Loose: substring matching
#[allow(clippy::should_implement_trait)]
pub fn from_str(s: &str, strict: bool) -> ViewPosition {
    let s_lower = s.trim().to_lowercase();

    // Strict patterns - exact matches
    if CC_STRINGS.iter().any(|&p| s_lower == p) || s_lower == "cc" {
        return ViewPosition::Cc;
    }

    // Check more specific patterns first (LMO before MLO, LM before ML)

    // LMO patterns - check before MLO since both contain "lateral" and "oblique"
    if s_lower == "lmo"
        || s_lower == "latero-medial oblique"
        || s_lower == "lateral-medial oblique"
        || (s_lower.contains("oblique") && s_lower.contains("latero"))
    {
        return ViewPosition::Lmo;
    }

    // MLO patterns
    if s_lower == "mlo"
        || s_lower == "medio-lateral oblique"
        || s_lower == "medial-lateral oblique"
        || (s_lower.contains("oblique") && s_lower.contains("medio"))
        || (s_lower.contains("oblique")
            && s_lower.contains("medial")
            && !s_lower.contains("latero"))
    {
        return ViewPosition::Mlo;
    }

    // LM patterns - check before ML
    if LM_STRINGS.iter().any(|&p| s_lower == p) || s_lower == "lm" {
        return ViewPosition::Lm;
    }

    // ML patterns
    if ML_STRINGS.iter().any(|&p| s_lower == p) || s_lower == "ml" {
        return ViewPosition::Ml;
    }

    // XCCL - CC exaggerated laterally
    if s_lower.contains("exaggerated laterally") || s_lower == "xccl" {
        return ViewPosition::Xccl;
    }

    // XCCM - CC exaggerated medially
    if s_lower.contains("exaggerated medially") || s_lower == "xccm" {
        return ViewPosition::Xccm;
    }

    // AT - Axillary tail
    if AT_STRINGS.iter().any(|&p| s_lower.contains(p)) || s_lower == "at" {
        return ViewPosition::At;
    }

    // CV - Cleavage view
    if CV_STRINGS.iter().any(|&p| s_lower.contains(p)) || s_lower == "cv" {
        return ViewPosition::Cv;
    }

    // If strict mode, return unknown if no match
    if strict {
        return ViewPosition::Unknown;
    }

    // Loose matching - check if any enum name is contained as substring
    if s_lower.contains("xccl") {
        ViewPosition::Xccl
    } else if s_lower.contains("xccm") {
        ViewPosition::Xccm
    } else if s_lower.contains("cc") {
        ViewPosition::Cc
    } else if s_lower.contains("mlo") {
        ViewPosition::Mlo
    } else if s_lower.contains("ml") {
        ViewPosition::Ml
    } else if s_lower.contains("lmo") {
        ViewPosition::Lmo
    } else if s_lower.contains("lm") {
        ViewPosition::Lm
    } else if s_lower.contains("at") {
        ViewPosition::At
    } else if s_lower.contains("cv") {
        ViewPosition::Cv
    } else {
        ViewPosition::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_from_str_unknown() {
        assert_eq!(from_str("", false), ViewPosition::Unknown);
        assert_eq!(from_str("invalid", false), ViewPosition::Unknown);
        assert_eq!(from_str("xyz", false), ViewPosition::Unknown);
    }
}
