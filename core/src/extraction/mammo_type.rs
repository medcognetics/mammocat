use crate::error::Result;
use crate::types::{ImageType, MammogramType};
use dicom_object::InMemDicomObject;

use super::tags::{
    get_int_value, get_multi_string_value, get_string_value, IMAGE_TYPE, MANUFACTURER_MODEL_NAME,
    MODALITY, NUMBER_OF_FRAMES, SERIES_DESCRIPTION,
};

/// Extracts mammogram type from DICOM file
///
/// Implements the classification algorithm from Python types.py:159-195
///
/// # Algorithm
///
/// 1. Validate modality is "MG"
/// 2. Check NumberOfFrames > 1 → TOMO
/// 3. Extract ImageType components (pixels, exam, flavor, extras)
/// 4. Apply classification rules IN ORDER:
///    a) is_sfm flag → SFM
///    b) SeriesDescription contains "s-view"/"c-view" → SYNTH
///    c) pixels contains "ORIGINAL" → FFDM
///    d) extras contains "generated_2d" → SYNTH
///    e) Machine-specific rule (fdr-3000aws) → SYNTH
/// 5. Default → FFDM
pub fn extract_mammogram_type(dcm: &InMemDicomObject, is_sfm: bool) -> Result<MammogramType> {
    extract_mammogram_type_impl(dcm, is_sfm, false)
}

/// Internal implementation with ignore_modality option
pub fn extract_mammogram_type_impl(
    dcm: &InMemDicomObject,
    is_sfm: bool,
    ignore_modality: bool,
) -> Result<MammogramType> {
    // 1. Check modality
    if !ignore_modality {
        let modality = get_string_value(dcm, MODALITY);
        if let Some(m) = modality.as_ref() {
            if m != "MG" {
                return Err(format!("Expected modality=MG, found {}", m).into());
            }
        }
    }

    // 2. If 3D volume (multi-frame), must be tomo
    let num_frames = get_int_value(dcm, NUMBER_OF_FRAMES).unwrap_or(1);
    if num_frames > 1 {
        return Ok(MammogramType::Tomo);
    }

    // 3. Extract ImageType components
    let img_type = extract_image_type(dcm);
    let pixels = img_type.pixels.to_lowercase();
    let exam = img_type.exam.to_lowercase();
    let flavor = img_type
        .flavor
        .as_ref()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // Get additional metadata
    let machine = get_string_value(dcm, MANUFACTURER_MODEL_NAME)
        .unwrap_or_default()
        .to_lowercase();
    let series_desc = get_string_value(dcm, SERIES_DESCRIPTION)
        .unwrap_or_default()
        .to_lowercase();

    // If fields 1 and 2 were missing, default to FFDM
    if img_type.pixels.is_empty() || img_type.exam.is_empty() {
        return Ok(MammogramType::Ffdm);
    }

    // 4. Apply classification rules

    // Very solid rules
    if is_sfm {
        return Ok(MammogramType::Sfm);
    }

    if !series_desc.is_empty() && (series_desc.contains("s-view") || series_desc.contains("c-view"))
    {
        return Ok(MammogramType::Synth);
    }

    if pixels.contains("original") {
        return Ok(MammogramType::Ffdm);
    }

    // Ok rules
    if flavor.contains("tomo_2d") {
        return Ok(MammogramType::Synth);
    }

    if let Some(ref extras) = img_type.extras {
        if extras
            .iter()
            .any(|x| x.to_lowercase().contains("generated_2d"))
        {
            return Ok(MammogramType::Synth);
        }
    }

    // Not good rules
    if pixels == "derived"
        && exam == "primary"
        && machine == "fdr-3000aws"
        && flavor != "post_contrast"
    {
        return Ok(MammogramType::Synth);
    }

    // Default
    Ok(MammogramType::Ffdm)
}

/// Extracts ImageType structure from DICOM file
pub fn extract_image_type(dcm: &InMemDicomObject) -> ImageType {
    let image_type_values = get_multi_string_value(dcm, IMAGE_TYPE);

    match image_type_values {
        None => ImageType::new(String::new(), String::new(), None, None),
        Some(values) => {
            let pixels = values.first().cloned().unwrap_or_default();
            let exam = values.get(1).cloned().unwrap_or_default();
            let flavor = values.get(2).cloned();
            let extras = if values.len() > 3 {
                Some(values[3..].to_vec())
            } else {
                None
            };

            ImageType::new(pixels, exam, flavor, extras)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_object::InMemDicomObject;

    /// Helper to create a minimal DICOM object for testing
    fn create_test_dicom(image_type: &str, modality: &str) -> InMemDicomObject {
        let mut obj = InMemDicomObject::new_empty();

        // Add MODALITY tag
        obj.put(DataElement::new(
            MODALITY,
            VR::CS,
            PrimitiveValue::from(modality),
        ));

        // Add IMAGE_TYPE tag (multi-valued string)
        let image_type_parts: Vec<&str> = image_type.split('|').collect();
        obj.put(DataElement::new(
            IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(image_type_parts.iter().map(|s| s.to_string()).collect()),
        ));

        obj
    }

    #[test]
    fn test_tomo_2d_classification() {
        // Test that TOMO_2D in flavor field is classified as SYNTH
        let dcm = create_test_dicom("DERIVED|PRIMARY|TOMO_2D|LEFT", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Synth);
    }

    #[test]
    fn test_tomo_2d_case_insensitive() {
        // Test that the check is case insensitive
        let dcm = create_test_dicom("DERIVED|PRIMARY|tomo_2d", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Synth);
    }

    #[test]
    fn test_original_pixels_classified_as_ffdm() {
        // Test that ORIGINAL in pixels field is classified as FFDM
        let dcm = create_test_dicom("ORIGINAL|PRIMARY", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Ffdm);
    }

    #[test]
    fn test_sfm_flag_takes_precedence() {
        // Test that is_sfm flag takes precedence over other rules
        let dcm = create_test_dicom("DERIVED|PRIMARY|TOMO_2D", "MG");
        let result = extract_mammogram_type(&dcm, true).unwrap();
        assert_eq!(result, MammogramType::Sfm);
    }

    #[test]
    fn test_multiframe_classified_as_tomo() {
        // Test that NumberOfFrames > 1 is classified as TOMO
        let mut dcm = create_test_dicom("ORIGINAL|PRIMARY", "MG");
        dcm.put(DataElement::new(
            NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from("10"),
        ));
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Tomo);
    }

    #[test]
    fn test_default_to_ffdm() {
        // Test that DERIVED|PRIMARY without special markers defaults to FFDM
        let dcm = create_test_dicom("DERIVED|PRIMARY", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Ffdm);
    }
}
