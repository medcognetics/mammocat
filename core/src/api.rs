use crate::error::Result;
use crate::extraction::mammo_type::extract_mammogram_type_impl;
use crate::extraction::tags::{
    get_int_value, get_string_value, BREAST_IMPLANT_PRESENT, CONCATENATION_UID,
    IMAGER_PIXEL_SPACING, MANUFACTURER, MANUFACTURER_MODEL_NAME, MODALITY, NUMBER_OF_FRAMES,
    PIXEL_SPACING, PRESENTATION_INTENT_TYPE, SOP_CLASS_UID,
    SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
};
use crate::extraction::{
    extract_dbt_object_kind, extract_image_type, extract_laterality, extract_view_descriptor,
};
use crate::types::{
    DbtObjectKind, ImageType, Laterality, MammogramType, MammogramView, MammographyViewModifier,
    PixelSpacing, ViewPosition,
};
use dicom::transfer_syntax::{TransferSyntaxIndex, TransferSyntaxRegistry};
use dicom_object::{FileDicomObject, InMemDicomObject};

const UNKNOWN_TRANSFER_SYNTAX: &str = "unknown transfer syntax";

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

    /// Extracts metadata from a full DICOM file object, including file meta.
    ///
    /// This is the preferred path when the source is an on-disk DICOM file
    /// because it preserves transfer syntax and compression information.
    pub fn extract_file(dcm: &FileDicomObject<InMemDicomObject>) -> Result<MammogramMetadata> {
        Self::extract_file_with_options(dcm, false)
    }

    /// Extracts metadata with optional SFM flag
    ///
    /// The `is_sfm` flag manually indicates if the mammogram is SFM
    /// instead of FFDM, which affects type classification.
    pub fn extract_with_options(dcm: &InMemDicomObject, is_sfm: bool) -> Result<MammogramMetadata> {
        Self::extract_with_options_and_modality_policy(dcm, is_sfm, false)
    }

    /// Extracts metadata with optional SFM flag and configurable modality strictness.
    pub(crate) fn extract_with_options_and_modality_policy(
        dcm: &InMemDicomObject,
        is_sfm: bool,
        ignore_modality: bool,
    ) -> Result<MammogramMetadata> {
        let mammogram_type = extract_mammogram_type_impl(dcm, is_sfm, ignore_modality)?;
        let view = extract_view_descriptor(dcm);
        Ok(MammogramMetadata {
            mammogram_type,
            dbt_object_kind: extract_dbt_object_kind(dcm, mammogram_type),
            laterality: extract_laterality(dcm)?,
            view_position: view.view_position,
            view_modifiers: view.modifiers,
            image_type: extract_image_type(dcm),
            is_for_processing: Self::extract_for_processing(dcm),
            has_implant: Self::extract_implant_status(dcm),
            manufacturer: get_string_value(dcm, MANUFACTURER),
            model: get_string_value(dcm, MANUFACTURER_MODEL_NAME),
            number_of_frames: get_int_value(dcm, NUMBER_OF_FRAMES).unwrap_or(1),
            pixel_spacing: Self::extract_pixel_spacing(dcm),
            concatenation_uid: get_string_value(dcm, CONCATENATION_UID),
            sop_instance_uid_of_concatenation_source: get_string_value(
                dcm,
                SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
            ),
            is_secondary_capture: Self::extract_secondary_capture(dcm),
            modality: Self::extract_modality(dcm),
            transfer_syntax_uid: None,
            transfer_syntax_name: None,
            compression_type: None,
        })
    }

    /// Extracts metadata from a full DICOM file object with optional SFM flag.
    pub fn extract_file_with_options(
        dcm: &FileDicomObject<InMemDicomObject>,
        is_sfm: bool,
    ) -> Result<MammogramMetadata> {
        Self::extract_file_with_options_and_modality_policy(dcm, is_sfm, false)
    }

    /// Extracts metadata from a full DICOM file object with configurable modality strictness.
    pub(crate) fn extract_file_with_options_and_modality_policy(
        dcm: &FileDicomObject<InMemDicomObject>,
        is_sfm: bool,
        ignore_modality: bool,
    ) -> Result<MammogramMetadata> {
        let mut metadata =
            Self::extract_with_options_and_modality_policy(dcm, is_sfm, ignore_modality)?;
        if let Some(transfer_syntax) = resolve_transfer_syntax_metadata(&dcm.meta().transfer_syntax)
        {
            metadata.transfer_syntax_uid = Some(transfer_syntax.uid);
            metadata.transfer_syntax_name = Some(transfer_syntax.name);
            metadata.compression_type = Some(transfer_syntax.compression_type);
        }
        Ok(metadata)
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

    /// Extracts secondary capture status
    ///
    /// Checks if SOP Class UID indicates a secondary capture image.
    /// Secondary Capture SOP Class UID: 1.2.840.10008.5.1.4.1.1.7
    /// Multi-frame variants: .7.1, .7.2, .7.3, .7.4
    fn extract_secondary_capture(dcm: &InMemDicomObject) -> bool {
        get_string_value(dcm, SOP_CLASS_UID)
            .map(|uid| uid.starts_with("1.2.840.10008.5.1.4.1.1.7"))
            .unwrap_or(false)
    }

    /// Extracts modality
    ///
    /// Returns the DICOM Modality tag value (should be "MG" for mammography)
    fn extract_modality(dcm: &InMemDicomObject) -> Option<String> {
        get_string_value(dcm, MODALITY)
    }

    /// Extracts pixel spacing from PixelSpacing with ImagerPixelSpacing fallback.
    fn extract_pixel_spacing(dcm: &InMemDicomObject) -> Option<PixelSpacing> {
        get_string_value(dcm, PIXEL_SPACING)
            .or_else(|| get_string_value(dcm, IMAGER_PIXEL_SPACING))
            .and_then(|value| PixelSpacing::parse(&value).ok())
    }
}

