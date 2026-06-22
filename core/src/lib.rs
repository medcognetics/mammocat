pub mod api;
pub mod cli;
pub mod dicom_files;
pub mod error;
pub mod extraction;
pub mod selection;
pub mod types;
pub mod validation;

// Python bindings module (optional)
#[cfg(feature = "python")]
pub mod python;

pub use api::{MammogramExtractor, MammogramMetadata};
pub use cli::report::TextReport;
pub use dicom_files::{collect_dicom_files, is_dicom_file};
pub use error::{MammocatError, Result};
pub use selection::{
    get_preferred_views, get_preferred_views_filtered, get_preferred_views_with_order,
    MammogramRecord,
};
pub use types::*;
pub use validation::{
    validate_dicom_file, validate_directory_path, validate_path, CheckStatus, Severity,
    ValidationMessage, ValidationOptions, ValidationProfile, ValidationReport,
    ValidationRuntimeError, ValidationStatus,
};
