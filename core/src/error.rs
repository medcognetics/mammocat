use thiserror::Error;

/// Result type for mammocat operations
pub type Result<T> = std::result::Result<T, MammocatError>;

/// Error types for mammocat operations
#[derive(Error, Debug)]
pub enum MammocatError {
    /// DICOM reading error
    #[error("DICOM error: {0}")]
    DicomError(String),

    /// Tag not found in DICOM file
    #[error("Tag not found: {0}")]
    TagNotFound(String),

    /// Invalid tag value
    #[error("Invalid tag value: {0}")]
    InvalidValue(String),

    /// Generic extraction error
    #[error("Extraction error: {0}")]
    ExtractionError(String),

    /// I/O error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

// Helper conversions
impl From<String> for MammocatError {
    fn from(s: String) -> Self {
        MammocatError::ExtractionError(s)
    }
}

impl From<&str> for MammocatError {
    fn from(s: &str) -> Self {
        MammocatError::ExtractionError(s.to_string())
    }
}

// Convert dicom-object errors
impl From<dicom_object::ReadError> for MammocatError {
    fn from(e: dicom_object::ReadError) -> Self {
        MammocatError::DicomError(format!("{}", e))
    }
}

impl From<dicom_core::value::ConvertValueError> for MammocatError {
    fn from(e: dicom_core::value::ConvertValueError) -> Self {
        MammocatError::InvalidValue(format!("{}", e))
    }
}
