//! Python wrapper for MammogramMetadata

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::enums::{PyImageType, PyLaterality, PyMammogramType, PyMammogramView, PyViewPosition};
use super::utils::option_string_to_py;

/// Python wrapper for MammogramMetadata
#[pyclass(name = "MammogramMetadata", module = "mammocat")]
#[derive(Clone)]
pub struct PyMammogramMetadata {
    pub(crate) inner: crate::api::MammogramMetadata,
}

#[pymethods]
impl PyMammogramMetadata {
    /// Mammogram type classification
    #[getter]
    fn mammogram_type(&self) -> PyMammogramType {
        self.inner.mammogram_type.into()
    }

    /// Laterality (left/right/bilateral)
    #[getter]
    fn laterality(&self) -> PyLaterality {
        self.inner.laterality.into()
    }

    /// View position (CC, MLO, etc.)
    #[getter]
    fn view_position(&self) -> PyViewPosition {
        self.inner.view_position.into()
    }

    /// Parsed ImageType field
    #[getter]
    fn image_type(&self) -> PyImageType {
        self.inner.image_type.clone().into()
    }

    /// Whether marked as "FOR PROCESSING"
    #[getter]
    fn is_for_processing(&self) -> bool {
        self.inner.is_for_processing
    }

    /// Whether breast implant is present
    #[getter]
    fn has_implant(&self) -> bool {
        self.inner.has_implant
    }

    /// Whether this is a spot compression view
    #[getter]
    fn is_spot_compression(&self) -> bool {
        self.inner.is_spot_compression
    }

    /// Whether this is a magnification view
    #[getter]
    fn is_magnified(&self) -> bool {
        self.inner.is_magnified
    }

    /// Whether this is an implant displaced view
    #[getter]
    fn is_implant_displaced(&self) -> bool {
        self.inner.is_implant_displaced
    }

    /// Manufacturer name (if available)
    #[getter]
    fn manufacturer(&self, py: Python) -> PyObject {
        option_string_to_py(py, self.inner.manufacturer.clone())
    }

    /// Manufacturer model name (if available)
    #[getter]
    fn model(&self, py: Python) -> PyObject {
        option_string_to_py(py, self.inner.model.clone())
    }

    /// Number of frames (for tomosynthesis)
    #[getter]
    fn number_of_frames(&self) -> i32 {
        self.inner.number_of_frames
    }

    /// Whether this is a secondary capture image
    #[getter]
    fn is_secondary_capture(&self) -> bool {
        self.inner.is_secondary_capture
    }

    /// DICOM Modality (should be "MG" for mammography)
    #[getter]
    fn modality(&self, py: Python) -> PyObject {
        option_string_to_py(py, self.inner.modality.clone())
    }

    /// Returns the mammogram view (laterality + view position)
    fn mammogram_view(&self) -> PyMammogramView {
        self.inner.mammogram_view().into()
    }

    /// Checks if this is a standard mammography view (CC or MLO)
    fn is_standard_view(&self) -> bool {
        self.inner.is_standard_view()
    }

    /// Checks if this is a 2D mammogram (not tomosynthesis)
    fn is_2d(&self) -> bool {
        self.inner.is_2d()
    }

    /// Convert metadata to dictionary
    pub fn to_dict(&self, py: Python) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new_bound(py);
        dict.set_item("mammogram_type", self.mammogram_type().simple_name())?;
        dict.set_item("laterality", self.laterality().simple_name())?;
        dict.set_item("view_position", self.view_position().simple_name())?;
        dict.set_item("image_type", format!("{}", self.inner.image_type))?;
        dict.set_item("is_for_processing", self.is_for_processing())?;
        dict.set_item("has_implant", self.has_implant())?;
        dict.set_item("is_spot_compression", self.is_spot_compression())?;
        dict.set_item("is_magnified", self.is_magnified())?;
        dict.set_item("is_implant_displaced", self.is_implant_displaced())?;
        dict.set_item("manufacturer", self.manufacturer(py))?;
        dict.set_item("model", self.model(py))?;
        dict.set_item("number_of_frames", self.number_of_frames())?;
        dict.set_item("is_secondary_capture", self.is_secondary_capture())?;
        dict.set_item("modality", self.modality(py))?;
        Ok(dict.unbind())
    }

    fn __repr__(&self) -> String {
        format!(
            "MammogramMetadata(type={}, laterality={}, view={}, frames={})",
            self.inner.mammogram_type,
            self.inner.laterality,
            self.inner.view_position,
            self.inner.number_of_frames
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

impl From<crate::api::MammogramMetadata> for PyMammogramMetadata {
    fn from(inner: crate::api::MammogramMetadata) -> Self {
        Self { inner }
    }
}
