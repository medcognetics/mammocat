"""Tests for mammocat main API (requires DICOM fixtures)."""

from pathlib import Path

import pytest

from mammocat import (
    DbtObjectKind,
    DicomError,
    FilterConfig,
    MammogramExtractor,
    MammogramRecord,
    MammogramType,
    MammographyViewModifier,
    PreferenceOrder,
    SelectionError,
    get_preferred_views,
    get_preferred_views_filtered,
    get_preferred_views_with_order,
)
from tests.conftest import create_old_format_dbt_slice


def _write_test_dicom(
    directory: Path,
    mammogram_dicom_factory,
    *,
    filename: str,
    study_uid: str,
    sop_suffix: str,
    laterality: str,
    view_position: str,
    mammogram_type: str = "FFDM",
) -> Path:
    path = directory / filename
    ds = mammogram_dicom_factory(
        mammogram_type=mammogram_type,
        laterality=laterality,
        view_position=view_position,
        study_instance_uid=study_uid,
        series_instance_uid=f"{study_uid}.1",
        sop_instance_uid=f"{study_uid}.{sop_suffix}",
    )
    ds.save_as(path, enforce_file_format=True)
    return path


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
        assert isinstance(metadata.view_modifiers, list)
        assert isinstance(metadata.number_of_frames, int)
        assert metadata.pixel_spacing == {"row": 0.07, "column": 0.07}
        assert metadata.concatenation_uid is None
        assert metadata.sop_instance_uid_of_concatenation_source is None
        assert metadata.transfer_syntax_uid == "1.2.840.10008.1.2.1"
        assert metadata.transfer_syntax_name == "Explicit VR Little Endian"
        assert metadata.compression_type == "uncompressed"

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
        assert d["view_modifiers"] == []
        assert "number_of_frames" in d
        assert d["pixel_spacing"] == {"row": 0.07, "column": 0.07}
        assert "dbt_object_kind" in d
        assert "concatenation_uid" in d
        assert "sop_instance_uid_of_concatenation_source" in d
        assert d["transfer_syntax_uid"] == "1.2.840.10008.1.2.1"
        assert d["transfer_syntax_name"] == "Explicit VR Little Endian"
        assert d["compression_type"] == "uncompressed"

    def test_synthesized_metadata_uses_canonical_machine_value(
        self, fixtures_dir, mammogram_dicom_factory
    ):
        """Test serialized metadata distinguishes the machine and display values."""
        dicom_path = fixtures_dir / "synthetic_2d.dcm"
        ds = mammogram_dicom_factory(mammogram_type="SYNTH")
        ds.ImageType = ["DERIVED", "PRIMARY", "TOMO_2D"]
        ds.PresentationIntentType = "FOR PRESENTATION"
        ds.save_as(dicom_path, enforce_file_format=True)

        metadata = MammogramExtractor.extract_from_file(dicom_path)

        assert metadata.to_dict()["mammogram_type"] == "synth"
        assert metadata.mammogram_type.value == "synth"
        assert str(metadata.mammogram_type) == "s-view"

    def test_canonical_nested_modifiers_are_exposed(self, fixtures_dir, mammogram_dicom_factory):
        path = fixtures_dir / "canonical_modifiers.dcm"
        ds = mammogram_dicom_factory(
            mammogram_type="FFDM",
            view_position="CC",
            is_spot_compression=True,
            is_magnified=True,
            is_implant_displaced=True,
            nested_view_modifiers=True,
        )
        ds.save_as(path, enforce_file_format=True)

        metadata = MammogramExtractor.extract_from_file(path)

        assert metadata.view_modifiers == [
            MammographyViewModifier.IMPLANT_DISPLACED,
            MammographyViewModifier.MAGNIFICATION,
            MammographyViewModifier.SPOT_COMPRESSION,
        ]
        assert metadata.is_implant_displaced
        assert metadata.is_magnified
        assert metadata.is_spot_compression

    def test_concat_metadata_to_dict(self, fixtures_dir, mammogram_dicom_factory):
        """Test concat identifiers are exposed in metadata and to_dict."""
        dicom_path = fixtures_dir / "concat_metadata.dcm"
        ds = mammogram_dicom_factory(mammogram_type="FFDM")
        ds.ConcatenationUID = "1.2.826.0.1.100"
        ds.SOPInstanceUIDOfConcatenationSource = "1.2.826.0.1.101"
        ds.save_as(dicom_path, enforce_file_format=True)

        metadata = MammogramExtractor.extract_from_file(dicom_path)
        metadata_dict = metadata.to_dict()

        assert metadata.concatenation_uid == "1.2.826.0.1.100"
        assert metadata.sop_instance_uid_of_concatenation_source == "1.2.826.0.1.101"
        assert metadata_dict["concatenation_uid"] == "1.2.826.0.1.100"
        assert metadata_dict["sop_instance_uid_of_concatenation_source"] == "1.2.826.0.1.101"

    def test_invalid_pixel_spacing_uses_imager_fallback(
        self, fixtures_dir, mammogram_dicom_factory
    ):
        """Test an invalid primary spacing does not suppress a valid fallback."""
        dicom_path = fixtures_dir / "spacing_fallback.dcm"
        ds = mammogram_dicom_factory(mammogram_type="FFDM")
        ds.PixelSpacing = ["-0.07", "0"]
        ds.ImagerPixelSpacing = ["0.09", "0.091"]
        ds.save_as(dicom_path, enforce_file_format=True)

        metadata = MammogramExtractor.extract_from_file(dicom_path)

        assert metadata.pixel_spacing == {"row": 0.09, "column": 0.091}

    def test_single_frame_tomo_slice_metadata(self, fixtures_dir):
        """Test single-frame DBT slices are TOMO with DBT slice kind."""
        dicom_path = fixtures_dir / "dbt_slice.dcm"
        ds = create_old_format_dbt_slice(
            sop_uid="1.2.826.0.1.3680043.10.543.9.1",
            instance_number=1,
            laterality="R",
            view="CC",
            modality="MG",
        )
        ds.save_as(dicom_path, enforce_file_format=True)

        metadata = MammogramExtractor.extract_from_file(dicom_path)
        metadata_dict = metadata.to_dict()

        assert metadata.mammogram_type == MammogramType.TOMO
        assert metadata.dbt_object_kind == DbtObjectKind.SLICE
        assert not metadata.is_2d()
        assert metadata_dict["mammogram_type"] == "tomo"
        assert metadata_dict["dbt_object_kind"] == "slice"

    def test_nested_view_modifiers_match_top_level_encoding(
        self, fixtures_dir, mammogram_dicom_factory
    ):
        modifier_options = {
            "is_spot_compression": True,
            "is_magnified": True,
            "is_implant_displaced": True,
        }
        top_level_path = fixtures_dir / "top_level_modifiers.dcm"
        nested_path = fixtures_dir / "nested_modifiers.dcm"
        mammogram_dicom_factory(**modifier_options).save_as(
            top_level_path, enforce_file_format=True
        )
        mammogram_dicom_factory(**modifier_options, nested_view_modifiers=True).save_as(
            nested_path, enforce_file_format=True
        )

        top_level = MammogramExtractor.extract_from_file(top_level_path)
        nested = MammogramExtractor.extract_from_file(nested_path)

        assert (
            top_level.is_spot_compression,
            top_level.is_magnified,
            top_level.is_implant_displaced,
        ) == (True, True, True)
        assert (
            nested.is_spot_compression,
            nested.is_magnified,
            nested.is_implant_displaced,
        ) == (
            top_level.is_spot_compression,
            top_level.is_magnified,
            top_level.is_implant_displaced,
        )


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
        assert isinstance(record.series_instance_uid, str)
        assert isinstance(record.is_implant_displaced, bool)
        assert isinstance(record.is_spot_compression, bool)
        assert isinstance(record.is_magnified, bool)
        assert isinstance(record.is_lossy_compressed, bool)
        assert isinstance(record.transfer_syntax_uid, str)
        assert not record.is_lossy_compressed

    def test_record_lossy_properties_from_file(self, lossy_dicom):
        """Test lossy compression property extraction from a DICOM file."""
        record = MammogramRecord.from_file(lossy_dicom)

        assert record.is_lossy_compressed
        assert record.transfer_syntax_uid is not None

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
        assert record_bytes.is_lossy_compressed == record_file.is_lossy_compressed
        assert record_bytes.transfer_syntax_uid == record_file.transfer_syntax_uid

    def test_from_bytes_lossy_properties(self, lossy_dicom):
        """Test lossy compression property extraction from in-memory DICOM bytes."""
        with Path(lossy_dicom).open("rb") as f:
            data = f.read()

        record = MammogramRecord.from_bytes(data, id="lossy_upload")

        assert record.is_lossy_compressed
        assert record.transfer_syntax_uid is not None

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

    def test_default_selects_one_most_complete_study(self, fixtures_dir, mammogram_dicom_factory):
        """Test default selection does not mix studies."""
        incomplete_study = "1.2.826.0.10"
        complete_study = "1.2.826.0.20"
        paths = [
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="a_l_mlo.dcm",
                study_uid=incomplete_study,
                sop_suffix="1",
                laterality="L",
                view_position="MLO",
            ),
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="a_r_mlo.dcm",
                study_uid=incomplete_study,
                sop_suffix="2",
                laterality="R",
                view_position="MLO",
            ),
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="b_l_mlo.dcm",
                study_uid=complete_study,
                sop_suffix="1",
                laterality="L",
                view_position="MLO",
                mammogram_type="TOMO",
            ),
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="b_r_mlo.dcm",
                study_uid=complete_study,
                sop_suffix="2",
                laterality="R",
                view_position="MLO",
                mammogram_type="TOMO",
            ),
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="b_l_cc.dcm",
                study_uid=complete_study,
                sop_suffix="3",
                laterality="L",
                view_position="CC",
                mammogram_type="TOMO",
            ),
        ]
        records = [MammogramRecord.from_file(str(path)) for path in paths]

        with pytest.warns(
            UserWarning,
            match="mixed study input detected.*selecting only the most complete study",
        ):
            result = get_preferred_views(records)

        selected = [record for record in result.values() if record is not None]
        assert len(selected) == 3
        assert {record.study_instance_uid for record in selected} == {complete_study}

    def test_strict_selection_errors_for_multiple_studies(
        self, fixtures_dir, mammogram_dicom_factory
    ):
        """Test strict selection rejects multiple usable studies."""
        paths = [
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="a_l_mlo.dcm",
                study_uid="1.2.826.0.10",
                sop_suffix="1",
                laterality="L",
                view_position="MLO",
            ),
            _write_test_dicom(
                fixtures_dir,
                mammogram_dicom_factory,
                filename="b_r_mlo.dcm",
                study_uid="1.2.826.0.20",
                sop_suffix="1",
                laterality="R",
                view_position="MLO",
            ),
        ]
        records = [MammogramRecord.from_file(str(path)) for path in paths]

        with pytest.raises(SelectionError, match="strict study selection"):
            get_preferred_views(records, strict=True)


