use std::process::Command;

use dicom_core::value::PrimitiveValue;
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use tempfile::tempdir;

fn write_test_dicom(path: &std::path::Path, sop_class_uid: &str) {
    let sop_instance_uid = "1.2.826.0.1.3680043.10.543.90";
    let object = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            PrimitiveValue::from(sop_class_uid.to_string()),
        ),
        DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            PrimitiveValue::from(sop_instance_uid),
        ),
        DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::from("ORIGINAL\\PRIMARY"),
        ),
        DataElement::new(tags::LATERALITY, VR::CS, PrimitiveValue::from("L")),
        DataElement::new(tags::VIEW_POSITION, VR::CS, PrimitiveValue::from("CC")),
    ]);
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(sop_class_uid)
                .media_storage_sop_instance_uid(sop_instance_uid)
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
        )
        .unwrap()
        .write_to_file(path)
        .unwrap();
}

#[test]
fn dry_run_json_is_machine_readable_and_keeps_stderr_clean() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.dcm");
    write_test_dicom(
        &input,
        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_mammofill"))
        .args(["--dry-run", "--format", "json", "--progress", "never"])
        .arg(&input)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["summary"]["discovered"], 1);
    assert_eq!(report["files"][0]["status"], "planned");
}

#[test]
fn unsupported_sop_is_a_completed_issue() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("ct.dcm");
    write_test_dicom(&input, uids::CT_IMAGE_STORAGE);

    let output = Command::new(env!("CARGO_BIN_EXE_mammofill"))
        .args(["--dry-run", "--format", "json", "--progress", "never"])
        .arg(&input)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["files"][0]["issues"][0]["code"],
        "unsupported_sop_class"
    );
}

#[test]
fn missing_input_is_a_runtime_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_mammofill"))
        .args(["--dry-run", "does-not-exist.dcm"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("input does not exist"));
}
