"""Pytest configuration and fixtures for mammocat tests."""

import pytest
from pydicom.dataset import Dataset
from pydicom.uid import ExplicitVRLittleEndian

BREAST_TOMOSYNTHESIS_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.13.1.3"
CT_IMAGE_STORAGE_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.2"
DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.1.2"


def _apply_mammogram_type(ds: Dataset, mammogram_type: str) -> None:
    """Apply mammogram-type-specific synthetic tags."""
    if mammogram_type == "FFDM":
        ds.ImageType = ["ORIGINAL", "PRIMARY", ""]
    elif mammogram_type == "TOMO":
        ds.ImageType = ["ORIGINAL", "PRIMARY", "VOLUME"]
        ds.NumberOfFrames = 50
    elif mammogram_type == "SYNTH":
        ds.ImageType = ["DERIVED", "SECONDARY", ""]
        ds.PresentationIntentType = "FOR PROCESSING"
    elif mammogram_type == "SFM":
        ds.ImageType = ["ORIGINAL", "PRIMARY", ""]
        # SFM is identified by manufacturer-specific fields or absence of digital indicators


def create_mammogram_dicom(
    mammogram_type: str = "FFDM",
    laterality: str = "L",
    view_position: str = "MLO",
    rows: int = 2048,
    columns: int = 1536,
    study_instance_uid: str = "1.2.3.4.5",
    series_instance_uid: str = "1.2.3.4.5.6",
    sop_instance_uid: str = "1.2.3.4.5.6.7.8.9",
    has_implant: bool = False,
    is_spot_compression: bool = False,
    is_magnified: bool = False,
    is_implant_displaced: bool = False,
    pixel_spacing: tuple[float, float] | None = (0.07, 0.07),
    transfer_syntax_uid: str = ExplicitVRLittleEndian,
    lossy_image_compression: str = "00",
) -> Dataset:
    """Create a synthetic mammography DICOM dataset.

    Args:
        mammogram_type: Type of mammogram (FFDM, TOMO, SYNTH, SFM)
        laterality: L (left), R (right), or B (bilateral)
        view_position: MLO, CC, etc.
        rows: Image height in pixels
        columns: Image width in pixels
        study_instance_uid: DICOM StudyInstanceUID value
        series_instance_uid: DICOM SeriesInstanceUID value
        sop_instance_uid: DICOM SOPInstanceUID value
        has_implant: Whether patient has implant
        is_spot_compression: Whether this is spot compression view
        is_magnified: Whether this is magnified view
        is_implant_displaced: Whether this is implant displaced view
        pixel_spacing: Optional row/column pixel spacing in millimeters
        transfer_syntax_uid: DICOM transfer syntax UID to write in file metadata
        lossy_image_compression: LossyImageCompression tag value

    Returns:
        A pydicom Dataset with mammography metadata
    """
    # Create file meta information
    file_meta = Dataset()
    file_meta.TransferSyntaxUID = transfer_syntax_uid
    file_meta.MediaStorageSOPClassUID = DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID
    file_meta.MediaStorageSOPInstanceUID = sop_instance_uid

    # Create a basic DICOM dataset
    ds = Dataset()
    ds.file_meta = file_meta  # type: ignore[assignment]

    # Patient Information
    ds.PatientName = "TEST^PATIENT"
    ds.PatientID = "TEST123"
    ds.PatientBirthDate = "19700101"
    ds.PatientSex = "F"

    # Study Information
    ds.StudyInstanceUID = study_instance_uid
    ds.StudyDate = "20240101"
    ds.StudyTime = "120000"
    ds.AccessionNumber = "ACC123"

    # Series Information
    ds.SeriesInstanceUID = series_instance_uid
    ds.SeriesNumber = "1"
    ds.Modality = "MG"

    # Instance Information
    ds.SOPClassUID = DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID
    ds.SOPInstanceUID = sop_instance_uid
    ds.InstanceNumber = "1"

    # Image Information
    ds.Rows = rows
    ds.Columns = columns
    ds.SamplesPerPixel = 1
    ds.PhotometricInterpretation = "MONOCHROME2"
    ds.BitsAllocated = 16
    ds.BitsStored = 16
    ds.HighBit = 15
    ds.PixelRepresentation = 0
    if pixel_spacing is not None:
        ds.PixelSpacing = [str(pixel_spacing[0]), str(pixel_spacing[1])]

    # Mammography-specific fields
    ds.ImageLaterality = laterality
    ds.ViewPosition = view_position

    _apply_mammogram_type(ds, mammogram_type)

    # Set PresentationIntentType if not already set
    if not hasattr(ds, "PresentationIntentType"):
        ds.PresentationIntentType = "FOR PROCESSING"

    # Set NumberOfFrames for non-TOMO images
    if not hasattr(ds, "NumberOfFrames"):
        ds.NumberOfFrames = 1

    ds.LossyImageCompression = lossy_image_compression

    # Implant information
    if has_implant:
        ds.BreastImplantPresent = "YES"

    # View modifiers for spot compression, magnification, implant displaced
    view_modifier_codes = []
    if is_spot_compression:
        view_modifier_codes.append(("R-102D1", "99SDM", "spot compression"))
    if is_magnified:
        view_modifier_codes.append(("R-102D3", "99SDM", "magnification"))
    if is_implant_displaced:
        view_modifier_codes.append(("R-4092C", "99SDM", "implant displaced"))

    if view_modifier_codes:
        ds.ViewModifierCodeSequence = []
        for code_value, coding_scheme, code_meaning in view_modifier_codes:
            modifier = Dataset()
            modifier.CodeValue = code_value
            modifier.CodingSchemeDesignator = coding_scheme
            modifier.CodeMeaning = code_meaning
            ds.ViewModifierCodeSequence.append(modifier)

    # Equipment Information
    ds.Manufacturer = "TEST_MANUFACTURER"
    ds.ManufacturerModelName = "TEST_MODEL"

    return ds