/// Resolved transfer syntax metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSyntaxMetadata {
    pub uid: String,
    pub name: String,
    pub compression_type: String,
}

/// Resolve transfer syntax UID, display name, and compression category.
pub fn resolve_transfer_syntax_metadata(uid: &str) -> Option<TransferSyntaxMetadata> {
    let uid = normalize_transfer_syntax_uid(uid)?;
    let name = TransferSyntaxRegistry
        .get(&uid)
        .map(|syntax| syntax.name().to_string())
        .unwrap_or_else(|| UNKNOWN_TRANSFER_SYNTAX.to_string());
    let compression_type = compression_type_for_transfer_syntax(&uid, &name).to_string();
    Some(TransferSyntaxMetadata {
        uid,
        name,
        compression_type,
    })
}

fn normalize_transfer_syntax_uid(uid: &str) -> Option<String> {
    let uid = uid.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if uid.is_empty() {
        None
    } else {
        Some(uid.to_string())
    }
}

fn compression_type_for_transfer_syntax(uid: &str, name: &str) -> &'static str {
    match uid {
        "1.2.840.10008.1.2"
        | "1.2.840.10008.1.2.1"
        | "1.2.840.10008.1.2.2"
        | "1.2.840.10008.1.2.7.1"
        | "1.2.840.10008.1.2.7.2"
        | "1.2.840.10008.1.2.7.3" => "uncompressed",
        "1.2.840.10008.1.2.1.98" => "encapsulated_uncompressed",
        "1.2.840.10008.1.2.1.99"
        | "1.2.840.10008.1.2.4.95"
        | "1.2.840.10008.1.2.4.205"
        | "1.2.840.10008.1.2.8.1" => "deflate",
        "1.2.840.10008.1.2.5" => "rle_lossless",
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.52"
        | "1.2.840.10008.1.2.4.53"
        | "1.2.840.10008.1.2.4.54"
        | "1.2.840.10008.1.2.4.55"
        | "1.2.840.10008.1.2.4.56" => "jpeg_lossy",
        "1.2.840.10008.1.2.4.57" | "1.2.840.10008.1.2.4.70" => "jpeg_lossless",
        "1.2.840.10008.1.2.4.80" => "jpeg_ls_lossless",
        "1.2.840.10008.1.2.4.81" => "jpeg_ls_lossy",
        "1.2.840.10008.1.2.4.90"
        | "1.2.840.10008.1.2.4.92"
        | "1.2.840.10008.1.2.4.201"
        | "1.2.840.10008.1.2.4.202" => "jpeg2000_lossless",
        "1.2.840.10008.1.2.4.91" | "1.2.840.10008.1.2.4.93" | "1.2.840.10008.1.2.4.203" => {
            "jpeg2000"
        }
        "1.2.840.10008.1.2.4.110" => "jpeg_xl_lossless",
        "1.2.840.10008.1.2.4.111" => "jpeg_xl_recompression",
        "1.2.840.10008.1.2.4.112" => "jpeg_xl",
        _ => compression_type_from_name(name),
    }
}

