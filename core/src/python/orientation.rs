//! Python bindings for conventional mammography orientation assessment.

use dicom_object::OpenFileOptions;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::extraction::ConventionalOrientationAssessment;

use super::utils::path_to_pathbuf;

/// Python representation of a conventional orientation assessment.
#[pyclass(name = "ConventionalOrientationAssessment", module = "mammocat")]
#[derive(Clone)]
pub struct PyConventionalOrientationAssessment {
    pub(crate) inner: ConventionalOrientationAssessment,
}

#[pymethods]
impl PyConventionalOrientationAssessment {
    #[getter]
    fn status(&self) -> &'static str {
        self.inner.status.as_str()
    }

    #[getter]
    fn expected_components(&self) -> Option<Vec<String>> {
        self.inner.expected_components.clone()
    }

    #[getter]
    fn observed_components(&self) -> Option<Vec<String>> {
        self.inner.observed_components.clone()
    }

    #[getter]
    fn horizontal_flip_required(&self) -> Option<bool> {
        self.inner.horizontal_flip_required
    }

    #[getter]
    fn vertical_flip_required(&self) -> Option<bool> {
        self.inner.vertical_flip_required
    }

    pub fn to_dict(&self, py: Python) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new_bound(py);
        dict.set_item("status", self.status())?;
        dict.set_item("expected_components", self.expected_components())?;
        dict.set_item("observed_components", self.observed_components())?;
        dict.set_item("horizontal_flip_required", self.horizontal_flip_required())?;
        dict.set_item("vertical_flip_required", self.vertical_flip_required())?;
        Ok(dict.unbind())
    }

    fn __repr__(&self) -> String {
        format!(
            "ConventionalOrientationAssessment(status={})",
            self.inner.status
        )
    }
}

impl From<ConventionalOrientationAssessment> for PyConventionalOrientationAssessment {
    fn from(inner: ConventionalOrientationAssessment) -> Self {
        Self { inner }
    }
}

/// Assess PatientOrientation in any readable DICOM file.
#[pyfunction(name = "assess_conventional_orientation")]
pub fn py_assess_conventional_orientation(
    path: &Bound<'_, PyAny>,
) -> PyResult<PyConventionalOrientationAssessment> {
    let path = path_to_pathbuf(path)?;
    let dcm = OpenFileOptions::new()
        .read_until(crate::extraction::tags::PIXEL_DATA_TAG)
        .open_file(&path)
        .map_err(|error| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to open DICOM file {}: {error}",
                path.display()
            ))
        })?;

    Ok(crate::assess_conventional_orientation(&dcm).into())
}
