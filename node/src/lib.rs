use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mammocat_core::{
    collect_dicom_files_recursively, get_preferred_views_filtered_with_study_mode_and_warnings,
    DbtObjectKind, FilterConfig, Laterality, MammogramRecord as CoreMammogramRecord, MammogramType,
    MammogramView, PreferenceOrder, StudySelectionMode, ViewPosition, STANDARD_MAMMO_VIEWS,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Serialize;

const DEFAULT_PREFERENCE_ORDER: &str = "default";

#[napi(object, js_name = "DicomInputInternal")]
pub struct DicomInput {
    pub path: Option<String>,
    pub bytes: Option<Uint8Array>,
    pub filename: Option<String>,
}

#[napi(object)]
pub struct SelectionOptions {
    #[napi(ts_type = "\"default\" | \"synthetic-2d-first\" | \"tomo-first\"")]
    pub preference_order: Option<String>,
    pub strict: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object)]
pub struct PixelSpacing {
    pub row: f64,
    pub column: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object, use_nullable = true)]
pub struct MammogramMetadata {
    pub mammogram_type: String,
    pub dbt_object_kind: String,
    pub laterality: String,
    pub view_position: String,
    pub image_type: String,
    pub pixel_spacing: Option<PixelSpacing>,
    pub is_for_processing: bool,
    pub has_implant: bool,
    pub is_spot_compression: bool,
    pub is_magnified: bool,
    pub is_implant_displaced: bool,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub number_of_frames: i32,
    pub concatenation_uid: Option<String>,
    pub sop_instance_uid_of_concatenation_source: Option<String>,
    pub is_secondary_capture: bool,
    pub modality: Option<String>,
    pub transfer_syntax_uid: Option<String>,
    pub transfer_syntax_name: Option<String>,
    pub compression_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object, use_nullable = true)]
pub struct MammogramRecord {
    pub source: String,
    pub input_index: Option<u32>,
    pub metadata: MammogramMetadata,
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub sop_instance_uid: Option<String>,
    pub rows: Option<u32>,
    pub columns: Option<u32>,
    pub transfer_syntax_uid: Option<String>,
    pub is_lossy_compressed: bool,
    pub is_implant_displaced: bool,
    pub is_spot_compression: bool,
    pub is_magnified: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object, use_nullable = true)]
pub struct PreferredViewSlots {
    pub rcc: Option<MammogramRecord>,
    pub lcc: Option<MammogramRecord>,
    pub rmlo: Option<MammogramRecord>,
    pub lmlo: Option<MammogramRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object)]
pub struct CandidateDiagnostic {
    pub input_index: u32,
    pub source: String,
    pub status: String,
    pub selected_as: Vec<String>,
    pub filter_reasons: Vec<String>,
    pub metadata: MammogramMetadata,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object, use_nullable = true)]
pub struct InputError {
    pub input_index: u32,
    pub source: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[napi(object)]
pub struct PreferredViewSelection {
    pub views: PreferredViewSlots,
    pub missing_views: Vec<String>,
    pub warnings: Vec<String>,
    pub input_errors: Vec<InputError>,
    pub candidates: Vec<CandidateDiagnostic>,
}

struct IndexedRecord {
    input_index: u32,
    record: CoreMammogramRecord,
}

enum ResolvedInput {
    Path(String),
    Bytes {
        bytes: Vec<u8>,
        filename: Option<String>,
    },
}

#[napi(
    ts_args_type = "input: DicomInput",
    ts_return_type = "MammogramMetadata"
)]
pub fn extract_metadata(env: Env, input: DicomInput) -> Result<Unknown<'static>> {
    let input = resolve_input(input)?;
    let record = record_from_resolved_input(input).map_err(to_napi_error)?;
    env.to_js_value(&metadata_to_dto(&record.metadata))
}

#[napi(
    ts_args_type = "inputs: DicomInput[], options?: SelectionOptions",
    ts_return_type = "PreferredViewSelection"
)]
pub fn select_preferred_views(
    env: Env,
    inputs: Vec<DicomInput>,
    options: Option<SelectionOptions>,
) -> Result<Unknown<'static>> {
    env.to_js_value(&select_preferred_views_dto(inputs, options)?)
}

#[napi(ts_return_type = "PreferredViewSelection")]
pub fn select_preferred_views_from_directory(
    env: Env,
    path: String,
    options: Option<SelectionOptions>,
) -> Result<Unknown<'static>> {
    let files = collect_dicom_files_recursively(&PathBuf::from(&path)).map_err(to_napi_io_error)?;
    let inputs = files
        .into_iter()
        .map(|file_path| DicomInput {
            path: Some(file_path.display().to_string()),
            bytes: None,
            filename: None,
        })
        .collect();
    env.to_js_value(&select_preferred_views_dto(inputs, options)?)
}

