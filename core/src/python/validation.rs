//! Python bindings for validation reports.

use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use super::enums::PyPreferenceOrder;
use super::filter::PyFilterConfig;
use super::utils::path_to_pathbuf;
use crate::types::{FilterConfig, PreferenceOrder};
use crate::validation::{
    validate_directory_path, validate_path, ValidationOptions, ValidationProfile,
    ValidationRuntimeError,
};

fn parse_profile(profile: &str) -> PyResult<ValidationProfile> {
    if profile.eq_ignore_ascii_case("selection") {
        Ok(ValidationProfile::Selection)
    } else if profile.eq_ignore_ascii_case("extraction") {
        Ok(ValidationProfile::Extraction)
    } else {
        Err(PyValueError::new_err(format!(
            "Invalid validation profile: {profile}. Expected 'selection' or 'extraction'"
        )))
    }
}

fn runtime_error_to_py(error: ValidationRuntimeError) -> PyErr {
    match error {
        ValidationRuntimeError::InvalidSourcePath { path } => {
            PyFileNotFoundError::new_err(format!("Invalid source path: {}", path.display()))
        }
        error => PyRuntimeError::new_err(error.to_string()),
    }
}

fn report_to_py<'py, T>(py: Python<'py>, report: &T) -> PyResult<Bound<'py, PyAny>>
where
    T: serde::Serialize,
{
    let report_json = serde_json::to_string(report).map_err(|error| {
        PyRuntimeError::new_err(format!("Failed to serialize validation report: {error}"))
    })?;
    py.import_bound("json")?
        .call_method1("loads", (report_json,))
}

#[pyfunction]
#[pyo3(name = "validate_dicom", signature = (path, profile="selection"))]
pub(crate) fn py_validate_dicom<'py>(
    py: Python<'py>,
    path: &Bound<'py, PyAny>,
    profile: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let path = path_to_pathbuf(path)?;
    if !path.is_file() {
        return Err(PyFileNotFoundError::new_err(format!(
            "File not found: {}",
            path.display()
        )));
    }
    let options = ValidationOptions {
        profile: parse_profile(profile)?,
        filter_config: FilterConfig::default(),
        preference_order: PreferenceOrder::Default,
    };
    let report = validate_path(&path, &options).map_err(runtime_error_to_py)?;
    report_to_py(py, &report)
}

#[pyfunction]
#[pyo3(
    name = "validate_directory",
    signature = (path, profile="selection", filter_config=None, preference_order=None)
)]
pub(crate) fn py_validate_directory<'py>(
    py: Python<'py>,
    path: &Bound<'py, PyAny>,
    profile: &str,
    filter_config: Option<PyFilterConfig>,
    preference_order: Option<PyPreferenceOrder>,
) -> PyResult<Bound<'py, PyAny>> {
    let path = path_to_pathbuf(path)?;
    let options = ValidationOptions {
        profile: parse_profile(profile)?,
        filter_config: filter_config.map(|config| config.inner).unwrap_or_default(),
        preference_order: preference_order
            .map(|order| order.inner)
            .unwrap_or(PreferenceOrder::Default),
    };
    let report = validate_directory_path(&path, &options).map_err(runtime_error_to_py)?;
    report_to_py(py, &report)
}

pub(crate) fn register<'py>(m: &Bound<'py, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_validate_dicom, m)?)?;
    m.add_function(wrap_pyfunction!(py_validate_directory, m)?)?;
    Ok(())
}
