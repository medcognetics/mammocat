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
    get_preferred_views, get_preferred_views_filtered, get_preferred_views_with_order,
    MammogramRecord,
};
pub use types::*;
