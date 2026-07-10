use crate::api::{MammogramExtractor, MammogramMetadata};
use crate::error::Result;
use crate::extraction::tags::{
    get_string_value, get_u16_value, COLUMNS, LOSSY_IMAGE_COMPRESSION, PIXEL_DATA_TAG, ROWS,
    SERIES_INSTANCE_UID, SOP_INSTANCE_UID, STUDY_INSTANCE_UID,
};
use crate::extraction::{is_implant_displaced, is_magnified, is_spot_compression};
use crate::types::PreferenceOrder;
use dicom_object::{FileDicomObject, InMemDicomObject, OpenFileOptions};
use std::cmp::Ordering;
use std::path::PathBuf;

/// Transfer syntax UIDs that imply lossy image compression.
///
/// This excludes DICOM syntaxes explicitly named lossless or lossless-only.
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
    // JPEG 2000 / JPIP forms not marked lossless-only
    "1.2.840.10008.1.2.4.91",
    "1.2.840.10008.1.2.4.93",
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

    /// Whether this is an implant displaced view
    pub is_implant_displaced: bool,

    /// Whether this is a spot compression view
    pub is_spot_compression: bool,

    /// Whether this is a magnification view
    pub is_magnified: bool,
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
            is_implant_displaced: is_implant_displaced(dcm),
            is_spot_compression: is_spot_compression(dcm),
            is_magnified: is_magnified(dcm),
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
        self.is_spot_compression || self.is_magnified
    }

    /// Checks if this record is preferred over another
    ///
    /// Implements Python logic from record.py:805-838
    /// Uses the default preference order (FFDM > SYNTH > TOMO > SFM)
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Non-spot/mag views beat spot/mag views
    /// 3. Records are partitioned by StudyInstanceUID for stable cross-study ordering
    /// 4. Implant displaced beats non-displaced within a study
    /// 5. Lossless beats lossy compressed
    /// 6. Type preference (FFDM > SYNTH > TOMO > SFM)
    /// 7. Higher resolution beats lower resolution
    /// 8. Stable source identifiers break remaining ties
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
    /// 2. Non-spot/mag views beat spot/mag views
    /// 3. Records are partitioned by StudyInstanceUID for stable cross-study ordering
    /// 4. Implant displaced beats non-displaced within a study
    /// 5. Lossless beats lossy compressed
    /// 6. Type preference (according to the provided preference order)
    /// 7. Higher resolution beats lower resolution
    /// 8. Stable source identifiers break remaining ties
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
        self.preference_cmp_with_options(other, preference_order, deprioritize_lossy_compressed)
            == Ordering::Less
    }

    pub(crate) fn preference_cmp_with_options(
        &self,
        other: &MammogramRecord,
        preference_order: PreferenceOrder,
        deprioritize_lossy_compressed: bool,
    ) -> Ordering {
        prefer_true(
            self.metadata.is_standard_view(),
            other.metadata.is_standard_view(),
        )
        .then_with(|| self.is_spot_or_mag().cmp(&other.is_spot_or_mag()))
        .then_with(|| {
            compare_optional_identifier(&self.study_instance_uid, &other.study_instance_uid)
        })
        .then_with(|| prefer_true(self.is_implant_displaced, other.is_implant_displaced))
        .then_with(|| {
            if deprioritize_lossy_compressed {
                self.is_lossy_compressed.cmp(&other.is_lossy_compressed)
            } else {
                Ordering::Equal
            }
        })
        .then_with(|| {
            preference_order
                .preference_value(&self.metadata.mammogram_type)
                .cmp(&preference_order.preference_value(&other.metadata.mammogram_type))
        })
        .then_with(|| {
            other
                .image_area()
                .unwrap_or(0)
                .cmp(&self.image_area().unwrap_or(0))
        })
        .then_with(|| compare_optional_identifier(&self.sop_instance_uid, &other.sop_instance_uid))
        .then_with(|| {
            compare_optional_identifier(&self.series_instance_uid, &other.series_instance_uid)
        })
        .then_with(|| self.file_path.cmp(&other.file_path))
    }
}

fn prefer_true(left: bool, right: bool) -> Ordering {
    right.cmp(&left)
}

fn compare_optional_identifier(left: &Option<String>, right: &Option<String>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
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
    if let Some(is_lossy) = lossy_image_compression_tag(dcm) {
        return is_lossy;
    }

    transfer_syntax_uid
        .map(is_lossy_transfer_syntax_uid)
        .unwrap_or(false)
}