class TestFilterConfig:
    def test_default_require_common_modality_false(self):
        """Test that FilterConfig default has require_common_modality == False."""
        config = FilterConfig()
        assert config.require_common_modality is False

    def test_require_common_modality_true(self):
        """Test FilterConfig with require_common_modality=True."""
        config = FilterConfig(require_common_modality=True)
        assert config.require_common_modality is True

    def test_default_static_method(self):
        """Test FilterConfig.default() has require_common_modality == False."""
        config = FilterConfig.default()
        assert config.require_common_modality is False

    def test_permissive_static_method(self):
        """Test FilterConfig.permissive() has require_common_modality == False."""
        config = FilterConfig.permissive()
        assert config.require_common_modality is False

    def test_all_default_properties(self):
        """Test all default FilterConfig properties."""
        config = FilterConfig()
        assert config.allowed_types is None
        assert config.allowed_dbt_object_kinds is None
        assert config.exclude_implants is False
        assert config.exclude_non_standard_views is False
        assert config.exclude_for_processing is True
        assert config.exclude_secondary_capture is True
        assert config.exclude_non_mg_modality is True
        assert config.require_common_modality is False
        assert config.exclude_lossy_compressed is False
        assert config.deprioritize_lossy_compressed is True

    def test_lossy_compression_options(self):
        """Test FilterConfig lossy compression options."""
        config = FilterConfig(
            exclude_lossy_compressed=True,
            deprioritize_lossy_compressed=False,
        )

        assert config.exclude_lossy_compressed is True
        assert config.deprioritize_lossy_compressed is False

    def test_dbt_object_kind_filter_options(self):
        """Test FilterConfig DBT object kind whitelist options."""
        config = FilterConfig(allowed_dbt_object_kinds=[DbtObjectKind.VOLUME, DbtObjectKind.SLICE])

        assert set(config.allowed_dbt_object_kinds or []) == {
            DbtObjectKind.VOLUME,
            DbtObjectKind.SLICE,
        }

    def test_get_preferred_views_filtered_empty(self):
        """Test get_preferred_views_filtered with empty list."""
        config = FilterConfig(require_common_modality=True)
        result = get_preferred_views_filtered([], config, PreferenceOrder.DEFAULT)
        assert isinstance(result, dict)
        assert len(result) == 4
        assert all(v is None for v in result.values())

    def test_get_preferred_views_filtered_with_records(self, sample_dicom_set):
        """Test get_preferred_views_filtered with actual DICOM files."""
        records = [MammogramRecord.from_file(str(f)) for f in sample_dicom_set]
        config = FilterConfig(require_common_modality=True)
        result = get_preferred_views_filtered(records, config, PreferenceOrder.DEFAULT)

        assert isinstance(result, dict)
        assert len(result) == 4
