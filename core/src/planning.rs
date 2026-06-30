//! Collection-level mammography input planning.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::dbt::{
    scan_dbt_study, DbtScanOptions, DbtScanReport, DbtSeriesFinding, DbtSkippedFile,
    DbtUnsupportedSeries,
};
use crate::dicom_files::collect_dicom_files;
use crate::error::{MammocatError, Result};
use crate::selection::{
    get_preferred_views_filtered_with_study_mode_and_warnings,
    refine_dbt_object_classification_with_diagnostics, DbtRefinementDiagnostic, MammogramRecord,
    StudySelectionMode,
};
use crate::types::{
    DbtObjectKind, FilterConfig, MammogramType, PreferenceOrder, STANDARD_MAMMO_VIEWS,
};

/// Input groups included in a collection-level mammography plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MammographyPlanSelection {
    pub two_d_views: bool,
    pub dbt: bool,
}

impl MammographyPlanSelection {
    pub const fn new(two_d_views: bool, dbt: bool) -> Self {
        Self { two_d_views, dbt }
    }

    pub const fn all() -> Self {
        Self::new(true, true)
    }

    pub const fn two_d_views_only() -> Self {
        Self::new(true, false)
    }

    pub const fn dbt_only() -> Self {
        Self::new(false, true)
    }

    fn is_empty(self) -> bool {
        !self.two_d_views && !self.dbt
    }
}

impl Default for MammographyPlanSelection {
    fn default() -> Self {
        Self::all()
    }
}

/// Options for collection-level mammography input planning.
#[derive(Debug, Clone)]
pub struct MammographyPlanOptions {
    pub selection: MammographyPlanSelection,
    pub preference_order: PreferenceOrder,
    pub study_selection_mode: StudySelectionMode,
}

impl Default for MammographyPlanOptions {
    fn default() -> Self {
        Self {
            selection: MammographyPlanSelection::default(),
            preference_order: PreferenceOrder::Default,
            study_selection_mode: StudySelectionMode::MostComplete,
        }
    }
}

/// Summary counts for a collection-level input plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MammographyPlanSummary {
    pub input_dicom_files: usize,
    pub mammogram_records: usize,
    pub source_objects: usize,
    pub two_d_selected_views: usize,
    pub dbt_composition_inputs: usize,
    pub dbt_multiframe_volume_candidates: usize,
    pub warnings: usize,
}

/// Top-level collection input plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MammographyInputPlan {
    pub input_path: String,
    pub plan: MammographyPlanSelection,
    pub summary: MammographyPlanSummary,
    pub two_d_views: Option<TwoDViewsPlan>,
    pub dbt: Option<DbtPlan>,
    pub source_objects: Vec<SourceObjectDiagnostic>,
    pub warnings: Vec<String>,
}

/// 2D mammography view input plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TwoDViewsPlan {
    pub selected_views: BTreeMap<String, TwoDViewSelection>,
    pub missing_views: Vec<String>,
    pub selection_warnings: Vec<String>,
}

/// One standard 2D view selection slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TwoDViewSelection {
    pub view: String,
    pub selected: bool,
    pub source_path: Option<String>,
    pub mammogram_type: Option<String>,
    pub dbt_object_kind: Option<String>,
    pub reason: Option<String>,
}

/// DBT input plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbtPlan {
    pub composition_inputs: Vec<DbtCompositionInput>,
    pub multiframe_volume_candidates: Vec<DbtVolumeCandidate>,
    pub fallback_slice_paths: Vec<String>,
    pub unsupported_series: Vec<DbtUnsupportedSeries>,
    pub skipped_files: Vec<DbtSkippedFile>,
}

/// Split slice-per-file DBT series that should be composed before use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbtCompositionInput {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub source_paths: Vec<String>,
    pub relative_parent: String,
    pub frame_count: usize,
    pub laterality: String,
    pub view_position: String,
    pub source_modality: String,
    pub series_description: Option<String>,
    pub reason: String,
}

/// Existing multi-frame DBT volume candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbtVolumeCandidate {
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub source_paths: Vec<String>,
    pub frame_count: usize,
    pub laterality: Option<String>,
    pub view_position: Option<String>,
    pub reason: String,
}

/// Per-source planning diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceObjectDiagnostic {
    pub source_path: String,
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub sop_instance_uid: Option<String>,
    pub original_mammogram_type: Option<String>,
    pub original_dbt_object_kind: Option<String>,
    pub refined_mammogram_type: Option<String>,
    pub refined_dbt_object_kind: Option<String>,
    pub refinement_reason: Option<String>,
    pub selected_as: Vec<String>,
    pub filtered_by: Vec<String>,
    pub status: String,
}

