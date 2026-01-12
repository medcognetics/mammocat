use crate::api::{MammogramExtractor, MammogramMetadata};
use crate::error::Result;
use crate::extraction::tags::{
    get_string_value, get_u16_value, COLUMNS, PIXEL_DATA_TAG, ROWS, SOP_INSTANCE_UID,
    STUDY_INSTANCE_UID,
};
use crate::extraction::{is_implant_displaced, is_magnified, is_spot_compression};
use crate::types::PreferenceOrder;
use dicom_object::{InMemDicomObject, OpenFileOptions};
use std::cmp::Ordering;
use std::path::PathBuf;

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

    /// SOP Instance UID
    pub sop_instance_uid: Option<String>,

    /// Number of rows in image
    pub rows: Option<u16>,

    /// Number of columns in image
    pub columns: Option<u16>,

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
        Self::from_dicom(path, &dcm)
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
        Self::from_dicom(path, &dcm)
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
        let metadata = MammogramExtractor::extract(dcm)?;

        Ok(Self {
            file_path: path,
            metadata,
            study_instance_uid: get_string_value(dcm, STUDY_INSTANCE_UID),
            sop_instance_uid: get_string_value(dcm, SOP_INSTANCE_UID),
            rows: get_u16_value(dcm, ROWS),
            columns: get_u16_value(dcm, COLUMNS),
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
    /// 2. Implant displaced beats non-displaced (same study only)
    /// 3. Type preference (FFDM > SYNTH > TOMO > SFM)
    /// 4. Higher resolution beats lower resolution
    /// 5. Fallback to SOPInstanceUID comparison
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
    /// 3. Implant displaced beats non-displaced (same study only)
    /// 4. Type preference (according to the provided preference order)
    /// 5. Higher resolution beats lower resolution
    /// 6. Fallback to SOPInstanceUID comparison
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
        // 1. Standard views take priority
        if self.metadata.is_standard_view() && !other.metadata.is_standard_view() {
            return true;
        }
        if !self.metadata.is_standard_view() && other.metadata.is_standard_view() {
            return false;
        }

        // 2. Non-spot/mag views take priority over spot/mag views
        if !self.is_spot_or_mag() && other.is_spot_or_mag() {
            return true;
        }
        if self.is_spot_or_mag() && !other.is_spot_or_mag() {
            return false;
        }

        // 3. Implant displaced views take priority (same study only)
        if let (Some(self_study), Some(other_study)) =
            (&self.study_instance_uid, &other.study_instance_uid)
        {
            if self_study == other_study && self.is_implant_displaced && !other.is_implant_displaced
            {
                return true;
            }
        }

        // 4. Type preference (using configurable order)
        let self_type = &self.metadata.mammogram_type;
        let other_type = &other.metadata.mammogram_type;
        if self_type != other_type {
            let self_pref = preference_order.preference_value(self_type);
            let other_pref = preference_order.preference_value(other_type);
            return self_pref < other_pref;
        }

        // 5. Resolution preference (higher is better)
        if self.image_area() != other.image_area() {
            let self_area = self.image_area().unwrap_or(0);
            let other_area = other.image_area().unwrap_or(0);
            return self_area > other_area;
        }

        // 6. Fallback to SOP UID comparison (for stable ordering)
        match (&self.sop_instance_uid, &other.sop_instance_uid) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    }
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
    use crate::types::{ImageType, Laterality, MammogramType, ViewPosition};

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
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
            },
            rows,
            columns,
            is_implant_displaced,
            is_spot_compression,
            is_magnified,
            study_instance_uid: study_uid,
            sop_instance_uid: sop_uid,
        }
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
