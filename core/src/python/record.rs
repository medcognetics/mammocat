//! Python wrapper for MammogramRecord

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::enums::PyPreferenceOrder;
use super::errors::convert_error;
use super::metadata::PyMammogramMetadata;
use super::utils::{option_string_to_py, option_u16_to_py, path_to_pathbuf};

/// Mammogram record combining file path and extracted metadata
///
/// Used for preferred view selection with comparison logic for
/// determining the best mammogram among multiple options.
#[pyclass(name = "MammogramRecord", module = "mammocat")]
#[derive(Clone)]
pub struct PyMammogramRecord {
    pub(crate) inner: crate::selection::MammogramRecord,
}

#[pymethods]
impl PyMammogramRecord {
    /// Create a record from a DICOM file path
    ///
    /// Only reads DICOM metadata (headers), not pixel data, for optimal performance.
    ///
    /// Args:
    ///     path: Path to the DICOM file (str or pathlib.Path)
    ///
    /// Returns:
    ///     MammogramRecord: Record with extracted metadata
    ///
    /// Raises:
    ///     DicomError: If the file cannot be read or parsed
    ///     ExtractionError: If metadata extraction fails
    ///
    /// Example:
    ///     >>> from mammocat import MammogramRecord
    ///     >>> record = MammogramRecord.from_file("mammogram.dcm")
    ///     >>> print(record.metadata.mammogram_type)
    #[staticmethod]
    fn from_file(path: &Bound<'_, PyAny>) -> PyResult<PyMammogramRecord> {
        let path_buf = path_to_pathbuf(path)?;
        let record =
            crate::selection::MammogramRecord::from_file(path_buf).map_err(convert_error)?;
        Ok(PyMammogramRecord { inner: record })
    }

    /// Path to the DICOM file
    #[getter]
    fn file_path(&self) -> String {
        self.inner.file_path.to_str().unwrap_or("").to_string()
    }

    /// Extracted mammography metadata
    #[getter]
    fn metadata(&self) -> PyMammogramMetadata {
        PyMammogramMetadata {
            inner: self.inner.metadata.clone(),
        }
    }

    /// Study Instance UID (if available)
    #[getter]
    fn study_instance_uid(&self, py: Python) -> PyObject {
        option_string_to_py(py, self.inner.study_instance_uid.clone())
    }

    /// SOP Instance UID (if available)
    #[getter]
    fn sop_instance_uid(&self, py: Python) -> PyObject {
        option_string_to_py(py, self.inner.sop_instance_uid.clone())
    }

    /// Number of rows in image (if available)
    #[getter]
    fn rows(&self, py: Python) -> PyObject {
        option_u16_to_py(py, self.inner.rows)
    }

    /// Number of columns in image (if available)
    #[getter]
    fn columns(&self, py: Python) -> PyObject {
        option_u16_to_py(py, self.inner.columns)
    }

    /// Whether this is an implant displaced view
    #[getter]
    fn is_implant_displaced(&self) -> bool {
        self.inner.is_implant_displaced
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

    /// Compute image area (rows * columns)
    ///
    /// Returns:
    ///     Optional[int]: Image area in pixels, or None if dimensions not available
    fn image_area(&self) -> Option<u32> {
        self.inner.image_area()
    }

    /// Check if this is a spot compression or magnification view
    ///
    /// These views are deprioritized during selection.
    ///
    /// Returns:
    ///     bool: True if either spot compression or magnification
    fn is_spot_or_mag(&self) -> bool {
        self.inner.is_spot_or_mag()
    }

    /// Check if this record is preferred over another (using default preference order)
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Non-spot/mag views beat spot/mag views
    /// 3. Implant displaced beats non-displaced (same study only)
    /// 4. Type preference (FFDM > SYNTH > TOMO > SFM)
    /// 5. Higher resolution beats lower resolution
    /// 6. Fallback to SOP Instance UID comparison
    ///
    /// Args:
    ///     other: Another MammogramRecord to compare against
    ///
    /// Returns:
    ///     bool: True if this record is preferred over the other
    fn is_preferred_to(&self, other: &PyMammogramRecord) -> bool {
        self.inner.is_preferred_to(&other.inner)
    }

    /// Check if this record is preferred over another using a specific preference order
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Non-spot/mag views beat spot/mag views
    /// 3. Implant displaced beats non-displaced (same study only)
    /// 4. Type preference (according to the provided preference order)
    /// 5. Higher resolution beats lower resolution
    /// 6. Fallback to SOP Instance UID comparison
    ///
    /// Args:
    ///     other: Another MammogramRecord to compare against
    ///     preference_order: The preference ordering strategy to use
    ///
    /// Returns:
    ///     bool: True if this record is preferred over the other
    fn is_preferred_to_with_order(
        &self,
        other: &PyMammogramRecord,
        preference_order: &PyPreferenceOrder,
    ) -> bool {
        self.inner
            .is_preferred_to_with_order(&other.inner, preference_order.inner)
    }

    /// Convert record to dictionary
    fn to_dict(&self, py: Python) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new_bound(py);
        dict.set_item("file_path", self.file_path())?;
        dict.set_item("metadata", self.metadata().to_dict(py)?)?;
        dict.set_item("study_instance_uid", self.study_instance_uid(py))?;
        dict.set_item("sop_instance_uid", self.sop_instance_uid(py))?;
        dict.set_item("rows", self.rows(py))?;
        dict.set_item("columns", self.columns(py))?;
        dict.set_item("is_implant_displaced", self.is_implant_displaced())?;
        dict.set_item("is_spot_compression", self.is_spot_compression())?;
        dict.set_item("is_magnified", self.is_magnified())?;
        Ok(dict.unbind())
    }

    fn __repr__(&self) -> String {
        format!(
            "MammogramRecord(file_path={}, type={}, view={})",
            self.file_path(),
            self.inner.metadata.mammogram_type,
            self.inner.metadata.view_position
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

impl From<crate::selection::MammogramRecord> for PyMammogramRecord {
    fn from(inner: crate::selection::MammogramRecord) -> Self {
        Self { inner }
    }
}