/// Plan 2D mammography view and/or DBT inputs from a DICOM directory.
pub fn plan_mammography_collection(
    input: impl AsRef<Path>,
    options: MammographyPlanOptions,
) -> Result<MammographyInputPlan> {
    validate_plan_selection(options.selection)?;

    let input = input.as_ref();
    if !input.is_dir() {
        return Err(MammocatError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a directory", input.display()),
        )));
    }

    let dicom_files = collect_dicom_files(input)?;
    let input_dicom_files = dicom_files.len();
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    for file_path in dicom_files {
        match MammogramRecord::from_file(file_path.clone()) {
            Ok(record) => records.push(record),
            Err(error) => warnings.push(format!("skipping {}: {error}", file_path.display())),
        }
    }

    let dbt_scan = if options.selection.dbt {
        Some(scan_dbt_study(input, DbtScanOptions)?)
    } else {
        None
    };

    build_mammography_plan(
        input,
        input_dicom_files,
        records,
        dbt_scan,
        warnings,
        options,
    )
}

fn build_mammography_plan(
    input: &Path,
    input_dicom_files: usize,
    records: Vec<MammogramRecord>,
    dbt_scan: Option<DbtScanReport>,
    mut warnings: Vec<String>,
    options: MammographyPlanOptions,
) -> Result<MammographyInputPlan> {
    validate_plan_selection(options.selection)?;

    let mammogram_records = records.len();
    let (refined_records, refinement_diagnostics) =
        refine_dbt_object_classification_with_diagnostics(&records);
    let two_d_filter = two_d_views_filter();

    let two_d_views = if options.selection.two_d_views {
        Some(build_two_d_views_plan(
            &refined_records,
            &two_d_filter,
            &options,
        )?)
    } else {
        None
    };

    let dbt = if options.selection.dbt {
        Some(build_dbt_plan(&refined_records, dbt_scan))
    } else {
        None
    };

    if let Some(plan) = &two_d_views {
        warnings.extend(plan.selection_warnings.iter().cloned());
    }

    let source_objects = build_source_diagnostics(
        input,
        &records,
        &refined_records,
        &refinement_diagnostics,
        two_d_views.as_ref(),
        dbt.as_ref(),
        options.selection.two_d_views.then_some(&two_d_filter),
    );

    let summary = MammographyPlanSummary {
        input_dicom_files,
        mammogram_records,
        source_objects: source_objects.len(),
        two_d_selected_views: two_d_views.as_ref().map_or(0, |plan| {
            plan.selected_views
                .values()
                .filter(|view| view.selected)
                .count()
        }),
        dbt_composition_inputs: dbt.as_ref().map_or(0, |plan| plan.composition_inputs.len()),
        dbt_multiframe_volume_candidates: dbt
            .as_ref()
            .map_or(0, |plan| plan.multiframe_volume_candidates.len()),
        warnings: warnings.len(),
    };

    Ok(MammographyInputPlan {
        input_path: input.display().to_string(),
        plan: options.selection,
        summary,
        two_d_views,
        dbt,
        source_objects,
        warnings,
    })
}

fn validate_plan_selection(selection: MammographyPlanSelection) -> Result<()> {
    if selection.is_empty() {
        return Err(MammocatError::InvalidValue(
            "mammography plan must include at least one input group".to_string(),
        ));
    }
    Ok(())
}

fn two_d_views_filter() -> FilterConfig {
    let allowed_types = HashSet::from([
        MammogramType::Ffdm,
        MammogramType::Synth,
        MammogramType::Sfm,
    ]);
    let allowed_dbt_object_kinds = HashSet::from([DbtObjectKind::None]);

    FilterConfig::default()
        .with_allowed_types(allowed_types)
        .with_allowed_dbt_object_kinds(allowed_dbt_object_kinds)
}

