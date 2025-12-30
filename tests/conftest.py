"""Pytest configuration and fixtures for mammocat tests."""

import pytest
from pydicom.dataset import Dataset
from pydicom.uid import ExplicitVRLittleEndian


def create_mammogram_dicom(
    mammogram_type: str = "FFDM",
    laterality: str = "L",
    view_position: str = "MLO",
    rows: int = 2048,
    columns: int = 1536,
    has_implant: bool = False,
    is_spot_compression: bool = False,
    is_magnified: bool = False,
    is_implant_displaced: bool = False,
) -> Dataset:
    """Create a synthetic mammography DICOM dataset.

    Args:
        mammogram_type: Type of mammogram (FFDM, TOMO, SYNTH, SFM)
        laterality: L (left), R (right), or B (bilateral)
        view_position: MLO, CC, etc.
        rows: Image height in pixels
        columns: Image width in pixels
        has_implant: Whether patient has implant
        is_spot_compression: Whether this is spot compression view
        is_magnified: Whether this is magnified view
        is_implant_displaced: Whether this is implant displaced view

    Returns:
        A pydicom Dataset with mammography metadata
    """
    # Create file meta information
    file_meta = Dataset()
    file_meta.TransferSyntaxUID = ExplicitVRLittleEndian
    file_meta.MediaStorageSOPClassUID = (
        "1.2.840.10008.5.1.4.1.1.1.2"  # Digital Mammography X-Ray Image Storage
    )
    file_meta.MediaStorageSOPInstanceUID = "1.2.3.4.5.6.7.8.9"

    # Create a basic DICOM dataset
    ds = Dataset()
    ds.file_meta = file_meta  # type: ignore[assignment]

    # Patient Information
    ds.PatientName = "TEST^PATIENT"
    ds.PatientID = "TEST123"
    ds.PatientBirthDate = "19700101"
    ds.PatientSex = "F"

    # Study Information
    ds.StudyInstanceUID = "1.2.3.4.5"
    ds.StudyDate = "20240101"
    ds.StudyTime = "120000"
    ds.AccessionNumber = "ACC123"

    # Series Information
    ds.SeriesInstanceUID = "1.2.3.4.5.6"
    ds.SeriesNumber = "1"
    ds.Modality = "MG"

    # Instance Information
    ds.SOPClassUID = "1.2.840.10008.5.1.4.1.1.1.2"
    ds.SOPInstanceUID = "1.2.3.4.5.6.7.8.9"
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

    # Mammography-specific fields
    ds.ImageLaterality = laterality
    ds.ViewPosition = view_position

    # Set ImageType based on mammogram type
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

    # Set PresentationIntentType if not already set
    if not hasattr(ds, "PresentationIntentType"):
        ds.PresentationIntentType = "FOR PROCESSING"

    # Set NumberOfFrames for non-TOMO images
    if not hasattr(ds, "NumberOfFrames"):
        ds.NumberOfFrames = 1

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


@pytest.fixture
def fixtures_dir(tmp_path):
    """Returns path to test fixtures directory using pytest's tmp_path."""
    return tmp_path


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
