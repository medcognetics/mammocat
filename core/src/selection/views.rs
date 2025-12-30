use crate::selection::record::MammogramRecord;
use crate::types::{FilterConfig, MammogramView, PreferenceOrder, STANDARD_MAMMO_VIEWS};
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

/// Selects preferred inference views from a filtered collection of mammogram records
///
/// Applies filters before selecting preferred views. For each of the 4 standard views
/// (L-MLO, R-MLO, L-CC, R-CC), selects the most preferred mammogram from filtered candidates.
///
/// # Arguments
///
/// * `records` - Slice of MammogramRecord to select from
/// * `filter_config` - Filter configuration to apply before selection
/// * `preference_order` - The preference ordering strategy to use
///
/// # Returns
///
/// HashMap mapping each standard view to the selected record (or None if not found)
///
/// # Example
///
/// ```
/// use mammocat_core::{FilterConfig, MammogramRecord, PreferenceOrder, get_preferred_views_filtered};
/// use std::collections::HashSet;
/// use mammocat_core::MammogramType;
///
/// // Create a filter that only allows FFDM and TOMO, excludes implants
/// let mut allowed_types = HashSet::new();
/// allowed_types.insert(MammogramType::Ffdm);
/// allowed_types.insert(MammogramType::Tomo);
///
/// let filter = FilterConfig::default()
///     .with_allowed_types(allowed_types)
///     .exclude_implants(true);
///
/// # let records = vec![]; // Would normally load from files
/// let selections = get_preferred_views_filtered(&records, &filter, PreferenceOrder::Default);
/// ```
pub fn get_preferred_views_filtered(
    records: &[MammogramRecord],
    filter_config: &FilterConfig,
    preference_order: PreferenceOrder,
) -> HashMap<MammogramView, Option<MammogramRecord>> {
    // Apply filters first
    let filtered_records = apply_filters(records, filter_config);

    // Then select preferred views from filtered set
    get_preferred_views_with_order(&filtered_records, preference_order)
}

/// Applies filters to a collection of records
///
/// Returns a new vector containing only records that pass all filters.
///
/// # Arguments
///
/// * `records` - Slice of MammogramRecord to filter
/// * `config` - Filter configuration
///
/// # Returns
///
/// Vector of records that pass all filters
fn apply_filters(records: &[MammogramRecord], config: &FilterConfig) -> Vec<MammogramRecord> {
    records
        .iter()
        .filter(|record| {
            // Filter: Allowed types (whitelist)
            if let Some(allowed_types) = &config.allowed_types {
                if !allowed_types.contains(&record.metadata.mammogram_type) {
                    return false;
                }
            }

            // Filter: Exclude implants
            if config.exclude_implants && record.metadata.has_implant {
                return false;
            }

            // Filter: Exclude non-standard views
            if config.exclude_non_standard_views && !record.metadata.is_standard_view() {
                return false;
            }

            // Filter: Exclude FOR PROCESSING
            if config.exclude_for_processing && record.metadata.is_for_processing {
                return false;
            }

            // Filter: Exclude secondary capture
            if config.exclude_secondary_capture && record.metadata.is_secondary_capture {
                return false;
            }

            // Filter: Exclude non-MG modality
            if config.exclude_non_mg_modality {
                if let Some(modality) = &record.metadata.modality {
                    if modality.to_uppercase() != "MG" {
                        return false;
                    }
                } else {
                    // No modality tag = exclude if filter is enabled
                    return false;
                }
            }

            true
        })
        .cloned()
        .collect()
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
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
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

    #[test]
    fn test_apply_filters_allowed_types() {
        use std::collections::HashSet;

        let mut allowed_types = HashSet::new();
        allowed_types.insert(MammogramType::Ffdm);

        let config = FilterConfig::default().with_allowed_types(allowed_types);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Synth),
        ];

        let filtered = apply_filters(&records, &config);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].metadata.mammogram_type, MammogramType::Ffdm);
    }

    #[test]
    fn test_apply_filters_exclude_implants() {
        let config = FilterConfig::default().exclude_implants(true);

        let mut record_with_implant =
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        record_with_implant.metadata.has_implant = true;

        let record_without_implant =
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm);

        let records = vec![record_with_implant, record_without_implant];
        let filtered = apply_filters(&records, &config);

        assert_eq!(filtered.len(), 1);
        assert!(!filtered[0].metadata.has_implant);
    }

    #[test]
    fn test_apply_filters_exclude_non_standard() {
        let config = FilterConfig::default().exclude_non_standard_views(true);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Ml, MammogramType::Ffdm),
        ];

        let filtered = apply_filters(&records, &config);
        assert_eq!(filtered.len(), 2); // Only MLO and CC
    }

    #[test]
    fn test_apply_filters_exclude_for_processing() {
        let config = FilterConfig::default().exclude_for_processing(true);

        let mut for_processing_record =
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        for_processing_record.metadata.is_for_processing = true;

        let presentation_record =
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm);

        let records = vec![for_processing_record, presentation_record];
        let filtered = apply_filters(&records, &config);

        assert_eq!(filtered.len(), 1);
        assert!(!filtered[0].metadata.is_for_processing);
    }

    #[test]
    fn test_apply_filters_exclude_secondary_capture() {
        let config = FilterConfig::default().exclude_secondary_capture(true);

        let mut secondary_capture_record =
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        secondary_capture_record.metadata.is_secondary_capture = true;

        let regular_record =
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm);

        let records = vec![secondary_capture_record, regular_record];
        let filtered = apply_filters(&records, &config);

        assert_eq!(filtered.len(), 1);
        assert!(!filtered[0].metadata.is_secondary_capture);
    }

    #[test]
    fn test_apply_filters_exclude_non_mg_modality() {
        let config = FilterConfig::default().exclude_non_mg_modality(true);

        let mut ct_record =
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm);
        ct_record.metadata.modality = Some("CT".to_string());

        let mut mg_record =
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm);
        mg_record.metadata.modality = Some("MG".to_string());

        let mut no_modality_record =
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm);
        no_modality_record.metadata.modality = None;

        let records = vec![ct_record, mg_record, no_modality_record];
        let filtered = apply_filters(&records, &config);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].metadata.modality.as_deref().unwrap(), "MG");
    }

    #[test]
    fn test_get_preferred_views_filtered() {
        use std::collections::HashSet;

        let mut allowed_types = HashSet::new();
        allowed_types.insert(MammogramType::Ffdm);

        let config = FilterConfig::default().with_allowed_types(allowed_types);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm),
        ];

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);

        // Should only select FFDM records
        assert!(selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)].is_some());
        assert_eq!(
            selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
                .as_ref()
                .unwrap()
                .metadata
                .mammogram_type,
            MammogramType::Ffdm
        );
    }
}
