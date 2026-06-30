use crate::error::{MammocatError, Result};
use crate::selection::record::MammogramRecord;
use crate::types::{
    DbtObjectKind, FilterConfig, Laterality, MammogramType, MammogramView, PreferenceOrder,
    ViewPosition, STANDARD_MAMMO_VIEWS,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

const MIXED_STUDY_WARNING_PREFIX: &str = "mixed study input detected";
const SPLIT_SLICE_SERIES_COUNT_THRESHOLD: usize = 12;

/// Study handling policy for preferred-view selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudySelectionMode {
    /// Choose the most complete study before selecting preferred views.
    MostComplete,
    /// Require all usable candidate records to belong to one known study.
    StrictSingleStudy,
}

impl StudySelectionMode {
    /// Converts a boolean strict flag from adapter layers into the core mode.
    pub fn from_strict(strict: bool) -> Self {
        if strict {
            Self::StrictSingleStudy
        } else {
            Self::MostComplete
        }
    }
}

/// Non-fatal warning produced during preferred-view selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionWarning {
    message: String,
}

impl SelectionWarning {
    /// User-facing warning message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Preferred-view selection result map.
pub type PreferredViewSelection = HashMap<MammogramView, Option<MammogramRecord>>;

/// Preferred-view selection result with non-fatal warnings.
pub type PreferredViewSelectionWithWarnings = (PreferredViewSelection, Vec<SelectionWarning>);

/// Collection-context reason for DBT classification refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbtRefinementReason {
    /// A same-series ambiguous group exceeded the split-slice cardinality threshold.
    SplitSliceSeriesCardinality,
    /// A singleton matched exactly one split DBT series by source SOP UID.
    UniqueSplitSeriesSourcePair,
    /// A singleton matched exactly one split DBT series by study/laterality/view.
    UniqueSplitSeriesViewPair,
}

impl DbtRefinementReason {
    /// Stable diagnostic code for reports and JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SplitSliceSeriesCardinality => "split_slice_series_cardinality",
            Self::UniqueSplitSeriesSourcePair => "unique_split_series_source_pair",
            Self::UniqueSplitSeriesViewPair => "unique_split_series_view_pair",
        }
    }
}

/// Diagnostic emitted when collection context changes a DBT classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbtRefinementDiagnostic {
    pub file_path: PathBuf,
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub sop_instance_uid: Option<String>,
    pub original_mammogram_type: MammogramType,
    pub original_dbt_object_kind: DbtObjectKind,
    pub refined_mammogram_type: MammogramType,
    pub refined_dbt_object_kind: DbtObjectKind,
    pub reason: DbtRefinementReason,
}

#[derive(Debug, Clone)]
struct StudyGroup {
    study_instance_uid: Option<String>,
    records: Vec<MammogramRecord>,
    standard_slot_count: usize,
    candidate_slot_count: usize,
    unknown_sort_key: Option<(String, String)>,
}

#[derive(Debug, Clone)]
struct SelectedStudyRecords {
    records: Vec<MammogramRecord>,
    warnings: Vec<SelectionWarning>,
}

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
pub fn get_preferred_views(records: &[MammogramRecord]) -> PreferredViewSelection {
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
) -> PreferredViewSelection {
    let (selection, warnings) =
        get_preferred_views_with_order_and_warnings(records, preference_order);
    log_selection_warnings(&warnings);
    selection
}

/// Selects preferred inference views and returns non-fatal selection warnings.
pub fn get_preferred_views_with_order_and_warnings(
    records: &[MammogramRecord],
    preference_order: PreferenceOrder,
) -> PreferredViewSelectionWithWarnings {
    let refined_records = refine_dbt_object_classification(records);
    let selected_study =
        select_study_records(&refined_records, StudySelectionMode::MostComplete, false)
            .expect("most-complete study selection should not fail");
    let selection =
        select_preferred_views_for_records(&selected_study.records, preference_order, true);
    (selection, selected_study.warnings)
}

fn select_preferred_views_for_records(
    records: &[MammogramRecord],
    preference_order: PreferenceOrder,
    deprioritize_lossy_compressed: bool,
) -> PreferredViewSelection {
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
                compare_record_preference(a, b, preference_order, deprioritize_lossy_compressed)
            })
            .cloned();
        result.insert(*standard_view, selection);
    }

    result
}

