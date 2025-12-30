//! Utility functions for Python bindings conversions

use pyo3::prelude::*;
use std::path::PathBuf;

/// Converts a Python path-like object (str or pathlib.Path) to PathBuf
pub fn path_to_pathbuf(path: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    // Try to convert as string first
    if let Ok(s) = path.extract::<String>() {
        return Ok(PathBuf::from(s));
    }

    // Try to call __str__() for pathlib.Path objects
    if let Ok(s) = path.str() {
        let path_str: String = s.extract()?;
        return Ok(PathBuf::from(path_str));
    }

    Err(pyo3::exceptions::PyTypeError::new_err(
        "Path must be a string or path-like object",
    ))
}

/// Converts an Option<String> to Python (None or str)
pub fn option_string_to_py(py: Python, opt: Option<String>) -> PyObject {
    match opt {
        Some(s) => s.into_py(py),
        None => py.None(),
    }
}

/// Converts an Option<u16> to Python (None or int)
pub fn option_u16_to_py(py: Python, opt: Option<u16>) -> PyObject {
    match opt {
        Some(v) => v.into_py(py),
        None => py.None(),
    }
}
