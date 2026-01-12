"""Tests for mammocat main API (requires DICOM fixtures)."""

from pathlib import Path

import pytest

from mammocat import (
    DicomError,
    MammogramExtractor,
    MammogramRecord,
    PreferenceOrder,
    get_preferred_views,
    get_preferred_views_with_order,
)


class TestMammogramExtractor:
    def test_extract_from_file(self, sample_dicom):
        """Test basic metadata extraction from a DICOM file."""
        metadata = MammogramExtractor.extract_from_file(sample_dicom)

        # Check that metadata object is returned
        assert metadata is not None

        # Check that all required fields are present
        assert metadata.mammogram_type is not None
        assert metadata.laterality is not None
        assert metadata.view_position is not None
        assert metadata.image_type is not None
        assert isinstance(metadata.is_for_processing, bool)
        assert isinstance(metadata.has_implant, bool)
        assert isinstance(metadata.is_spot_compression, bool)
        assert isinstance(metadata.is_magnified, bool)
        assert isinstance(metadata.is_implant_displaced, bool)
        assert isinstance(metadata.number_of_frames, int)

    def test_extract_from_nonexistent_file(self):
        """Test that extracting from nonexistent file raises error."""
        with pytest.raises((DicomError, IOError)):
            MammogramExtractor.extract_from_file("/nonexistent/file.dcm")

    def test_extract_with_options(self, sample_dicom):
        """Test extraction with SFM option."""
        metadata = MammogramExtractor.extract_from_file_with_options(sample_dicom, is_sfm=False)
        assert metadata is not None

    def test_metadata_methods(self, sample_dicom):
        """Test metadata helper methods."""
        metadata = MammogramExtractor.extract_from_file(sample_dicom)

        # Test mammogram_view method
        view = metadata.mammogram_view()
        assert view is not None
        assert view.laterality == metadata.laterality
        assert view.view == metadata.view_position

        # Test is_2d method
        assert isinstance(metadata.is_2d(), bool)

        # Test is_standard_view method
        assert isinstance(metadata.is_standard_view(), bool)

    def test_metadata_to_dict(self, sample_dicom):
        """Test metadata to_dict conversion."""
        metadata = MammogramExtractor.extract_from_file(sample_dicom)
        d = metadata.to_dict()

        # Check that dict contains expected keys
        assert isinstance(d, dict)
        assert "mammogram_type" in d
        assert "laterality" in d
        assert "view_position" in d
        assert "number_of_frames" in d


class TestMammogramRecord:
    def test_from_file(self, sample_dicom):
        """Test creating record from DICOM file."""
        record = MammogramRecord.from_file(sample_dicom)

        assert record is not None
        assert record.file_path is not None
        assert record.metadata is not None

    def test_record_properties(self, sample_dicom):
        """Test record property access."""
        record = MammogramRecord.from_file(sample_dicom)

        # Test all properties are accessible
        assert isinstance(record.file_path, str)
        assert record.metadata is not None
        assert isinstance(record.is_implant_displaced, bool)
        assert isinstance(record.is_spot_compression, bool)
        assert isinstance(record.is_magnified, bool)

    def test_image_area(self, sample_dicom):
        """Test image_area calculation."""
        record = MammogramRecord.from_file(sample_dicom)
        area = record.image_area()

        # Area might be None if rows/columns not available
        if area is not None:
            assert isinstance(area, int)
            assert area > 0

    def test_is_spot_or_mag(self, sample_dicom):
        """Test is_spot_or_mag method."""
        record = MammogramRecord.from_file(sample_dicom)
        assert isinstance(record.is_spot_or_mag(), bool)

    def test_record_to_dict(self, sample_dicom):
        """Test record to_dict conversion."""
        record = MammogramRecord.from_file(sample_dicom)
        d = record.to_dict()

        assert isinstance(d, dict)
        assert "file_path" in d
        assert "metadata" in d


