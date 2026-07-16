use dicom_core::dicom_value;
use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use mammocat_core::{
    convert_dbt_study, scan_dbt_study, DbtConvertOptions, DbtScanOptions,
    BREAST_TOMOSYNTHESIS_SOP_CLASS_UID,
};
use std::path::Path;

#[test]
fn rust_api_scans_and_converts_old_format_dbt_series() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    let dbt_dir = input.join("nested/a");
    let ffdm_dir = input.join("nested/b");
    std::fs::create_dir_all(&dbt_dir)?;
    std::fs::create_dir_all(&ffdm_dir)?;

    let study_uid = "1.2.826.0.1.3680043.10.54321.1";
    let dbt_series_uid = "1.2.826.0.1.3680043.10.54321.2";
    let ffdm_series_uid = "1.2.826.0.1.3680043.10.54321.3";

    for instance_number in 1..=3 {
        write_dbt_slice(
            &dbt_dir.join(format!("slice_{instance_number}.dcm")),
            study_uid,
            dbt_series_uid,
            instance_number,
        )?;
    }
    write_ffdm(&ffdm_dir.join("ffdm.dcm"), study_uid, ffdm_series_uid)?;

    let scan = scan_dbt_study(&input, DbtScanOptions)?;
    assert_eq!(scan.summary.conversion_needed_series, 1);
    assert_eq!(scan.summary.copy_through_files, 1);
    assert_eq!(scan.conversion_needed_series[0].frame_count, 3);
    assert_eq!(scan.conversion_needed_series[0].view_position, "MLO");

    let report = convert_dbt_study(&input, &output, DbtConvertOptions::default())?;
    assert_eq!(report.summary.converted_series, 1);
    assert_eq!(report.summary.copied_files, 1);

    let converted = dicom_object::open_file(&report.converted_series[0].output_path)?;
    assert_eq!(
        converted.element(tags::SOP_CLASS_UID)?.to_str()?,
        BREAST_TOMOSYNTHESIS_SOP_CLASS_UID
    );
    assert_eq!(converted.element(tags::MODALITY)?.to_str()?, "MG");
    assert_eq!(
        converted.element(tags::NUMBER_OF_FRAMES)?.to_int::<i32>()?,
        3
    );
    assert!(converted
        .element(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .is_ok());
    assert_eq!(
        converted
            .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)?
            .items()
            .map_or(0, <[_]>::len),
        3
    );
    assert!(output.join("nested/b/ffdm.dcm").exists());

    Ok(())
}

fn write_dbt_slice(
    path: &Path,
    study_uid: &str,
    series_uid: &str,
    instance_number: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let sop_instance_uid = format!("{series_uid}.{instance_number}");
    let mut obj = base_object(
        study_uid,
        series_uid,
        &sop_instance_uid,
        uids::CT_IMAGE_STORAGE,
    );
    obj.put(DataElement::new(tags::MODALITY, VR::CS, "CT"));
    obj.put(DataElement::new(
        tags::SERIES_DESCRIPTION,
        VR::LO,
        "TOMO R-MLO 2D+",
    ));
    obj.put(DataElement::new(
        tags::IMAGE_TYPE,
        VR::CS,
        dicom_value!(Strs, ["DERIVED", "PRIMARY", "TOMO"]),
    ));
    obj.put(DataElement::new(tags::IMAGE_LATERALITY, VR::CS, "R"));
    obj.put(DataElement::new(
        tags::INSTANCE_NUMBER,
        VR::IS,
        instance_number.to_string(),
    ));
    obj.put(DataElement::new(
        tags::IMAGE_POSITION_PATIENT,
        VR::DS,
        dicom_value!(Strs, ["0", "0", instance_number.to_string()]),
    ));
    obj.put(DataElement::new(
        tags::IMAGE_ORIENTATION_PATIENT,
        VR::DS,
        dicom_value!(Strs, ["1", "0", "0", "0", "1", "0"]),
    ));
    obj.put(DataElement::new(
        tags::PIXEL_SPACING,
        VR::DS,
        dicom_value!(Strs, ["0.1", "0.1"]),
    ));
    obj.put(DataElement::new(tags::SLICE_THICKNESS, VR::DS, "1"));
    obj.put(DataElement::new(
        tags::FRAME_OF_REFERENCE_UID,
        VR::UI,
        format!("{study_uid}.4"),
    ));
    obj.put(DataElement::new(tags::WINDOW_CENTER, VR::DS, "2048"));
    obj.put(DataElement::new(tags::WINDOW_WIDTH, VR::DS, "4096"));
    obj.put(DataElement::new(tags::RESCALE_INTERCEPT, VR::DS, "0"));
    obj.put(DataElement::new(tags::RESCALE_SLOPE, VR::DS, "1"));
    obj.put(DataElement::new(tags::RESCALE_TYPE, VR::LO, "US"));
    obj.put(DataElement::new(tags::BREAST_IMPLANT_PRESENT, VR::CS, "NO"));
    obj.put(DataElement::new(tags::BURNED_IN_ANNOTATION, VR::CS, "NO"));
    obj.put(code_sequence(
        tags::ANATOMIC_REGION_SEQUENCE,
        "76752008",
        "SCT",
        "Breast",
    ));
    obj.put(code_sequence(
        tags::VIEW_CODE_SEQUENCE,
        "399368009",
        "SCT",
        "Mediolateral oblique",
    ));
    write_object(path, obj, uids::CT_IMAGE_STORAGE, &sop_instance_uid)
}