fn compression_type_from_name(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("jpeg-ls") && name.contains("lossless") && !name.contains("near-lossless") {
        "jpeg_ls_lossless"
    } else if name.contains("jpeg-ls") {
        "jpeg_ls"
    } else if name.contains("jpeg 2000") && name.contains("lossless only") {
        "jpeg2000_lossless"
    } else if name.contains("jpeg 2000") {
        "jpeg2000"
    } else if name.contains("jpeg xl") && name.contains("lossless") {
        "jpeg_xl_lossless"
    } else if name.contains("jpeg xl") {
        "jpeg_xl"
    } else if name.contains("jpeg") && name.contains("lossless") {
        "jpeg_lossless"
    } else if name.contains("jpeg") {
        "jpeg"
    } else if name.contains("rle") {
        "rle_lossless"
    } else if name.contains("deflat") {
        "deflate"
    } else if name.contains("mpeg2") {
        "mpeg2"
    } else if name.contains("mpeg-4") || name.contains("h.264") {
        "mpeg4_avc"
    } else if name.contains("hevc") || name.contains("h.265") {
        "hevc"
    } else if name.contains("uncompressed") {
        "uncompressed"
    } else {
        "unknown"
    }
}

/// Extracted mammography metadata
///
/// Contains all the key metadata fields extracted from a mammography DICOM file.
#[derive(Debug, Clone, PartialEq)]
pub struct MammogramMetadata {
    /// Mammogram type (TOMO, FFDM, SYNTH, SFM, or UNKNOWN)
    pub mammogram_type: MammogramType,

    /// DBT object representation (volume, slice, unknown, or none)
    pub dbt_object_kind: DbtObjectKind,

    /// Laterality (Left, Right, Bilateral)
    pub laterality: Laterality,

    /// View position (CC, MLO, etc.)
    pub view_position: ViewPosition,

    /// Standard CID 4015 view modifiers.
    pub view_modifiers: std::collections::BTreeSet<MammographyViewModifier>,

    /// Parsed ImageType field
    pub image_type: ImageType,

    /// Whether this is marked "FOR PROCESSING"
    pub is_for_processing: bool,

    /// Whether breast implant is present
    pub has_implant: bool,

    /// Manufacturer name
    pub manufacturer: Option<String>,

    /// Manufacturer model name
    pub model: Option<String>,

    /// Number of frames (for DBT/tomosynthesis)
    pub number_of_frames: i32,

    /// Physical pixel spacing in millimeters, when available.
    pub pixel_spacing: Option<PixelSpacing>,

    /// DICOM ConcatenationUID, when present
    pub concatenation_uid: Option<String>,

    /// SOPInstanceUIDOfConcatenationSource, when present
    pub sop_instance_uid_of_concatenation_source: Option<String>,

    /// Whether this is a secondary capture image
    pub is_secondary_capture: bool,

    /// DICOM Modality (should be "MG" for mammography)
    pub modality: Option<String>,

    /// DICOM Transfer Syntax UID from file meta information
    pub transfer_syntax_uid: Option<String>,

    /// Human-readable DICOM transfer syntax name
    pub transfer_syntax_name: Option<String>,

    /// Derived compression category from the transfer syntax
    pub compression_type: Option<String>,
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

    /// Checks if this belongs to the explicit 2D mammogram group.
    pub fn is_2d(&self) -> bool {
        self.mammogram_type.is_2d_group()
    }

    /// Whether this is a spot compression view.
    pub fn is_spot_compression(&self) -> bool {
        self.view_modifiers
            .contains(&MammographyViewModifier::SpotCompression)
    }

