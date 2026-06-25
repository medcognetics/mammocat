//! Python wrappers for DBT scan and conversion APIs.

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde::Serialize;

use super::errors::convert_error;
use super::utils::path_to_pathbuf;

/// Recursively scan a study directory for old-format DBT series.
#[pyfunction]
#[pyo3(name = "scan_dbt_study", signature = (input_dir))]
pub fn py_scan_dbt_study(py: Python, input_dir: &Bound<'_, PyAny>) -> PyResult<PyObject> {
    let input_dir = path_to_pathbuf(input_dir)?;
    let report = crate::scan_dbt_study(input_dir, crate::DbtScanOptions).map_err(convert_error)?;
    report_to_py(py, &report)
}

/// Convert old-format DBT series and copy through other DICOM files.
#[pyfunction]
#[pyo3(name = "convert_dbt_study", signature = (input_dir, output_dir, dry_run=false, force=false))]
pub fn py_convert_dbt_study(
    py: Python,
    input_dir: &Bound<'_, PyAny>,
    output_dir: &Bound<'_, PyAny>,
    dry_run: bool,
    force: bool,
) -> PyResult<PyObject> {
    let input_dir = path_to_pathbuf(input_dir)?;
    let output_dir = path_to_pathbuf(output_dir)?;
    let report = crate::convert_dbt_study(
        input_dir,
        output_dir,
        crate::DbtConvertOptions { dry_run, force },
    )
    .map_err(convert_error)?;
    report_to_py(py, &report)
}

fn report_to_py<T: Serialize>(py: Python, report: &T) -> PyResult<PyObject> {
    let json = serde_json::to_string(report).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("failed to serialize report: {}", e))
    })?;
    let json_module = PyModule::import_bound(py, "json")?;
    Ok(json_module.call_method1("loads", (json,))?.unbind())
}
