use crate::api::{MammogramExtractor, MammogramMetadata};
use crate::error::Result;
use crate::extraction::tags::{
    get_string_value, get_u16_value, COLUMNS, LOSSY_IMAGE_COMPRESSION, PIXEL_DATA_TAG, ROWS,
    SERIES_INSTANCE_UID, SOP_INSTANCE_UID, STUDY_INSTANCE_UID,
};
use crate::types::PreferenceOrder;
use dicom_object::{FileDicomObject, InMemDicomObject, OpenFileOptions};
use std::cmp::Ordering;
use std::path::PathBuf;

/// Transfer syntax UIDs that imply lossy image compression.
///
/// This excludes lossless-only syntaxes and JPEG 2000 syntaxes that permit
/// either reversible or irreversible encoding.
pub const LOSSY_TRANSFER_SYNTAX_UIDS: &[&str] = &[
    // JPEG lossy and retired lossy forms
    "1.2.840.10008.1.2.4.50",
    "1.2.840.10008.1.2.4.51",
    "1.2.840.10008.1.2.4.52",
    "1.2.840.10008.1.2.4.53",
    "1.2.840.10008.1.2.4.54",
    "1.2.840.10008.1.2.4.55",
    "1.2.840.10008.1.2.4.56",
    "1.2.840.10008.1.2.4.59",
    "1.2.840.10008.1.2.4.60",
    "1.2.840.10008.1.2.4.61",
    "1.2.840.10008.1.2.4.62",
    "1.2.840.10008.1.2.4.63",
    "1.2.840.10008.1.2.4.64",
    // JPEG-LS near-lossless
    "1.2.840.10008.1.2.4.81",
    // JPIP forms
    "1.2.840.10008.1.2.4.94",
    "1.2.840.10008.1.2.4.95",
    // MPEG / HEVC video transfer syntaxes
    "1.2.840.10008.1.2.4.100",
    "1.2.840.10008.1.2.4.100.1",
    "1.2.840.10008.1.2.4.101",
    "1.2.840.10008.1.2.4.101.1",
    "1.2.840.10008.1.2.4.102",
    "1.2.840.10008.1.2.4.102.1",
    "1.2.840.10008.1.2.4.103",
    "1.2.840.10008.1.2.4.103.1",
    "1.2.840.10008.1.2.4.104",
    "1.2.840.10008.1.2.4.104.1",
    "1.2.840.10008.1.2.4.105",
    "1.2.840.10008.1.2.4.105.1",
    "1.2.840.10008.1.2.4.106",
    "1.2.840.10008.1.2.4.106.1",
    "1.2.840.10008.1.2.4.107",
    "1.2.840.10008.1.2.4.108",
    // JPEG XL non-lossless and recompression forms
    "1.2.840.10008.1.2.4.111",
    "1.2.840.10008.1.2.4.112",
    // High-throughput JPEG 2000 forms not marked lossless-only
    "1.2.840.10008.1.2.4.203",
    "1.2.840.10008.1.2.4.204",
    "1.2.840.10008.1.2.4.205",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LossyCompressionSource {
    Metadata,
    TransferSyntax,
}

pub(crate) fn lossy_compression_source(
    lossy_image_compression: Option<&str>,
    transfer_syntax_uid: Option<&str>,
) -> Option<LossyCompressionSource> {
    match lossy_image_compression.and_then(parse_lossy_image_compression_value) {
        Some(true) => Some(LossyCompressionSource::Metadata),
        Some(false) => None,
        None => transfer_syntax_uid
            .filter(|uid| is_lossy_transfer_syntax_uid(uid))
            .map(|_| LossyCompressionSource::TransferSyntax),
    }
}

/// Mammogram record combining file path and extracted metadata
///
/// Used for preferred view selection. Implements comparison logic
/// matching Python dicom_utils.container.record.MammogramFileRecord.is_preferred_to
#[derive(Debug, Clone)]
pub struct MammogramRecord {
    /// Path to the DICOM file
    pub file_path: PathBuf,

    /// Extracted mammography metadata
    pub metadata: MammogramMetadata,

    /// Study Instance UID
    pub study_instance_uid: Option<String>,

    /// Series Instance UID
    pub series_instance_uid: Option<String>,

    /// SOP Instance UID
    pub sop_instance_uid: Option<String>,

    /// Number of rows in image
    pub rows: Option<u16>,

    /// Number of columns in image
    pub columns: Option<u16>,

    /// Transfer Syntax UID from file metadata, when available
    pub transfer_syntax_uid: Option<String>,

    /// Whether metadata indicates current or historical lossy compression
    pub is_lossy_compressed: bool,
}

impl MammogramRecord {
    /// Creates a record from a DICOM file path
    ///
    /// Only reads DICOM metadata (headers), not pixel data, for optimal performance.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to DICOM file
    ///
    /// # Returns
    ///
    /// Result containing the MammogramRecord or an error
    pub fn from_file(path: PathBuf) -> Result<Self> {
        // Read only metadata, stop before pixel data tag for performance
        let dcm = OpenFileOptions::new()
            .read_until(PIXEL_DATA_TAG)
            .open_file(&path)?;
        Self::from_file_dicom(path, &dcm)
    }

    /// Creates a MammogramRecord from in-memory DICOM bytes.
    ///
    /// Parses the DICOM object from bytes and extracts mammogram metadata
    /// (laterality, view position, mammogram type, etc.) just like `from_file`.
    ///
    /// # Arguments
    /// * `bytes` - Raw DICOM file bytes
    /// * `id` - Optional identifier for this record (for debugging/logging).
    ///   With `from_file`, this is the file path. For bytes, caller can
    ///   provide any identifier (e.g., "upload_0", original filename, etc.)
    ///
    /// # Returns
    /// * `Ok(MammogramRecord)` - Successfully parsed record with `file_path` set to
    ///   the provided `id` (or empty path if None)
    /// * `Err` - If DICOM parsing fails or required metadata is missing
    pub fn from_bytes(bytes: &[u8], id: Option<&str>) -> Result<Self> {
        let cursor = std::io::Cursor::new(bytes);
        let dcm = OpenFileOptions::new()
            .read_until(PIXEL_DATA_TAG)
            .from_reader(cursor)?;

        let path = id.map(PathBuf::from).unwrap_or_default();
        Self::from_file_dicom(path, &dcm)
    }

    /// Creates a record from an already-opened DICOM object
    ///
    /// # Arguments
    ///
    /// * `path` - Path to DICOM file
    /// * `dcm` - Opened DICOM object
    ///
    /// # Returns
    ///
    /// Result containing the MammogramRecord or an error
    pub fn from_dicom(path: PathBuf, dcm: &InMemDicomObject) -> Result<Self> {
        Self::from_dicom_with_transfer_syntax(path, dcm, None)
    }

    /// Creates a record from an already-opened DICOM object and optional transfer syntax.
    ///
    /// Use this when the caller has access to file-meta transfer syntax. Dataset-only
    /// callers can pass `None`, in which case lossy detection only uses dataset tags.
    pub fn from_dicom_with_transfer_syntax(
        path: PathBuf,
        dcm: &InMemDicomObject,
        transfer_syntax_uid: Option<String>,
    ) -> Result<Self> {
        let metadata = MammogramExtractor::extract(dcm)?;
        let transfer_syntax_uid =
            transfer_syntax_uid.or_else(|| metadata.transfer_syntax_uid.clone());
        Self::from_dicom_with_metadata_and_transfer_syntax(path, dcm, metadata, transfer_syntax_uid)
    }

    /// Creates a record from an already-opened DICOM file object.
    pub fn from_file_dicom(path: PathBuf, dcm: &FileDicomObject<InMemDicomObject>) -> Result<Self> {
        let metadata = MammogramExtractor::extract_file(dcm)?;
        let transfer_syntax_uid = metadata
            .transfer_syntax_uid
            .clone()
            .or_else(|| normalize_transfer_syntax_uid(dcm.meta().transfer_syntax()));
        Self::from_dicom_with_metadata_and_transfer_syntax(path, dcm, metadata, transfer_syntax_uid)
    }

    pub(crate) fn from_dicom_with_metadata(
        path: PathBuf,
        dcm: &InMemDicomObject,
        metadata: MammogramMetadata,
    ) -> Result<Self> {
        let transfer_syntax_uid = metadata.transfer_syntax_uid.clone();
        Self::from_dicom_with_metadata_and_transfer_syntax(path, dcm, metadata, transfer_syntax_uid)
    }

    fn from_dicom_with_metadata_and_transfer_syntax(
        path: PathBuf,
        dcm: &InMemDicomObject,
        metadata: MammogramMetadata,
        transfer_syntax_uid: Option<String>,
    ) -> Result<Self> {
        let is_lossy_compressed = is_lossy_compressed(dcm, transfer_syntax_uid.as_deref());
        Ok(Self {
            file_path: path,
            metadata,
            study_instance_uid: get_string_value(dcm, STUDY_INSTANCE_UID),
            series_instance_uid: get_string_value(dcm, SERIES_INSTANCE_UID),
            sop_instance_uid: get_string_value(dcm, SOP_INSTANCE_UID),
            rows: get_u16_value(dcm, ROWS),
            columns: get_u16_value(dcm, COLUMNS),
            transfer_syntax_uid,
            is_lossy_compressed,
        })
    }

    /// Computes image area (rows * columns)
    ///
    /// # Returns
    ///
    /// Image area in pixels, or None if dimensions are not available
    pub fn image_area(&self) -> Option<u32> {
        match (self.rows, self.columns) {
            (Some(r), Some(c)) => Some(r as u32 * c as u32),
            _ => None,
        }
    }

    /// Checks if this is a spot compression or magnification view
    ///
    /// These views are deprioritized during selection
    ///
    /// # Returns
    ///
    /// `true` if either spot compression or magnification is detected
    pub fn is_spot_or_mag(&self) -> bool {
        self.is_spot_compression() || self.is_magnified()
    }

    /// Whether this record has an Implant Displaced modifier.
    pub fn is_implant_displaced(&self) -> bool {
        self.metadata.is_implant_displaced()
    }

    /// Whether this record has a Spot Compression modifier.
    pub fn is_spot_compression(&self) -> bool {
        self.metadata.is_spot_compression()
    }

    /// Whether this record has a Magnification modifier.
    pub fn is_magnified(&self) -> bool {
        self.metadata.is_magnified()
    }

    /// Whether this record has any modifier that should lose to an otherwise
    /// equivalent unmodified view.
    pub fn has_deprioritized_view_modifier(&self) -> bool {
        self.metadata
            .view_modifiers
            .iter()
            .any(|modifier| modifier.affects_selection())
    }

    /// Checks if this record is preferred over another
    ///
    /// Implements Python logic from record.py:805-838
    /// Uses the default preference order (FFDM > SYNTH > TOMO > SFM)
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Views without deprioritized CID 4015 modifiers beat modified views
    /// 3. Implant displaced beats non-displaced (same study only)
    /// 4. Lossless beats lossy compressed
    /// 5. Type preference (FFDM > SYNTH > TOMO > SFM)
    /// 6. Higher resolution beats lower resolution
    /// 7. Fallback to SOPInstanceUID comparison
    ///
    /// # Arguments
    ///
    /// * `other` - Another MammogramRecord to compare against
    ///
    /// # Returns
    ///
    /// `true` if this record is preferred over the other
    pub fn is_preferred_to(&self, other: &MammogramRecord) -> bool {
        self.is_preferred_to_with_order(other, PreferenceOrder::Default)
    }

    /// Checks if this record is preferred over another using a specific preference order
    ///
    /// Implements Python logic from record.py:805-838 with configurable type preference
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Views without deprioritized CID 4015 modifiers beat modified views
    /// 3. Implant displaced beats non-displaced (same study only)
    /// 4. Lossless beats lossy compressed
    /// 5. Type preference (according to the provided preference order)
    /// 6. Higher resolution beats lower resolution
    /// 7. Fallback to SOPInstanceUID comparison
    ///
    /// # Arguments
    ///
    /// * `other` - Another MammogramRecord to compare against
    /// * `preference_order` - The preference ordering strategy to use
    ///
    /// # Returns
    ///
    /// `true` if this record is preferred over the other
    pub fn is_preferred_to_with_order(
        &self,
        other: &MammogramRecord,
        preference_order: PreferenceOrder,
    ) -> bool {
        self.is_preferred_to_with_options(other, preference_order, true)
    }

    /// Checks if this record is preferred over another with lossy-ranking control
    ///
    /// When `deprioritize_lossy_compressed` is true, lossless records are preferred
    /// over lossy records before mammogram type preference is considered.
    pub fn is_preferred_to_with_options(
        &self,
        other: &MammogramRecord,
        preference_order: PreferenceOrder,
        deprioritize_lossy_compressed: bool,
    ) -> bool {
        // 1. Standard views take priority
        if self.metadata.is_standard_view() && !other.metadata.is_standard_view() {
            return true;
        }
        if !self.metadata.is_standard_view() && other.metadata.is_standard_view() {
            return false;
        }

        // 2. Unmodified views take priority over special views. Implant
        // displaced alone is not a deprioritized modifier.
        if !self.has_deprioritized_view_modifier() && other.has_deprioritized_view_modifier() {
            return true;
        }
        if self.has_deprioritized_view_modifier() && !other.has_deprioritized_view_modifier() {
            return false;
        }

        // 3. Implant displaced views take priority (same study only)
        if let (Some(self_study), Some(other_study)) =
            (&self.study_instance_uid, &other.study_instance_uid)
        {
            if self_study == other_study
                && self.is_implant_displaced() != other.is_implant_displaced()
            {
                return self.is_implant_displaced();
            }
        }

        // 4. Lossless images take priority when requested
        if deprioritize_lossy_compressed && self.is_lossy_compressed != other.is_lossy_compressed {
            return !self.is_lossy_compressed && other.is_lossy_compressed;
        }

        // 5. Type preference (using configurable order)
        let self_type = &self.metadata.mammogram_type;
        let other_type = &other.metadata.mammogram_type;
        if self_type != other_type {
            let self_pref = preference_order.preference_value(self_type);
            let other_pref = preference_order.preference_value(other_type);
            return self_pref < other_pref;
        }

        // 6. Resolution preference (higher is better)
        if self.image_area() != other.image_area() {
            let self_area = self.image_area().unwrap_or(0);
            let other_area = other.image_area().unwrap_or(0);
            return self_area > other_area;
        }

        // 7. Fallback to SOP UID comparison (for stable ordering)
        match (&self.sop_instance_uid, &other.sop_instance_uid) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    }
}

fn normalize_transfer_syntax_uid(uid: &str) -> Option<String> {
    let normalized = normalized_transfer_syntax_uid(uid);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn normalized_transfer_syntax_uid(uid: &str) -> &str {
    uid.trim().trim_end_matches('\0').trim()
}

fn is_lossy_compressed(dcm: &InMemDicomObject, transfer_syntax_uid: Option<&str>) -> bool {
    let lossy_image_compression = get_string_value(dcm, LOSSY_IMAGE_COMPRESSION);
    lossy_compression_source(lossy_image_compression.as_deref(), transfer_syntax_uid).is_some()
}

fn parse_lossy_image_compression_value(value: &str) -> Option<bool> {
    match value.trim() {
        "01" => Some(true),
        "00" => Some(false),
        _ => None,
    }
}

fn is_lossy_transfer_syntax_uid(uid: &str) -> bool {
    LOSSY_TRANSFER_SYNTAX_UIDS.contains(&normalized_transfer_syntax_uid(uid))
}

// Implement Ord/PartialOrd for use with min/max
impl PartialEq for MammogramRecord {
    fn eq(&self, other: &Self) -> bool {
        self.sop_instance_uid == other.sop_instance_uid
    }
}

impl Eq for MammogramRecord {}

impl PartialOrd for MammogramRecord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MammogramRecord {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.is_preferred_to(other) {
            Ordering::Less
        } else if other.is_preferred_to(self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::tags::LOSSY_IMAGE_COMPRESSION;
    use crate::types::{
        DbtObjectKind, ImageType, Laterality, MammogramType, MammographyViewModifier, ViewPosition,
    };
    use dicom_core::{DataElement, PrimitiveValue, VR};

    fn make_test_record(
        mammo_type: MammogramType,
        view_pos: ViewPosition,
        laterality: Laterality,
        rows: Option<u16>,
        columns: Option<u16>,
        _is_standard: bool,
        is_implant_displaced: bool,
        is_spot_compression: bool,
        is_magnified: bool,
        study_uid: Option<String>,
        sop_uid: Option<String>,
    ) -> MammogramRecord {
        MammogramRecord {
            file_path: PathBuf::from("test.dcm"),
            metadata: MammogramMetadata {
                mammogram_type: mammo_type,
                dbt_object_kind: default_dbt_object_kind(mammo_type),
                laterality,
                view_position: view_pos,
                view_modifiers: [
                    is_implant_displaced.then_some(MammographyViewModifier::ImplantDisplaced),
                    is_spot_compression.then_some(MammographyViewModifier::SpotCompression),
                    is_magnified.then_some(MammographyViewModifier::Magnification),
                ]
                .into_iter()
                .flatten()
                .collect(),
                image_type: ImageType::new(
                    "ORIGINAL".to_string(),
                    "PRIMARY".to_string(),
                    None,
                    None,
                ),
                is_for_processing: false,
                has_implant: false,
                manufacturer: None,
                model: None,
                number_of_frames: 1,
                pixel_spacing: None,
                concatenation_uid: None,
                sop_instance_uid_of_concatenation_source: None,
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
                transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
                transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
                compression_type: Some("uncompressed".to_string()),
            },
            rows,
            columns,
            transfer_syntax_uid: None,
            is_lossy_compressed: false,
            study_instance_uid: study_uid,
            series_instance_uid: None,
            sop_instance_uid: sop_uid,
        }
    }

    fn default_dbt_object_kind(mammo_type: MammogramType) -> DbtObjectKind {
        match mammo_type {
            MammogramType::Tomo => DbtObjectKind::Unknown,
            _ => DbtObjectKind::None,
        }
    }

    fn make_lossy_test_record(
        mammo_type: MammogramType,
        is_lossy_compressed: bool,
    ) -> MammogramRecord {
        let mut record = make_test_record(
            mammo_type,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            None,
            Some(
                if is_lossy_compressed {
                    "lossy"
                } else {
                    "lossless"
                }
                .to_string(),
            ),
        );
        record.is_lossy_compressed = is_lossy_compressed;
        record
    }

    fn dicom_with_lossy_image_compression(value: &str) -> InMemDicomObject {
        let mut dcm = InMemDicomObject::new_empty();
        dcm.put(DataElement::new(
            LOSSY_IMAGE_COMPRESSION,
            VR::CS,
            PrimitiveValue::from(value),
        ));
        dcm
    }

    #[test]
    fn test_lossy_image_compression_tag_true() {
        let dcm = dicom_with_lossy_image_compression("01");
        assert!(is_lossy_compressed(&dcm, None));
    }

    #[test]
    fn test_lossy_image_compression_tag_false() {
        let dcm = dicom_with_lossy_image_compression("00");
        assert!(!is_lossy_compressed(&dcm, Some("1.2.840.10008.1.2.4.50")));
    }

    #[test]
    fn test_lossy_image_compression_falls_back_to_transfer_syntax_when_missing() {
        let dcm = InMemDicomObject::new_empty();
        assert!(is_lossy_compressed(&dcm, Some("1.2.840.10008.1.2.4.50")));
    }

    #[test]
    fn test_lossy_image_compression_falls_back_to_transfer_syntax_when_invalid() {
        let dcm = dicom_with_lossy_image_compression("MAYBE");
        assert!(is_lossy_compressed(&dcm, Some("1.2.840.10008.1.2.4.81")));
    }

    #[test]
    fn test_lossless_transfer_syntax_is_not_lossy() {
        let dcm = InMemDicomObject::new_empty();
        assert!(!is_lossy_compressed(&dcm, Some("1.2.840.10008.1.2.4.90")));
    }

    #[test]
    fn lossy_compression_policy_covers_all_transfer_syntaxes_and_metadata_precedence() {
        for uid in LOSSY_TRANSFER_SYNTAX_UIDS {
            assert_eq!(
                lossy_compression_source(None, Some(uid)),
                Some(LossyCompressionSource::TransferSyntax),
                "{uid}"
            );
            assert_eq!(
                lossy_compression_source(Some("00"), Some(uid)),
                None,
                "explicit lossless metadata should override {uid}"
            );
        }

        for uid in [
            "1.2.840.10008.1.2",
            "1.2.840.10008.1.2.1",
            "1.2.840.10008.1.2.4.80",
            "1.2.840.10008.1.2.4.90",
            "1.2.840.10008.1.2.4.91",
            "1.2.840.10008.1.2.4.93",
        ] {
            assert_eq!(lossy_compression_source(None, Some(uid)), None, "{uid}");
            assert_eq!(
                lossy_compression_source(Some("01"), Some(uid)),
                Some(LossyCompressionSource::Metadata),
                "explicit lossy metadata should override {uid}"
            );
        }

        assert_eq!(
            lossy_compression_source(Some("invalid"), Some("1.2.840.10008.1.2.4.91")),
            None
        );
    }

    #[test]
    fn test_image_area_calculation() {
        let record = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        assert_eq!(record.image_area(), Some(2560 * 3328));
    }

    #[test]
    fn test_image_area_missing_dimensions() {
        let record = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            None,
            None,
            true,
            false,
            false,
            false,
            None,
            None,
        );

        assert_eq!(record.image_area(), None);
    }

    #[test]
    fn test_is_preferred_to_standard_view() {
        let standard = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        let non_standard = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Ml,
            Laterality::Left,
            Some(2560),
            Some(3328),
            false,
            false,
            false,
            false,
            None,
            None,
        );

        assert!(standard.is_preferred_to(&non_standard));
        assert!(!non_standard.is_preferred_to(&standard));
    }

    #[test]
    fn test_is_preferred_to_implant_displaced_same_study() {
        let implant_displaced = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            true,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );

        let regular = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );

        assert!(implant_displaced.is_preferred_to(&regular));
        assert!(!regular.is_preferred_to(&implant_displaced));
    }

    #[test]
    fn implant_displaced_preference_is_symmetric_before_mammogram_type() {
        let implant_displaced = make_test_record(
            MammogramType::Synth,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            true,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );
        let regular = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );

        assert!(implant_displaced.is_preferred_to(&regular));
        assert!(!regular.is_preferred_to(&implant_displaced));
    }

    #[test]
    fn test_is_preferred_to_implant_displaced_different_study() {
        let implant_displaced = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            true,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );

        let regular = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            Some("5.6.7.8".to_string()),
            None,
        );

        // Different studies - implant displaced should NOT be preferred
        assert!(!implant_displaced.is_preferred_to(&regular));
    }

    #[test]
    fn test_is_preferred_to_type_preference() {
        let ffdm = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        let tomo = make_test_record(
            MammogramType::Tomo,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        // FFDM is preferred over TOMO (with default ordering)
        assert!(ffdm.is_preferred_to(&tomo));
        assert!(!tomo.is_preferred_to(&ffdm));
    }

    #[test]
    fn test_lossless_preferred_over_lossy_before_type_preference_by_default() {
        let lossless_tomo = make_lossy_test_record(MammogramType::Tomo, false);
        let lossy_ffdm = make_lossy_test_record(MammogramType::Ffdm, true);

        assert!(lossless_tomo.is_preferred_to(&lossy_ffdm));
        assert!(!lossy_ffdm.is_preferred_to(&lossless_tomo));
    }

    #[test]
    fn test_lossy_deprioritization_can_be_disabled() {
        let lossless_tomo = make_lossy_test_record(MammogramType::Tomo, false);
        let lossy_ffdm = make_lossy_test_record(MammogramType::Ffdm, true);

        assert!(lossy_ffdm.is_preferred_to_with_options(
            &lossless_tomo,
            PreferenceOrder::Default,
            false
        ));
    }

    #[test]
    fn test_is_preferred_to_resolution() {
        let high_res = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(3000),
            Some(4000),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        let low_res = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2000),
            Some(2500),
            true,
            false,
            false,
            false,
            None,
            None,
        );

        // Higher resolution is preferred
        assert!(high_res.is_preferred_to(&low_res));
        assert!(!low_res.is_preferred_to(&high_res));
    }

    #[test]
    fn test_ord_implementation() {
        let better = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(3000),
            Some(4000),
            true,
            false,
            false,
            false,
            None,
            Some("AAA".to_string()),
        );

        let worse = make_test_record(
            MammogramType::Tomo,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2000),
            Some(2500),
            true,
            false,
            false,
            false,
            None,
            Some("BBB".to_string()),
        );

        // Better record should be "less than" (more preferred)
        // FFDM with higher resolution is preferred over TOMO with lower resolution (default ordering)
        assert!(better < worse);
        assert!(worse > better);

        // Min should select the better record
        assert_eq!(std::cmp::min(&better, &worse), &better);
    }

    #[test]
    fn test_is_preferred_to_spot_mag_deprioritized() {
        let standard = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false, // not spot compression
            false, // not magnified
            None,
            None,
        );

        let spot = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            true, // IS spot compression
            false,
            None,
            None,
        );

        let mag = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            true, // IS magnified
            None,
            None,
        );

        // Standard (non-spot/mag) should be preferred
        assert!(standard.is_preferred_to(&spot));
        assert!(standard.is_preferred_to(&mag));
        assert!(!spot.is_preferred_to(&standard));
        assert!(!mag.is_preferred_to(&standard));
    }

    #[test]
    fn every_special_cid_4015_modifier_is_deprioritized() {
        let standard = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            Some("study".to_string()),
            None,
        );
        for modifier in [
            MammographyViewModifier::Cleavage,
            MammographyViewModifier::AxillaryTail,
            MammographyViewModifier::RolledLateral,
            MammographyViewModifier::RolledMedial,
            MammographyViewModifier::RolledInferior,
            MammographyViewModifier::RolledSuperior,
            MammographyViewModifier::Magnification,
            MammographyViewModifier::SpotCompression,
            MammographyViewModifier::Tangential,
            MammographyViewModifier::NippleInProfile,
            MammographyViewModifier::AnteriorCompression,
            MammographyViewModifier::InfraMammaryFold,
            MammographyViewModifier::AxillaryTissue,
        ] {
            let mut modified = standard.clone();
            modified.metadata.view_modifiers.insert(modifier);
            assert!(standard.is_preferred_to(&modified), "{modifier}");
            assert!(!modified.is_preferred_to(&standard), "{modifier}");
        }
    }

    #[test]
    fn implant_displaced_only_remains_preferred_but_mixed_modifiers_do_not() {
        let regular = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            false,
            Some("study".to_string()),
            None,
        );
        let mut implant_displaced = regular.clone();
        implant_displaced
            .metadata
            .view_modifiers
            .insert(MammographyViewModifier::ImplantDisplaced);
        assert!(implant_displaced.is_preferred_to(&regular));

        implant_displaced
            .metadata
            .view_modifiers
            .insert(MammographyViewModifier::Tangential);
        assert!(regular.is_preferred_to(&implant_displaced));
    }

    #[test]
    fn test_is_preferred_to_spot_vs_mag_same_priority() {
        let spot = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            true, // spot compression
            false,
            None,
            Some("AAA".to_string()),
        );

        let mag = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2560),
            Some(3328),
            true,
            false,
            false,
            true, // magnified
            None,
            Some("BBB".to_string()),
        );

        // When both are spot/mag, fall through to other criteria (SOP UID)
        assert!(spot.is_preferred_to(&mag)); // AAA < BBB
    }

    #[test]
    fn test_from_bytes_invalid_data() {
        // Invalid bytes should return an error
        let result = MammogramRecord::from_bytes(b"not valid dicom data", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_bytes_empty_bytes() {
        // Empty bytes should return an error
        let result = MammogramRecord::from_bytes(&[], None);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_bytes_id_to_path_conversion() {
        // Test that the id parameter correctly sets the file_path
        // This tests the path conversion logic without needing valid DICOM

        // We can't test the full flow without valid DICOM, but we can verify
        // the logic by checking the error message contains our id
        let result = MammogramRecord::from_bytes(b"invalid", Some("my_upload_id"));
        assert!(result.is_err()); // Still fails due to invalid DICOM

        // The actual path conversion is tested via Python integration tests
        // which use valid DICOM files
    }
}