    /// Whether this is a magnification view.
    pub fn is_magnified(&self) -> bool {
        self.view_modifiers
            .contains(&MammographyViewModifier::Magnification)
    }

    /// Whether this is an implant displaced view.
    pub fn is_implant_displaced(&self) -> bool {
        self.view_modifiers
            .contains(&MammographyViewModifier::ImplantDisplaced)
    }
}

#[cfg(feature = "json")]
impl serde::Serialize for MammogramMetadata {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("MammogramMetadata", 22)?;
        state.serialize_field("mammogram_type", &self.mammogram_type)?;
        state.serialize_field("dbt_object_kind", &self.dbt_object_kind)?;
        state.serialize_field("laterality", &self.laterality)?;
        state.serialize_field("view_position", &self.view_position)?;
        state.serialize_field("view_modifiers", &self.view_modifiers)?;
        state.serialize_field("image_type", &self.image_type)?;
        state.serialize_field("is_for_processing", &self.is_for_processing)?;
        state.serialize_field("has_implant", &self.has_implant)?;
        state.serialize_field("is_spot_compression", &self.is_spot_compression())?;
        state.serialize_field("is_magnified", &self.is_magnified())?;
        state.serialize_field("is_implant_displaced", &self.is_implant_displaced())?;
        state.serialize_field("manufacturer", &self.manufacturer)?;
        state.serialize_field("model", &self.model)?;
        state.serialize_field("number_of_frames", &self.number_of_frames)?;
        state.serialize_field("pixel_spacing", &self.pixel_spacing)?;
        state.serialize_field("concatenation_uid", &self.concatenation_uid)?;
        state.serialize_field(
            "sop_instance_uid_of_concatenation_source",
            &self.sop_instance_uid_of_concatenation_source,
        )?;
        state.serialize_field("is_secondary_capture", &self.is_secondary_capture)?;
        state.serialize_field("modality", &self.modality)?;
        state.serialize_field("transfer_syntax_uid", &self.transfer_syntax_uid)?;
        state.serialize_field("transfer_syntax_name", &self.transfer_syntax_name)?;
        state.serialize_field("compression_type", &self.compression_type)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
    use dicom_object::InMemDicomObject;

