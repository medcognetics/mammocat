use crate::error::Result;
use crate::types::{DbtObjectKind, ImageType, MammogramType};
use dicom_object::InMemDicomObject;

use super::tags::{
    get_int_value, get_lowercase_string, get_multi_string_value, get_string_value,
    ACQUISITION_DEVICE_PROCESSING_DESCRIPTION, CONCATENATION_UID, IMAGE_TYPE,
    MANUFACTURER_MODEL_NAME, MODALITY, NUMBER_OF_FRAMES, NUMBER_OF_TOMOSYNTHESIS_SOURCE_IMAGES,
    SERIES_DESCRIPTION, SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE, TOMO_CLASS,
    VOLUMETRIC_PROPERTIES, VOLUME_BASED_CALCULATION_TECHNIQUE,
};

/// Extracts mammogram type from DICOM file
///
/// Implements the classification algorithm from Python types.py:159-195,
/// with Mammocat-specific DBT slice and ambiguity handling.
///
/// # Algorithm
///
/// 1. Validate modality is "MG"
/// 2. Check NumberOfFrames > 1 → TOMO
/// 3. Extract ImageType components (pixels, exam, flavor, extras)
/// 4. Apply classification rules IN ORDER:
///    a) is_sfm flag → SFM
///    b) SeriesDescription contains "s-view"/"c-view" → SYNTH
///    c) exact ImageType component "TOMO_2D" → SYNTH
///    d) extras contains "generated_2d" → SYNTH
///    e) exact ImageType component "TOMO" → TOMO
///    f) ambiguous single-frame volumetric tomo evidence → UNKNOWN
///    g) pixels contains "ORIGINAL" → FFDM
///    h) Machine-specific rule (fdr-3000aws) → SYNTH
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
    let machine = get_lowercase_string(dcm, MANUFACTURER_MODEL_NAME);
    let series_desc = get_lowercase_string(dcm, SERIES_DESCRIPTION);

    // If fields 1 and 2 were missing, default to FFDM
    if img_type.pixels.is_empty() || img_type.exam.is_empty() {
        return Ok(MammogramType::Ffdm);
    }

    // 4. Apply classification rules

    // High-confidence explicit rules
    if is_sfm {
        return Ok(MammogramType::Sfm);
    }

    if !series_desc.is_empty() && (series_desc.contains("s-view") || series_desc.contains("c-view"))
    {
        return Ok(MammogramType::Synth);
    }

    if image_type_component_eq(&img_type, "tomo_2d") {
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

    if image_type_component_eq(&img_type, "tomo") {
        return Ok(MammogramType::Tomo);
    }

    if has_ambiguous_single_frame_volumetric_tomo_evidence(dcm, &img_type) {
        return Ok(MammogramType::Unknown);
    }

    if pixels.contains("original") {
        return Ok(MammogramType::Ffdm);
    }

    // Vendor fallback inherited from the Python classifier
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

/// Extracts DBT object representation from a DICOM file and mammogram type.
pub fn extract_dbt_object_kind(
    dcm: &InMemDicomObject,
    mammogram_type: MammogramType,
) -> DbtObjectKind {
    let img_type = extract_image_type(dcm);

    if mammogram_type == MammogramType::Unknown {
        if has_ambiguous_single_frame_volumetric_tomo_evidence(dcm, &img_type) {
            return DbtObjectKind::Unknown;
        }
        return DbtObjectKind::None;
    }

    if mammogram_type != MammogramType::Tomo {
        return DbtObjectKind::None;
    }

    let num_frames = get_int_value(dcm, NUMBER_OF_FRAMES).unwrap_or(1);
    if num_frames > 1 {
        return DbtObjectKind::Volume;
    }

    if image_type_component_eq(&img_type, "tomo") {
        return DbtObjectKind::Slice;
    }

    DbtObjectKind::Unknown
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

fn image_type_component_eq(img_type: &ImageType, expected: &str) -> bool {
    component_eq(&img_type.pixels, expected)
        || component_eq(&img_type.exam, expected)
        || img_type
            .flavor
            .as_ref()
            .is_some_and(|flavor| component_eq(flavor, expected))
        || img_type
            .extras
            .as_ref()
            .is_some_and(|extras| extras.iter().any(|extra| component_eq(extra, expected)))
}

fn component_eq(value: &str, expected: &str) -> bool {
    value.trim().eq_ignore_ascii_case(expected)
}

fn has_ambiguous_single_frame_volumetric_tomo_evidence(
    dcm: &InMemDicomObject,
    img_type: &ImageType,
) -> bool {
    // This signature is strong evidence that the object came from a DBT acquisition,
    // but Fuji can copy it onto both reconstructed slices and singleton SYN2D files.
    // Single-file extraction therefore reports ambiguity; collection refinement can
    // use series cardinality and source relationships to resolve the distinction.
    if get_int_value(dcm, NUMBER_OF_FRAMES).is_some_and(|frames| frames > 1) {
        return false;
    }

    if !image_type_is_exact_derived_primary(img_type) {
        return false;
    }

    if !string_tag_eq(dcm, VOLUMETRIC_PROPERTIES, "volume") {
        return false;
    }

    if !volume_based_calculation_allows_slice(dcm) {
        return false;
    }

    if !has_non_empty_tag(dcm, CONCATENATION_UID)
        && !has_non_empty_tag(dcm, SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE)
    {
        return false;
    }

    has_supporting_tomosynthesis_evidence(dcm)
}

fn image_type_is_exact_derived_primary(img_type: &ImageType) -> bool {
    component_eq(&img_type.pixels, "derived")
        && component_eq(&img_type.exam, "primary")
        && img_type
            .flavor
            .as_ref()
            .is_none_or(|flavor| flavor.trim().is_empty())
        && img_type
            .extras
            .as_ref()
            .is_none_or(|extras| extras.iter().all(|extra| extra.trim().is_empty()))
}

fn volume_based_calculation_allows_slice(dcm: &InMemDicomObject) -> bool {
    match get_string_value(dcm, VOLUME_BASED_CALCULATION_TECHNIQUE) {
        Some(value) => component_eq(&value, "none"),
        None => true,
    }
}

fn has_supporting_tomosynthesis_evidence(dcm: &InMemDicomObject) -> bool {
    // These attributes support the storage evidence above; they are not sufficient
    // by themselves because vendors may copy acquisition metadata onto 2D objects.
    // TomoAngle is intentionally excluded here because it is less specific.
    string_tag_eq(dcm, TOMO_CLASS, "tomosynthesis")
        || get_int_value(dcm, NUMBER_OF_TOMOSYNTHESIS_SOURCE_IMAGES)
            .is_some_and(|source_images| source_images > 1)
        || get_string_value(dcm, ACQUISITION_DEVICE_PROCESSING_DESCRIPTION)
            .is_some_and(|description| contains_exact_token(&description, "tomo"))
}

fn string_tag_eq(dcm: &InMemDicomObject, tag: dicom_core::Tag, expected: &str) -> bool {
    get_string_value(dcm, tag).is_some_and(|value| component_eq(&value, expected))
}

fn has_non_empty_tag(dcm: &InMemDicomObject, tag: dicom_core::Tag) -> bool {
    get_string_value(dcm, tag).is_some_and(|value| !value.is_empty())
}

fn contains_exact_token(value: &str, expected: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| token.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::Tag;
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_object::InMemDicomObject;

    const FUJI_SPLIT_SLICE_IMAGE_TYPE: &str = "DERIVED|PRIMARY";
    const FUJI_SYNTH_IMAGE_TYPE: &str = "DERIVED|PRIMARY|TOMOSYNTHESIS|GENERATED_2D|||||100000";

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

    fn put_str(dcm: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
        dcm.put(DataElement::new(
            tag,
            vr,
            PrimitiveValue::from(value.to_string()),
        ));
    }

    fn put_frames(dcm: &mut InMemDicomObject, frames: &str) {
        put_str(dcm, NUMBER_OF_FRAMES, VR::IS, frames);
    }

    fn add_split_slice_core_evidence(dcm: &mut InMemDicomObject) {
        put_str(dcm, VOLUMETRIC_PROPERTIES, VR::CS, "VOLUME");
        put_str(dcm, VOLUME_BASED_CALCULATION_TECHNIQUE, VR::CS, "NONE");
        put_str(
            dcm,
            SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
            VR::UI,
            "1.2.392.200036.9125.4.0.1141837742.1426916712.298501606",
        );
        put_str(
            dcm,
            CONCATENATION_UID,
            VR::UI,
            "1.2.392.200036.9125.4.0.1141837742.1426916712.298501606",
        );
    }

    fn add_tomosynthesis_supporting_evidence(dcm: &mut InMemDicomObject) {
        put_str(dcm, TOMO_CLASS, VR::CS, "TOMOSYNTHESIS");
        put_str(dcm, NUMBER_OF_TOMOSYNTHESIS_SOURCE_IMAGES, VR::IS, "15");
        put_str(
            dcm,
            ACQUISITION_DEVICE_PROCESSING_DESCRIPTION,
            VR::LO,
            "TOMO R MAMMOGRAPHY,CC",
        );
    }

    fn add_fuji_split_slice_evidence(dcm: &mut InMemDicomObject) {
        add_split_slice_core_evidence(dcm);
        add_tomosynthesis_supporting_evidence(dcm);
    }

    #[test]
    fn test_tomo_2d_classification() {
        // Test that TOMO_2D in flavor field is classified as SYNTH
        let dcm = create_test_dicom("DERIVED|PRIMARY|TOMO_2D|LEFT", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Synth);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::None);
    }

    #[test]
    fn test_tomo_2d_case_insensitive() {
        // Test that the check is case insensitive
        let dcm = create_test_dicom("DERIVED|PRIMARY|tomo_2d", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Synth);
    }

    #[test]
    fn test_single_frame_tomo_slice_classification() {
        let dcm = create_test_dicom("DERIVED|PRIMARY|TOMO|RIGHT", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Tomo);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::Slice);
    }

    #[test]
    fn test_single_frame_tomo_slice_classification_case_insensitive() {
        let dcm = create_test_dicom("DERIVED|PRIMARY|tomo|RIGHT", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Tomo);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::Slice);
    }

    #[test]
    fn test_fuji_like_signature_classifies_as_unknown_without_collection_context() {
        let mut dcm = create_test_dicom(FUJI_SPLIT_SLICE_IMAGE_TYPE, "MG");
        add_fuji_split_slice_evidence(&mut dcm);

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Unknown);
        assert_eq!(
            extract_dbt_object_kind(&dcm, result),
            DbtObjectKind::Unknown
        );
    }

    #[test]
    fn test_fuji_like_one_frame_signature_classifies_as_unknown_without_collection_context() {
        let mut dcm = create_test_dicom(FUJI_SPLIT_SLICE_IMAGE_TYPE, "MG");
        put_frames(&mut dcm, "1");
        add_fuji_split_slice_evidence(&mut dcm);

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Unknown);
        assert_eq!(
            extract_dbt_object_kind(&dcm, result),
            DbtObjectKind::Unknown
        );
    }

    #[test]
    fn test_tomo_acquisition_tags_without_slice_storage_evidence_do_not_classify_as_tomo() {
        let mut dcm = create_test_dicom(FUJI_SPLIT_SLICE_IMAGE_TYPE, "MG");
        add_tomosynthesis_supporting_evidence(&mut dcm);

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Ffdm);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::None);
    }

    #[test]
    fn test_sampled_tomosynthesis_generated_2d_remains_synth() {
        let mut dcm = create_test_dicom(FUJI_SYNTH_IMAGE_TYPE, "MG");
        put_str(&mut dcm, VOLUMETRIC_PROPERTIES, VR::CS, "SAMPLED");
        put_str(
            &mut dcm,
            VOLUME_BASED_CALCULATION_TECHNIQUE,
            VR::CS,
            "TOMOSYNTHESIS",
        );
        add_tomosynthesis_supporting_evidence(&mut dcm);

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Synth);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::None);
    }

    #[test]
    fn test_max_ip_single_frame_object_is_not_tomo_slice() {
        let mut dcm = create_test_dicom(FUJI_SPLIT_SLICE_IMAGE_TYPE, "MG");
        add_split_slice_core_evidence(&mut dcm);
        add_tomosynthesis_supporting_evidence(&mut dcm);
        put_str(
            &mut dcm,
            VOLUME_BASED_CALCULATION_TECHNIQUE,
            VR::CS,
            "MAX_IP",
        );

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Ffdm);
        assert_eq!(
            extract_dbt_object_kind(&dcm, MammogramType::Tomo),
            DbtObjectKind::Unknown
        );
    }

    #[test]
    fn test_tomo_2d_precedes_split_slice_evidence() {
        let mut dcm = create_test_dicom("DERIVED|PRIMARY|TOMO_2D|RIGHT", "MG");
        add_fuji_split_slice_evidence(&mut dcm);

        let result = extract_mammogram_type(&dcm, false).unwrap();

        assert_eq!(result, MammogramType::Synth);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::None);
    }

    #[test]
    fn test_tomo_proj_is_not_single_frame_tomo_slice() {
        let dcm = create_test_dicom("DERIVED|PRIMARY|TOMO_PROJ|RIGHT", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Ffdm);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::None);
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
        put_frames(&mut dcm, "10");
        put_str(&mut dcm, VOLUMETRIC_PROPERTIES, VR::CS, "VOLUME");
        put_str(
            &mut dcm,
            VOLUME_BASED_CALCULATION_TECHNIQUE,
            VR::CS,
            "TOMOSYNTHESIS",
        );
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Tomo);
        assert_eq!(extract_dbt_object_kind(&dcm, result), DbtObjectKind::Volume);
    }

    #[test]
    fn test_tomo_unknown_dbt_object_kind_without_volume_or_slice_evidence() {
        let dcm = create_test_dicom("DERIVED|PRIMARY", "MG");
        assert_eq!(
            extract_dbt_object_kind(&dcm, MammogramType::Tomo),
            DbtObjectKind::Unknown
        );
    }

    #[test]
    fn test_default_to_ffdm() {
        // Test that DERIVED|PRIMARY without special markers defaults to FFDM
        let dcm = create_test_dicom("DERIVED|PRIMARY", "MG");
        let result = extract_mammogram_type(&dcm, false).unwrap();
        assert_eq!(result, MammogramType::Ffdm);
    }
}