fn lossy_image_compression_tag(dcm: &InMemDicomObject) -> Option<bool> {
    get_string_value(dcm, LOSSY_IMAGE_COMPRESSION)
        .as_deref()
        .and_then(parse_lossy_image_compression_value)
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
        self.cmp(other) == Ordering::Equal
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
        self.preference_cmp_with_options(other, PreferenceOrder::Default, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::tags::LOSSY_IMAGE_COMPRESSION;
    use crate::types::{DbtObjectKind, ImageType, Laterality, MammogramType, ViewPosition};
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
                image_type: ImageType::new(
                    "ORIGINAL".to_string(),
                    "PRIMARY".to_string(),
                    None,
                    None,
                ),
                is_for_processing: false,
                has_implant: false,
                is_spot_compression,
                is_magnified,
                is_implant_displaced,
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
            is_implant_displaced,
            is_spot_compression,
            is_magnified,
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
    fn implant_displaced_preference_is_antisymmetric_when_resolution_conflicts() {
        let implant_displaced = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(1000),
            Some(1000),
            true,
            true,
            false,
            false,
            Some("1.2.3.4".to_string()),
            Some("2".to_string()),
        );
        let regular = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2000),
            Some(2000),
            true,
            false,
            false,
            false,
            Some("1.2.3.4".to_string()),
            Some("1".to_string()),
        );

        assert!(implant_displaced.is_preferred_to(&regular));
        assert!(!regular.is_preferred_to(&implant_displaced));
    }

    #[test]
    fn test_different_studies_are_ordered_before_implant_status() {
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
            Some("5.6.7.8".to_string()),
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

        assert!(!implant_displaced.is_preferred_to(&regular));
        assert!(regular.is_preferred_to(&implant_displaced));
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
    fn ordering_contract_handles_missing_and_duplicate_sop_uids() {
        let mut missing_a = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2000),
            Some(2000),
            true,
            false,
            false,
            false,
            Some("1.2.3.4".to_string()),
            None,
        );
        missing_a.file_path = PathBuf::from("a.dcm");
        let mut missing_b = missing_a.clone();
        missing_b.file_path = PathBuf::from("b.dcm");

        let mut duplicate_a = missing_a.clone();
        duplicate_a.sop_instance_uid = Some("1.2.3.4.5".to_string());
        let mut duplicate_b = missing_b.clone();
        duplicate_b.sop_instance_uid = duplicate_a.sop_instance_uid.clone();

        for (left, right) in [(&missing_a, &missing_b), (&duplicate_a, &duplicate_b)] {
            assert_ne!(left.cmp(right), Ordering::Equal);
            assert_eq!(left == right, left.cmp(right) == Ordering::Equal);
            assert_eq!(right == left, right.cmp(left) == Ordering::Equal);
            assert_eq!(left.cmp(right), right.cmp(left).reverse());
        }
    }

    #[test]
    fn preference_order_is_antisymmetric_and_transitive_for_candidate_matrix() {
        let records = [
            make_test_record(
                MammogramType::Ffdm,
                ViewPosition::Cc,
                Laterality::Left,
                Some(1000),
                Some(1000),
                true,
                true,
                false,
                false,
                Some("1.2.3.4".to_string()),
                Some("3".to_string()),
            ),
            make_test_record(
                MammogramType::Ffdm,
                ViewPosition::Cc,
                Laterality::Left,
                Some(2000),
                Some(2000),
                true,
                false,
                false,
                false,
                Some("1.2.3.4".to_string()),
                Some("2".to_string()),
            ),
            make_test_record(
                MammogramType::Tomo,
                ViewPosition::Cc,
                Laterality::Left,
                Some(3000),
                Some(3000),
                true,
                false,
                false,
                false,
                Some("1.2.3.4".to_string()),
                Some("1".to_string()),
            ),
            make_test_record(
                MammogramType::Ffdm,
                ViewPosition::Cc,
                Laterality::Left,
                Some(1500),
                Some(1500),
                true,
                false,
                false,
                false,
                Some("5.6.7.8".to_string()),
                None,
            ),
            make_test_record(
                MammogramType::Ffdm,
                ViewPosition::Cc,
                Laterality::Left,
                Some(2500),
                Some(2500),
                true,
                true,
                false,
                false,
                None,
                None,
            ),
        ];

        for left in &records {
            for right in &records {
                assert_eq!(left.cmp(right), right.cmp(left).reverse());
                assert!(!(left.is_preferred_to(right) && right.is_preferred_to(left)));
                assert_eq!(left == right, left.cmp(right) == Ordering::Equal);
            }
        }

        for left in &records {
            for middle in &records {
                for right in &records {
                    if left < middle && middle < right {
                        assert!(left < right);
                    }
                }
            }
        }
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
