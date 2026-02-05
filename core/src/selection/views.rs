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

    // Run initial selection
    let selection = get_preferred_views_with_order(&filtered_records, preference_order);

    // Optionally enforce common modality
    if filter_config.require_common_modality {
        enforce_common_modality(&filtered_records, selection, preference_order)
    } else {
        selection
    }
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

/// Enforces that all selected views come from a single modality group (2D or DBT)
///
/// If the initial selection is already single-modality, returns it as-is.
/// Otherwise, re-runs selection on 2D-only and DBT-only record pools separately,
/// then picks the candidate with higher coverage, breaking ties by preference score
/// and defaulting to 2D.
fn enforce_common_modality(
    filtered_records: &[MammogramRecord],
    initial_selection: HashMap<MammogramView, Option<MammogramRecord>>,
    preference_order: PreferenceOrder,
) -> HashMap<MammogramView, Option<MammogramRecord>> {
    // If already single-modality, return as-is
    if is_single_modality(&initial_selection) {
        return initial_selection;
    }

    // Split records into 2D and DBT pools (Unknown excluded from both)
    let records_2d: Vec<MammogramRecord> = filtered_records
        .iter()
        .filter(|r| r.metadata.mammogram_type.is_2d_group())
        .cloned()
        .collect();

    let records_dbt: Vec<MammogramRecord> = filtered_records
        .iter()
        .filter(|r| r.metadata.mammogram_type.is_dbt_group())
        .cloned()
        .collect();

    let selection_2d = get_preferred_views_with_order(&records_2d, preference_order);
    let selection_dbt = get_preferred_views_with_order(&records_dbt, preference_order);

    let coverage_2d = count_coverage(&selection_2d);
    let coverage_dbt = count_coverage(&selection_dbt);

    if coverage_2d > coverage_dbt {
        selection_2d
    } else if coverage_dbt > coverage_2d {
        selection_dbt
    } else {
        // Equal coverage: tie-break by total preference score (lower wins)
        let score_2d = total_preference_score(&selection_2d, preference_order);
        let score_dbt = total_preference_score(&selection_dbt, preference_order);

        if score_dbt < score_2d {
            selection_dbt
        } else {
            // Equal score or 2D better: default to 2D for determinism
            selection_2d
        }
    }
}

/// Checks if all present views in a selection belong to a single modality group
fn is_single_modality(selection: &HashMap<MammogramView, Option<MammogramRecord>>) -> bool {
    let mut has_2d = false;
    let mut has_dbt = false;

    for record in selection.values().flatten() {
        let mt = &record.metadata.mammogram_type;
        if mt.is_2d_group() {
            has_2d = true;
        } else if mt.is_dbt_group() {
            has_dbt = true;
        } else {
            // Unknown type — not in either group, triggers re-computation
            return false;
        }
    }

    // Single-modality if we don't have both groups
    !(has_2d && has_dbt)
}

/// Counts the number of non-None entries in a selection
fn count_coverage(selection: &HashMap<MammogramView, Option<MammogramRecord>>) -> usize {
    selection.values().filter(|v| v.is_some()).count()
}

