//! DICOM metadata extraction algorithms
//!
//! This module contains the classification and extraction logic for mammography
//! metadata, implementing the same algorithms as the Python dicom-utils library.
//!
//! - [`laterality`]: Laterality extraction with fallback hierarchy
//! - [`mammo_type`]: Mammogram type classification (TOMO/FFDM/SYNTH/SFM)
//! - [`view_position`]: View position parsing from multiple DICOM fields
//! - [`view_modifiers`]: Spot compression, magnification, and implant displaced detection
//! - [`tags`]: DICOM tag constants and helper functions

pub mod laterality;
pub mod mammo_type;
pub mod tags;
pub mod view_modifiers;
pub mod view_position;

pub use laterality::extract_laterality;
pub use mammo_type::{extract_image_type, extract_mammogram_type};
pub use tags::*;
pub use view_modifiers::{
    extract_view_modifier_meanings, is_implant_displaced, is_magnified, is_spot_compression,
};
pub use view_position::{extract_view_position, from_str as parse_view_position};