fn build_two_d_views_plan(
    records: &[MammogramRecord],
    filter_config: &FilterConfig,
    options: &MammographyPlanOptions,
) -> Result<TwoDViewsPlan> {
    let (selection, warnings) = get_preferred_views_filtered_with_study_mode_and_warnings(
        records,
        filter_config,
        options.preference_order,
        options.study_selection_mode,
    )?;

    let mut selected_views = BTreeMap::new();
    let mut missing_views = Vec::new();
    for view in &STANDARD_MAMMO_VIEWS {
        let view_name = view.to_string();
        let selected = selection.get(view).and_then(Option::as_ref);
        if let Some(record) = selected {
            selected_views.insert(
                view_name.clone(),
                TwoDViewSelection {
                    view: view_name,
                    selected: true,
                    source_path: Some(record.file_path.display().to_string()),
                    mammogram_type: Some(record.metadata.mammogram_type.to_string()),
                    dbt_object_kind: Some(record.metadata.dbt_object_kind.to_string()),
                    reason: Some("selected_2d_view".to_string()),
                },
            );
        } else {
            missing_views.push(view_name.clone());
            selected_views.insert(
                view_name.clone(),
                TwoDViewSelection {
                    view: view_name,
                    selected: false,
                    source_path: None,
                    mammogram_type: None,
                    dbt_object_kind: None,
                    reason: Some("no_eligible_2d_view_candidate".to_string()),
                },
            );
        }
    }

    Ok(TwoDViewsPlan {
        selected_views,
        missing_views,
        selection_warnings: warnings
            .iter()
            .map(|warning| warning.message().to_string())
            .collect(),
    })
}

fn build_dbt_plan(records: &[MammogramRecord], dbt_scan: Option<DbtScanReport>) -> DbtPlan {
    let mut composition_inputs = Vec::new();
    let mut volume_candidates = Vec::new();
    let mut fallback_slice_paths = BTreeSet::new();
    let mut unsupported_series = Vec::new();
    let mut skipped_files = Vec::new();

    if let Some(scan) = dbt_scan {
        for series in scan.conversion_needed_series {
            fallback_slice_paths.extend(series.source_paths.iter().cloned());
            composition_inputs.push(composition_input_from_series(series));
        }
        for series in scan.already_multiframe_dbt_series {
            volume_candidates.push(volume_candidate_from_series(series));
        }
        unsupported_series = scan.unsupported_series;
        skipped_files = scan.skipped_files;
    }

    let mut seen_volume_sources: BTreeSet<Vec<String>> = volume_candidates
        .iter()
        .map(|candidate| candidate.source_paths.clone())
        .collect();
    for record in records
        .iter()
        .filter(|record| record.metadata.dbt_object_kind == DbtObjectKind::Volume)
    {
        let source_paths = vec![record.file_path.display().to_string()];
        if seen_volume_sources.insert(source_paths.clone()) {
            volume_candidates.push(volume_candidate_from_record(record, source_paths));
        }
    }

    DbtPlan {
        composition_inputs,
        multiframe_volume_candidates: volume_candidates,
        fallback_slice_paths: fallback_slice_paths.into_iter().collect(),
        unsupported_series,
        skipped_files,
    }
}

fn composition_input_from_series(series: DbtSeriesFinding) -> DbtCompositionInput {
    DbtCompositionInput {
        study_instance_uid: series.study_instance_uid,
        series_instance_uid: series.series_instance_uid,
        source_paths: series.source_paths,
        relative_parent: series.relative_parent,
        frame_count: series.frame_count,
        laterality: series.laterality,
        view_position: series.view_position,
        source_modality: series.source_modality,
        series_description: series.series_description,
        reason: "split_slice_series_needs_composition".to_string(),
    }
}

fn volume_candidate_from_series(series: DbtSeriesFinding) -> DbtVolumeCandidate {
    DbtVolumeCandidate {
        study_instance_uid: Some(series.study_instance_uid),
        series_instance_uid: Some(series.series_instance_uid),
        source_paths: series.source_paths,
        frame_count: series.frame_count,
        laterality: Some(series.laterality),
        view_position: Some(series.view_position),
        reason: "already_multiframe_dbt_series".to_string(),
    }
}

fn volume_candidate_from_record(
    record: &MammogramRecord,
    source_paths: Vec<String>,
) -> DbtVolumeCandidate {
    DbtVolumeCandidate {
        study_instance_uid: record.study_instance_uid.clone(),
        series_instance_uid: record.series_instance_uid.clone(),
        source_paths,
        frame_count: usize::try_from(record.metadata.number_of_frames).unwrap_or_default(),
        laterality: Some(record.metadata.laterality.to_string()),
        view_position: Some(record.metadata.view_position.to_string()),
        reason: "refined_or_extracted_multiframe_dbt_volume".to_string(),
    }
}

