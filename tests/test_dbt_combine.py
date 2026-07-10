"""Regression tests for old-format DBT study scanning and conversion."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pydicom
import pytest
from pydicom.filewriter import dcmwrite
from pydicom.uid import UID, ExplicitVRBigEndian, ImplicitVRLittleEndian

from mammocat import (
    BREAST_TOMOSYNTHESIS_SOP_CLASS_UID,
    DbtObjectKind,
    Laterality,
    MammogramExtractor,
    MammogramType,
    ViewPosition,
    convert_dbt_study,
    scan_dbt_study,
)

from .conftest import (
    BREAST_TOMOSYNTHESIS_SOP_CLASS_UID as EXPECTED_DBT_SOP_CLASS_UID,
)
from .conftest import (
    create_mammogram_dicom,
    create_old_format_dbt_series,
    create_old_format_dbt_slice,
)


def _write_ffdm(path: Path) -> Path:
    ds = create_mammogram_dicom(mammogram_type="FFDM", laterality="R", view_position="CC")
    ds.SeriesInstanceUID = "1.2.826.0.1.3680043.10.543.2.1"
    ds.SOPInstanceUID = "1.2.826.0.1.3680043.10.543.2.1.1"
    ds.file_meta.MediaStorageSOPInstanceUID = UID(ds.SOPInstanceUID)
    ds.PixelData = b"\x00\x00" * (ds.Rows * ds.Columns)
    ds.save_as(path, enforce_file_format=True)
    return path


def _write_multifile_ffdm_series(directory: Path, series_uid: str) -> None:
    for index in range(2):
        ds = create_mammogram_dicom(mammogram_type="FFDM", laterality="R", view_position="CC")
        ds.SeriesInstanceUID = series_uid
        ds.SOPInstanceUID = f"{series_uid}.{index + 1}"
        ds.file_meta.MediaStorageSOPInstanceUID = UID(ds.SOPInstanceUID)
        ds.InstanceNumber = str(index + 1)
        ds.save_as(directory / f"ffdm_{index}.dcm", enforce_file_format=True)


def _write_multiframe_dbt(path: Path, series_uid: str, instance_number: int) -> None:
    ds = create_mammogram_dicom(
        mammogram_type="TOMO",
        laterality="L",
        view_position="MLO",
        rows=4,
        columns=3,
    )
    ds.StudyInstanceUID = "1.2.826.0.1.3680043.10.543.12"
    ds.SeriesInstanceUID = series_uid
    ds.SOPInstanceUID = f"{series_uid}.{instance_number}"
    ds.SOPClassUID = EXPECTED_DBT_SOP_CLASS_UID
    ds.file_meta.MediaStorageSOPClassUID = UID(EXPECTED_DBT_SOP_CLASS_UID)
    ds.file_meta.MediaStorageSOPInstanceUID = UID(ds.SOPInstanceUID)
    ds.InstanceNumber = str(instance_number)
    ds.NumberOfFrames = "2"
    ds.PixelData = b"\x00\x00" * (int(ds.NumberOfFrames) * ds.Rows * ds.Columns)
    ds.save_as(path, enforce_file_format=True)


def _run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    command = [
        "cargo",
        "run",
        "--quiet",
        "--features",
        "python",
        "--bin",
        "dbt-combine",
        "--",
        *args,
    ]
    return subprocess.run(command, check=False, capture_output=True, text=True)


def test_scan_detects_old_format_dbt_series(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)
    _write_ffdm(input_dir / "ffdm.dcm")

    report = scan_dbt_study(input_dir)

    assert report["summary"]["dicom_files"] == 4
    assert report["summary"]["conversion_needed_series"] == 1
    assert report["summary"]["copy_through_files"] == 1
    assert report["summary"]["unsupported_series"] == 0
    series = report["conversion_needed_series"][0]
    assert series["frame_count"] == 3
    assert series["laterality"] == "L"
    assert series["view_position"] == "MLO"
    assert series["source_modality"] == "CT"


def test_scan_reports_no_conversion_for_single_ffdm(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    input_dir.mkdir()
    _write_ffdm(input_dir / "ffdm.dcm")

    report = scan_dbt_study(input_dir)

    assert report["summary"]["dicom_files"] == 1
    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["copy_through_files"] == 1


def test_convert_combines_dbt_and_copies_ffdm(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    source_paths = create_old_format_dbt_series(input_dir)
    _write_ffdm(input_dir / "ffdm.dcm")

    report = convert_dbt_study(input_dir, output_dir)

    assert report["summary"]["converted_series"] == 1
    assert report["summary"]["copied_files"] == 1
    output_path = Path(report["converted_series"][0]["output_path"])
    assert output_path.exists()
    assert (output_dir / "ffdm.dcm").exists()

    ds = pydicom.dcmread(output_path)
    assert ds.Modality == "MG"
    assert ds.SOPClassUID == EXPECTED_DBT_SOP_CLASS_UID
    assert ds.file_meta.MediaStorageSOPClassUID == EXPECTED_DBT_SOP_CLASS_UID
    assert ds.NumberOfFrames == "3"
    assert "ViewPosition" not in ds
    assert "ImageLaterality" not in ds
    assert len(ds.PixelData) == 3 * ds.Rows * ds.Columns * 2
    assert ds.ImageType == ["DERIVED", "PRIMARY", "TOMOSYNTHESIS", "NONE"]
    assert ds.VolumetricProperties == "VOLUME"
    assert ds.VolumeBasedCalculationTechnique == "TOMOSYNTHESIS"
    assert ds.PresentationLUTShape == "IDENTITY"

    assert len(ds.SharedFunctionalGroupsSequence) == 1
    shared = ds.SharedFunctionalGroupsSequence[0]
    assert shared.PixelMeasuresSequence[0].PixelSpacing == [0.1, 0.1]
    assert shared.PixelMeasuresSequence[0].SliceThickness == 1.0
    assert shared.PlaneOrientationSequence[0].ImageOrientationPatient == [
        1.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
    assert shared.FrameAnatomySequence[0].FrameLaterality == "L"
    assert shared.PixelValueTransformationSequence[0].RescaleIntercept == 0
    assert shared.PixelValueTransformationSequence[0].RescaleSlope == 1
    assert shared.FrameVOILUTSequence[0].WindowCenter == 2048
    assert shared.FrameVOILUTSequence[0].WindowWidth == 4096

    assert len(ds.PerFrameFunctionalGroupsSequence) == 3
    source_uids = [
        pydicom.dcmread(path, stop_before_pixels=True).SOPInstanceUID for path in source_paths
    ]
    for index, frame in enumerate(ds.PerFrameFunctionalGroupsSequence, start=1):
        assert frame.FrameContentSequence[0].InStackPositionNumber == index
        assert frame.PlanePositionSequence[0].ImagePositionPatient == [0.0, 0.0, float(index - 1)]
        assert frame.XRay3DFrameTypeSequence[0].FrameType == [
            "DERIVED",
            "PRIMARY",
            "TOMOSYNTHESIS",
            "NONE",
        ]
        source = frame.DerivationImageSequence[0].SourceImageSequence[0]
        assert source.ReferencedSOPInstanceUID == source_uids[index - 1]
        assert source.SpatialLocationsPreserved == "YES"

    metadata = MammogramExtractor.extract_from_file(output_path)
    assert metadata.mammogram_type == MammogramType.TOMO
    assert metadata.dbt_object_kind == DbtObjectKind.VOLUME
    assert metadata.laterality == Laterality.LEFT
    assert metadata.view_position == ViewPosition.MLO


def test_convert_streams_implicit_vr_little_endian_pixels(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    source_paths = create_old_format_dbt_series(input_dir, frame_count=2)
    for path in source_paths:
        dataset = pydicom.dcmread(path)
        dataset.file_meta.TransferSyntaxUID = ImplicitVRLittleEndian
        dcmwrite(path, dataset, enforce_file_format=True, implicit_vr=True, little_endian=True)

    report = convert_dbt_study(input_dir, output_dir)

    output = pydicom.dcmread(report["converted_series"][0]["output_path"])
    frame_bytes = output.Rows * output.Columns * 2
    assert output.PixelData[:frame_bytes] == b"\0\0" * (output.Rows * output.Columns)
    assert output.PixelData[frame_bytes:] == b"\1\0" * (output.Rows * output.Columns)


def test_convert_dry_run_reports_planned_outputs_without_writes(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)

    report = convert_dbt_study(input_dir, output_dir, dry_run=True)

    assert report["dry_run"] is True
    assert report["summary"]["converted_series"] == 1
    assert not output_dir.exists()


def test_convert_refuses_unsupported_series_without_writes(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    _write_multifile_ffdm_series(input_dir, "1.2.826.0.1.3680043.10.543.11.1")

    with pytest.raises(Exception, match="unsupported"):
        convert_dbt_study(input_dir, output_dir)

    assert not output_dir.exists()


def test_convert_rejects_invalid_pixel_data_length(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)
    path = input_dir / "dbt_slice_1.dcm"
    ds = pydicom.dcmread(path)
    ds.PixelData = ds.PixelData[:-2]
    ds.save_as(path, enforce_file_format=True)

    with pytest.raises(Exception, match="PixelData length"):
        convert_dbt_study(input_dir, output_dir)

    assert not output_dir.exists()


def test_convert_rejects_truncated_pixel_data_stream(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    paths = create_old_format_dbt_series(input_dir)
    truncated = paths[1]
    with truncated.open("r+b") as handle:
        handle.truncate(truncated.stat().st_size - 2)

    with pytest.raises(Exception, match="ended after"):
        convert_dbt_study(input_dir, output_dir)

    assert not list(output_dir.glob("*.dcm"))


def test_convert_rejects_missing_functional_group_metadata(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    paths = create_old_format_dbt_series(input_dir)
    ds = pydicom.dcmread(paths[0])
    del ds.PixelSpacing
    ds.save_as(paths[0], enforce_file_format=True)

    with pytest.raises(Exception, match="missing PixelSpacing"):
        convert_dbt_study(input_dir, output_dir)

    assert not output_dir.exists()


def test_convert_rejects_inconsistent_functional_group_metadata(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    paths = create_old_format_dbt_series(input_dir)
    ds = pydicom.dcmread(paths[-1])
    ds.WindowCenter = 1024
    ds.save_as(paths[-1], enforce_file_format=True)

    with pytest.raises(Exception, match="inconsistent functional-group metadata"):
        convert_dbt_study(input_dir, output_dir)

    assert not output_dir.exists()


def test_convert_preflights_copy_collisions_before_writes(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    output_dir.mkdir()
    create_old_format_dbt_series(input_dir)
    _write_ffdm(input_dir / "ffdm.dcm")
    (output_dir / "ffdm.dcm").write_bytes(b"existing")

    with pytest.raises(Exception, match="already exists"):
        convert_dbt_study(input_dir, output_dir)

    assert not list(output_dir.glob("dbt_*.dcm"))


def test_python_and_cli_check_reports_match(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)

    py_report = scan_dbt_study(input_dir)
    result = _run_cli("check", "--format", "json", str(input_dir))

    assert result.returncode == 1
    cli_report = json.loads(result.stdout)
    assert cli_report["summary"] == py_report["summary"]


def test_cli_convert_dry_run_json_is_parseable(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)

    result = _run_cli("convert", "--dry-run", "--format", "json", str(input_dir), str(output_dir))

    assert result.returncode == 0
    report = json.loads(result.stdout)
    assert report["dry_run"] is True
    assert report["summary"]["converted_series"] == 1


def test_scan_flags_gapped_instance_numbers(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir, frame_count=2)
    path = input_dir / "dbt_slice_1.dcm"
    ds = pydicom.dcmread(path)
    ds.InstanceNumber = "3"
    ds.ImagePositionPatient = [0.0, 0.0, 3.0]
    ds.save_as(path, enforce_file_format=True)

    report = scan_dbt_study(input_dir)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "InstanceNumber" in report["unsupported_series"][0]["reason"]


def test_scan_groups_dbt_series_across_sibling_directories(tmp_path: Path) -> None:
    series_uid = "1.2.826.0.1.3680043.10.543.7.1"
    for instance_number, parent in enumerate(["a", "a", "b"]):
        directory = tmp_path / parent
        directory.mkdir(exist_ok=True)
        ds = create_old_format_dbt_slice(
            series_uid=series_uid,
            sop_uid=f"{series_uid}.{instance_number + 1}",
            instance_number=instance_number,
        )
        ds.save_as(directory / f"slice_{instance_number}.dcm", enforce_file_format=True)

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 1
    series = report["conversion_needed_series"][0]
    assert series["frame_count"] == 3
    assert series["source_paths"] == ["a/slice_0.dcm", "a/slice_1.dcm", "b/slice_2.dcm"]


def test_scan_flags_unsupported_transfer_syntax(tmp_path: Path) -> None:
    paths = create_old_format_dbt_series(tmp_path, frame_count=2)
    for path in paths:
        ds = pydicom.dcmread(path)
        ds.file_meta.TransferSyntaxUID = ExplicitVRBigEndian
        dcmwrite(path, ds, enforce_file_format=True, implicit_vr=False, little_endian=False)

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "unsupported transfer syntax" in report["unsupported_series"][0]["reason"]


def test_scan_flags_mixed_dimensions(tmp_path: Path) -> None:
    for instance_number, rows in enumerate([4, 5]):
        ds = create_old_format_dbt_slice(
            sop_uid=f"1.2.826.0.1.3680043.10.543.8.{instance_number + 1}",
            instance_number=instance_number,
            rows=rows,
        )
        ds.save_as(tmp_path / f"slice_{instance_number}.dcm", enforce_file_format=True)

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "mixed image dimensions" in report["unsupported_series"][0]["reason"]


def test_scan_flags_duplicate_instance_numbers(tmp_path: Path) -> None:
    for index in range(2):
        ds = create_old_format_dbt_slice(
            sop_uid=f"1.2.826.0.1.3680043.10.543.9.{index + 1}",
            instance_number=1,
        )
        ds.save_as(tmp_path / f"slice_{index}.dcm", enforce_file_format=True)

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "duplicate InstanceNumber" in report["unsupported_series"][0]["reason"]


def test_scan_flags_mixed_view_positions(tmp_path: Path) -> None:
    series_uid = "1.2.826.0.1.3680043.10.543.13.1"
    for index, view in enumerate(["MLO", "CC"]):
        ds = create_old_format_dbt_slice(
            series_uid=series_uid,
            sop_uid=f"{series_uid}.{index + 1}",
            instance_number=index,
            view=view,
        )
        ds.ViewPosition = view
        ds.save_as(tmp_path / f"slice_{index}.dcm", enforce_file_format=True)

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "mixed view position" in report["unsupported_series"][0]["reason"]


def test_scan_flags_ambiguous_multifile_non_dbt_series(tmp_path: Path) -> None:
    _write_multifile_ffdm_series(tmp_path, "1.2.826.0.1.3680043.10.543.10.1")

    report = scan_dbt_study(tmp_path)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "no DBT evidence" in report["unsupported_series"][0]["reason"]


def test_scan_flags_missing_view(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    input_dir.mkdir()
    create_old_format_dbt_series(input_dir)
    for path in input_dir.glob("*.dcm"):
        ds = pydicom.dcmread(path)
        ds.SeriesDescription = "TOMO volume without view"
        ds.save_as(path, enforce_file_format=True)

    report = scan_dbt_study(input_dir)

    assert report["summary"]["conversion_needed_series"] == 0
    assert report["summary"]["unsupported_series"] == 1
    assert "view position" in report["unsupported_series"][0]["reason"]


def test_convert_copies_multi_instance_multiframe_dbt_series(tmp_path: Path) -> None:
    input_dir = tmp_path / "input"
    output_dir = tmp_path / "output"
    input_dir.mkdir()
    series_uid = "1.2.826.0.1.3680043.10.543.12.1"
    for instance_number in range(1, 3):
        _write_multiframe_dbt(
            input_dir / f"dbt_multiframe_{instance_number}.dcm",
            series_uid,
            instance_number,
        )

    report = scan_dbt_study(input_dir)

    assert report["summary"]["already_multiframe_dbt_series"] == 1
    assert report["summary"]["unsupported_series"] == 0
    assert report["already_multiframe_dbt_series"][0]["source_paths"] == [
        "dbt_multiframe_1.dcm",
        "dbt_multiframe_2.dcm",
    ]

    convert_report = convert_dbt_study(input_dir, output_dir)

    assert convert_report["summary"]["converted_series"] == 0
    assert convert_report["summary"]["copied_files"] == 2
    assert (output_dir / "dbt_multiframe_1.dcm").exists()
    assert (output_dir / "dbt_multiframe_2.dcm").exists()


def test_exported_sop_class_constant_matches_expected() -> None:
    assert BREAST_TOMOSYNTHESIS_SOP_CLASS_UID == EXPECTED_DBT_SOP_CLASS_UID