fn code_sequence(
    tag: dicom_core::Tag,
    code_value: &str,
    scheme: &str,
    meaning: &str,
) -> DataElement<InMemDicomObject> {
    let item = InMemDicomObject::from_element_iter([
        DataElement::new(tags::CODE_VALUE, VR::SH, code_value),
        DataElement::new(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        DataElement::new(tags::CODE_MEANING, VR::LO, meaning),
    ]);
    DataElement::new(tag, VR::SQ, DataSetSequence::from(vec![item]))
}

fn write_ffdm(
    path: &Path,
    study_uid: &str,
    series_uid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sop_instance_uid = format!("{series_uid}.1");
    let mut obj = base_object(
        study_uid,
        series_uid,
        &sop_instance_uid,
        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    );
    obj.put(DataElement::new(tags::MODALITY, VR::CS, "MG"));
    obj.put(DataElement::new(tags::IMAGE_LATERALITY, VR::CS, "L"));
    obj.put(DataElement::new(tags::VIEW_POSITION, VR::CS, "CC"));
    write_object(
        path,
        obj,
        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
        &sop_instance_uid,
    )
}

fn base_object(
    study_uid: &str,
    series_uid: &str,
    sop_instance_uid: &str,
    sop_class_uid: &str,
) -> InMemDicomObject {
    let mut obj = InMemDicomObject::new_empty();
    obj.put(DataElement::new(
        tags::STUDY_INSTANCE_UID,
        VR::UI,
        study_uid,
    ));
    obj.put(DataElement::new(
        tags::SERIES_INSTANCE_UID,
        VR::UI,
        series_uid,
    ));
    obj.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    ));
    obj.put(DataElement::new(tags::SOP_CLASS_UID, VR::UI, sop_class_uid));
    obj.put(DataElement::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(4_u16),
    ));
    obj.put(DataElement::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(3_u16),
    ));
    obj.put(DataElement::new(
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        PrimitiveValue::from(1_u16),
    ));
    obj.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "MONOCHROME2",
    ));
    obj.put(DataElement::new(
        tags::BITS_ALLOCATED,
        VR::US,
        PrimitiveValue::from(16_u16),
    ));
    obj.put(DataElement::new(
        tags::BITS_STORED,
        VR::US,
        PrimitiveValue::from(16_u16),
    ));
    obj.put(DataElement::new(
        tags::HIGH_BIT,
        VR::US,
        PrimitiveValue::from(15_u16),
    ));
    obj.put(DataElement::new(
        tags::PIXEL_REPRESENTATION,
        VR::US,
        PrimitiveValue::from(0_u16),
    ));
    obj.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OW,
        PrimitiveValue::from(vec![0_u8; 4 * 3 * 2]),
    ));
    obj
}

fn write_object(
    path: &Path,
    obj: InMemDicomObject,
    sop_class_uid: &str,
    sop_instance_uid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_obj = obj.with_meta(
        FileMetaTableBuilder::new()
            .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
            .media_storage_sop_class_uid(sop_class_uid)
            .media_storage_sop_instance_uid(sop_instance_uid),
    )?;
    file_obj.write_to_file(path)?;
    Ok(())
}
