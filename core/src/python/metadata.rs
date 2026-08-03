//! Python wrapper for MammogramMetadata

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::enums::{
    PyDbtObjectKind, PyImageType, PyLaterality, PyMammogramType, PyMammogramView,
    PyMammographyViewModifier, PyViewPosition,
};
use super::orientation::PyConventionalOrientationAssessment;
/// Python wrapper for MammogramMetadata
#[pyclass(name = "MammogramMetadata", module = "mammocat", from_py_object)]
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

    /// DBT object representation
    #[getter]
    fn dbt_object_kind(&self) -> PyDbtObjectKind {
        self.inner.dbt_object_kind.into()
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

    /// Standard CID 4015 view modifiers.
    #[getter]
    fn view_modifiers(&self) -> Vec<PyMammographyViewModifier> {
        self.inner
            .view_modifiers
            .iter()
            .copied()
            .map(Into::into)
            .collect()
    }

    /// Conventional PatientOrientation assessment.
    #[getter]
    fn conventional_orientation(&self) -> PyConventionalOrientationAssessment {
        self.inner.conventional_orientation.clone().into()
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
        self.inner.is_spot_compression()
    }

    /// Whether this is a magnification view
    #[getter]
    fn is_magnified(&self) -> bool {
        self.inner.is_magnified()
    }

    /// Whether this is an implant displaced view
    #[getter]
    fn is_implant_displaced(&self) -> bool {
        self.inner.is_implant_displaced()
    }

    /// Manufacturer name (if available)
    #[getter]
    fn manufacturer(&self) -> Option<String> {
        self.inner.manufacturer.clone()
    }

    /// Manufacturer model name (if available)
    #[getter]
    fn model(&self) -> Option<String> {
        self.inner.model.clone()
    }

    /// Number of frames (for tomosynthesis)
    #[getter]
    fn number_of_frames(&self) -> i32 {
        self.inner.number_of_frames
    }

    /// Pixel spacing in millimeters, when available.
    #[getter]
    fn pixel_spacing(&self, py: Python) -> PyResult<Option<Py<PyDict>>> {
        let Some(pixel_spacing) = self.inner.pixel_spacing else {
            return Ok(None);
        };

        let dict = PyDict::new(py);
        dict.set_item("row", pixel_spacing.row)?;
        dict.set_item("column", pixel_spacing.col)?;
        Ok(Some(dict.unbind()))
    }

    /// DICOM ConcatenationUID, when present
    #[getter]
    fn concatenation_uid(&self) -> Option<String> {
        self.inner.concatenation_uid.clone()
    }

    /// DICOM SOPInstanceUIDOfConcatenationSource, when present
    #[getter]
    fn sop_instance_uid_of_concatenation_source(&self) -> Option<String> {
        self.inner.sop_instance_uid_of_concatenation_source.clone()
    }

    /// Whether this is a secondary capture image
    #[getter]
    fn is_secondary_capture(&self) -> bool {
        self.inner.is_secondary_capture
    }

    /// DICOM Modality (should be "MG" for mammography)
    #[getter]
    fn modality(&self) -> Option<String> {
        self.inner.modality.clone()
    }

    /// DICOM Transfer Syntax UID from file meta information
    #[getter]
    fn transfer_syntax_uid(&self) -> Option<String> {
        self.inner.transfer_syntax_uid.clone()
    }

    /// Human-readable DICOM transfer syntax name
    #[getter]
    fn transfer_syntax_name(&self) -> Option<String> {
        self.inner.transfer_syntax_name.clone()
    }

    /// Derived compression category from the transfer syntax
    #[getter]
    fn compression_type(&self) -> Option<String> {
        self.inner.compression_type.clone()
    }

    /// Returns the mammogram view (laterality + view position)
    fn mammogram_view(&self) -> PyMammogramView {
        self.inner.mammogram_view().into()
    }

    /// Checks if this is a standard mammography view (CC or MLO)
    fn is_standard_view(&self) -> bool {
        self.inner.is_standard_view()
    }

    /// Checks if this belongs to the explicit 2D mammogram group.
    fn is_2d(&self) -> bool {
        self.inner.is_2d()
    }

    /// Convert metadata to dictionary
    pub fn to_dict(&self, py: Python) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item(
            "mammogram_type",
            self.inner.mammogram_type.serialized_name(),
        )?;
        dict.set_item("dbt_object_kind", self.dbt_object_kind().simple_name())?;
        dict.set_item("laterality", self.laterality().simple_name())?;
        dict.set_item("view_position", self.view_position().simple_name())?;
        dict.set_item(
            "view_modifiers",
            self.inner
                .view_modifiers
                .iter()
                .map(|modifier| modifier.simple_name())
                .collect::<Vec<_>>(),
        )?;
        dict.set_item(
            "conventional_orientation",
            self.conventional_orientation().to_dict(py)?,
        )?;
        dict.set_item("image_type", format!("{}", self.inner.image_type))?;
        dict.set_item("is_for_processing", self.is_for_processing())?;
        dict.set_item("has_implant", self.has_implant())?;
        dict.set_item("is_spot_compression", self.is_spot_compression())?;
        dict.set_item("is_magnified", self.is_magnified())?;
        dict.set_item("is_implant_displaced", self.is_implant_displaced())?;
        dict.set_item("manufacturer", self.manufacturer())?;
        dict.set_item("model", self.model())?;
        dict.set_item("number_of_frames", self.number_of_frames())?;
        dict.set_item("pixel_spacing", self.pixel_spacing(py)?)?;
        dict.set_item("concatenation_uid", self.concatenation_uid())?;
        dict.set_item(
            "sop_instance_uid_of_concatenation_source",
            self.sop_instance_uid_of_concatenation_source(),
        )?;
        dict.set_item("is_secondary_capture", self.is_secondary_capture())?;
        dict.set_item("modality", self.modality())?;
        dict.set_item("transfer_syntax_uid", self.transfer_syntax_uid())?;
        dict.set_item("transfer_syntax_name", self.transfer_syntax_name())?;
        dict.set_item("compression_type", self.compression_type())?;
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