fn select_preferred_views_dto(
    inputs: Vec<DicomInput>,
    options: Option<SelectionOptions>,
) -> Result<PreferredViewSelection> {
    let mut records = Vec::new();
    let mut input_errors = Vec::new();

    for (index, input) in inputs.into_iter().enumerate() {
        let input_index = index as u32;
        let resolved_input = resolve_input(input)?;
        let source = source_for_resolved_input(&resolved_input);
        match record_from_resolved_input(resolved_input) {
            Ok(record) => records.push(IndexedRecord {
                input_index,
                record,
            }),
            Err(error) => input_errors.push(InputError {
                input_index,
                source,
                code: error_code(&error).to_string(),
                message: error.to_string(),
            }),
        }
    }

    build_selection(records, input_errors, options)
}

fn resolve_input(input: DicomInput) -> Result<ResolvedInput> {
    match (input.path, input.bytes) {
        (Some(path), None) if !path.is_empty() => Ok(ResolvedInput::Path(path)),
        (None, Some(bytes)) => Ok(ResolvedInput::Bytes {
            bytes: bytes.to_vec(),
            filename: input.filename,
        }),
        (Some(_), Some(_)) => Err(invalid_arg(
            "DicomInput must provide either path or bytes, not both",
        )),
        (Some(_), None) => Err(invalid_arg("DicomInput path must not be empty")),
        (None, None) => Err(invalid_arg("DicomInput must provide path or bytes")),
    }
}

fn record_from_resolved_input(input: ResolvedInput) -> mammocat_core::Result<CoreMammogramRecord> {
    match input {
        ResolvedInput::Path(path) => CoreMammogramRecord::from_file(PathBuf::from(path)),
        ResolvedInput::Bytes { bytes, filename } => {
            CoreMammogramRecord::from_bytes(&bytes, filename.as_deref())
        }
    }
}

fn source_for_resolved_input(input: &ResolvedInput) -> Option<String> {
    match input {
        ResolvedInput::Path(path) => Some(path.clone()),
        ResolvedInput::Bytes { filename, .. } => filename.clone(),
    }
}

fn build_selection(
    records: Vec<IndexedRecord>,
    input_errors: Vec<InputError>,
    options: Option<SelectionOptions>,
) -> Result<PreferredViewSelection> {
    let preference_order = preference_order_from_options(options.as_ref())?;
    let filter_config = views_filter(preference_order);
    let study_selection_mode = StudySelectionMode::from_strict(
        options
            .as_ref()
            .and_then(|opts| opts.strict)
            .unwrap_or(false),
    );
    let core_records: Vec<CoreMammogramRecord> = records
        .iter()
        .map(|indexed| indexed.record.clone())
        .collect();

    let (selection, warnings) = match get_preferred_views_filtered_with_study_mode_and_warnings(
        &core_records,
        &filter_config,
        preference_order,
        study_selection_mode,
    ) {
        Ok(result) => result,
        Err(error) => {
            return Ok(empty_selection(
                input_errors,
                records,
                vec![format!("Selection failed: {error}")],
                &filter_config,
            ));
        }
    };

    let selected_lookup = selected_view_lookup(&selection);
    let views = PreferredViewSlots {
        rcc: selected_record_for_view(&selection, &records, Laterality::Right, ViewPosition::Cc),
        lcc: selected_record_for_view(&selection, &records, Laterality::Left, ViewPosition::Cc),
        rmlo: selected_record_for_view(&selection, &records, Laterality::Right, ViewPosition::Mlo),
        lmlo: selected_record_for_view(&selection, &records, Laterality::Left, ViewPosition::Mlo),
    };
    let missing_views = missing_views(&views);
    let candidates = candidate_diagnostics(&records, &selected_lookup, &filter_config);

    Ok(PreferredViewSelection {
        views,
        missing_views,
        warnings: warnings
            .iter()
            .map(|warning| warning.message().to_string())
            .collect(),
        input_errors,
        candidates,
    })
}

fn empty_selection(
    input_errors: Vec<InputError>,
    records: Vec<IndexedRecord>,
    warnings: Vec<String>,
    filter_config: &FilterConfig,
) -> PreferredViewSelection {
    PreferredViewSelection {
        views: PreferredViewSlots {
            rcc: None,
            lcc: None,
            rmlo: None,
            lmlo: None,
        },
        missing_views: STANDARD_MAMMO_VIEWS
            .iter()
            .map(|view| view.to_string())
            .collect(),
        warnings,
        input_errors,
        candidates: candidate_diagnostics(&records, &HashMap::new(), filter_config),
    }
}

