//! Python wrappers for mammocat enums and data structures

use pyo3::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::macros::impl_py_from;
use crate::types::{
    ImageType, Laterality, MammogramType, MammogramView, PhotometricInterpretation,
    PreferenceOrder, ViewPosition,
};

// ============================================================================
// MammogramType
// ============================================================================

#[pyclass(name = "MammogramType", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyMammogramType {
    pub(crate) inner: MammogramType,
}

#[pymethods]
impl PyMammogramType {
    #[classattr]
    const UNKNOWN: Self = Self {
        inner: MammogramType::Unknown,
    };
    #[classattr]
    const TOMO: Self = Self {
        inner: MammogramType::Tomo,
    };
    #[classattr]
    const FFDM: Self = Self {
        inner: MammogramType::Ffdm,
    };
    #[classattr]
    const SYNTH: Self = Self {
        inner: MammogramType::Synth,
    };
    #[classattr]
    const SFM: Self = Self {
        inner: MammogramType::Sfm,
    };

    fn is_unknown(&self) -> bool {
        self.inner.is_unknown()
    }

    pub fn simple_name(&self) -> &'static str {
        self.inner.simple_name()
    }

    fn is_preferred_to(&self, other: &PyMammogramType) -> bool {
        self.inner.is_preferred_to(&other.inner)
    }

    fn __str__(&self) -> String {
        self.inner.simple_name().to_string()
    }

    fn __repr__(&self) -> String {
        format!("MammogramType.{:?}", self.inner)
    }

    fn __eq__(&self, other: &PyMammogramType) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    fn __lt__(&self, other: &PyMammogramType) -> bool {
        self.inner < other.inner
    }

    fn __le__(&self, other: &PyMammogramType) -> bool {
        self.inner <= other.inner
    }

    fn __gt__(&self, other: &PyMammogramType) -> bool {
        self.inner > other.inner
    }

    fn __ge__(&self, other: &PyMammogramType) -> bool {
        self.inner >= other.inner
    }

    #[getter]
    fn value(&self) -> &str {
        self.inner.simple_name()
    }
}

impl_py_from!(PyMammogramType, MammogramType);

// ============================================================================
// Laterality
// ============================================================================

#[pyclass(name = "Laterality", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyLaterality {
    pub(crate) inner: Laterality,
}

#[pymethods]
impl PyLaterality {
    #[classattr]
    const UNKNOWN: Self = Self {
        inner: Laterality::Unknown,
    };
    #[classattr]
    const NONE: Self = Self {
        inner: Laterality::None,
    };
    #[classattr]
    const LEFT: Self = Self {
        inner: Laterality::Left,
    };
    #[classattr]
    const RIGHT: Self = Self {
        inner: Laterality::Right,
    };
    #[classattr]
    const BILATERAL: Self = Self {
        inner: Laterality::Bilateral,
    };

    fn is_unknown(&self) -> bool {
        self.inner.is_unknown()
    }

    fn is_unilateral(&self) -> bool {
        self.inner.is_unilateral()
    }

    fn is_unknown_or_none(&self) -> bool {
        self.inner.is_unknown_or_none()
    }

    fn opposite(&self) -> PyLaterality {
        PyLaterality {
            inner: self.inner.opposite(),
        }
    }

    fn short_str(&self) -> &'static str {
        self.inner.short_str()
    }

    pub fn simple_name(&self) -> &'static str {
        self.inner.simple_name()
    }

    fn __str__(&self) -> String {
        self.inner.simple_name().to_string()
    }

    fn __repr__(&self) -> String {
        format!("Laterality.{:?}", self.inner)
    }

    fn __eq__(&self, other: &PyLaterality) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    #[getter]
    fn value(&self) -> &str {
        self.inner.simple_name()
    }
}

impl_py_from!(PyLaterality, Laterality);

// ============================================================================
// ViewPosition
// ============================================================================

#[pyclass(name = "ViewPosition", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyViewPosition {
    pub(crate) inner: ViewPosition,
}