fn compare_record_preference(
    a: &MammogramRecord,
    b: &MammogramRecord,
    preference_order: PreferenceOrder,
    deprioritize_lossy_compressed: bool,
) -> Ordering {
    if a.is_preferred_to_with_options(b, preference_order, deprioritize_lossy_compressed) {
        Ordering::Less
    } else if b.is_preferred_to_with_options(a, preference_order, deprioritize_lossy_compressed) {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
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
) -> PreferredViewSelection {
    get_preferred_views_filtered_with_study_mode(
        records,
        filter_config,
        preference_order,
        StudySelectionMode::MostComplete,
    )
    .expect("most-complete study selection should not fail")
}

/// Selects preferred inference views with filtering and explicit study handling.
///
/// Filters are applied before study selection. In [`StudySelectionMode::MostComplete`],
/// the most complete study is selected deterministically before view ranking. In
/// [`StudySelectionMode::StrictSingleStudy`], selection fails if usable candidate
/// records span multiple studies or lack `StudyInstanceUID`.
pub fn get_preferred_views_filtered_with_study_mode(
    records: &[MammogramRecord],
    filter_config: &FilterConfig,
    preference_order: PreferenceOrder,
    study_selection_mode: StudySelectionMode,
) -> Result<PreferredViewSelection> {
    let (selection, warnings) = get_preferred_views_filtered_with_study_mode_and_warnings(
        records,
        filter_config,
        preference_order,
        study_selection_mode,
    )?;
    log_selection_warnings(&warnings);
    Ok(selection)
}

/// Selects preferred inference views with filtering and returns non-fatal warnings.
pub fn get_preferred_views_filtered_with_study_mode_and_warnings(
    records: &[MammogramRecord],
    filter_config: &FilterConfig,
    preference_order: PreferenceOrder,
    study_selection_mode: StudySelectionMode,
) -> Result<PreferredViewSelectionWithWarnings> {
    let refined_records = refine_dbt_object_classification(records);
    let filtered_records = apply_filters(&refined_records, filter_config);
    let selected_study = select_study_records(
        &filtered_records,
        study_selection_mode,
        filter_config.require_common_modality,
    )?;

    // Run initial selection
    let selection = select_preferred_views_for_records(
        &selected_study.records,
        preference_order,
        filter_config.deprioritize_lossy_compressed,
    );

    // Optionally enforce common modality
    let selection = if filter_config.require_common_modality {
        enforce_common_modality_with_options(
            &selected_study.records,
            selection,
            preference_order,
            filter_config.deprioritize_lossy_compressed,
        )
    } else {
        selection
    };

    Ok((selection, selected_study.warnings))
}

/// Refines ambiguous single-file DBT classifications using collection context.
///
/// Single-file extraction intentionally reports Fuji-like split-slice/SYN2D
/// signatures as `Unknown/Unknown` because those objects can be metadata-identical.
/// When a collection is available, series cardinality and source-object pairing can
/// safely resolve the common Fuji layout without relying on filenames or UID suffixes.
pub fn refine_dbt_object_classification(records: &[MammogramRecord]) -> Vec<MammogramRecord> {
    let (records, _) = refine_dbt_object_classification_with_diagnostics(records);
    records
}

/// Refines ambiguous DBT classifications and reports why records changed.
pub fn refine_dbt_object_classification_with_diagnostics(
    records: &[MammogramRecord],
) -> (Vec<MammogramRecord>, Vec<DbtRefinementDiagnostic>) {
    let mut refined_records = records.to_vec();
    let mut diagnostics = Vec::new();
    let series_infos = build_series_infos(records);
    let split_slice_series = split_slice_series_keys_from_cardinality(&series_infos);
    let split_series_by_source =
        index_split_series_by_source_uid(&series_infos, &split_slice_series);
    let view_pairing = view_pairing_candidates_by_study_view(&series_infos, &split_slice_series);

    for (index, record) in records.iter().enumerate() {
        if !is_ambiguous_dbt_record(record) {
            continue;
        }
        let Some(series_key) = series_key(record) else {
            continue;
        };

        if split_slice_series.contains(&series_key) {
            refine_record_with_diagnostic(
                &mut refined_records[index],
                MammogramType::Tomo,
                DbtObjectKind::Slice,
                DbtRefinementReason::SplitSliceSeriesCardinality,
                &mut diagnostics,
            );
            continue;
        }

        let Some(series_info) = series_infos.get(&series_key) else {
            continue;
        };
        if series_info.ambiguous_count != 1 {
            continue;
        }

        if has_unique_split_series_source_pair(record, &split_series_by_source) {
            refine_record_with_diagnostic(
                &mut refined_records[index],
                MammogramType::Synth,
                DbtObjectKind::None,
                DbtRefinementReason::UniqueSplitSeriesSourcePair,
                &mut diagnostics,
            );
            continue;
        }

        if record
            .metadata
            .sop_instance_uid_of_concatenation_source
            .as_deref()
            .is_none_or(str::is_empty)
            && has_unique_split_series_view_pair(record, &series_key, &view_pairing)
        {
            refine_record_with_diagnostic(
                &mut refined_records[index],
                MammogramType::Synth,
                DbtObjectKind::None,
                DbtRefinementReason::UniqueSplitSeriesViewPair,
                &mut diagnostics,
            );
        }
    }

    (refined_records, diagnostics)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SeriesKey {
    study_uid: String,
    series_uid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SourceKey {
    study_uid: String,
    source_sop_uid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ViewKey {
    laterality: Laterality,
    view_position: ViewPosition,
}

#[derive(Debug, Default)]
struct SeriesInfo {
    ambiguous_count: usize,
    source_sop_uids: BTreeSet<String>,
    view_keys: HashSet<ViewKey>,
}

#[derive(Debug, Default)]
struct ViewPairingInfo {
    split_series: BTreeSet<SeriesKey>,
    singleton_series: BTreeSet<SeriesKey>,
}

fn build_series_infos(records: &[MammogramRecord]) -> HashMap<SeriesKey, SeriesInfo> {
    let mut series_infos = HashMap::new();
    for record in records {
        let Some(series_key) = series_key(record) else {
            continue;
        };
        let info: &mut SeriesInfo = series_infos.entry(series_key).or_default();
        if is_ambiguous_dbt_record(record) {
            info.ambiguous_count += 1;
            if let Some(source_uid) = non_empty(
                record
                    .metadata
                    .sop_instance_uid_of_concatenation_source
                    .as_deref(),
            ) {
                info.source_sop_uids.insert(source_uid.to_string());
            }
            if let Some(view_key) = view_key(record) {
                info.view_keys.insert(view_key);
            }
        }
    }
    series_infos
}

fn split_slice_series_keys_from_cardinality(
    series_infos: &HashMap<SeriesKey, SeriesInfo>,
) -> HashSet<SeriesKey> {
    series_infos
        .iter()
        .filter(|(_, info)| info.ambiguous_count > SPLIT_SLICE_SERIES_COUNT_THRESHOLD)
        .map(|(series_key, _)| series_key.clone())
        .collect()
}

fn index_split_series_by_source_uid(
    series_infos: &HashMap<SeriesKey, SeriesInfo>,
    split_slice_series: &HashSet<SeriesKey>,
) -> HashMap<SourceKey, BTreeSet<SeriesKey>> {
    let mut by_source: HashMap<SourceKey, BTreeSet<SeriesKey>> = HashMap::new();
    for series_key in split_slice_series {
        let Some(info) = series_infos.get(series_key) else {
            continue;
        };
        for source_uid in &info.source_sop_uids {
            by_source
                .entry(SourceKey {
                    study_uid: series_key.study_uid.clone(),
                    source_sop_uid: source_uid.clone(),
                })
                .or_default()
                .insert(series_key.clone());
        }
    }
    by_source
}

fn view_pairing_candidates_by_study_view(
    series_infos: &HashMap<SeriesKey, SeriesInfo>,
    split_slice_series: &HashSet<SeriesKey>,
) -> HashMap<(String, ViewKey), ViewPairingInfo> {
    let mut by_view = HashMap::new();
    for (series_key, info) in series_infos {
        if info.ambiguous_count == 0 || info.view_keys.len() != 1 {
            continue;
        }
        let Some(&view_key) = info.view_keys.iter().next() else {
            continue;
        };
        let pairing: &mut ViewPairingInfo = by_view
            .entry((series_key.study_uid.clone(), view_key))
            .or_default();
        if split_slice_series.contains(series_key) {
            pairing.split_series.insert(series_key.clone());
        } else if info.ambiguous_count == 1 {
            pairing.singleton_series.insert(series_key.clone());
        }
    }
    by_view
}

fn is_ambiguous_dbt_record(record: &MammogramRecord) -> bool {
    record.metadata.mammogram_type == MammogramType::Unknown
        && record.metadata.dbt_object_kind == DbtObjectKind::Unknown
}

fn series_key(record: &MammogramRecord) -> Option<SeriesKey> {
    Some(SeriesKey {
        study_uid: non_empty(record.study_instance_uid.as_deref())?.to_string(),
        series_uid: non_empty(record.series_instance_uid.as_deref())?.to_string(),
    })
}

fn view_key(record: &MammogramRecord) -> Option<ViewKey> {
    if !record.metadata.laterality.is_unilateral() || record.metadata.view_position.is_unknown() {
        return None;
    }
    Some(ViewKey {
        laterality: record.metadata.laterality,
        view_position: record.metadata.view_position,
    })
}

fn has_unique_split_series_source_pair(
    record: &MammogramRecord,
    split_series_by_source: &HashMap<SourceKey, BTreeSet<SeriesKey>>,
) -> bool {
    let Some(study_uid) = non_empty(record.study_instance_uid.as_deref()) else {
        return false;
    };
    let Some(source_uid) = non_empty(
        record
            .metadata
            .sop_instance_uid_of_concatenation_source
            .as_deref(),
    ) else {
        return false;
    };
    let source_key = SourceKey {
        study_uid: study_uid.to_string(),
        source_sop_uid: source_uid.to_string(),
    };
    split_series_by_source
        .get(&source_key)
        .is_some_and(|series| series.len() == 1)
}

fn has_unique_split_series_view_pair(
    record: &MammogramRecord,
    series_key: &SeriesKey,
    view_pairing: &HashMap<(String, ViewKey), ViewPairingInfo>,
) -> bool {
    let Some(view_key) = view_key(record) else {
        return false;
    };
    let Some(pairing) = view_pairing.get(&(series_key.study_uid.clone(), view_key)) else {
        return false;
    };
    pairing.split_series.len() == 1
        && pairing.singleton_series.len() == 1
        && pairing.singleton_series.contains(series_key)
}

fn refine_record(
    record: &mut MammogramRecord,
    mammogram_type: MammogramType,
    dbt_object_kind: DbtObjectKind,
) {
    record.metadata.mammogram_type = mammogram_type;
    record.metadata.dbt_object_kind = dbt_object_kind;
}

fn refine_record_with_diagnostic(
    record: &mut MammogramRecord,
    mammogram_type: MammogramType,
    dbt_object_kind: DbtObjectKind,
    reason: DbtRefinementReason,
    diagnostics: &mut Vec<DbtRefinementDiagnostic>,
) {
    let original_mammogram_type = record.metadata.mammogram_type;
    let original_dbt_object_kind = record.metadata.dbt_object_kind;
    refine_record(record, mammogram_type, dbt_object_kind);
    diagnostics.push(DbtRefinementDiagnostic {
        file_path: record.file_path.clone(),
        study_instance_uid: record.study_instance_uid.clone(),
        series_instance_uid: record.series_instance_uid.clone(),
        sop_instance_uid: record.sop_instance_uid.clone(),
        original_mammogram_type,
        original_dbt_object_kind,
        refined_mammogram_type: mammogram_type,
        refined_dbt_object_kind: dbt_object_kind,
        reason,
    });
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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

            // Filter: Allowed DBT object kinds (whitelist)
            if let Some(allowed_dbt_object_kinds) = &config.allowed_dbt_object_kinds {
                if !allowed_dbt_object_kinds.contains(&record.metadata.dbt_object_kind) {
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

            // Filter: Exclude lossy compressed images
            if config.exclude_lossy_compressed && record.is_lossy_compressed {
                return false;
            }

            true
        })
        .cloned()
        .collect()
}

fn select_study_records(
    records: &[MammogramRecord],
    study_selection_mode: StudySelectionMode,
    require_common_modality: bool,
) -> Result<SelectedStudyRecords> {
    let candidate_records: Vec<MammogramRecord> = records
        .iter()
        .filter(|record| is_candidate_for_any_standard_view(record))
        .cloned()
        .collect();

    if candidate_records.is_empty() {
        return Ok(SelectedStudyRecords {
            records: Vec::new(),
            warnings: Vec::new(),
        });
    }

    match study_selection_mode {
        StudySelectionMode::MostComplete => {
            let mut groups = build_study_groups(&candidate_records, require_common_modality);
            groups.sort_by(compare_study_groups);
            let selected_group = groups
                .first()
                .expect("candidate records always form at least one study group");
            Ok(SelectedStudyRecords {
                records: selected_group.records.clone(),
                warnings: mixed_study_warnings(&groups, selected_group),
            })
        }
        StudySelectionMode::StrictSingleStudy => {
            let records = select_strict_study_records(candidate_records)?;
            Ok(SelectedStudyRecords {
                records,
                warnings: Vec::new(),
            })
        }
    }
}

fn select_strict_study_records(
    candidate_records: Vec<MammogramRecord>,
) -> Result<Vec<MammogramRecord>> {
    let missing_uid_paths = missing_study_uid_paths(&candidate_records);
    if !missing_uid_paths.is_empty() {
        return Err(MammocatError::SelectionError(format!(
            "strict study selection requires StudyInstanceUID on every usable candidate; missing for: {}",
            missing_uid_paths.join(", ")
        )));
    }

    let records_by_uid = group_records_by_study_uid(candidate_records);
    if records_by_uid.len() > 1 {
        let study_uids: Vec<String> = records_by_uid.keys().cloned().collect();
        return Err(MammocatError::SelectionError(format!(
            "strict study selection requires exactly one StudyInstanceUID; found: {}",
            study_uids.join(", ")
        )));
    }

    Ok(records_by_uid.into_values().next().unwrap_or_default())
}

fn missing_study_uid_paths(records: &[MammogramRecord]) -> Vec<String> {
    records
        .iter()
        .filter(|record| is_missing_study_uid(record))
        .map(|record| record.file_path.display().to_string())
        .collect()
}

fn group_records_by_study_uid(
    records: Vec<MammogramRecord>,
) -> BTreeMap<String, Vec<MammogramRecord>> {
    let mut records_by_uid: BTreeMap<String, Vec<MammogramRecord>> = BTreeMap::new();

    for record in records {
        if let Some(study_uid) = &record.study_instance_uid {
            records_by_uid
                .entry(study_uid.clone())
                .or_default()
                .push(record);
        }
    }

    records_by_uid
}

fn mixed_study_warnings(
    groups: &[StudyGroup],
    selected_group: &StudyGroup,
) -> Vec<SelectionWarning> {
    if groups.len() <= 1 {
        return Vec::new();
    }

    let study_labels = groups
        .iter()
        .map(study_group_label)
        .collect::<Vec<_>>()
        .join(", ");
    vec![SelectionWarning {
        message: format!(
            "{MIXED_STUDY_WARNING_PREFIX}: usable candidates span multiple study groups ({study_labels}); selecting only the most complete study {}",
            study_group_label(selected_group)
        ),
    }]
}

fn study_group_label(group: &StudyGroup) -> String {
    group
        .study_instance_uid
        .as_ref()
        .map(|study_uid| format!("StudyInstanceUID {study_uid}"))
        .unwrap_or_else(|| {
            let file_path = group
                .unknown_sort_key
                .as_ref()
                .map(|(_, file_path)| file_path.as_str())
                .unwrap_or("unknown file");
            format!("missing StudyInstanceUID at {file_path}")
        })
}

fn build_study_groups(
    records: &[MammogramRecord],
    require_common_modality: bool,
) -> Vec<StudyGroup> {
    let mut records_by_uid: BTreeMap<String, Vec<MammogramRecord>> = BTreeMap::new();
    let mut unknown_groups = Vec::new();

    for record in records {
        if is_missing_study_uid(record) {
            unknown_groups.push(vec![record.clone()]);
        } else if let Some(study_uid) = &record.study_instance_uid {
            records_by_uid
                .entry(study_uid.clone())
                .or_default()
                .push(record.clone());
        }
    }

    let mut groups: Vec<StudyGroup> = records_by_uid
        .into_iter()
        .map(|(study_uid, records)| {
            make_study_group(Some(study_uid), records, require_common_modality)
        })
        .collect();

    groups.extend(
        unknown_groups
            .into_iter()
            .map(|records| make_study_group(None, records, require_common_modality)),
    );

    groups
}

fn make_study_group(
    study_instance_uid: Option<String>,
    records: Vec<MammogramRecord>,
    require_common_modality: bool,
) -> StudyGroup {
    let (standard_slot_count, candidate_slot_count) =
        count_study_slots(&records, require_common_modality);
    let unknown_sort_key = study_instance_uid.is_none().then(|| {
        records
            .iter()
            .map(|record| {
                (
                    record.sop_instance_uid.clone().unwrap_or_default(),
                    record.file_path.display().to_string(),
                )
            })
            .min()
            .unwrap_or_default()
    });

    StudyGroup {
        study_instance_uid,
        records,
        standard_slot_count,
        candidate_slot_count,
        unknown_sort_key,
    }
}

fn compare_study_groups(left: &StudyGroup, right: &StudyGroup) -> Ordering {
    right
        .standard_slot_count
        .cmp(&left.standard_slot_count)
        .then_with(|| right.candidate_slot_count.cmp(&left.candidate_slot_count))
        .then_with(
            || match (&left.study_instance_uid, &right.study_instance_uid) {
                (Some(left_uid), Some(right_uid)) => left_uid.cmp(right_uid),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => left.unknown_sort_key.cmp(&right.unknown_sort_key),
            },
        )
}

fn is_missing_study_uid(record: &MammogramRecord) -> bool {
    match record.study_instance_uid.as_deref() {
        Some(uid) => uid.trim().is_empty(),
        None => true,
    }
}

fn count_study_slots(records: &[MammogramRecord], require_common_modality: bool) -> (usize, usize) {
    if require_common_modality {
        return count_common_modality_study_slots(records);
    }

    (
        count_standard_slots(records),
        count_candidate_slots(records),
    )
}

fn count_common_modality_study_slots(records: &[MammogramRecord]) -> (usize, usize) {
    let records_2d: Vec<MammogramRecord> = records
        .iter()
        .filter(|record| record.metadata.mammogram_type.is_2d_group())
        .cloned()
        .collect();
    let records_dbt: Vec<MammogramRecord> = records
        .iter()
        .filter(|record| record.metadata.mammogram_type.is_dbt_group())
        .cloned()
        .collect();

    [
        (
            count_standard_slots(&records_2d),
            count_candidate_slots(&records_2d),
        ),
        (
            count_standard_slots(&records_dbt),
            count_candidate_slots(&records_dbt),
        ),
    ]
    .into_iter()
    .max()
    .unwrap_or_default()
}

fn count_standard_slots(records: &[MammogramRecord]) -> usize {
    STANDARD_MAMMO_VIEWS
        .iter()
        .filter(|standard_view| {
            records.iter().any(|record| {
                let candidate_view = record.metadata.mammogram_view();
                candidate_view.laterality == standard_view.laterality
                    && candidate_view.view == standard_view.view
                    && record.metadata.is_standard_view()
            })
        })
        .count()
}

fn count_candidate_slots(records: &[MammogramRecord]) -> usize {
    STANDARD_MAMMO_VIEWS
        .iter()
        .filter(|standard_view| {
            records
                .iter()
                .any(|record| is_candidate_for_view(record, standard_view))
        })
        .count()
}

fn log_selection_warnings(warnings: &[SelectionWarning]) {
    for warning in warnings {
        log::warn!("{}", warning.message());
    }
}

/// Enforces that all selected views come from a single modality group (2D or DBT)
///
/// If the initial selection is already single-modality, returns it as-is.
/// Otherwise, re-runs selection on 2D-only and DBT-only record pools separately,
/// then picks the candidate with higher coverage, breaking ties by preference score
/// and defaulting to 2D.
#[cfg(test)]
fn enforce_common_modality(
    filtered_records: &[MammogramRecord],
    initial_selection: HashMap<MammogramView, Option<MammogramRecord>>,
    preference_order: PreferenceOrder,
) -> HashMap<MammogramView, Option<MammogramRecord>> {
    enforce_common_modality_with_options(
        filtered_records,
        initial_selection,
        preference_order,
        true,
    )
}

fn enforce_common_modality_with_options(
    filtered_records: &[MammogramRecord],
    initial_selection: HashMap<MammogramView, Option<MammogramRecord>>,
    preference_order: PreferenceOrder,
    deprioritize_lossy_compressed: bool,
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

    let selection_2d = select_preferred_views_for_records(
        &records_2d,
        preference_order,
        deprioritize_lossy_compressed,
    );
    let selection_dbt = select_preferred_views_for_records(
        &records_dbt,
        preference_order,
        deprioritize_lossy_compressed,
    );

    let coverage_2d = count_coverage(&selection_2d);
    let coverage_dbt = count_coverage(&selection_dbt);

    if coverage_2d > coverage_dbt {
        selection_2d
    } else if coverage_dbt > coverage_2d {
        selection_dbt
    } else {
        if deprioritize_lossy_compressed {
            let lossy_2d = count_lossy(&selection_2d);
            let lossy_dbt = count_lossy(&selection_dbt);

            if lossy_2d < lossy_dbt {
                return selection_2d;
            }
            if lossy_dbt < lossy_2d {
                return selection_dbt;
            }
        }

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

/// Counts the number of selected records marked as lossy compressed
fn count_lossy(selection: &HashMap<MammogramView, Option<MammogramRecord>>) -> usize {
    selection
        .values()
        .flatten()
        .filter(|record| record.is_lossy_compressed)
        .count()
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

fn is_candidate_for_any_standard_view(record: &MammogramRecord) -> bool {
    STANDARD_MAMMO_VIEWS
        .iter()
        .any(|standard_view| is_candidate_for_view(record, standard_view))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MammocatError;
    use crate::types::{
        DbtObjectKind, ImageType, Laterality, MammogramType, PreferenceOrder, ViewPosition,
    };
    use std::path::PathBuf;

    const DEFAULT_STUDY_UID: &str = "1.2.826.0.1";
    const MIN_SPLIT_SLICE_SERIES_COUNT: usize = SPLIT_SLICE_SERIES_COUNT_THRESHOLD + 1;
    const SPLIT_SLICE_SERIES_UID: &str = "1.2.826.0.1.10";
    const SYNTH_SINGLETON_SERIES_UID: &str = "1.2.826.0.1.20";
    const SECOND_SYNTH_SINGLETON_SERIES_UID: &str = "1.2.826.0.1.21";
    const SOURCE_SOP_UID_RCC: &str = "1.2.826.0.1.30";
    const SYNTH_SINGLETON_SOP_UID: &str = "1.2.826.0.1.40";
    const SECOND_SYNTH_SINGLETON_SOP_UID: &str = "1.2.826.0.1.41";
    const AMBIGUOUS_SINGLETON_SOP_UID: &str = "1.2.826.0.1.50";

    fn make_test_record(
        laterality: Laterality,
        view_pos: ViewPosition,
        mammo_type: MammogramType,
    ) -> MammogramRecord {
        make_test_record_with_study(laterality, view_pos, mammo_type, Some(DEFAULT_STUDY_UID))
    }

    fn make_test_record_with_study(
        laterality: Laterality,
        view_pos: ViewPosition,
        mammo_type: MammogramType,
        study_uid: Option<&str>,
    ) -> MammogramRecord {
        let study_label = study_uid.unwrap_or("missing");
        MammogramRecord {
            file_path: PathBuf::from(format!("{study_label}_{laterality:?}_{view_pos:?}.dcm")),
            metadata: crate::api::MammogramMetadata {
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
                is_spot_compression: false,
                is_magnified: false,
                is_implant_displaced: false,
                manufacturer: None,
                model: None,
                number_of_frames: 1,
                concatenation_uid: None,
                sop_instance_uid_of_concatenation_source: None,
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
                transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
                transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
                compression_type: Some("uncompressed".to_string()),
            },
            rows: Some(2560),
            columns: Some(3328),
            is_implant_displaced: false,
            is_spot_compression: false,
            is_magnified: false,
            transfer_syntax_uid: None,
            is_lossy_compressed: false,
            study_instance_uid: study_uid.map(str::to_string),
            series_instance_uid: study_uid.map(|uid| format!("{uid}.series")),
            sop_instance_uid: Some(format!(
                "{}.{}.{}.{}",
                study_label,
                laterality.short_str(),
                view_pos.short_str(),
                mammo_type.simple_name()
            )),
        }
    }

    fn default_dbt_object_kind(mammo_type: MammogramType) -> DbtObjectKind {
        match mammo_type {
            MammogramType::Tomo => DbtObjectKind::Unknown,
            _ => DbtObjectKind::None,
        }
    }

    fn with_allowed_types(base_config: FilterConfig, types: &[MammogramType]) -> FilterConfig {
        base_config.with_allowed_types(types.iter().copied().collect())
    }

    fn with_allowed_dbt_object_kinds(
        base_config: FilterConfig,
        kinds: &[DbtObjectKind],
    ) -> FilterConfig {
        base_config.with_allowed_dbt_object_kinds(kinds.iter().copied().collect())
    }

    fn make_lossy_test_record(
        laterality: Laterality,
        view_pos: ViewPosition,
        mammo_type: MammogramType,
        is_lossy_compressed: bool,
    ) -> MammogramRecord {
        let mut record = make_test_record(laterality, view_pos, mammo_type);
        record.is_lossy_compressed = is_lossy_compressed;
        record
    }

    fn make_tomo_slice_test_record(
        laterality: Laterality,
        view_pos: ViewPosition,
    ) -> MammogramRecord {
        let mut record = make_test_record(laterality, view_pos, MammogramType::Tomo);
        record.metadata.dbt_object_kind = DbtObjectKind::Slice;
        record
    }

    fn make_ambiguous_dbt_record(
        study_uid: &str,
        series_uid: &str,
        sop_uid: &str,
        source_sop_uid: Option<&str>,
        laterality: Laterality,
        view_pos: ViewPosition,
    ) -> MammogramRecord {
        let mut record = make_test_record_with_study(
            laterality,
            view_pos,
            MammogramType::Unknown,
            Some(study_uid),
        );
        record.file_path = PathBuf::from(format!("{series_uid}_{sop_uid}.dcm"));
        record.series_instance_uid = Some(series_uid.to_string());
        record.sop_instance_uid = Some(sop_uid.to_string());
        record.metadata.dbt_object_kind = DbtObjectKind::Unknown;
        record.metadata.image_type =
            ImageType::new("DERIVED".to_string(), "PRIMARY".to_string(), None, None);
        record.metadata.concatenation_uid =
            source_sop_uid.map(|source_uid| format!("{source_uid}.1"));
        record.metadata.sop_instance_uid_of_concatenation_source =
            source_sop_uid.map(str::to_string);
        record
    }

    fn make_ambiguous_series(
        study_uid: &str,
        series_uid: &str,
        source_sop_uid: Option<&str>,
        laterality: Laterality,
        view_pos: ViewPosition,
        count: usize,
    ) -> Vec<MammogramRecord> {
        (0..count)
            .map(|index| {
                make_ambiguous_dbt_record(
                    study_uid,
                    series_uid,
                    &format!("{series_uid}.{index}"),
                    source_sop_uid,
                    laterality,
                    view_pos,
                )
            })
            .collect()
    }

    fn make_non_ambiguous_record_in_series(
        study_uid: &str,
        series_uid: &str,
        sop_uid: &str,
        mammogram_type: MammogramType,
    ) -> MammogramRecord {
        let mut record = make_test_record_with_study(
            Laterality::Right,
            ViewPosition::Cc,
            mammogram_type,
            Some(study_uid),
        );
        record.file_path = PathBuf::from(format!("{series_uid}_{sop_uid}.dcm"));
        record.series_instance_uid = Some(series_uid.to_string());
        record.sop_instance_uid = Some(sop_uid.to_string());
        record
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
    fn test_get_preferred_views_chooses_complete_study_without_mixing() {
        let incomplete_study = "1.2.826.0.10";
        let complete_study = "1.2.826.0.20";
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(incomplete_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(incomplete_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Ffdm,
                Some(incomplete_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                Some(complete_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                Some(complete_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Tomo,
                Some(complete_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
                Some(complete_study),
            ),
        ];

        let (selections, warnings) =
            get_preferred_views_with_order_and_warnings(&records, PreferenceOrder::Default);

        assert_eq!(count_coverage(&selections), 4);
        assert_eq!(warnings.len(), 1);
        let warning = warnings[0].message();
        assert!(warning.contains(MIXED_STUDY_WARNING_PREFIX));
        assert!(warning.contains("1.2.826.0.10"));
        assert!(warning.contains("1.2.826.0.20"));
        assert!(warning.contains("selecting only the most complete study"));
        assert!(warning.contains(complete_study));
        for record in selections.values().flatten() {
            assert_eq!(record.study_instance_uid.as_deref(), Some(complete_study));
        }
    }

    #[test]
    fn test_get_preferred_views_ties_by_lowest_study_uid() {
        let higher_study = "1.2.826.0.20";
        let lower_study = "1.2.826.0.10";
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(higher_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(higher_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Ffdm,
                Some(higher_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                Some(lower_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                Some(lower_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Tomo,
                Some(lower_study),
            ),
        ];

        let selections = get_preferred_views(&records);

        assert_eq!(count_coverage(&selections), 3);
        for record in selections.values().flatten() {
            assert_eq!(record.study_instance_uid.as_deref(), Some(lower_study));
        }
    }

    #[test]
    fn test_get_preferred_views_prioritizes_standard_slots_over_nonstandard_candidates() {
        let standard_study = "1.2.826.0.10";
        let nonstandard_study = "1.2.826.0.20";
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(standard_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(standard_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Ffdm,
                Some(standard_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(nonstandard_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(nonstandard_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Xccl,
                MammogramType::Ffdm,
                Some(nonstandard_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Xccm,
                MammogramType::Ffdm,
                Some(nonstandard_study),
            ),
        ];

        let selections = get_preferred_views(&records);

        assert_eq!(count_coverage(&selections), 3);
        for record in selections.values().flatten() {
            assert_eq!(record.study_instance_uid.as_deref(), Some(standard_study));
        }
    }

    #[test]
    fn test_common_modality_study_selection_uses_common_modality_coverage() {
        let mixed_modality_study = "1.2.826.0.10";
        let single_modality_study = "1.2.826.0.20";
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(mixed_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(mixed_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Tomo,
                Some(mixed_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
                Some(mixed_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(single_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some(single_modality_study),
            ),
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Ffdm,
                Some(single_modality_study),
            ),
        ];
        let config = FilterConfig::permissive().require_common_modality(true);

        let selections = get_preferred_views_filtered_with_study_mode(
            &records,
            &config,
            PreferenceOrder::Default,
            StudySelectionMode::MostComplete,
        )
        .unwrap();

        assert_eq!(count_coverage(&selections), 3);
        for record in selections.values().flatten() {
            assert_eq!(
                record.study_instance_uid.as_deref(),
                Some(single_modality_study)
            );
        }
    }

    #[test]
    fn test_strict_single_study_succeeds_for_one_study() {
        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_test_record(Laterality::Right, ViewPosition::Mlo, MammogramType::Ffdm),
        ];
        let config = FilterConfig::permissive();

        let selections = get_preferred_views_filtered_with_study_mode(
            &records,
            &config,
            PreferenceOrder::Default,
            StudySelectionMode::StrictSingleStudy,
        )
        .unwrap();

        assert_eq!(count_coverage(&selections), 2);
    }

    #[test]
    fn test_strict_single_study_errors_for_multiple_studies() {
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some("1.2.826.0.10"),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some("1.2.826.0.20"),
            ),
        ];
        let config = FilterConfig::permissive();

        let error = get_preferred_views_filtered_with_study_mode(
            &records,
            &config,
            PreferenceOrder::Default,
            StudySelectionMode::StrictSingleStudy,
        )
        .unwrap_err();

        assert!(matches!(error, MammocatError::SelectionError(_)));
        assert!(error.to_string().contains("1.2.826.0.10"));
        assert!(error.to_string().contains("1.2.826.0.20"));
    }

    #[test]
    fn test_strict_single_study_errors_for_missing_study_uid() {
        let records = vec![make_test_record_with_study(
            Laterality::Left,
            ViewPosition::Mlo,
            MammogramType::Ffdm,
            None,
        )];
        let config = FilterConfig::permissive();

        let error = get_preferred_views_filtered_with_study_mode(
            &records,
            &config,
            PreferenceOrder::Default,
            StudySelectionMode::StrictSingleStudy,
        )
        .unwrap_err();

        assert!(matches!(error, MammocatError::SelectionError(_)));
        assert!(error.to_string().contains("StudyInstanceUID"));
    }

    #[test]
    fn test_filters_run_before_strict_study_selection() {
        let config = with_allowed_types(FilterConfig::permissive(), &[MammogramType::Ffdm]);
        let records = vec![
            make_test_record_with_study(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                Some("1.2.826.0.10"),
            ),
            make_test_record_with_study(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                Some("1.2.826.0.20"),
            ),
        ];

        let selections = get_preferred_views_filtered_with_study_mode(
            &records,
            &config,
            PreferenceOrder::Default,
            StudySelectionMode::StrictSingleStudy,
        )
        .unwrap();

        assert_eq!(count_coverage(&selections), 1);
        assert_eq!(
            selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
                .as_ref()
                .unwrap()
                .study_instance_uid
                .as_deref(),
            Some("1.2.826.0.10")
        );
    }

    #[test]
    fn test_apply_filters_allowed_types() {
        let config = with_allowed_types(FilterConfig::default(), &[MammogramType::Ffdm]);

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
    fn test_apply_filters_allowed_dbt_object_kinds() {
        let config =
            with_allowed_dbt_object_kinds(FilterConfig::permissive(), &[DbtObjectKind::None]);

        let records = vec![
            make_test_record(Laterality::Left, ViewPosition::Mlo, MammogramType::Ffdm),
            make_tomo_slice_test_record(Laterality::Left, ViewPosition::Mlo),
        ];

        let filtered = apply_filters(&records, &config);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].metadata.mammogram_type, MammogramType::Ffdm);
        assert_eq!(filtered[0].metadata.dbt_object_kind, DbtObjectKind::None);
    }

    #[test]
    fn test_two_d_allowed_types_excludes_tomo_slice() {
        let config = with_allowed_types(
            FilterConfig::permissive(),
            &[
                MammogramType::Ffdm,
                MammogramType::Synth,
                MammogramType::Sfm,
            ],
        );
        let records = vec![make_tomo_slice_test_record(
            Laterality::Left,
            ViewPosition::Mlo,
        )];

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);

        assert!(selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)].is_none());
    }

    #[test]
    fn test_allowed_tomo_types_can_select_tomo_slice() {
        let config = with_allowed_types(FilterConfig::permissive(), &[MammogramType::Tomo]);
        let records = vec![make_tomo_slice_test_record(
            Laterality::Left,
            ViewPosition::Mlo,
        )];

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);
        let selected = selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)]
            .as_ref()
            .unwrap();

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Tomo);
        assert_eq!(selected.metadata.dbt_object_kind, DbtObjectKind::Slice);
    }

    #[test]
    fn collection_refinement_marks_large_ambiguous_series_as_tomo_slice() {
        let records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );

        let refined = refine_dbt_object_classification(&records);

        assert!(refined.iter().all(|record| {
            record.metadata.mammogram_type == MammogramType::Tomo
                && record.metadata.dbt_object_kind == DbtObjectKind::Slice
        }));
    }

    #[test]
    fn collection_refinement_marks_source_paired_singleton_as_synth() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        ));

        let refined = refine_dbt_object_classification(&records);
        let singleton = refined
            .iter()
            .find(|record| {
                record.series_instance_uid.as_deref() == Some(SYNTH_SINGLETON_SERIES_UID)
            })
            .expect("singleton record");

        assert_eq!(singleton.metadata.mammogram_type, MammogramType::Synth);
        assert_eq!(singleton.metadata.dbt_object_kind, DbtObjectKind::None);
    }

    #[test]
    fn collection_refinement_leaves_unpaired_singleton_unknown() {
        let records = vec![make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        )];

        let refined = refine_dbt_object_classification(&records);

        assert_eq!(refined[0].metadata.mammogram_type, MammogramType::Unknown);
        assert_eq!(refined[0].metadata.dbt_object_kind, DbtObjectKind::Unknown);
    }

    #[test]
    fn collection_refinement_counts_only_ambiguous_records_for_slice_cardinality() {
        let mut records = vec![make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            AMBIGUOUS_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        )];
        records.extend((0..MIN_SPLIT_SLICE_SERIES_COUNT).map(|index| {
            make_non_ambiguous_record_in_series(
                DEFAULT_STUDY_UID,
                SPLIT_SLICE_SERIES_UID,
                &format!("1.2.826.0.1.60.{index}"),
                MammogramType::Ffdm,
            )
        }));

        let refined = refine_dbt_object_classification(&records);
        let ambiguous_record = refined
            .iter()
            .find(|record| record.sop_instance_uid.as_deref() == Some(AMBIGUOUS_SINGLETON_SOP_UID))
            .expect("ambiguous singleton record");

        assert_eq!(
            ambiguous_record.metadata.mammogram_type,
            MammogramType::Unknown
        );
        assert_eq!(
            ambiguous_record.metadata.dbt_object_kind,
            DbtObjectKind::Unknown
        );
    }

    #[test]
    fn collection_refinement_uses_view_pair_fallback_without_source_uid() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            None,
            Laterality::Left,
            ViewPosition::Mlo,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            None,
            Laterality::Left,
            ViewPosition::Mlo,
        ));

        let refined = refine_dbt_object_classification(&records);
        let singleton = refined
            .iter()
            .find(|record| {
                record.series_instance_uid.as_deref() == Some(SYNTH_SINGLETON_SERIES_UID)
            })
            .expect("singleton record");

        assert_eq!(singleton.metadata.mammogram_type, MammogramType::Synth);
        assert_eq!(singleton.metadata.dbt_object_kind, DbtObjectKind::None);
    }

    #[test]
    fn collection_refinement_leaves_conflicting_singletons_unknown() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            None,
            Laterality::Left,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            None,
            Laterality::Left,
            ViewPosition::Cc,
        ));
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SECOND_SYNTH_SINGLETON_SERIES_UID,
            SECOND_SYNTH_SINGLETON_SOP_UID,
            None,
            Laterality::Left,
            ViewPosition::Cc,
        ));

        let refined = refine_dbt_object_classification(&records);
        let singleton_types: Vec<_> = refined
            .iter()
            .filter(|record| {
                record.series_instance_uid.as_deref().is_some_and(|series| {
                    series == SYNTH_SINGLETON_SERIES_UID
                        || series == SECOND_SYNTH_SINGLETON_SERIES_UID
                })
            })
            .map(|record| {
                (
                    record.metadata.mammogram_type,
                    record.metadata.dbt_object_kind,
                )
            })
            .collect();

        assert_eq!(
            singleton_types,
            vec![
                (MammogramType::Unknown, DbtObjectKind::Unknown),
                (MammogramType::Unknown, DbtObjectKind::Unknown),
            ]
        );
    }

    #[test]
    fn two_d_filter_uses_refined_singleton_synth_and_excludes_refined_slices() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        ));
        let config = with_allowed_types(
            FilterConfig::permissive(),
            &[
                MammogramType::Ffdm,
                MammogramType::Synth,
                MammogramType::Sfm,
            ],
        );

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);
        let selected = selections[&MammogramView::new(Laterality::Right, ViewPosition::Cc)]
            .as_ref()
            .expect("refined SYN2D singleton selected");

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Synth);
        assert_eq!(selected.metadata.dbt_object_kind, DbtObjectKind::None);
    }

    #[test]
    fn two_d_filter_can_require_non_dbt_object_kind() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        ));
        records.push(make_tomo_slice_test_record(
            Laterality::Left,
            ViewPosition::Mlo,
        ));
        let config = with_allowed_dbt_object_kinds(
            with_allowed_types(
                FilterConfig::permissive(),
                &[
                    MammogramType::Ffdm,
                    MammogramType::Synth,
                    MammogramType::Sfm,
                ],
            ),
            &[DbtObjectKind::None],
        );

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);

        let selected = selections[&MammogramView::new(Laterality::Right, ViewPosition::Cc)]
            .as_ref()
            .expect("refined SYN2D singleton selected");
        assert_eq!(selected.metadata.mammogram_type, MammogramType::Synth);
        assert_eq!(selected.metadata.dbt_object_kind, DbtObjectKind::None);
        assert!(selections[&MammogramView::new(Laterality::Left, ViewPosition::Mlo)].is_none());
    }

    #[test]
    fn tomo_filter_uses_refined_slices_and_excludes_refined_singleton_synth() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        ));
        let config = with_allowed_types(FilterConfig::permissive(), &[MammogramType::Tomo]);

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);
        let selected = selections[&MammogramView::new(Laterality::Right, ViewPosition::Cc)]
            .as_ref()
            .expect("refined slice selected");

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Tomo);
        assert_eq!(selected.metadata.dbt_object_kind, DbtObjectKind::Slice);
    }

    #[test]
    fn dbt_filter_selects_refined_tomo_slices() {
        let mut records = make_ambiguous_series(
            DEFAULT_STUDY_UID,
            SPLIT_SLICE_SERIES_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
            MIN_SPLIT_SLICE_SERIES_COUNT,
        );
        records.push(make_ambiguous_dbt_record(
            DEFAULT_STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID_RCC),
            Laterality::Right,
            ViewPosition::Cc,
        ));
        let config = with_allowed_dbt_object_kinds(
            with_allowed_types(FilterConfig::permissive(), &[MammogramType::Tomo]),
            &[DbtObjectKind::Volume, DbtObjectKind::Slice],
        );

        let selections = get_preferred_views_filtered(&records, &config, PreferenceOrder::Default);
        let selected = selections[&MammogramView::new(Laterality::Right, ViewPosition::Cc)]
            .as_ref()
            .expect("refined slice selected");

        assert_eq!(selected.metadata.mammogram_type, MammogramType::Tomo);
        assert_eq!(selected.metadata.dbt_object_kind, DbtObjectKind::Slice);
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
    fn test_apply_filters_exclude_lossy_compressed() {
        let config = FilterConfig::default().exclude_lossy_compressed(true);

        let lossy_record = make_lossy_test_record(
            Laterality::Left,
            ViewPosition::Mlo,
            MammogramType::Ffdm,
            true,
        );
        let lossless_record = make_lossy_test_record(
            Laterality::Left,
            ViewPosition::Cc,
            MammogramType::Ffdm,
            false,
        );

        let filtered = apply_filters(&[lossy_record, lossless_record], &config);

        assert_eq!(filtered.len(), 1);
        assert!(!filtered[0].is_lossy_compressed);
    }

    #[test]
    fn test_get_preferred_views_filtered() {
        let config = with_allowed_types(FilterConfig::default(), &[MammogramType::Ffdm]);

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
    fn test_enforce_common_modality_equal_coverage_prefers_fewer_lossy_records() {
        // Equal coverage: DBT would lose the default type-score tie-breaker,
        // but should win because both 2D candidates are lossy.
        let records = vec![
            make_lossy_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                true,
            ),
            make_lossy_test_record(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                false,
            ),
            make_lossy_test_record(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Ffdm,
                true,
            ),
            make_lossy_test_record(
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
                false,
            ),
        ];
        let initial = get_preferred_views_with_order(&records, PreferenceOrder::Default);
        let result = enforce_common_modality(&records, initial, PreferenceOrder::Default);

        assert_eq!(count_coverage(&result), 2);
        for record in result.values().flatten() {
            assert!(record.metadata.mammogram_type.is_dbt_group());
            assert!(!record.is_lossy_compressed);
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
