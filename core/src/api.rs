use crate::error::Result;
use crate::extraction::tags::{
    get_int_value, get_string_value, BREAST_IMPLANT_PRESENT, MANUFACTURER, MANUFACTURER_MODEL_NAME,
    NUMBER_OF_FRAMES, PRESENTATION_INTENT_TYPE,
};
use crate::extraction::{
    extract_image_type, extract_laterality, extract_mammogram_type, extract_view_position,
    is_implant_displaced, is_magnified, is_spot_compression,
};
use crate::types::{ImageType, Laterality, MammogramType, MammogramView, ViewPosition};
use dicom_object::InMemDicomObject;

/// Main extractor for mammography metadata
///
/// Provides a high-level API for extracting all relevant mammography
/// metadata from a DICOM file.
///
/// # Example
///
/// ```
/// use mammocat_core::MammogramExtractor;
/// use dicom_object::InMemDicomObject;
/// use dicom_core::{DataElement, PrimitiveValue, VR, Tag};
///
/// // Create a minimal mammography DICOM object for testing
/// let mut dcm = InMemDicomObject::new_empty();
///
/// // Add required tags
/// dcm.put(DataElement::new(
///     Tag(0x0008, 0x0060), // Modality
///     VR::CS,
///     PrimitiveValue::from("MG"),
/// ));
/// dcm.put(DataElement::new(
///     Tag(0x0008, 0x0008), // ImageType
///     VR::CS,
///     PrimitiveValue::Strs(vec!["ORIGINAL".to_string(), "PRIMARY".to_string()].into()),
/// ));
/// dcm.put(DataElement::new(
///     Tag(0x0020, 0x0062), // ImageLaterality
///     VR::CS,
///     PrimitiveValue::from("L"),
/// ));
/// dcm.put(DataElement::new(
///     Tag(0x0018, 0x5101), // ViewPosition
///     VR::CS,
///     PrimitiveValue::from("MLO"),
/// ));
///
/// // Extract metadata
/// let metadata = MammogramExtractor::extract(&dcm).unwrap();
///
/// // Verify extracted values
/// assert_eq!(metadata.mammogram_type.to_string(), "ffdm");
/// assert_eq!(metadata.laterality.to_string(), "left");
/// assert_eq!(metadata.view_position.to_string(), "mlo");
/// ```
pub struct MammogramExtractor;

impl MammogramExtractor {
    /// Extracts all mammography metadata from a DICOM file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The DICOM file has an invalid modality (not "MG")
    /// - Required tags cannot be read
    pub fn extract(dcm: &InMemDicomObject) -> Result<MammogramMetadata> {
        Self::extract_with_options(dcm, false)
    }

    /// Extracts metadata with optional SFM flag
    ///
    /// The `is_sfm` flag manually indicates if the mammogram is SFM
    /// instead of FFDM, which affects type classification.
    pub fn extract_with_options(dcm: &InMemDicomObject, is_sfm: bool) -> Result<MammogramMetadata> {
        Ok(MammogramMetadata {
            mammogram_type: extract_mammogram_type(dcm, is_sfm)?,
            laterality: extract_laterality(dcm)?,
            view_position: extract_view_position(dcm)?,
            image_type: extract_image_type(dcm),
            is_for_processing: Self::extract_for_processing(dcm),
            has_implant: Self::extract_implant_status(dcm),
            is_spot_compression: is_spot_compression(dcm),
            is_magnified: is_magnified(dcm),
            is_implant_displaced: is_implant_displaced(dcm),
            manufacturer: get_string_value(dcm, MANUFACTURER),
            model: get_string_value(dcm, MANUFACTURER_MODEL_NAME),
            number_of_frames: get_int_value(dcm, NUMBER_OF_FRAMES).unwrap_or(1),
        })
    }

    /// Extracts "FOR PROCESSING" status
    fn extract_for_processing(dcm: &InMemDicomObject) -> bool {
        get_string_value(dcm, PRESENTATION_INTENT_TYPE)
            .map(|s| s.to_lowercase() == "for processing")
            .unwrap_or(false)
    }

    /// Extracts breast implant status
    fn extract_implant_status(dcm: &InMemDicomObject) -> bool {
        get_string_value(dcm, BREAST_IMPLANT_PRESENT)
            .map(|s| s.to_uppercase() == "YES")
            .unwrap_or(false)
    }
}

/// Extracted mammography metadata
///
/// Contains all the key metadata fields extracted from a mammography DICOM file.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct MammogramMetadata {
    /// Mammogram type (TOMO, FFDM, SYNTH, SFM)
    pub mammogram_type: MammogramType,

    /// Laterality (Left, Right, Bilateral)
    pub laterality: Laterality,

    /// View position (CC, MLO, etc.)
    pub view_position: ViewPosition,

    /// Parsed ImageType field
    pub image_type: ImageType,

    /// Whether this is marked "FOR PROCESSING"
    pub is_for_processing: bool,

    /// Whether breast implant is present
    pub has_implant: bool,

    /// Whether this is a spot compression view
    pub is_spot_compression: bool,

    /// Whether this is a magnification view
    pub is_magnified: bool,

    /// Whether this is an implant displaced view
    pub is_implant_displaced: bool,

    /// Manufacturer name
    pub manufacturer: Option<String>,

    /// Manufacturer model name
    pub model: Option<String>,

    /// Number of frames (for DBT/tomosynthesis)
    pub number_of_frames: i32,
}

impl MammogramMetadata {
    /// Returns the mammogram view (laterality + view position)
    pub fn mammogram_view(&self) -> MammogramView {
        MammogramView::new(self.laterality, self.view_position)
    }

    /// Checks if this is a standard mammography view (CC or MLO)
    pub fn is_standard_view(&self) -> bool {
        self.view_position.is_standard_view()
    }

    /// Checks if this is a 2D mammogram (not tomosynthesis)
    pub fn is_2d(&self) -> bool {
        !matches!(self.mammogram_type, MammogramType::Tomo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mammogram_metadata_view() {
        let metadata = MammogramMetadata {
            mammogram_type: MammogramType::Ffdm,
            laterality: Laterality::Left,
            view_position: ViewPosition::Cc,
            image_type: ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            is_spot_compression: false,
            is_magnified: false,
            is_implant_displaced: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 1,
        };

        let view = metadata.mammogram_view();
        assert_eq!(view.laterality, Laterality::Left);
        assert_eq!(view.view, ViewPosition::Cc);
        assert!(metadata.is_standard_view());
        assert!(metadata.is_2d());
    }

    #[test]
    fn test_mammogram_metadata_tomo() {
        let metadata = MammogramMetadata {
            mammogram_type: MammogramType::Tomo,
            laterality: Laterality::Right,
            view_position: ViewPosition::Mlo,
            image_type: ImageType::new("DERIVED".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            is_spot_compression: false,
            is_magnified: false,
            is_implant_displaced: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 50,
        };

        assert!(!metadata.is_2d());
    }
}
