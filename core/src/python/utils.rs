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
