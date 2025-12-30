//! Python bindings for mammocat
//!
//! This module provides PyO3 bindings enabling Python users to extract
//! mammography metadata from DICOM files.

// Suppress false positive warnings from PyO3 macro expansion
#![allow(clippy::useless_conversion)]

use pyo3::prelude::*;

mod enums;
mod errors;
mod extractor;
mod filter;
#[macro_use]
mod macros;
mod metadata;
mod record;
mod selection;
mod utils;

pub use enums::*;
pub use errors::*;
pub use extractor::*;
pub use filter::*;
pub use metadata::*;
pub use record::*;
pub use selection::*;

/// Python module definition
#[pymodule]
fn _mammocat(py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register exception classes
    m.add(
        "MammocatError",
        py.get_type_bound::<errors::PyMammocatError>(),
    )?;
    m.add("DicomError", py.get_type_bound::<errors::PyDicomError>())?;
    m.add(
        "TagNotFoundError",
        py.get_type_bound::<errors::PyTagNotFoundError>(),
    )?;
    m.add(
        "InvalidValueError",
        py.get_type_bound::<errors::PyInvalidValueError>(),
    )?;
    m.add(
        "ExtractionError",
        py.get_type_bound::<errors::PyExtractionError>(),
    )?;

    // Register enum classes
    m.add_class::<PyMammogramType>()?;
    m.add_class::<PyLaterality>()?;
    m.add_class::<PyViewPosition>()?;
    m.add_class::<PyPreferenceOrder>()?;
    m.add_class::<PyPhotometricInterpretation>()?;

    // Register data structure classes
    m.add_class::<PyImageType>()?;
    m.add_class::<PyMammogramView>()?;
    m.add_class::<PyMammogramMetadata>()?;
    m.add_class::<PyMammogramRecord>()?;
    m.add_class::<PyFilterConfig>()?;

    // Register main API
    m.add_class::<PyMammogramExtractor>()?;

    // Register functions
    m.add_function(wrap_pyfunction!(py_get_preferred_views, m)?)?;
    m.add_function(wrap_pyfunction!(py_get_preferred_views_with_order, m)?)?;
    m.add_function(wrap_pyfunction!(py_get_preferred_views_filtered, m)?)?;

    // Add version
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
