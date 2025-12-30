"""Tests for mammocat enum types."""

from mammocat import (
    ImageType,
    Laterality,
    MammogramType,
    MammogramView,
    PhotometricInterpretation,
    PreferenceOrder,
    ViewPosition,
)


class TestMammogramType:
    def test_enum_values(self):
        """Test MammogramType enum values."""
        assert MammogramType.FFDM.value == "ffdm"
        assert MammogramType.TOMO.value == "tomo"
        assert MammogramType.SYNTH.value == "s-view"
        assert MammogramType.SFM.value == "sfm"
        assert MammogramType.UNKNOWN.value == "unknown"

    def test_string_representation(self):
        """Test string representation."""
        assert str(MammogramType.FFDM) == "ffdm"
        assert str(MammogramType.TOMO) == "tomo"

    def test_is_unknown(self):
        """Test is_unknown method."""
        assert MammogramType.UNKNOWN.is_unknown()
        assert not MammogramType.FFDM.is_unknown()

    def test_equality(self):
        """Test equality comparison."""
        assert MammogramType.FFDM == MammogramType.FFDM
        assert MammogramType.FFDM != MammogramType.TOMO

    def test_hash(self):
        """Test that enums are hashable."""
        enum_set = {MammogramType.FFDM, MammogramType.TOMO, MammogramType.FFDM}
        assert len(enum_set) == 2

    def test_ordering(self):
        """Test ordering comparisons."""
        assert MammogramType.TOMO < MammogramType.FFDM
        assert MammogramType.FFDM < MammogramType.SYNTH
        assert MammogramType.SYNTH < MammogramType.SFM

    def test_is_preferred_to(self):
        """Test preference comparison."""
        assert MammogramType.TOMO.is_preferred_to(MammogramType.FFDM)
        assert MammogramType.FFDM.is_preferred_to(MammogramType.SYNTH)
        assert not MammogramType.SYNTH.is_preferred_to(MammogramType.FFDM)


class TestLaterality:
    def test_enum_values(self):
        """Test Laterality enum values."""
        assert Laterality.LEFT.value == "left"
        assert Laterality.RIGHT.value == "right"
        assert Laterality.BILATERAL.value == "bilateral"
        assert Laterality.NONE.value == "none"
        assert Laterality.UNKNOWN.value == "unknown"

    def test_string_representation(self):
        """Test string representation."""
        assert str(Laterality.LEFT) == "left"
        assert str(Laterality.RIGHT) == "right"

    def test_is_unilateral(self):
        """Test is_unilateral method."""
        assert Laterality.LEFT.is_unilateral()
        assert Laterality.RIGHT.is_unilateral()
        assert not Laterality.BILATERAL.is_unilateral()
        assert not Laterality.UNKNOWN.is_unilateral()

    def test_opposite(self):
        """Test opposite method."""
        assert Laterality.LEFT.opposite() == Laterality.RIGHT
        assert Laterality.RIGHT.opposite() == Laterality.LEFT
        assert Laterality.BILATERAL.opposite() == Laterality.UNKNOWN

    def test_equality(self):
        """Test equality comparison."""
        assert Laterality.LEFT == Laterality.LEFT
        assert Laterality.LEFT != Laterality.RIGHT

    def test_hash(self):
        """Test that enums are hashable."""
        lat_set = {Laterality.LEFT, Laterality.RIGHT, Laterality.LEFT}
        assert len(lat_set) == 2


class TestViewPosition:
    def test_enum_values(self):
        """Test ViewPosition enum values."""
        assert ViewPosition.CC.value == "cc"
        assert ViewPosition.MLO.value == "mlo"
        assert ViewPosition.XCCL.value == "xccl"
        assert ViewPosition.UNKNOWN.value == ""

    def test_is_standard_view(self):
        """Test is_standard_view method."""
        assert ViewPosition.CC.is_standard_view()
        assert ViewPosition.MLO.is_standard_view()
        assert not ViewPosition.ML.is_standard_view()
        assert not ViewPosition.UNKNOWN.is_standard_view()

    def test_is_mlo_like(self):
        """Test is_mlo_like method."""
        assert ViewPosition.MLO.is_mlo_like()
        assert ViewPosition.ML.is_mlo_like()
        assert ViewPosition.LMO.is_mlo_like()
        assert not ViewPosition.CC.is_mlo_like()

    def test_is_cc_like(self):
        """Test is_cc_like method."""
        assert ViewPosition.CC.is_cc_like()
        assert ViewPosition.XCCL.is_cc_like()
        assert ViewPosition.XCCM.is_cc_like()
        assert not ViewPosition.MLO.is_cc_like()

    def test_ordering(self):
        """Test ordering comparisons."""
        assert ViewPosition.UNKNOWN < ViewPosition.XCCL
        assert ViewPosition.CC < ViewPosition.MLO