def create_old_format_dbt_slice(
    *,
    study_uid: str = "1.2.826.0.1.3680043.10.543.1",
    series_uid: str = "1.2.826.0.1.3680043.10.543.1.1",
    sop_uid: str,
    instance_number: int,
    laterality: str = "L",
    view: str = "MLO",
    rows: int = 4,
    columns: int = 3,
    modality: str = "CT",
    pixel_value: int | None = None,
) -> Dataset:
    """Create one old-format DBT slice stored as a single-frame CT-like DICOM."""
    file_meta = Dataset()
    file_meta.TransferSyntaxUID = ExplicitVRLittleEndian
    file_meta.MediaStorageSOPClassUID = CT_IMAGE_STORAGE_SOP_CLASS_UID
    file_meta.MediaStorageSOPInstanceUID = sop_uid

    ds = Dataset()
    ds.file_meta = file_meta  # type: ignore[assignment]
    ds.PatientName = "TEST^DBT"
    ds.PatientID = "DBT123"
    ds.PatientBirthDate = "19700101"
    ds.PatientSex = "F"
    ds.StudyInstanceUID = study_uid
    ds.StudyDate = "20240102"
    ds.StudyTime = "130000"
    ds.SeriesInstanceUID = series_uid
    ds.SeriesNumber = "7"
    ds.SeriesDescription = f"TOMO {laterality}-{view} 2D+, Diagnosis"
    ds.Modality = modality
    ds.SOPClassUID = CT_IMAGE_STORAGE_SOP_CLASS_UID
    ds.SOPInstanceUID = sop_uid
    ds.InstanceNumber = str(instance_number)
    ds.ImagePositionPatient = [0.0, 0.0, float(instance_number)]
    ds.Rows = rows
    ds.Columns = columns
    ds.SamplesPerPixel = 1
    ds.PhotometricInterpretation = "MONOCHROME2"
    ds.BitsAllocated = 16
    ds.BitsStored = 12
    ds.HighBit = 11
    ds.PixelRepresentation = 0
    ds.ImageLaterality = laterality
    ds.ImageType = ["DERIVED", "PRIMARY", "TOMO", "LEFT" if laterality == "L" else "RIGHT"]

    value = instance_number if pixel_value is None else pixel_value
    pixels = [value] * (rows * columns)
    ds.PixelData = b"".join(int(pixel).to_bytes(2, "little") for pixel in pixels)
    return ds


