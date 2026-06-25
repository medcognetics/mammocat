pub mod api;
pub mod cli;
pub mod error;
pub mod extraction;
pub mod selection;
pub mod types;

// Python bindings module (optional)
#[cfg(feature = "python")]
pub mod python;

pub use api::{MammogramExtractor, MammogramMetadata};
pub use cli::report::TextReport;
pub use error::{MammocatError, Result};
pub use selection::{
    get_preferred_views, get_preferred_views_filtered,
    get_preferred_views_filtered_with_study_mode,
    get_preferred_views_filtered_with_study_mode_and_warnings, get_preferred_views_with_order,
    get_preferred_views_with_order_and_warnings, MammogramRecord, PreferredViewSelection,
    PreferredViewSelectionWithWarnings, SelectionWarning, StudySelectionMode,
};
pub use types::*;
