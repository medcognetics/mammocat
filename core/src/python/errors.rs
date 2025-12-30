//! Python exception types for mammocat
//!
//! This module defines Python exception classes that map to Rust error types.

// Suppress warnings from PyO3's create_exception! macro about gil-refs feature
#![allow(unexpected_cfgs)]

use pyo3::{create_exception, exceptions::PyException, prelude::*};

// Base exception
create_exception!(
    mammocat,
    PyMammocatError,
    PyException,
    "Base exception for all mammocat errors"
);

// Specific exceptions
create_exception!(
    mammocat,
    PyDicomError,
    PyMammocatError,
    "DICOM reading or parsing error"
);

create_exception!(
    mammocat,
    PyTagNotFoundError,
    PyMammocatError,
    "Required DICOM tag not found in file"
);

create_exception!(
    mammocat,
    PyInvalidValueError,
    PyMammocatError,
    "Invalid DICOM tag value encountered"
);

create_exception!(
    mammocat,
    PyExtractionError,
    PyMammocatError,
    "Generic metadata extraction error"
);

/// Convert Rust MammocatError to appropriate Python exception
pub fn convert_error(err: crate::error::MammocatError) -> PyErr {
    match err {
        crate::error::MammocatError::DicomError(msg) => PyDicomError::new_err(msg),
        crate::error::MammocatError::TagNotFound(msg) => PyTagNotFoundError::new_err(msg),
        crate::error::MammocatError::InvalidValue(msg) => PyInvalidValueError::new_err(msg),
        crate::error::MammocatError::ExtractionError(msg) => PyExtractionError::new_err(msg),
        crate::error::MammocatError::IoError(e) => {
            PyDicomError::new_err(format!("IO error: {}", e))
        }
    }
}
