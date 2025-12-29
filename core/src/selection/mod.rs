//! Preferred view selection logic
//!
//! Implements mammogram record comparison and view selection algorithms
//! matching the Python dicom-utils behavior.

mod record;
mod views;

pub use record::MammogramRecord;
pub use views::get_preferred_views;
