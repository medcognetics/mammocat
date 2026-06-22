"""Tests for mammocat validation bindings."""

from pathlib import Path
from typing import Any, cast
from zipfile import ZipFile

import pytest

from mammocat import FilterConfig, PreferenceOrder, validate_dicom, validate_directory

from .conftest import create_mammogram_dicom

VALIDATION_ROWS = 8
VALIDATION_COLUMNS = 8
BYTES_PER_PIXEL = 2


def create_validation_dicom(
    path: Path,
    *,
    laterality: str = "L",
    view_position: str = "MLO",
    include_pixel_data: bool = True,
    samples_per_pixel: int = 1,
    photometric_interpretation: str = "MONOCHROME2",
    lossy_compression: bool = False,
) -> Path:
    """Create a small mammography DICOM for validation tests."""
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality=laterality,
        view_position=view_position,
        rows=VALIDATION_ROWS,
        columns=VALIDATION_COLUMNS,
    )
    ds.PresentationIntentType = "FOR PRESENTATION"
    ds.SamplesPerPixel = samples_per_pixel
    ds.PhotometricInterpretation = photometric_interpretation
    if samples_per_pixel > 1:
        ds.PlanarConfiguration = 0
    if lossy_compression:
        ds.LossyImageCompression = "01"
        ds.LossyImageCompressionMethod = "ISO_10918_1"
    if include_pixel_data:
        pixel_bytes = VALIDATION_ROWS * VALIDATION_COLUMNS * BYTES_PER_PIXEL * samples_per_pixel
        ds.PixelData = b"\0" * pixel_bytes
    ds.save_as(path, enforce_file_format=True)
    return path


@pytest.fixture(params=[Path, str])
def validation_dicom(request: pytest.FixtureRequest, tmp_path: Path) -> Path | str:
    path = create_validation_dicom(tmp_path / "valid.dcm")
    return request.param(path)


def test_validate_dicom_selection_report_passes(validation_dicom: Path | str) -> None:
    report = validate_dicom(validation_dicom)
    file_report = report["files"][0]

    assert report["status"] == "pass"
    assert report["summary"]["valid"] is True
    assert report["summary"]["profile"] == "selection"
    assert report["summary"]["source_type"] == "file"
    assert file_report["pixel"]["pixel_data_present"] is True
    assert file_report["selection"]["eligible"] is True


def test_validate_dicom_selection_failure_returns_report(tmp_path: Path) -> None:
    path = create_validation_dicom(tmp_path / "missing-pixel-data.dcm", include_pixel_data=False)

    report = validate_dicom(path)

    assert report["status"] == "fail"
    assert report["summary"]["valid"] is False
    assert any(error["code"] == "missing_pixel_data" for error in report["files"][0]["errors"])


def test_validate_dicom_extraction_profile_allows_missing_pixel_data(tmp_path: Path) -> None:
    path = create_validation_dicom(tmp_path / "metadata-only.dcm", include_pixel_data=False)

    report = validate_dicom(path, profile="extraction")

    assert report["status"] == "pass"
    assert report["summary"]["profile"] == "extraction"
    assert any(
        warning["code"] == "missing_pixel_data" for warning in report["files"][0]["warnings"]
    )


def test_validate_dicom_warns_for_rgb_without_failing(tmp_path: Path) -> None:
    path = create_validation_dicom(
        tmp_path / "rgb.dcm",
        samples_per_pixel=3,
        photometric_interpretation="RGB",
    )

    report = validate_dicom(path)
    warning_codes = {warning["code"] for warning in report["files"][0]["warnings"]}

    assert report["status"] == "pass"
    assert "unexpected_samples_per_pixel" in warning_codes
    assert "unexpected_photometric_interpretation" in warning_codes


def test_validate_dicom_warns_for_lossy_compression_without_failing(
    tmp_path: Path,
) -> None:
    path = create_validation_dicom(tmp_path / "lossy.dcm", lossy_compression=True)

    report = validate_dicom(path)
    warning_codes = {warning["code"] for warning in report["files"][0]["warnings"]}

    assert report["status"] == "pass"
    assert "lossy_compression" in warning_codes


def test_validate_dicom_rejects_invalid_profile(validation_dicom: Path | str) -> None:
    with pytest.raises(ValueError, match="Invalid validation profile"):
        validate_dicom(validation_dicom, profile=cast(Any, "bad"))


def test_validate_dicom_missing_path_raises(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match="File not found"):
        validate_dicom(tmp_path / "missing.dcm")


def test_validate_directory_reports_standard_view_coverage(tmp_path: Path) -> None:
    for laterality, view_position in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")]:
        create_validation_dicom(
            tmp_path / f"{laterality.lower()}_{view_position.lower()}.dcm",
            laterality=laterality,
            view_position=view_position,
        )

    report = validate_directory(tmp_path)

    assert report["status"] == "pass"
    assert report["summary"]["file_count"] == 4
    assert report["directory"]["missing_views"] == []
    assert all(view["selected"] for view in report["directory"]["selected_views"].values())


def test_validate_directory_accepts_zip_archive(tmp_path: Path) -> None:
    dicom_paths = []
    for laterality, view_position in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")]:
        dicom_paths.append(
            create_validation_dicom(
                tmp_path / f"{laterality.lower()}_{view_position.lower()}.dcm",
                laterality=laterality,
                view_position=view_position,
            )
        )
    zip_path = tmp_path / "dicoms.zip"
    with ZipFile(zip_path, "w") as archive:
        for dicom_path in dicom_paths:
            archive.write(dicom_path, arcname=f"nested/{dicom_path.name}")
        archive.writestr("notes.txt", "not a dicom")

    report = validate_directory(zip_path)

    assert report["status"] == "pass"
    assert report["summary"]["source_type"] == "zip"
    assert report["summary"]["file_count"] == 4
    assert report["directory"]["missing_views"] == []
    assert all("dicoms.zip::nested/" in file["file"]["path"] for file in report["files"])


def test_validate_directory_accepts_filter_and_preference(tmp_path: Path) -> None:
    create_validation_dicom(tmp_path / "l_mlo.dcm", laterality="L", view_position="MLO")

    report = validate_directory(
        tmp_path,
        profile="extraction",
        filter_config=FilterConfig.permissive(),
        preference_order=PreferenceOrder.TOMO_FIRST,
    )

    assert report["summary"]["profile"] == "extraction"
    assert report["summary"]["file_count"] == 1