class TestMammogramRecordFromBytes:
    """Tests for MammogramRecord.from_bytes method."""

    def test_from_bytes_basic(self, sample_dicom):
        """Test basic from_bytes functionality."""
        with Path(sample_dicom).open("rb") as f:
            data = f.read()
        record = MammogramRecord.from_bytes(data, id="test_upload")

        assert record is not None
        assert record.file_path == "test_upload"
        assert record.metadata is not None
        assert record.metadata.mammogram_type is not None
        assert record.metadata.laterality is not None
        assert record.metadata.view_position is not None

    def test_from_bytes_no_id(self, sample_dicom):
        """Test from_bytes with no id (empty path)."""
        with Path(sample_dicom).open("rb") as f:
            data = f.read()
        record = MammogramRecord.from_bytes(data)

        assert record is not None
        assert record.file_path == ""
        assert record.metadata is not None

    def test_from_bytes_matches_from_file(self, sample_dicom):
        """Test that from_bytes produces same metadata as from_file."""
        record_file = MammogramRecord.from_file(sample_dicom)

        with Path(sample_dicom).open("rb") as f:
            data = f.read()
        record_bytes = MammogramRecord.from_bytes(data, id=str(sample_dicom))

        # Metadata should match exactly
        assert record_bytes.metadata.mammogram_type == record_file.metadata.mammogram_type
        assert record_bytes.metadata.laterality == record_file.metadata.laterality
        assert record_bytes.metadata.view_position == record_file.metadata.view_position
        assert record_bytes.metadata.is_for_processing == record_file.metadata.is_for_processing
        assert record_bytes.metadata.has_implant == record_file.metadata.has_implant

        # Other properties should also match
        assert record_bytes.rows == record_file.rows
        assert record_bytes.columns == record_file.columns
        assert record_bytes.is_implant_displaced == record_file.is_implant_displaced
        assert record_bytes.is_spot_compression == record_file.is_spot_compression
        assert record_bytes.is_magnified == record_file.is_magnified

    def test_from_bytes_invalid_data(self):
        """Test from_bytes with invalid DICOM data."""
        with pytest.raises(DicomError):
            MammogramRecord.from_bytes(b"not valid dicom data")

    def test_from_bytes_empty_data(self):
        """Test from_bytes with empty bytes."""
        with pytest.raises(DicomError):
            MammogramRecord.from_bytes(b"")

    def test_from_bytes_in_view_selection(self, sample_dicom_set):
        """Test that records from from_bytes work in view selection."""
        # Load files as bytes and create records
        records = []
        for i, filepath in enumerate(sample_dicom_set):
            with Path(filepath).open("rb") as f:
                data = f.read()
            record = MammogramRecord.from_bytes(data, id=f"upload_{i}")
            records.append(record)

        # Should work with view selection
        result = get_preferred_views(records)
        assert isinstance(result, dict)
        assert len(result) == 4


class TestPreferredViews:
    def test_get_preferred_views_empty(self):
        """Test get_preferred_views with empty list."""
        result = get_preferred_views([])

        # Should return dict with 4 standard views (all None)
        assert isinstance(result, dict)
        assert len(result) == 4
        assert all(v is None for v in result.values())

    def test_get_preferred_views_with_order_empty(self):
        """Test get_preferred_views_with_order with empty list."""
        result = get_preferred_views_with_order([], PreferenceOrder.DEFAULT)

        # Should return dict with 4 standard views (all None)
        assert isinstance(result, dict)
        assert len(result) == 4
        assert all(v is None for v in result.values())

    def test_get_preferred_views_with_records(self, sample_dicom_set):
        """Test get_preferred_views with actual DICOM files."""
        # Load all DICOM files from fixtures
        records = [MammogramRecord.from_file(str(f)) for f in sample_dicom_set]
        result = get_preferred_views(records)

        # Should return dict with 4 standard views
        assert isinstance(result, dict)
        assert len(result) == 4

        # Check that keys are MammogramView objects
        for view in result:
            assert view is not None

    def test_preference_order_variants(self, sample_dicom_set):
        """Test different preference orders."""
        records = [MammogramRecord.from_file(str(f)) for f in sample_dicom_set]

        # Test both preference orders
        result_default = get_preferred_views_with_order(records, PreferenceOrder.DEFAULT)
        result_tomo = get_preferred_views_with_order(records, PreferenceOrder.TOMO_FIRST)

        assert isinstance(result_default, dict)
        assert isinstance(result_tomo, dict)
        assert len(result_default) == 4
        assert len(result_tomo) == 4
