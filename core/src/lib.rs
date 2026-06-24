pub mod api;
pub mod cli;
pub mod dbt;
pub mod error;
pub mod extraction;
pub mod selection;
pub mod types;

// Python bindings module (optional)
#[cfg(feature = "python")]
pub mod python;

pub use api::{MammogramExtractor, MammogramMetadata};
pub use cli::report::TextReport;
pub use dbt::{
    convert_dbt_study, scan_dbt_study, DbtConvertOptions, DbtConvertReport, DbtConvertSummary,
    DbtConvertedSeries, DbtCopiedFile, DbtFileFinding, DbtScanOptions, DbtScanReport,
    DbtScanSummary, DbtSeriesFinding, DbtSkippedFile, DbtUnsupportedSeries,
    BREAST_TOMOSYNTHESIS_SOP_CLASS_UID,
};
pub use error::{MammocatError, Result};
pub use selection::{
    get_preferred_views, get_preferred_views_filtered, get_preferred_views_with_order,
    MammogramRecord,
};
pub use types::*;
