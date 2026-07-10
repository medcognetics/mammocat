use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use mammocat_core::MammogramExtractor;
use std::path::Path;
use std::process::Command;

#[test]
fn nested_view_modifiers_match_top_level_in_rust_and_cli() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let top_level_path = temp.path().join("top-level.dcm");
    let nested_path = temp.path().join("nested.dcm");
    write_mammogram(&top_level_path, false)?;
    write_mammogram(&nested_path, true)?;

    let top_level = MammogramExtractor::extract_file(&dicom_object::open_file(&top_level_path)?)?;
    let nested = MammogramExtractor::extract_file(&dicom_object::open_file(&nested_path)?)?;
    let top_level_flags = (
        top_level.is_implant_displaced,
        top_level.is_spot_compression,
        top_level.is_magnified,
    );
    let nested_flags = (
        nested.is_implant_displaced,
        nested.is_spot_compression,
        nested.is_magnified,
    );
    assert_eq!(top_level_flags, (true, true, true));
    assert_eq!(nested_flags, top_level_flags);

    let output = Command::new(env!("CARGO_BIN_EXE_mammocat"))
        .arg(&nested_path)
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    for label in ["Implant Displaced", "Spot Compression", "Magnification"] {
        assert!(stdout
            .lines()
            .any(|line| line.contains(label) && line.trim_end().ends_with("true")));
    }

    Ok(())
}

fn write_mammogram(path: &Path, nested_modifiers: bool) -> Result<(), Box<dyn std::error::Error>> {
    let sop_instance_uid = if nested_modifiers {
        "1.2.826.0.1.3680043.10.543.36.2"
    } else {
        "1.2.826.0.1.3680043.10.543.36.1"
    };
    let mut object = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
        ),
        DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, sop_instance_uid),
        DataElement::new(
            tags::STUDY_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.543.36",
        ),
        DataElement::new(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.543.36.1",
        ),
        DataElement::new(tags::MODALITY, VR::CS, "MG"),
        DataElement::new(tags::IMAGE_TYPE, VR::CS, "ORIGINAL\\PRIMARY"),
        DataElement::new(tags::PRESENTATION_INTENT_TYPE, VR::CS, "FOR PRESENTATION"),
        DataElement::new(tags::IMAGE_LATERALITY, VR::CS, "L"),
        DataElement::new(tags::VIEW_POSITION, VR::CS, "MLO"),
        DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(32_u16)),
        DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(32_u16)),
        DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(1_u16)),
        DataElement::new(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "MONOCHROME2"),
        DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(15_u16)),
        DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0_u16),
        ),
    ]);

    if nested_modifiers {
        let view_item = InMemDicomObject::from_element_iter([
            DataElement::new(tags::CODE_MEANING, VR::LO, "MLO"),
            modifier_sequence(tags::VIEW_MODIFIER_CODE_SEQUENCE),
        ]);
        object.put(sequence(tags::VIEW_CODE_SEQUENCE, vec![view_item]));
    } else {
        object.put(modifier_sequence(tags::VIEW_MODIFIER_CODE_SEQUENCE));
    }

    object
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(
                    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                )
                .media_storage_sop_instance_uid(sop_instance_uid),
        )?
        .write_to_file(path)?;
    Ok(())
}

fn modifier_sequence(tag: Tag) -> DataElement<InMemDicomObject> {
    sequence(
        tag,
        ["Implant Displaced", "Spot Compression", "Magnification"]
            .into_iter()
            .map(|meaning| {
                InMemDicomObject::from_element_iter([DataElement::new(
                    tags::CODE_MEANING,
                    VR::LO,
                    meaning,
                )])
            })
            .collect(),
    )
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}