#[pymethods]
impl PyViewPosition {
    #[classattr]
    const UNKNOWN: Self = Self {
        inner: ViewPosition::Unknown,
    };
    #[classattr]
    const XCCL: Self = Self {
        inner: ViewPosition::Xccl,
    };
    #[classattr]
    const XCCM: Self = Self {
        inner: ViewPosition::Xccm,
    };
    #[classattr]
    const CC: Self = Self {
        inner: ViewPosition::Cc,
    };
    #[classattr]
    const MLO: Self = Self {
        inner: ViewPosition::Mlo,
    };
    #[classattr]
    const ML: Self = Self {
        inner: ViewPosition::Ml,
    };
    #[classattr]
    const LMO: Self = Self {
        inner: ViewPosition::Lmo,
    };
    #[classattr]
    const LM: Self = Self {
        inner: ViewPosition::Lm,
    };
    #[classattr]
    const AT: Self = Self {
        inner: ViewPosition::At,
    };
    #[classattr]
    const CV: Self = Self {
        inner: ViewPosition::Cv,
    };

    fn is_unknown(&self) -> bool {
        self.inner.is_unknown()
    }

    fn is_standard_view(&self) -> bool {
        self.inner.is_standard_view()
    }

    fn is_mlo_like(&self) -> bool {
        self.inner.is_mlo_like()
    }

    fn is_cc_like(&self) -> bool {
        self.inner.is_cc_like()
    }

    fn short_str(&self) -> &'static str {
        self.inner.short_str()
    }

    pub fn simple_name(&self) -> &'static str {
        self.inner.simple_name()
    }

    fn __str__(&self) -> String {
        self.inner.simple_name().to_string()
    }

    fn __repr__(&self) -> String {
        format!("ViewPosition.{:?}", self.inner)
    }

    fn __eq__(&self, other: &PyViewPosition) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    fn __lt__(&self, other: &PyViewPosition) -> bool {
        self.inner < other.inner
    }

    fn __le__(&self, other: &PyViewPosition) -> bool {
        self.inner <= other.inner
    }

    fn __gt__(&self, other: &PyViewPosition) -> bool {
        self.inner > other.inner
    }

    fn __ge__(&self, other: &PyViewPosition) -> bool {
        self.inner >= other.inner
    }

    #[getter]
    fn value(&self) -> &str {
        self.inner.simple_name()
    }
}

impl_py_from!(PyViewPosition, ViewPosition);

// ============================================================================
// PreferenceOrder
// ============================================================================

#[pyclass(name = "PreferenceOrder", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyPreferenceOrder {
    pub(crate) inner: PreferenceOrder,
}

#[pymethods]
impl PyPreferenceOrder {
    #[classattr]
    const DEFAULT: Self = Self {
        inner: PreferenceOrder::Default,
    };
    #[classattr]
    const TOMO_FIRST: Self = Self {
        inner: PreferenceOrder::TomoFirst,
    };

    fn __str__(&self) -> &'static str {
        match self.inner {
            PreferenceOrder::Default => "default",
            PreferenceOrder::TomoFirst => "tomo-first",
        }
    }

    fn __repr__(&self) -> String {
        format!("PreferenceOrder.{:?}", self.inner)
    }

    fn __eq__(&self, other: &PyPreferenceOrder) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    #[getter]
    fn value(&self) -> &str {
        self.__str__()
    }
}

impl From<PreferenceOrder> for PyPreferenceOrder {
    fn from(inner: PreferenceOrder) -> Self {
        Self { inner }
    }
}

// ============================================================================
// PhotometricInterpretation
// ============================================================================

#[pyclass(name = "PhotometricInterpretation", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyPhotometricInterpretation {
    pub(crate) inner: PhotometricInterpretation,
}