fn build_source_diagnostics(
    input: &Path,
    original_records: &[MammogramRecord],
    refined_records: &[MammogramRecord],
    refinement_diagnostics: &[DbtRefinementDiagnostic],
    two_d_views: Option<&TwoDViewsPlan>,
    dbt: Option<&DbtPlan>,
    two_d_filter: Option<&FilterConfig>,
) -> Vec<SourceObjectDiagnostic> {
    let refinement_by_path: HashMap<PathBuf, &DbtRefinementDiagnostic> = refinement_diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.file_path.clone(), diagnostic))
        .collect();
    let refined_by_path: HashMap<PathBuf, &MammogramRecord> = refined_records
        .iter()
        .map(|record| (record.file_path.clone(), record))
        .collect();
    let two_d_roles = two_d_roles_by_path(two_d_views);
    let dbt_roles = dbt_roles_by_source(dbt);

    let mut diagnostics = Vec::new();
    let mut seen_source_keys = BTreeSet::new();
    for original in original_records {
        let refined = refined_by_path
            .get(&original.file_path)
            .copied()
            .unwrap_or(original);
        let mut selected_as = Vec::new();
        let source_path = original.file_path.display().to_string();
        if let Some(role) = two_d_roles.get(&source_path) {
            selected_as.push(role.clone());
        }
        for key in source_lookup_keys(input, &original.file_path) {
            if let Some(roles) = dbt_roles.get(&key) {
                selected_as.extend(roles.iter().cloned());
            }
            seen_source_keys.insert(key);
        }

        selected_as.sort();
        selected_as.dedup();
        let filtered_by = two_d_filter
            .map(|config| filter_reasons(refined, config))
            .unwrap_or_default();
        let refinement = refinement_by_path.get(&original.file_path).copied();
        let status = source_status(&selected_as, &filtered_by);
        diagnostics.push(SourceObjectDiagnostic {
            source_path,
            study_instance_uid: refined.study_instance_uid.clone(),
            series_instance_uid: refined.series_instance_uid.clone(),
            sop_instance_uid: refined.sop_instance_uid.clone(),
            original_mammogram_type: Some(original.metadata.mammogram_type.to_string()),
            original_dbt_object_kind: Some(original.metadata.dbt_object_kind.to_string()),
            refined_mammogram_type: Some(refined.metadata.mammogram_type.to_string()),
            refined_dbt_object_kind: Some(refined.metadata.dbt_object_kind.to_string()),
            refinement_reason: refinement.map(|diagnostic| diagnostic.reason.as_str().to_string()),
            selected_as,
            filtered_by,
            status,
        });
    }

    for (source_path, roles) in dbt_roles {
        if seen_source_keys.contains(&source_path) {
            continue;
        }
        diagnostics.push(SourceObjectDiagnostic {
            source_path,
            study_instance_uid: None,
            series_instance_uid: None,
            sop_instance_uid: None,
            original_mammogram_type: None,
            original_dbt_object_kind: None,
            refined_mammogram_type: None,
            refined_dbt_object_kind: None,
            refinement_reason: None,
            selected_as: roles,
            filtered_by: Vec::new(),
            status: "selected".to_string(),
        });
    }

    diagnostics.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    diagnostics
}

fn two_d_roles_by_path(two_d_views: Option<&TwoDViewsPlan>) -> HashMap<String, String> {
    let mut roles = HashMap::new();
    if let Some(plan) = two_d_views {
        for (view, selection) in &plan.selected_views {
            if let Some(source_path) = &selection.source_path {
                roles.insert(source_path.clone(), format!("two_d_view:{view}"));
            }
        }
    }
    roles
}

fn dbt_roles_by_source(dbt: Option<&DbtPlan>) -> BTreeMap<String, Vec<String>> {
    let mut roles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(plan) = dbt {
        for composition in &plan.composition_inputs {
            for source_path in &composition.source_paths {
                roles.entry(source_path.clone()).or_default().push(format!(
                    "dbt_composition_source:{}",
                    composition.series_instance_uid
                ));
            }
        }
        for candidate in &plan.multiframe_volume_candidates {
            for source_path in &candidate.source_paths {
                roles
                    .entry(source_path.clone())
                    .or_default()
                    .push("dbt_volume_candidate".to_string());
            }
        }
    }
    for values in roles.values_mut() {
        values.sort();
        values.dedup();
    }
    roles
}

fn source_lookup_keys(input: &Path, path: &Path) -> Vec<String> {
    let mut keys = vec![path.display().to_string()];
    if let Ok(relative) = path.strip_prefix(input) {
        keys.push(relative.display().to_string());
    }
    keys
}

