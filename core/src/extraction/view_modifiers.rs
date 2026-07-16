use std::collections::BTreeSet;

use crate::types::MammographyViewModifier;
use dicom_object::InMemDicomObject;

use super::view_position::extract_view_descriptor;

/// Extract all recognized CID 4015 modifiers from standard coded sequences and
/// supported legacy evidence.
pub fn extract_view_modifiers(dcm: &InMemDicomObject) -> BTreeSet<MammographyViewModifier> {
    extract_view_descriptor(dcm).modifiers
}

/// Return normalized modifier names for callers that used the former
/// CodeMeaning-only API.
pub fn extract_view_modifier_meanings(dcm: &InMemDicomObject) -> Vec<String> {
    extract_view_modifiers(dcm)
        .into_iter()
        .map(|modifier| modifier.simple_name().to_string())
        .collect()
}

pub fn is_implant_displaced(dcm: &InMemDicomObject) -> bool {
    extract_view_modifiers(dcm).contains(&MammographyViewModifier::ImplantDisplaced)
}

pub fn is_spot_compression(dcm: &InMemDicomObject) -> bool {
    extract_view_modifiers(dcm).contains(&MammographyViewModifier::SpotCompression)
}

pub fn is_magnified(dcm: &InMemDicomObject) -> bool {
    extract_view_modifiers(dcm).contains(&MammographyViewModifier::Magnification)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dataset_has_no_modifiers() {
        let dcm = InMemDicomObject::new_empty();
        assert!(extract_view_modifiers(&dcm).is_empty());
        assert!(!is_implant_displaced(&dcm));
        assert!(!is_spot_compression(&dcm));
        assert!(!is_magnified(&dcm));
    }
}