    fn minimal_mammo_dicom() -> InMemDicomObject {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            Tag(0x0008, 0x0060),
            VR::CS,
            PrimitiveValue::from("MG"),
        ));
        dcm.put(DataElement::new(
            Tag(0x0008, 0x0008),
            VR::CS,
            PrimitiveValue::Strs(vec!["ORIGINAL".to_string(), "PRIMARY".to_string()].into()),
        ));
        dcm.put(DataElement::new(
            Tag(0x0020, 0x0062),
            VR::CS,
            PrimitiveValue::from("L"),
        ));
        dcm.put(DataElement::new(
            Tag(0x0018, 0x5101),
            VR::CS,
            PrimitiveValue::from("MLO"),
        ));
        dcm
    }

    #[test]
    fn test_mammogram_metadata_view() {
        let metadata = MammogramMetadata {
            mammogram_type: MammogramType::Ffdm,
            dbt_object_kind: DbtObjectKind::None,
            laterality: Laterality::Left,
            view_position: ViewPosition::Cc,
            view_modifiers: Default::default(),
            image_type: ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 1,
            pixel_spacing: None,
            concatenation_uid: None,
            sop_instance_uid_of_concatenation_source: None,
            is_secondary_capture: false,
            modality: Some("MG".to_string()),
            transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
            transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
            compression_type: Some("uncompressed".to_string()),
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
            dbt_object_kind: DbtObjectKind::Volume,
            laterality: Laterality::Right,
            view_position: ViewPosition::Mlo,
            view_modifiers: Default::default(),
            image_type: ImageType::new("DERIVED".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 50,
            pixel_spacing: Some(PixelSpacing::new(0.07, 0.08)),
            concatenation_uid: None,
            sop_instance_uid_of_concatenation_source: None,
            is_secondary_capture: false,
            modality: Some("MG".to_string()),
            transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
            transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
            compression_type: Some("uncompressed".to_string()),
        };

        assert!(!metadata.is_2d());
    }

    #[test]
    fn modifier_convenience_properties_follow_the_canonical_set() {
        let mut metadata = MammogramExtractor::extract(&minimal_mammo_dicom()).unwrap();
        metadata
            .view_modifiers
            .insert(MammographyViewModifier::SpotCompression);
        metadata
            .view_modifiers
            .insert(MammographyViewModifier::Magnification);
        metadata
            .view_modifiers
            .insert(MammographyViewModifier::ImplantDisplaced);

        assert!(metadata.is_spot_compression());
        assert!(metadata.is_magnified());
        assert!(metadata.is_implant_displaced());
    }

    #[test]
    fn extracts_pixel_spacing() {
        let mut dcm = minimal_mammo_dicom();
        dcm.put(DataElement::new(
            Tag(0x0028, 0x0030),
            VR::DS,
            PrimitiveValue::from("0.070\\0.071"),
        ));

        let metadata = MammogramExtractor::extract(&dcm).unwrap();
        let pixel_spacing = metadata.pixel_spacing.unwrap();

        assert_eq!(pixel_spacing.row, 0.070);
        assert_eq!(pixel_spacing.col, 0.071);
    }

    #[test]
    fn extracts_imager_pixel_spacing_fallback() {
        let mut dcm = minimal_mammo_dicom();
        dcm.put(DataElement::new(
            Tag(0x0018, 0x1164),
            VR::DS,
            PrimitiveValue::from("0.090\\0.091"),
        ));

        let metadata = MammogramExtractor::extract(&dcm).unwrap();
        let pixel_spacing = metadata.pixel_spacing.unwrap();

        assert_eq!(pixel_spacing.row, 0.090);
        assert_eq!(pixel_spacing.col, 0.091);
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_mammogram_metadata_json_includes_dbt_object_kind() {
        let metadata = MammogramMetadata {
            mammogram_type: MammogramType::Tomo,
            dbt_object_kind: DbtObjectKind::Slice,
            laterality: Laterality::Right,
            view_position: ViewPosition::Cc,
            view_modifiers: [
                MammographyViewModifier::ImplantDisplaced,
                MammographyViewModifier::Magnification,
                MammographyViewModifier::SpotCompression,
            ]
            .into_iter()
            .collect(),
            image_type: ImageType::new(
                "DERIVED".to_string(),
                "PRIMARY".to_string(),
                Some("TOMO".to_string()),
                None,
            ),
            is_for_processing: false,
            has_implant: false,
            manufacturer: None,
            model: None,
            number_of_frames: 1,
            pixel_spacing: Some(PixelSpacing::new(0.07, 0.08)),
            concatenation_uid: Some("1.2.826.0.1.100".to_string()),
            sop_instance_uid_of_concatenation_source: Some("1.2.826.0.1.101".to_string()),
            is_secondary_capture: false,
            modality: Some("MG".to_string()),
            transfer_syntax_uid: None,
            transfer_syntax_name: None,
            compression_type: None,
        };

        let value = serde_json::to_value(metadata).unwrap();

        assert_eq!(value["mammogram_type"], "tomo");
        assert_eq!(value["dbt_object_kind"], "slice");
        assert_eq!(
            value["view_modifiers"],
            serde_json::json!(["implant_displaced", "magnification", "spot_compression"])
        );
        assert_eq!(value["is_spot_compression"], true);
        assert_eq!(value["is_magnified"], true);
        assert_eq!(value["is_implant_displaced"], true);
        assert_eq!(value["pixel_spacing"]["row"], 0.07);
        assert_eq!(value["pixel_spacing"]["column"], 0.08);
        assert_eq!(value["concatenation_uid"], "1.2.826.0.1.100");
        assert_eq!(
            value["sop_instance_uid_of_concatenation_source"],
            "1.2.826.0.1.101"
        );
    }

    #[test]
    fn transfer_syntax_metadata_resolves_compression_type() {
        let metadata = resolve_transfer_syntax_metadata("1.2.840.10008.1.2.4.90").unwrap();

        assert_eq!(metadata.uid, "1.2.840.10008.1.2.4.90");
        assert_eq!(metadata.name, "JPEG 2000 Image Compression (Lossless Only)");
        assert_eq!(metadata.compression_type, "jpeg2000_lossless");
    }
}