def create_old_format_dbt_series(directory, *, frame_count: int = 3, **kwargs):
    """Write an old-format DBT series and return its file paths."""
    paths = []
    for index in range(frame_count):
        path = directory / f"dbt_slice_{index}.dcm"
        ds = create_old_format_dbt_slice(
            sop_uid=f"1.2.826.0.1.3680043.10.543.9.{index + 1}",
            instance_number=index,
            **kwargs,
        )
        ds.save_as(path, enforce_file_format=True)
        paths.append(path)
    return paths


@pytest.fixture
def fixtures_dir(tmp_path):
    """Returns path to test fixtures directory using pytest's tmp_path."""
    return tmp_path


@pytest.fixture
def mammogram_dicom_factory():
    """Returns the synthetic mammography DICOM factory."""
    return create_mammogram_dicom


@pytest.fixture
def sample_dicom(fixtures_dir):
    """Creates and returns a sample FFDM DICOM file."""
    dicom_path = fixtures_dir / "sample_ffdm_l_mlo.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="L",
        view_position="MLO",
        rows=2048,
        columns=1536,
    )
    ds.save_as(dicom_path, enforce_file_format=True)
    return str(dicom_path)


@pytest.fixture
def lossy_dicom(fixtures_dir):
    """Creates and returns a sample DICOM file marked as lossy compressed."""
    dicom_path = fixtures_dir / "lossy_ffdm_l_mlo.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="L",
        view_position="MLO",
        rows=2048,
        columns=1536,
        lossy_image_compression="01",
    )
    ds.save_as(dicom_path, enforce_file_format=True)
    return str(dicom_path)


@pytest.fixture
def has_dicom_fixtures():
    """Always returns True since we're creating fixtures programmatically."""
    return True


@pytest.fixture
def sample_dicom_set(fixtures_dir):
    """Creates a set of diverse DICOM files for comprehensive testing.

    Creates files representing different mammogram types, views, and modifiers.
    """
    dicom_files = []

    # Standard 4-view FFDM screening set
    for laterality, view in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")]:
        path = fixtures_dir / f"ffdm_{laterality.lower()}_{view.lower()}.dcm"
        ds = create_mammogram_dicom(
            mammogram_type="FFDM",
            laterality=laterality,
            view_position=view,
        )
        ds.save_as(path, enforce_file_format=True)
        dicom_files.append(path)

    # TOMO images
    for laterality, view in [("L", "MLO"), ("R", "CC")]:
        path = fixtures_dir / f"tomo_{laterality.lower()}_{view.lower()}.dcm"
        ds = create_mammogram_dicom(
            mammogram_type="TOMO",
            laterality=laterality,
            view_position=view,
        )
        ds.save_as(path, enforce_file_format=True)
        dicom_files.append(path)

    # SYNTH (synthetic 2D from TOMO)
    path = fixtures_dir / "synth_l_mlo.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="SYNTH",
        laterality="L",
        view_position="MLO",
    )
    ds.save_as(path, enforce_file_format=True)
    dicom_files.append(path)

    # Special views
    # Spot compression
    path = fixtures_dir / "ffdm_l_cc_spot.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="L",
        view_position="CC",
        is_spot_compression=True,
    )
    ds.save_as(path, enforce_file_format=True)
    dicom_files.append(path)

    # Magnified view
    path = fixtures_dir / "ffdm_r_mlo_mag.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="R",
        view_position="MLO",
        is_magnified=True,
    )
    ds.save_as(path, enforce_file_format=True)
    dicom_files.append(path)

    # Implant displaced
    path = fixtures_dir / "ffdm_l_cc_implant_displaced.dcm"
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="L",
        view_position="CC",
        has_implant=True,
        is_implant_displaced=True,
    )
    ds.save_as(path, enforce_file_format=True)
    dicom_files.append(path)

    return dicom_files
