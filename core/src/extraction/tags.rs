use dicom_core::Tag;
use dicom_object::InMemDicomObject;

// Core Image Tags
pub const IMAGE_TYPE: Tag = Tag(0x0008, 0x0008);
pub const MODALITY: Tag = Tag(0x0008, 0x0060);
pub const NUMBER_OF_FRAMES: Tag = Tag(0x0028, 0x0008);
pub const PHOTOMETRIC_INTERPRETATION: Tag = Tag(0x0028, 0x0004);

// Image Geometry Tags
pub const ROWS: Tag = Tag(0x0028, 0x0010);
pub const COLUMNS: Tag = Tag(0x0028, 0x0011);
pub const BITS_STORED: Tag = Tag(0x0028, 0x0101);
pub const PIXEL_SPACING: Tag = Tag(0x0028, 0x0030);
pub const IMAGER_PIXEL_SPACING: Tag = Tag(0x0018, 0x1164);

// View Position Tags
pub const VIEW_POSITION: Tag = Tag(0x0018, 0x5101);
pub const VIEW_CODE_SEQUENCE: Tag = Tag(0x0054, 0x0220);
pub const VIEW_MODIFIER_CODE_SEQUENCE: Tag = Tag(0x0054, 0x0222);
pub const CODE_MEANING: Tag = Tag(0x0008, 0x0104);
pub const FRAME_LATERALITY: Tag = Tag(0x0020, 0x9072);
pub const FRAME_ANATOMY_SEQUENCE: Tag = Tag(0x0020, 0x9071);

// Laterality Tags
pub const LATERALITY: Tag = Tag(0x0020, 0x0060);
pub const IMAGE_LATERALITY: Tag = Tag(0x0020, 0x0062);
pub const PATIENT_ORIENTATION: Tag = Tag(0x0020, 0x0020);

// Anatomical Tags
pub const SHARED_FUNCTIONAL_GROUPS_SEQUENCE: Tag = Tag(0x5200, 0x9229);
pub const BODY_PART_EXAMINED: Tag = Tag(0x0018, 0x0015);

// Device/Manufacturer Tags
pub const MANUFACTURER_MODEL_NAME: Tag = Tag(0x0008, 0x1090);
pub const MANUFACTURER: Tag = Tag(0x0008, 0x0070);
pub const MANUFACTURER_MODEL_NUMBER: Tag = Tag(0x0018, 0x1020);
pub const TRANSFER_SYNTAX_UID: Tag = Tag(0x0002, 0x0010);

// Study/Series Identification Tags
pub const STUDY_INSTANCE_UID: Tag = Tag(0x0020, 0x000D);
pub const SERIES_INSTANCE_UID: Tag = Tag(0x0020, 0x000E);
pub const SOP_INSTANCE_UID: Tag = Tag(0x0008, 0x0018);
pub const SOP_CLASS_UID: Tag = Tag(0x0008, 0x0016);
pub const STUDY_DATE: Tag = Tag(0x0008, 0x0020);
pub const CONTENT_DATE: Tag = Tag(0x0008, 0x0023);
pub const ACQUISITION_DATE: Tag = Tag(0x0018, 0x1012);

// Description Tags
pub const SERIES_DESCRIPTION: Tag = Tag(0x0008, 0x103E);
pub const STUDY_DESCRIPTION: Tag = Tag(0x0008, 0x1030);
pub const PERFORMED_PROCEDURE_STEP_DESCRIPTION: Tag = Tag(0x0040, 0x0254);

// Patient Tags
pub const PATIENT_NAME: Tag = Tag(0x0010, 0x0010);
pub const PATIENT_ID: Tag = Tag(0x0010, 0x0020);
pub const PATIENT_AGE: Tag = Tag(0x0010, 0x1010);
pub const PATIENT_BIRTH_DATE: Tag = Tag(0x0010, 0x0030);
pub const STUDY_ID: Tag = Tag(0x0020, 0x0010);

// Institution/Site Tags
pub const INSTITUTION_NAME: Tag = Tag(0x0008, 0x0080);
pub const INSTITUTION_ADDRESS: Tag = Tag(0x0008, 0x0081);

// Breast-Specific Tags
pub const PADDLE_DESCRIPTION: Tag = Tag(0x0018, 0x1405);
pub const BREAST_IMPLANT_PRESENT: Tag = Tag(0x0028, 0x1300);
pub const BODY_PART_THICKNESS: Tag = Tag(0x0018, 0x1075);

// Other Tags
pub const PRESENTATION_INTENT_TYPE: Tag = Tag(0x0008, 0x0068);
pub const ACCESSION_NUMBER: Tag = Tag(0x0008, 0x0050);

/// Helper to get string value from DICOM tag
///
/// Returns `None` if the tag is not present or cannot be converted to string
pub fn get_string_value(dcm: &InMemDicomObject, tag: Tag) -> Option<String> {
    dcm.element(tag)
        .ok()
        .and_then(|elem| elem.to_str().ok())
        .map(|s| s.trim().to_string())
}

/// Helper to get integer value from DICOM tag
///
/// Returns `None` if the tag is not present or cannot be converted to i32
pub fn get_int_value(dcm: &InMemDicomObject, tag: Tag) -> Option<i32> {
    dcm.element(tag)
        .ok()
        .and_then(|elem| elem.to_int::<i32>().ok())
}

/// Helper to get multi-string value from DICOM tag
///
/// Returns `None` if the tag is not present or cannot be converted to Vec<String>
pub fn get_multi_string_value(dcm: &InMemDicomObject, tag: Tag) -> Option<Vec<String>> {
    dcm.element(tag).ok().and_then(|elem| {
        // Try to get as multi-string
        if let Ok(strs) = elem.to_multi_str() {
            Some(strs.iter().map(|s| s.to_string()).collect())
        } else {
            // Fallback: try to get as single string and split by backslash
            elem.to_str()
                .ok()
                .map(|s| s.split('\\').map(|part| part.trim().to_string()).collect())
        }
    })
}

/// Helper to get u16 value from DICOM tag
///
/// Returns `None` if the tag is not present or cannot be converted to u16
pub fn get_u16_value(dcm: &InMemDicomObject, tag: Tag) -> Option<u16> {
    dcm.element(tag)
        .ok()
        .and_then(|elem| elem.to_int::<u16>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_values() {
        // Just ensure tags are correctly defined
        assert_eq!(IMAGE_TYPE, Tag(0x0008, 0x0008));
        assert_eq!(MODALITY, Tag(0x0008, 0x0060));
        assert_eq!(NUMBER_OF_FRAMES, Tag(0x0028, 0x0008));
        assert_eq!(LATERALITY, Tag(0x0020, 0x0060));
        assert_eq!(VIEW_POSITION, Tag(0x0018, 0x5101));
    }
}
