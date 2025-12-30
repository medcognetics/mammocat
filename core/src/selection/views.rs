use crate::selection::record::MammogramRecord;
use crate::types::{MammogramView, PreferenceOrder, STANDARD_MAMMO_VIEWS};
use std::collections::HashMap;

/// Selects preferred inference views from a collection of mammogram records
///
/// For each of the 4 standard views (L-MLO, R-MLO, L-CC, R-CC), selects the
/// most preferred mammogram based on comparison logic.
///
/// Implements Python: dicom_utils/container/record.py:968-1002
///
/// # Arguments
///
/// * `records` - Slice of MammogramRecord to select from
///
/// # Returns
///
/// HashMap mapping each standard view to the selected record (or None if not found)
pub fn get_preferred_views(
    records: &[MammogramRecord],
) -> HashMap<MammogramView, Option<MammogramRecord>> {
    get_preferred_views_with_order(records, PreferenceOrder::default())
}

/// Selects preferred inference views using a specific preference order
///
/// For each of the 4 standard views (L-MLO, R-MLO, L-CC, R-CC), selects the
/// most preferred mammogram based on comparison logic using the specified preference order.
///
/// # Arguments
///
/// * `records` - Slice of MammogramRecord to select from
/// * `preference_order` - The preference ordering strategy to use
///
/// # Returns
///
/// HashMap mapping each standard view to the selected record (or None if not found)
pub fn get_preferred_views_with_order(
    records: &[MammogramRecord],
    preference_order: PreferenceOrder,
) -> HashMap<MammogramView, Option<MammogramRecord>> {
    let mut result = HashMap::new();

    // Try each standard view
    for standard_view in STANDARD_MAMMO_VIEWS.iter() {
        let candidates: Vec<_> = records
            .iter()
            .filter(|record| is_candidate_for_view(record, standard_view))
            .collect();

        // Select most preferred from candidates using the specified preference order
        let selection = candidates
            .into_iter()
            .min_by(|a, b| {
                if a.is_preferred_to_with_order(b, preference_order) {
                    std::cmp::Ordering::Less
                } else if b.is_preferred_to_with_order(a, preference_order) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .cloned();
        result.insert(*standard_view, selection);
    }

    result
}

/// Checks if a record is a candidate for a standard view
///
/// Matches Python logic:
/// - Laterality must match exactly
/// - View must be MLO-like or CC-like (depending on target view)
///
/// # Arguments
///
/// * `record` - Record to check
/// * `target` - Target standard view
///
/// # Returns
///
/// `true` if the record is a candidate for the target view
fn is_candidate_for_view(record: &MammogramRecord, target: &MammogramView) -> bool {
    let candidate_view = record.metadata.mammogram_view();

    // Laterality must match
    if candidate_view.laterality != target.laterality {
        return false;
    }

    // View must be appropriate type (MLO-like or CC-like)
    if target.view.is_mlo_like() {
        candidate_view.is_mlo_like()
    } else if target.view.is_cc_like() {
        candidate_view.is_cc_like()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageType, Laterality, MammogramType, PreferenceOrder, ViewPosition};
    use std::path::PathBuf;

    fn make_test_record(
        laterality: Laterality,
        view_pos: ViewPosition,
        mammo_type: MammogramType,
    ) -> MammogramRecord {
        MammogramRecord {
            file_path: PathBuf::from(format!("{:?}_{:?}.dcm", laterality, view_pos)),
            metadata: crate::api::MammogramMetadata {
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
                is_spot_compression: false,
                is_magnified: false,
                is_implant_displaced: false,
                manufacturer: None,
                model: None,
                number_of_frames: 1,
            },
            rows: Some(2560),
            columns: Some(3328),
            is_implant_displaced: false,
            is_spot_compression: false,
            is_magnified: false,
            study_instance_uid: None,
            sop_instance_uid: None,
        }
    }

    #[test]
    fn test_is_candidate_for_view_laterality_match() {
        let l_mlo_view = MammogramView::new(Laterality::Left, ViewPosition::Mlo);
        let r_mlo_view = MammogramView::new(Laterality::Right, ViewPosition::Mlo);

        let left_record =
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        let right_record =
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm);

        // Left record should be candidate for left view only
        assert!(is_candidate_for_view(&left_record, &l_mlo_view));
        assert!(!is_candidate_for_view(&left_record, &r_mlo_view));

        // Right record should be candidate for right view only
        assert!(!is_candidate_for_view(&right_record, &l_mlo_view));
        assert!(is_candidate_for_view(&right_record, &r_mlo_view));
    }

    #[test]
    fn test_is_candidate_for_view_mlo_like() {
        let mlo_view = MammogramView::new(Laterality::Left, ViewPosition::Mlo);

        // MLO, ML, Lmo, Lm are all MLO-like
        let mlo_record = make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        let ml_record = make_test_record(Laterality::Left, ViewPosition::Ml, MammogramType::Ffdm);
        let lmo_record = make_test_record(Laterality::Left, ViewPosition::Lmo, MammogramType::Ffdm);
        let lm_record = make_test_record(Laterality::Left, ViewPosition::Lm, MammogramType::Ffdm);

        assert!(is_candidate_for_view(&mlo_record, &mlo_view));
        assert!(is_candidate_for_view(&ml_record, &mlo_view));
        assert!(is_candidate_for_view(&lmo_record, &mlo_view));
        assert!(is_candidate_for_view(&lm_record, &mlo_view));

        // CC should not be candidate for MLO view
        let cc_record = make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm);
        assert!(!is_candidate_for_view(&cc_record, &mlo_view));
    }

    #[test]
    fn test_is_candidate_for_view_cc_like() {
        let cc_view = MammogramView::new(Laterality::Right, ViewPosition::Cc);

        // CC, XCCL, XCCM are all CC-like
        let cc_record = make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm);
        let xccl_record =
            make_test_record(Laterality::Right, ViewPosition::Xccl, MammogramType::Ffdm);
        let xccm_record =
            make_test_record(Laterality::Right, ViewPosition::Xccm, MammogramType::Ffdm);

        assert!(is_candidate_for_view(&cc_record, &cc_view));
        assert!(is_candidate_for_view(&xccl_record, &cc_view));
        assert!(is_candidate_for_view(&xccm_record, &cc_view));

        // MLO should not be candidate for CC view
        let mlo_record =
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm);
        assert!(!is_candidate_for_view(&mlo_record, &cc_view));
    }

    #[test]
    fn test_get_preferred_views_basic() {
        // Create 4 standard views
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm),
        ];

        let selections = get_preferred_views(&records);

        // Should have all 4 standard views
        assert_eq!(selections.len(), 4);

        // Each should have a selection
        for view in STANDARD_MAMMO_VIEWS.iter() {
            assert!(selections.contains_key(view));
            assert!(selections[view].is_some());
        }
    }

    #[test]
    fn test_get_preferred_views_missing() {
        // Only create 3 views (missing R-CC)
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
        ];

        let selections = get_preferred_views(&records);

        // Should have all 4 standard views in result
        assert_eq!(selections.len(), 4);

        // First 3 should have selections
        assert!(selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)].is_some());
        assert!(selections[&MammogramView::new(Laterality::Right, ViewPosition::Mlo)].is_some());
        assert!(selections[&MammogramView::new(Laterality::Left, ViewPosition::Cc)].is_some());

        // R-CC should be None
        assert!(selections[&MammogramView::new(Laterality::Right, ViewPosition::Cc)].is_none());
    }

    #[test]
    fn test_get_preferred_views_type_preference() {
        // Create multiple of same view with different types
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Synth),
        ];

        let selections = get_preferred_views(&records);

        // Should select FFDM (most preferred with default ordering)
        let selected = selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
            .as_ref()
            .unwrap();

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Ffdm);
    }

    #[test]
    fn test_get_preferred_views_default_order() {
        // Create multiple of same view with different types
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Synth),
        ];

        let selections = get_preferred_views_with_order(&records, PreferenceOrder::Default);

        // Should select FFDM (most preferred with Default ordering)
        let selected = selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
            .as_ref()
            .unwrap();

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Ffdm);
    }

    #[test]
    fn test_get_preferred_views_tomo_first_order() {
        // Create multiple of same view with different types
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Synth),
        ];

        let selections = get_preferred_views_with_order(&records, PreferenceOrder::TomoFirst);

        // Should select TOMO (most preferred with TomoFirst ordering)
        let selected = selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
            .as_ref()
            .unwrap();

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Tomo);
    }

    #[test]
    fn test_get_preferred_views_empty() {
        let records: Vec<MammogramRecord> = vec![];
        let selections = get_preferred_views(&records);

        // Should have all 4 standard views, but all None
        assert_eq!(selections.len(), 4);
        for view in STANDARD_MAMMO_VIEWS.iter() {
            assert!(selections[view].is_none());
        }
    }
}