#[pymethods]
impl PyPhotometricInterpretation {
    #[classattr]
    const UNKNOWN: Self = Self {
        inner: PhotometricInterpretation::Unknown,
    };
    #[classattr]
    const MONOCHROME1: Self = Self {
        inner: PhotometricInterpretation::Monochrome1,
    };
    #[classattr]
    const MONOCHROME2: Self = Self {
        inner: PhotometricInterpretation::Monochrome2,
    };
    #[classattr]
    const PALETTE_COLOR: Self = Self {
        inner: PhotometricInterpretation::PaletteColor,
    };
    #[classattr]
    const RGB: Self = Self {
        inner: PhotometricInterpretation::Rgb,
    };
    #[classattr]
    const HSV: Self = Self {
        inner: PhotometricInterpretation::Hsv,
    };
    #[classattr]
    const ARGB: Self = Self {
        inner: PhotometricInterpretation::Argb,
    };
    #[classattr]
    const CMYK: Self = Self {
        inner: PhotometricInterpretation::Cmyk,
    };
    #[classattr]
    const YBR_FULL: Self = Self {
        inner: PhotometricInterpretation::YbrFull,
    };
    #[classattr]
    const YBR_FULL_422: Self = Self {
        inner: PhotometricInterpretation::YbrFull422,
    };
    #[classattr]
    const YBR_PARTIAL_422: Self = Self {
        inner: PhotometricInterpretation::YbrPartial422,
    };
    #[classattr]
    const YBR_PARTIAL_420: Self = Self {
        inner: PhotometricInterpretation::YbrPartial420,
    };
    #[classattr]
    const YBR_ICT: Self = Self {
        inner: PhotometricInterpretation::YbrIct,
    };
    #[classattr]
    const YBR_RCT: Self = Self {
        inner: PhotometricInterpretation::YbrRct,
    };

    fn is_monochrome(&self) -> bool {
        self.inner.is_monochrome()
    }

    fn is_inverted(&self) -> bool {
        self.inner.is_inverted()
    }

    fn num_channels(&self) -> usize {
        self.inner.num_channels()
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __repr__(&self) -> String {
        format!("PhotometricInterpretation({})", self.inner)
    }

    fn __eq__(&self, other: &PyPhotometricInterpretation) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    #[getter]
    fn value(&self) -> String {
        format!("{}", self.inner)
    }
}

impl_py_from!(PyPhotometricInterpretation, PhotometricInterpretation);

// ============================================================================
// ImageType
// ============================================================================

#[pyclass(name = "ImageType", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyImageType {
    pub(crate) inner: ImageType,
}

#[pymethods]
impl PyImageType {
    #[new]
    #[pyo3(signature = (pixels, exam, flavor=None, extras=None))]
    fn new(
        pixels: String,
        exam: String,
        flavor: Option<String>,
        extras: Option<Vec<String>>,
    ) -> Self {
        Self {
            inner: ImageType::new(pixels, exam, flavor, extras),
        }
    }

    #[getter]
    fn pixels(&self) -> String {
        self.inner.pixels.clone()
    }

    #[getter]
    fn exam(&self) -> String {
        self.inner.exam.clone()
    }

    #[getter]
    fn flavor(&self) -> Option<String> {
        self.inner.flavor.clone()
    }

    #[getter]
    fn extras(&self) -> Option<Vec<String>> {
        self.inner.extras.clone()
    }

    fn contains(&self, value: &str) -> bool {
        self.inner.contains(value)
    }

    fn is_valid(&self) -> bool {
        self.inner.is_valid()
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __repr__(&self) -> String {
        format!("ImageType({})", self.inner)
    }

    fn __eq__(&self, other: &PyImageType) -> bool {
        self.inner == other.inner
    }
}

impl_py_from!(PyImageType, ImageType);

// ============================================================================
// MammogramView
// ============================================================================

#[pyclass(name = "MammogramView", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyMammogramView {
    pub(crate) inner: MammogramView,
}

#[pymethods]
impl PyMammogramView {
    #[new]
    fn new(laterality: PyLaterality, view: PyViewPosition) -> Self {
        Self {
            inner: MammogramView::new(laterality.inner, view.inner),
        }
    }

    #[getter]
    fn laterality(&self) -> PyLaterality {
        PyLaterality {
            inner: self.inner.laterality,
        }
    }

    #[getter]
    fn view(&self) -> PyViewPosition {
        PyViewPosition {
            inner: self.inner.view,
        }
    }

    fn is_standard_mammo_view(&self) -> bool {
        self.inner.is_standard_mammo_view()
    }

    fn is_mlo_like(&self) -> bool {
        self.inner.is_mlo_like()
    }

    fn is_cc_like(&self) -> bool {
        self.inner.is_cc_like()
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __repr__(&self) -> String {
        format!(
            "MammogramView({}, {})",
            self.inner.laterality, self.inner.view
        )
    }

    fn __eq__(&self, other: &PyMammogramView) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }
}

impl_py_from!(PyMammogramView, MammogramView);