class TestPreferenceOrder:
    def test_enum_values(self):
        """Test PreferenceOrder enum values."""
        assert PreferenceOrder.DEFAULT.value == "default"
        assert PreferenceOrder.TOMO_FIRST.value == "tomo-first"

    def test_string_representation(self):
        """Test string representation."""
        assert str(PreferenceOrder.DEFAULT) == "default"
        assert str(PreferenceOrder.TOMO_FIRST) == "tomo-first"


class TestPhotometricInterpretation:
    def test_enum_values(self):
        """Test PhotometricInterpretation enum values."""
        assert PhotometricInterpretation.MONOCHROME1 is not None
        assert PhotometricInterpretation.MONOCHROME2 is not None
        assert PhotometricInterpretation.RGB is not None

    def test_is_monochrome(self):
        """Test is_monochrome method."""
        assert PhotometricInterpretation.MONOCHROME1.is_monochrome()
        assert PhotometricInterpretation.MONOCHROME2.is_monochrome()
        assert not PhotometricInterpretation.RGB.is_monochrome()

    def test_num_channels(self):
        """Test num_channels method."""
        assert PhotometricInterpretation.MONOCHROME1.num_channels() == 1
        assert PhotometricInterpretation.MONOCHROME2.num_channels() == 1
        assert PhotometricInterpretation.RGB.num_channels() == 3


class TestImageType:
    def test_constructor(self):
        """Test ImageType construction."""
        img_type = ImageType("ORIGINAL", "PRIMARY")
        assert img_type.pixels == "ORIGINAL"
        assert img_type.exam == "PRIMARY"
        assert img_type.flavor is None
        assert img_type.extras is None

    def test_constructor_with_all_fields(self):
        """Test ImageType with all fields."""
        img_type = ImageType("DERIVED", "PRIMARY", "TOMO", ["GENERATED_2D"])
        assert img_type.pixels == "DERIVED"
        assert img_type.exam == "PRIMARY"
        assert img_type.flavor == "TOMO"
        assert img_type.extras == ["GENERATED_2D"]

    def test_contains(self):
        """Test contains method."""
        img_type = ImageType("ORIGINAL", "PRIMARY", "POST_PROCESSED", ["SUBTRACTION"])
        assert img_type.contains("ORIGINAL")
        assert img_type.contains("PRIMARY")
        assert img_type.contains("POST_PROCESSED")
        assert img_type.contains("SUBTRACTION")
        assert not img_type.contains("DERIVED")

    def test_is_valid(self):
        """Test is_valid method."""
        assert ImageType("ORIGINAL", "PRIMARY").is_valid()
        assert not ImageType("", "PRIMARY").is_valid()
        assert not ImageType("ORIGINAL", "").is_valid()

    def test_string_representation(self):
        """Test string representation."""
        img_type = ImageType("ORIGINAL", "PRIMARY")
        assert "ORIGINAL" in str(img_type)
        assert "PRIMARY" in str(img_type)


class TestMammogramView:
    def test_constructor(self):
        """Test MammogramView construction."""
        view = MammogramView(Laterality.LEFT, ViewPosition.CC)
        assert view.laterality == Laterality.LEFT
        assert view.view == ViewPosition.CC

    def test_is_standard_mammo_view(self):
        """Test is_standard_mammo_view method."""
        view_cc = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view_mlo = MammogramView(Laterality.RIGHT, ViewPosition.MLO)
        view_ml = MammogramView(Laterality.LEFT, ViewPosition.ML)

        assert view_cc.is_standard_mammo_view()
        assert view_mlo.is_standard_mammo_view()
        assert not view_ml.is_standard_mammo_view()

    def test_is_mlo_like(self):
        """Test is_mlo_like method."""
        view_mlo = MammogramView(Laterality.LEFT, ViewPosition.MLO)
        view_cc = MammogramView(Laterality.LEFT, ViewPosition.CC)

        assert view_mlo.is_mlo_like()
        assert not view_cc.is_mlo_like()

    def test_is_cc_like(self):
        """Test is_cc_like method."""
        view_cc = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view_mlo = MammogramView(Laterality.LEFT, ViewPosition.MLO)

        assert view_cc.is_cc_like()
        assert not view_mlo.is_cc_like()

    def test_equality(self):
        """Test equality comparison."""
        view1 = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view2 = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view3 = MammogramView(Laterality.RIGHT, ViewPosition.CC)

        assert view1 == view2
        assert view1 != view3

    def test_hash(self):
        """Test that views are hashable."""
        view1 = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view2 = MammogramView(Laterality.LEFT, ViewPosition.CC)
        view_set = {view1, view2}
        assert len(view_set) == 1
