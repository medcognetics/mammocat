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
/// 2. Extract from ViewCodeSequence → CodeMeaning (not yet implemented)
/// 3. Extract from ViewModifierCodeSequence → CodeMeaning (not yet implemented)
/// 4. Return highest priority match
///
/// Note: Sequence navigation for ViewCodeSequence and ViewModifierCodeSequence
/// requires dicom-rs sequence traversal. Most DICOM files have the ViewPosition
/// tag, so this fallback is rarely needed.
pub fn extract_view_position(dcm: &InMemDicomObject) -> Result<ViewPosition> {
    if let Some(vp) = get_string_value(dcm, VIEW_POSITION_TAG) {
        let result = from_str(&vp, false);
        if !result.is_unknown() {
            return Ok(result);
        }
    }
    Ok(ViewPosition::Unknown)
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
    } else if s.contains("cc") {
        Some(ViewPosition::Cc)
    } else if s.contains("ml") {
        Some(ViewPosition::Ml)
    } else if s.contains("lm") {
        Some(ViewPosition::Lm)
    } else if s.contains("at") {
        Some(ViewPosition::At)
    } else if s.contains("cv") {
        Some(ViewPosition::Cv)
    } else {
        None
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
