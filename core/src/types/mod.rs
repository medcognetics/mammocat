//! Core type definitions for mammography metadata
//!
//! This module provides the fundamental types used throughout the mammocat library:
//! - [`MammogramType`]: Classification of mammogram imaging types (FFDM, TOMO, SYNTH, SFM)
//! - [`Laterality`]: Breast laterality (Left, Right, Bilateral)
//! - [`ViewPosition`]: View positions (CC, MLO, etc.)
//! - [`MammogramView`]: Combined laterality and view position
//! - [`ImageType`]: Decomposed DICOM ImageType field
//! - [`PreferenceOrder`]: Strategies for selecting preferred mammograms
//! - [`FilterConfig`]: Configuration for filtering mammogram records during selection

mod enums;
mod filter;
mod image_type;
mod pixel_spacing;
mod view;

pub use enums::{
    Laterality, MammogramType, PhotometricInterpretation, PreferenceOrder, ViewPosition,
};
pub use filter::FilterConfig;
pub use image_type::ImageType;
pub use pixel_spacing::PixelSpacing;
pub use view::{MammogramView, STANDARD_MAMMO_VIEWS};
