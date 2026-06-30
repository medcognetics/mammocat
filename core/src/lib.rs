pub mod api;
pub mod cli;
pub mod dbt;
pub mod dicom_files;
pub mod error;
pub mod extraction;
pub mod planning;
pub mod selection;
pub mod types;
pub mod validation;

// Python bindings module (optional)
#[cfg(feature = "python")]
pub mod python;

pub use api::{MammogramExtractor, MammogramMetadata};
pub use cli::report::TextReport;
pub use dbt::{
    convert_dbt_study, scan_dbt_study, write_combined_dbt_series, DbtConvertOptions,
    DbtConvertReport, DbtConvertSummary, DbtConvertedSeries, DbtCopiedFile, DbtFileFinding,
    DbtScanOptions, DbtScanReport, DbtScanSummary, DbtSeriesFinding, DbtSkippedFile,
    DbtUnsupportedSeries, BREAST_TOMOSYNTHESIS_SOP_CLASS_UID,
};
pub use dicom_files::{collect_dicom_files, is_dicom_file};
pub use error::{MammocatError, Result};
pub use planning::{
    plan_mammography_collection, DbtCompositionInput, DbtPlan, DbtVolumeCandidate, MammographyPlan,
    MammographyPlanOptions, MammographyPlanSelection, MammographyPlanSummary,
    SourceObjectDiagnostic, ViewSelection, ViewsPlan,
};
pub use selection::{
    get_preferred_views, get_preferred_views_filtered,
    get_preferred_views_filtered_with_study_mode,
    get_preferred_views_filtered_with_study_mode_and_warnings, get_preferred_views_with_order,
    get_preferred_views_with_order_and_warnings, refine_dbt_object_classification,
    refine_dbt_object_classification_with_diagnostics, DbtRefinementDiagnostic,
    DbtRefinementReason, MammogramRecord, PreferredViewSelection,
    PreferredViewSelectionWithWarnings, SelectionWarning, StudySelectionMode,
};
pub use types::*;
pub use validation::{
    validate_dicom_file, validate_directory_path, validate_path, CheckStatus, Severity,
    ValidationMessage, ValidationOptions, ValidationProfile, ValidationReport,
    ValidationRuntimeError, ValidationStatus,
};
