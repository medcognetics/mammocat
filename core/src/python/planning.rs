//! Python wrapper for collection-level mammography input planning.

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde::Serialize;

use super::enums::PyPreferenceOrder;
use super::errors::convert_error;
use super::utils::path_to_pathbuf;

/// Plan 2D mammography view and/or DBT inputs from a DICOM directory.
#[pyfunction]
#[pyo3(
    name = "plan_mammography_collection",
    signature = (path, include_2d=true, include_dbt=true, preference_order=None, strict=false)
)]
pub fn py_plan_mammography_collection(
    py: Python,
    path: &Bound<'_, PyAny>,
    include_2d: bool,
    include_dbt: bool,
    preference_order: Option<&PyPreferenceOrder>,
    strict: bool,
) -> PyResult<PyObject> {
    let path = path_to_pathbuf(path)?;
    let options = crate::MammographyPlanOptions {
        selection: crate::MammographyPlanSelection::new(include_2d, include_dbt),
        preference_order: preference_order
            .map(|order| order.inner)
            .unwrap_or(crate::PreferenceOrder::Default),
        study_selection_mode: crate::StudySelectionMode::from_strict(strict),
    };
    let report = crate::plan_mammography_collection(path, options).map_err(convert_error)?;
    report_to_py(py, &report)
}

fn report_to_py<T: Serialize>(py: Python, report: &T) -> PyResult<PyObject> {
    let json = serde_json::to_string(report).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("failed to serialize report: {}", e))
    })?;
    let json_module = PyModule::import_bound(py, "json")?;
    Ok(json_module.call_method1("loads", (json,))?.unbind())
}
