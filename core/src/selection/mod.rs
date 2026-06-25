//! Preferred view selection logic
//!
//! Implements mammogram record comparison and view selection algorithms
//! matching the Python dicom-utils behavior.

mod record;
mod views;

pub use record::MammogramRecord;
pub use views::{
    get_preferred_views, get_preferred_views_filtered,
    get_preferred_views_filtered_with_study_mode,
    get_preferred_views_filtered_with_study_mode_and_warnings, get_preferred_views_with_order,
    get_preferred_views_with_order_and_warnings, PreferredViewSelection,
    PreferredViewSelectionWithWarnings, SelectionWarning, StudySelectionMode,
};
