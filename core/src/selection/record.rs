use crate::api::{MammogramExtractor, MammogramMetadata};
use crate::error::Result;
use crate::extraction::is_implant_displaced;
use crate::extraction::tags::{
    get_string_value, get_u16_value, COLUMNS, ROWS, SOP_INSTANCE_UID, STUDY_INSTANCE_UID,
};
use dicom_object::{open_file, InMemDicomObject};
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
}

impl MammogramRecord {
    /// Creates a record from a DICOM file path
    ///
    /// # Arguments
    ///
    /// * `path` - Path to DICOM file
    ///
    /// # Returns
    ///
    /// Result containing the MammogramRecord or an error
    pub fn from_file(path: PathBuf) -> Result<Self> {
        let dcm = open_file(&path)?;
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

    /// Checks if this record is preferred over another
    ///
    /// Implements Python logic from record.py:805-838
    ///
    /// Priority order:
    /// 1. Standard views beat non-standard views
    /// 2. Implant displaced beats non-displaced (same study only)
    /// 3. Type preference (TOMO < FFDM < SYNTH < SFM)
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
        // 1. Standard views take priority
        if self.metadata.is_standard_view() && !other.metadata.is_standard_view() {
            return true;
        }

        // 2. Implant displaced views take priority (same study only)
        if let (Some(self_study), Some(other_study)) =
            (&self.study_instance_uid, &other.study_instance_uid)
        {
            if self_study == other_study && self.is_implant_displaced && !other.is_implant_displaced
            {
                return true;
            }
        }

        // 3. Type preference
        let self_type = &self.metadata.mammogram_type;
        let other_type = &other.metadata.mammogram_type;
        if self_type != other_type {
            return self_type.is_preferred_to(other_type);
        }

        // 4. Resolution preference (higher is better)
        if self.image_area() != other.image_area() {
            let self_area = self.image_area().unwrap_or(0);
            let other_area = other.image_area().unwrap_or(0);
            return self_area > other_area;
        }

        // 5. Fallback to SOP UID comparison (for stable ordering)
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
                manufacturer: None,
                model: None,
                number_of_frames: 1,
            },
            rows,
            columns,
            is_implant_displaced,
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
            None,
            None,
        );

        // TOMO is preferred over FFDM
        assert!(tomo.is_preferred_to(&ffdm));
        assert!(!ffdm.is_preferred_to(&tomo));
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
            MammogramType::Tomo,
            ViewPosition::Cc,
            Laterality::Left,
            Some(3000),
            Some(4000),
            true,
            false,
            None,
            Some("AAA".to_string()),
        );

        let worse = make_test_record(
            MammogramType::Ffdm,
            ViewPosition::Cc,
            Laterality::Left,
            Some(2000),
            Some(2500),
            true,
            false,
            None,
            Some("BBB".to_string()),
        );

        // Better record should be "less than" (more preferred)
        assert!(better < worse);
        assert!(worse > better);

        // Min should select the better record
        assert_eq!(std::cmp::min(&better, &worse), &better);
    }
}