/// Sums preference values for all present views in a selection
fn total_preference_score(
    selection: &HashMap<MammogramView, Option<MammogramRecord>>,
    preference_order: PreferenceOrder,
) -> i32 {
    selection
        .values()
        .flatten()
        .map(|r| preference_order.preference_value(&r.metadata.mammogram_type))
        .sum()
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

    // --- Common modality enforcement tests ---

    #[test]
    fn test_is_single_modality_all_2d() {
        let mut selection = HashMap::new();
        selection.insert(
            MammogramView::new(Laterality::Left, ViewPosition::Mlo),
            Some(make_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
            )),
        );
        selection.insert(
            MammogramView::new(Laterality::Right, ViewPosition::Cc),
            Some(make_test_record(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Synth,
            )),
        );
        assert!(is_single_modality(&selection));
    }

    #[test]
    fn test_is_single_modality_all_dbt() {
        let mut selection = HashMap::new();
        selection.insert(
            MammogramView::new(Laterality::Left, ViewPosition::Mlo),
            Some(make_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Tomo,
            )),
        );
        selection.insert(
            MammogramView::new(Laterality::Right, ViewPosition::Cc),
            Some(make_test_record(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
            )),
        );
        assert!(is_single_modality(&selection));
    }

    #[test]
    fn test_is_single_modality_mixed() {
        let mut selection = HashMap::new();
        selection.insert(
            MammogramView::new(Laterality::Left, ViewPosition::Mlo),
            Some(make_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
            )),
        );
        selection.insert(
            MammogramView::new(Laterality::Right, ViewPosition::Cc),
            Some(make_test_record(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
            )),
        );
        assert!(!is_single_modality(&selection));
    }

    #[test]
    fn test_is_single_modality_empty() {
        let mut selection = HashMap::new();
        for view in STANDARD_MAMMO_VIEWS.iter() {
            selection.insert(*view, None);
        }
        // Vacuously single-modality
        assert!(is_single_modality(&selection));
    }

    #[test]
    fn test_is_single_modality_unknown_triggers_recomputation() {
        let mut selection = HashMap::new();
        selection.insert(
            MammogramView::new(Laterality::Left, ViewPosition::Mlo),
            Some(make_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Unknown,
            )),
        );
        assert!(!is_single_modality(&selection));
    }

    #[test]
    fn test_enforce_common_modality_already_single_2d() {
        // All 2D → returns early
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Synth),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial.clone(), PreferenceOrder::Default);

        for view in STANDARD_MAMMO_VIEWS.iter() {
            assert!(result[view].is_some());
            assert!(result[view]
                .as_ref()
                .unwrap()
                .metadata
                .mammogram_type
                .is_2d_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_already_single_dbt() {
        // All TOMO → returns early
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Tomo),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial.clone(), PreferenceOrder::Default);

        for view in STANDARD_MAMMO_VIEWS.iter() {
            assert!(result[view].is_some());
            assert_eq!(
                result[view].as_ref().unwrap().metadata.mammogram_type,
                MammogramType::Tomo
            );
        }
    }

    #[test]
    fn test_enforce_common_modality_mixed_higher_2d_coverage() {
        // 3 FFDM views + 1 TOMO view → 2D has 3, DBT has 1 → picks 2D
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        // Should pick 2D: 3 views vs 1
        assert_eq!(count_coverage(&result), 3);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_2d_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_mixed_higher_dbt_coverage() {
        // 1 FFDM view + 3 TOMO views → DBT has 3, 2D has 1 → picks DBT
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Tomo),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        assert_eq!(count_coverage(&result), 3);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_dbt_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_equal_coverage_tiebreak_by_score() {
        // 2 FFDM + 2 TOMO (equal coverage), Default order prefers FFDM (score 1) over TOMO (score 3)
        // 2D total score: 2, DBT total score: 6 → 2D wins by score
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        assert_eq!(count_coverage(&result), 2);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_2d_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_equal_coverage_tomo_first_prefers_dbt() {
        // With TomoFirst: TOMO score=1, FFDM score=2
        // 2 TOMO + 2 FFDM → DBT total=2, 2D total=4 → DBT wins by score
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Tomo),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::TomoFirst);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::TomoFirst);

        assert_eq!(count_coverage(&result), 2);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_dbt_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_equal_score_defaults_to_2d() {
        // Edge case: equal coverage AND equal score → defaults to 2D
        // This is hard to construct with real preference values, but let's test the logic
        // by using SFM (score 4 in Default) vs TOMO (score 3 in Default)
        // We can't get exact equal scores easily, so test with 0 coverage on both
        // Use unknown type that triggers re-computation
        let records_unknown = vec![make_test_record(
            Laterality::Left,
            ViewPosition::Mlo,
            MammogramType::Unknown,
        )];
        let initial = get_preferred_views_with_order(&records_unknown, PreferenceOrder::Default);
        let result = enforce_common_modality(&records_unknown, initial, PreferenceOrder::Default);

        // Unknown is excluded from both pools → both empty → 0 coverage each → 0 score → 2D wins
        assert_eq!(count_coverage(&result), 0);
    }

    #[test]
    fn test_enforce_common_modality_incomplete_single_modality() {
        // 2 FFDM views, no TOMO → single modality, returns early even if incomplete
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        assert_eq!(count_coverage(&result), 2);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_2d_group());
        }
    }

    #[test]
    fn test_enforce_common_modality_unknown_excluded_from_pools() {
        // Mix of FFDM + Unknown → not single-modality due to Unknown
        // Re-run: 2D pool has FFDM, DBT pool is empty → 2D wins
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Unknown),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Ffdm),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        // 2D pool: 3 FFDM views, DBT pool: 0 → 2D wins with 3 coverage
        assert_eq!(count_coverage(&result), 3);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_2d_group());
        }
    }

    #[test]
    fn test_get_preferred_views_filtered_with_common_modality() {
        // Integration test via get_preferred_views_filtered
        let config = FilterConfig::permissive().require_common_modality(true);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);

        // Should enforce common modality: 2D has 3 views, DBT has 1 → picks 2D
        assert_eq!(count_coverage(&selections), 3);
        for record in selections.values().flatten() {
            assert!(record.metadata.mammogram_type.is_2d_group());
        }
    }

    #[test]
    fn test_get_preferred_views_filtered_without_common_modality() {
        // Without flag, mixed results are kept
        let config = FilterConfig::permissive().require_common_modality(false);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Left, ViewPosition::Cc, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Cc, MammogramType::Tomo),
        ];

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);

        // All 4 views present (mixed is fine without the flag)
        assert_eq!(count_coverage(&selections), 4);
    }
}