fn views_filter(preference_order: PreferenceOrder) -> FilterConfig {
    let allowed_types = match preference_order {
        PreferenceOrder::TomoFirst => HashSet::from([
            MammogramType::Ffdm,
            MammogramType::Synth,
            MammogramType::Sfm,
            MammogramType::Tomo,
        ]),
        PreferenceOrder::Default | PreferenceOrder::Synthetic2dFirst => HashSet::from([
            MammogramType::Ffdm,
            MammogramType::Synth,
            MammogramType::Sfm,
        ]),
    };
    let allowed_dbt_object_kinds = match preference_order {
        PreferenceOrder::TomoFirst => HashSet::from([
            DbtObjectKind::None,
            DbtObjectKind::Volume,
            DbtObjectKind::Slice,
            DbtObjectKind::Unknown,
        ]),
        PreferenceOrder::Default | PreferenceOrder::Synthetic2dFirst => {
            HashSet::from([DbtObjectKind::None])
        }
    };

    FilterConfig::default()
        .with_allowed_types(allowed_types)
        .with_allowed_dbt_object_kinds(allowed_dbt_object_kinds)
}

fn preference_order_from_options(options: Option<&SelectionOptions>) -> Result<PreferenceOrder> {
    match options
        .and_then(|opts| opts.preference_order.as_deref())
        .unwrap_or(DEFAULT_PREFERENCE_ORDER)
    {
        "default" => Ok(PreferenceOrder::Default),
        "synthetic-2d-first" => Ok(PreferenceOrder::Synthetic2dFirst),
        "tomo-first" => Ok(PreferenceOrder::TomoFirst),
        value => Err(invalid_arg(format!(
            "Unsupported preferenceOrder '{value}'. Expected default, synthetic-2d-first, or tomo-first"
        ))),
    }
}

fn selected_record_for_view(
    selection: &HashMap<MammogramView, Option<CoreMammogramRecord>>,
    records: &[IndexedRecord],
    laterality: Laterality,
    view_position: ViewPosition,
) -> Option<MammogramRecord> {
    let view = MammogramView::new(laterality, view_position);
    selection
        .get(&view)
        .and_then(Option::as_ref)
        .map(|record| record_to_dto(record, input_index_for_record(records, record)))
}

fn selected_view_lookup(
    selection: &HashMap<MammogramView, Option<CoreMammogramRecord>>,
) -> HashMap<RecordKey, Vec<String>> {
    let mut lookup: HashMap<RecordKey, Vec<String>> = HashMap::new();
    for (view, selected) in selection {
        if let Some(record) = selected {
            lookup
                .entry(record_key(record))
                .or_default()
                .push(view.to_string());
        }
    }
    lookup
}

fn input_index_for_record(
    records: &[IndexedRecord],
    selected: &CoreMammogramRecord,
) -> Option<u32> {
    let selected_key = record_key(selected);
    records
        .iter()
        .find(|indexed| record_key(&indexed.record) == selected_key)
        .map(|indexed| indexed.input_index)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RecordKey {
    path: String,
    sop_instance_uid: Option<String>,
}

fn record_key(record: &CoreMammogramRecord) -> RecordKey {
    RecordKey {
        path: source_for_record(record),
        sop_instance_uid: record.sop_instance_uid.clone(),
    }
}

fn missing_views(views: &PreferredViewSlots) -> Vec<String> {
    let mut missing = Vec::new();
    if views.rcc.is_none() {
        missing.push("rcc".to_string());
    }
    if views.lcc.is_none() {
        missing.push("lcc".to_string());
    }
    if views.rmlo.is_none() {
        missing.push("rmlo".to_string());
    }
    if views.lmlo.is_none() {
        missing.push("lmlo".to_string());
    }
    missing
}

fn candidate_diagnostics(
    records: &[IndexedRecord],
    selected_lookup: &HashMap<RecordKey, Vec<String>>,
    filter_config: &FilterConfig,
) -> Vec<CandidateDiagnostic> {
    records
        .iter()
        .map(|indexed| {
            let selected_as = selected_lookup
                .get(&record_key(&indexed.record))
                .cloned()
                .unwrap_or_default();
            let filter_reasons = filter_reasons(&indexed.record, filter_config);
            let status = if !selected_as.is_empty() {
                "selected"
            } else if !filter_reasons.is_empty() {
                "excluded"
            } else {
                "unused"
            };

            CandidateDiagnostic {
                input_index: indexed.input_index,
                source: source_for_record(&indexed.record),
                status: status.to_string(),
                selected_as,
                filter_reasons,
                metadata: metadata_to_dto(&indexed.record.metadata),
            }
        })
        .collect()
}

fn filter_reasons(record: &CoreMammogramRecord, filter_config: &FilterConfig) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(allowed_types) = &filter_config.allowed_types {
        if !allowed_types.contains(&record.metadata.mammogram_type) {
            reasons.push("allowedTypes".to_string());
        }
    }
    if let Some(allowed_kinds) = &filter_config.allowed_dbt_object_kinds {
        if !allowed_kinds.contains(&record.metadata.dbt_object_kind) {
            reasons.push("allowedDbtObjectKinds".to_string());
        }
    }
    if filter_config.exclude_implants && record.metadata.has_implant {
        reasons.push("excludeImplants".to_string());
    }
    if filter_config.exclude_non_standard_views && !record.metadata.is_standard_view() {
        reasons.push("excludeNonStandardViews".to_string());
    }
    if filter_config.exclude_for_processing && record.metadata.is_for_processing {
        reasons.push("excludeForProcessing".to_string());
    }
    if filter_config.exclude_secondary_capture && record.metadata.is_secondary_capture {
        reasons.push("excludeSecondaryCapture".to_string());
    }
    if filter_config.exclude_non_mg_modality {
        match record.metadata.modality.as_deref() {
            Some("MG") => {}
            Some(_) => reasons.push("excludeNonMgModality".to_string()),
            None => reasons.push("missingModality".to_string()),
        }
    }
    if filter_config.exclude_lossy_compressed && record.is_lossy_compressed {
        reasons.push("excludeLossyCompressed".to_string());
    }
    reasons
}