fn source_status(selected_as: &[String], filtered_by: &[String]) -> String {
    if !selected_as.is_empty() {
        "selected".to_string()
    } else if !filtered_by.is_empty() {
        "excluded".to_string()
    } else {
        "unused".to_string()
    }
}

fn filter_reasons(record: &MammogramRecord, config: &FilterConfig) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(allowed_types) = &config.allowed_types {
        if !allowed_types.contains(&record.metadata.mammogram_type) {
            reasons.push("allowed_types".to_string());
        }
    }
    if let Some(allowed_dbt_object_kinds) = &config.allowed_dbt_object_kinds {
        if !allowed_dbt_object_kinds.contains(&record.metadata.dbt_object_kind) {
            reasons.push("allowed_dbt_object_kinds".to_string());
        }
    }
    if config.exclude_implants && record.metadata.has_implant {
        reasons.push("exclude_implants".to_string());
    }
    if config.exclude_non_standard_views && !record.metadata.is_standard_view() {
        reasons.push("only_standard_views".to_string());
    }
    if config.exclude_for_processing && record.metadata.is_for_processing {
        reasons.push("exclude_for_processing".to_string());
    }
    if config.exclude_secondary_capture && record.metadata.is_secondary_capture {
        reasons.push("exclude_secondary_capture".to_string());
    }
    if config.exclude_non_mg_modality {
        match &record.metadata.modality {
            Some(modality) if modality.eq_ignore_ascii_case("MG") => {}
            Some(_) => reasons.push("exclude_non_mg".to_string()),
            None => reasons.push("missing_modality".to_string()),
        }
    }
    if config.exclude_lossy_compressed && record.is_lossy_compressed {
        reasons.push("exclude_lossy_compressed".to_string());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MammogramMetadata;
    use crate::types::{ImageType, Laterality, ViewPosition};

    const STUDY_UID: &str = "1.2.826.0.1";
    const SERIES_UID: &str = "1.2.826.0.1.1";

    fn test_options(selection: MammographyPlanSelection) -> MammographyPlanOptions {
        MammographyPlanOptions {
            selection,
            preference_order: PreferenceOrder::Default,
            study_selection_mode: StudySelectionMode::MostComplete,
        }
    }

    fn make_record(
        file_name: &str,
        laterality: Laterality,
        view_position: ViewPosition,
        mammogram_type: MammogramType,
        dbt_object_kind: DbtObjectKind,
    ) -> MammogramRecord {
        MammogramRecord {
            file_path: PathBuf::from(file_name),
            metadata: MammogramMetadata {
                mammogram_type,
                dbt_object_kind,
                laterality,
                view_position,
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
                number_of_frames: if dbt_object_kind == DbtObjectKind::Volume {
                    50
                } else {
                    1
                },
                concatenation_uid: None,
                sop_instance_uid_of_concatenation_source: None,
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
                transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
                transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
                compression_type: Some("uncompressed".to_string()),
            },
            study_instance_uid: Some(STUDY_UID.to_string()),
            series_instance_uid: Some(SERIES_UID.to_string()),
            sop_instance_uid: Some(format!("{SERIES_UID}.{file_name}")),
            rows: Some(2560),
            columns: Some(3328),
            transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
            is_lossy_compressed: false,
            is_implant_displaced: false,
            is_spot_compression: false,
            is_magnified: false,
        }
    }

    fn make_ambiguous_record(file_name: &str, series_uid: &str, index: usize) -> MammogramRecord {
        let mut record = make_record(
            file_name,
            Laterality::Right,
            ViewPosition::Cc,
            MammogramType::Unknown,
            DbtObjectKind::Unknown,
        );
        record.series_instance_uid = Some(series_uid.to_string());
        record.sop_instance_uid = Some(format!("{series_uid}.{index}"));
        record
    }

    fn split_series_scan_report() -> DbtScanReport {
        DbtScanReport {
            input_path: "input".to_string(),
            summary: crate::DbtScanSummary {
                total_files: 3,
                dicom_files: 3,
                conversion_needed_series: 1,
                already_multiframe_dbt_series: 0,
                copy_through_files: 0,
                unsupported_series: 0,
                skipped_files: 0,
            },
            conversion_needed_series: vec![DbtSeriesFinding {
                study_instance_uid: STUDY_UID.to_string(),
                series_instance_uid: "1.2.826.0.1.dbt".to_string(),
                source_paths: vec![
                    "slice_1.dcm".to_string(),
                    "slice_2.dcm".to_string(),
                    "slice_3.dcm".to_string(),
                ],
                relative_parent: ".".to_string(),
                frame_count: 3,
                laterality: "R".to_string(),
                view_position: "CC".to_string(),
                source_modality: "CT".to_string(),
                series_description: Some("TOMO R-CC".to_string()),
            }],
            already_multiframe_dbt_series: Vec::new(),
            copy_through_files: Vec::new(),
            unsupported_series: Vec::new(),
            skipped_files: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn two_d_views_plan_excludes_tomo_slice_records() {
        let records = vec![
            make_record(
                "2d.dcm",
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                DbtObjectKind::None,
            ),
            make_record(
                "slice.dcm",
                Laterality::Right,
                ViewPosition::Cc,
                MammogramType::Tomo,
                DbtObjectKind::Slice,
            ),
        ];

        let plan = build_mammography_plan(
            Path::new("."),
            records.len(),
            records,
            None,
            Vec::new(),
            test_options(MammographyPlanSelection::two_d_views_only()),
        )
        .unwrap();

        let slice_diag = plan
            .source_objects
            .iter()
            .find(|object| object.source_path == "slice.dcm")
            .expect("slice diagnostic");
        assert!(slice_diag
            .filtered_by
            .contains(&"allowed_types".to_string()));
        assert!(slice_diag
            .filtered_by
            .contains(&"allowed_dbt_object_kinds".to_string()));
        assert_eq!(slice_diag.status, "excluded");
        assert_eq!(plan.summary.two_d_selected_views, 1);
    }

    #[test]
    fn split_dbt_series_appears_as_composition_input() {
        let plan = build_mammography_plan(
            Path::new("."),
            3,
            Vec::new(),
            Some(split_series_scan_report()),
            Vec::new(),
            test_options(MammographyPlanSelection::dbt_only()),
        )
        .unwrap();

        let dbt = plan.dbt.expect("dbt plan");
        assert_eq!(dbt.composition_inputs.len(), 1);
        assert_eq!(dbt.composition_inputs[0].frame_count, 3);
        assert_eq!(dbt.fallback_slice_paths.len(), 3);
        assert!(plan.source_objects.iter().any(|object| object
            .selected_as
            .iter()
            .any(|role| { role.starts_with("dbt_composition_source:") })));
    }

    #[test]
    fn multiframe_dbt_record_appears_as_volume_candidate() {
        let records = vec![make_record(
            "volume.dcm",
            Laterality::Left,
            ViewPosition::Mlo,
            MammogramType::Tomo,
            DbtObjectKind::Volume,
        )];

        let plan = build_mammography_plan(
            Path::new("."),
            records.len(),
            records,
            None,
            Vec::new(),
            test_options(MammographyPlanSelection::dbt_only()),
        )
        .unwrap();

        let dbt = plan.dbt.expect("dbt plan");
        assert_eq!(dbt.multiframe_volume_candidates.len(), 1);
        assert_eq!(dbt.multiframe_volume_candidates[0].frame_count, 50);
    }

    #[test]
    fn combined_plan_keeps_2d_views_and_dbt_surfaces_separate() {
        let records = vec![make_record(
            "2d.dcm",
            Laterality::Left,
            ViewPosition::Mlo,
            MammogramType::Ffdm,
            DbtObjectKind::None,
        )];

        let plan = build_mammography_plan(
            Path::new("."),
            4,
            records,
            Some(split_series_scan_report()),
            Vec::new(),
            test_options(MammographyPlanSelection::all()),
        )
        .unwrap();

        assert!(plan.two_d_views.is_some());
        assert_eq!(plan.summary.two_d_selected_views, 1);
        assert_eq!(plan.summary.dbt_composition_inputs, 1);
    }

    #[test]
    fn refinement_reasons_appear_for_changed_records() {
        let series_uid = "1.2.826.0.1.refine";
        let records: Vec<_> = (0..13)
            .map(|index| make_ambiguous_record(&format!("slice_{index}.dcm"), series_uid, index))
            .collect();

        let plan = build_mammography_plan(
            Path::new("."),
            records.len(),
            records,
            None,
            Vec::new(),
            test_options(MammographyPlanSelection::two_d_views_only()),
        )
        .unwrap();

        assert!(plan.source_objects.iter().all(|object| {
            object.refinement_reason.as_deref() == Some("split_slice_series_cardinality")
        }));
    }
}