fn record_to_dto(record: &CoreMammogramRecord, input_index: Option<u32>) -> MammogramRecord {
    MammogramRecord {
        source: source_for_record(record),
        input_index,
        metadata: metadata_to_dto(&record.metadata),
        study_instance_uid: record.study_instance_uid.clone(),
        series_instance_uid: record.series_instance_uid.clone(),
        sop_instance_uid: record.sop_instance_uid.clone(),
        rows: record.rows.map(u32::from),
        columns: record.columns.map(u32::from),
        transfer_syntax_uid: record.transfer_syntax_uid.clone(),
        is_lossy_compressed: record.is_lossy_compressed,
        is_implant_displaced: record.is_implant_displaced,
        is_spot_compression: record.is_spot_compression,
        is_magnified: record.is_magnified,
    }
}

fn metadata_to_dto(metadata: &mammocat_core::MammogramMetadata) -> MammogramMetadata {
    MammogramMetadata {
        mammogram_type: metadata.mammogram_type.to_string(),
        dbt_object_kind: metadata.dbt_object_kind.to_string(),
        laterality: metadata.laterality.simple_name().to_string(),
        view_position: metadata.view_position.simple_name().to_string(),
        image_type: metadata.image_type.to_string(),
        pixel_spacing: metadata.pixel_spacing.map(|spacing| PixelSpacing {
            row: spacing.row,
            column: spacing.col,
        }),
        is_for_processing: metadata.is_for_processing,
        has_implant: metadata.has_implant,
        is_spot_compression: metadata.is_spot_compression,
        is_magnified: metadata.is_magnified,
        is_implant_displaced: metadata.is_implant_displaced,
        manufacturer: metadata.manufacturer.clone(),
        model: metadata.model.clone(),
        number_of_frames: metadata.number_of_frames,
        concatenation_uid: metadata.concatenation_uid.clone(),
        sop_instance_uid_of_concatenation_source: metadata
            .sop_instance_uid_of_concatenation_source
            .clone(),
        is_secondary_capture: metadata.is_secondary_capture,
        modality: metadata.modality.clone(),
        transfer_syntax_uid: metadata.transfer_syntax_uid.clone(),
        transfer_syntax_name: metadata.transfer_syntax_name.clone(),
        compression_type: metadata.compression_type.clone(),
    }
}

fn source_for_record(record: &CoreMammogramRecord) -> String {
    record.file_path.display().to_string()
}

fn error_code(error: &mammocat_core::MammocatError) -> &'static str {
    match error {
        mammocat_core::MammocatError::DicomError(_) => "dicom_error",
        mammocat_core::MammocatError::TagNotFound(_) => "tag_not_found",
        mammocat_core::MammocatError::InvalidValue(_) => "invalid_value",
        mammocat_core::MammocatError::ExtractionError(_) => "extraction_error",
        mammocat_core::MammocatError::SelectionError(_) => "selection_error",
        mammocat_core::MammocatError::IoError(_) => "io_error",
    }
}

fn to_napi_error(error: mammocat_core::MammocatError) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}

fn to_napi_io_error(error: std::io::Error) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}

fn invalid_arg(message: impl Into<String>) -> Error {
    Error::new(Status::InvalidArg, message.into())
}
