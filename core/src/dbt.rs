//! DBT slice-series scanning and conversion.

use crate::dicom_files::RecursiveFileInventory;
use crate::error::{MammocatError, Result};
use crate::extraction::parse_view_position;
use crate::extraction::tags::PIXEL_DATA_TAG;
use crate::selection::MammogramRecord;
use crate::types::ViewPosition;
use dicom_core::value::PrimitiveValue;
use dicom_core::{DataElement, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{
    DicomAttribute, DicomObject, FileDicomObject, FileMetaTableBuilder, InMemDicomObject,
    OpenFileOptions,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// SOP Class UID for Breast Tomosynthesis Image Storage.
pub const BREAST_TOMOSYNTHESIS_SOP_CLASS_UID: &str = uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE;

const SUPPORTED_TRANSFER_SYNTAXES: &[&str] = &[
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::IMPLICIT_VR_LITTLE_ENDIAN,
];

/// Scan options for DBT study detection.
#[derive(Debug, Clone, Default)]
pub struct DbtScanOptions;

/// Conversion options for DBT study conversion.
#[derive(Debug, Clone, Default)]
pub struct DbtConvertOptions {
    pub dry_run: bool,
    pub force: bool,
}

/// Summary counts for a DBT scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtScanSummary {
    pub total_files: usize,
    pub dicom_files: usize,
    pub conversion_needed_series: usize,
    pub already_multiframe_dbt_series: usize,
    pub copy_through_files: usize,
    pub unsupported_series: usize,
    pub skipped_files: usize,
}

/// Summary counts for a DBT conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtConvertSummary {
    pub total_files: usize,
    pub dicom_files: usize,
    pub conversion_needed_series: usize,
    pub converted_series: usize,
    pub copied_files: usize,
    pub unsupported_series: usize,
    pub skipped_files: usize,
}

/// Scan report for DBT conversion readiness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtScanReport {
    pub input_path: String,
    pub summary: DbtScanSummary,
    pub conversion_needed_series: Vec<DbtSeriesFinding>,
    pub already_multiframe_dbt_series: Vec<DbtSeriesFinding>,
    pub copy_through_files: Vec<DbtFileFinding>,
    pub unsupported_series: Vec<DbtUnsupportedSeries>,
    pub skipped_files: Vec<DbtSkippedFile>,
    pub warnings: Vec<String>,
}

/// Conversion report for DBT study conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtConvertReport {
    pub input_path: String,
    pub output_path: String,
    pub dry_run: bool,
    pub summary: DbtConvertSummary,
    pub converted_series: Vec<DbtConvertedSeries>,
    pub copied_files: Vec<DbtCopiedFile>,
    pub unsupported_series: Vec<DbtUnsupportedSeries>,
    pub skipped_files: Vec<DbtSkippedFile>,
    pub warnings: Vec<String>,
}

/// Series-level finding from a DBT scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtSeriesFinding {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub source_paths: Vec<String>,
    pub relative_parent: String,
    pub frame_count: usize,
    pub laterality: String,
    pub view_position: String,
    pub source_modality: String,
    pub series_description: Option<String>,
}

/// Single-file finding copied through during conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtFileFinding {
    pub source_path: String,
    pub relative_path: String,
    pub modality: Option<String>,
    pub sop_class_uid: Option<String>,
    pub series_instance_uid: Option<String>,
}

/// Unsupported series details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtUnsupportedSeries {
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub source_paths: Vec<String>,
    pub reason: String,
}

/// Non-DICOM or unreadable file details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtSkippedFile {
    pub path: String,
    pub reason: String,
}

/// Converted series output details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtConvertedSeries {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub output_path: String,
    pub frame_count: usize,
    pub source_paths: Vec<String>,
}

/// Copied file output details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbtCopiedFile {
    pub source_path: String,
    pub output_path: String,
}

#[derive(Debug, Clone)]
struct PlannedCopy {
    source_path: PathBuf,
    output_path: PathBuf,
    report: DbtCopiedFile,
}

#[derive(Debug, Clone)]
struct DicomFileInfo {
    relative_path: PathBuf,
    study_instance_uid: Option<String>,
    series_instance_uid: Option<String>,
    sop_class_uid: Option<String>,
    transfer_syntax_uid: Option<String>,
    modality: Option<String>,
    number_of_frames: Option<i32>,
    instance_number: Option<i32>,
    image_position_z: Option<f64>,
    rows: Option<u16>,
    columns: Option<u16>,
    samples_per_pixel: Option<u16>,
    photometric_interpretation: Option<String>,
    bits_allocated: Option<u16>,
    bits_stored: Option<u16>,
    high_bit: Option<u16>,
    pixel_representation: Option<u16>,
    image_laterality: Option<String>,
    laterality: Option<String>,
    view_position: Option<String>,
    series_description: Option<String>,
    image_type: Vec<String>,
}

pub(crate) struct DbtPlanningScan {
    pub report: DbtScanReport,
    pub records: Vec<MammogramRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SeriesKey {
    study_instance_uid: String,
    series_instance_uid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Geometry {
    rows: u16,
    columns: u16,
    samples_per_pixel: u16,
    photometric_interpretation: String,
    bits_allocated: u16,
    bits_stored: u16,
    high_bit: u16,
    pixel_representation: u16,
}

/// Recursively scan an input directory for old-format DBT series.
pub fn scan_dbt_study(input: impl AsRef<Path>, _options: DbtScanOptions) -> Result<DbtScanReport> {
    let input = input.as_ref();
    if !input.is_dir() {
        return Err(MammocatError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a directory", input.display()),
        )));
    }

    let files = collect_files(input)?;
    scan_dbt_study_from_files(input, files)
}

pub(crate) fn scan_dbt_study_from_files(
    input: &Path,
    files: Vec<PathBuf>,
) -> Result<DbtScanReport> {
    let total_files = files.len();
    scan_dbt_study_from_file_parts(input, files, total_files, Vec::new())
}

pub(crate) fn scan_dbt_study_for_planning(
    input: &Path,
    inventory: &RecursiveFileInventory,
    record_errors_as_warnings: bool,
) -> Result<DbtPlanningScan> {
    let mut skipped_files: Vec<DbtSkippedFile> = inventory
        .dbt_skipped_files
        .iter()
        .map(|path| DbtSkippedFile {
            path: path.display().to_string(),
            reason: "not a readable DICOM file: missing DICM magic bytes".to_string(),
        })
        .collect();
    let mut dicom_infos = Vec::with_capacity(inventory.dbt_files.len());
    let mut records = Vec::with_capacity(inventory.dicom_files.len());
    let mut warnings = Vec::new();

    for path in &inventory.dbt_files {
        if inventory.dicom_files.binary_search(path).is_ok() {
            match read_dicom_info_with_record(input, path, record_errors_as_warnings) {
                Ok((info, record_result)) => {
                    dicom_infos.push(info);
                    if let Some(record_result) = record_result {
                        match record_result {
                            Ok(record) => records.push(record),
                            Err(error) => {
                                warnings.push(format!("skipping {}: {error}", path.display()))
                            }
                        }
                    }
                }
                Err(reason) => {
                    warnings.push(format!("skipping {}: {reason}", path.display()));
                    skipped_files.push(DbtSkippedFile {
                        path: path.display().to_string(),
                        reason,
                    });
                }
            }
        } else {
            match read_dicom_info(input, path) {
                Ok(info) => dicom_infos.push(info),
                Err(reason) => skipped_files.push(DbtSkippedFile {
                    path: path.display().to_string(),
                    reason,
                }),
            }
        }
    }

    let report =
        build_dbt_scan_report(input, inventory.all_files.len(), dicom_infos, skipped_files)?;

    Ok(DbtPlanningScan {
        report,
        records,
        warnings,
    })
}

fn scan_dbt_study_from_file_parts(
    input: &Path,
    files: Vec<PathBuf>,
    total_files: usize,
    mut skipped_files: Vec<DbtSkippedFile>,
) -> Result<DbtScanReport> {
    let mut dicom_infos = Vec::with_capacity(files.len());

    for path in files {
        match read_dicom_info(input, &path) {
            Ok(info) => dicom_infos.push(info),
            Err(reason) => skipped_files.push(DbtSkippedFile {
                path: path.display().to_string(),
                reason,
            }),
        }
    }

    build_dbt_scan_report(input, total_files, dicom_infos, skipped_files)
}

fn build_dbt_scan_report(
    input: &Path,
    total_files: usize,
    dicom_infos: Vec<DicomFileInfo>,
    mut skipped_files: Vec<DbtSkippedFile>,
) -> Result<DbtScanReport> {
    let dicom_files = dicom_infos.len();
    let mut grouped: BTreeMap<SeriesKey, Vec<DicomFileInfo>> = BTreeMap::new();
    let mut unsupported_series = Vec::new();
    let mut copy_through_files = Vec::new();

    for info in dicom_infos {
        match (&info.study_instance_uid, &info.series_instance_uid) {
            (Some(study), Some(series)) => grouped
                .entry(SeriesKey {
                    study_instance_uid: study.clone(),
                    series_instance_uid: series.clone(),
                })
                .or_default()
                .push(info),
            _ => unsupported_series.push(DbtUnsupportedSeries {
                study_instance_uid: info.study_instance_uid.clone(),
                series_instance_uid: info.series_instance_uid.clone(),
                source_paths: vec![relative_string(&info.relative_path)],
                reason: "missing StudyInstanceUID or SeriesInstanceUID".to_string(),
            }),
        }
    }

    let mut conversion_needed_series = Vec::new();
    let mut already_multiframe_dbt_series = Vec::new();

    for (key, mut items) in grouped {
        items.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        if items.len() == 1 {
            let info = items.remove(0);
            if is_multiframe_dbt(&info) {
                already_multiframe_dbt_series.push(series_finding_from_single(&key, &info));
            } else {
                copy_through_files.push(file_finding(&info));
            }
            continue;
        }

        if items.iter().all(is_multiframe_dbt) {
            already_multiframe_dbt_series.push(series_finding_from_multiframe_group(&key, &items));
            continue;
        }

        if !has_dbt_evidence(&items) {
            unsupported_series.push(DbtUnsupportedSeries {
                study_instance_uid: Some(key.study_instance_uid),
                series_instance_uid: Some(key.series_instance_uid),
                source_paths: items
                    .iter()
                    .map(|i| relative_string(&i.relative_path))
                    .collect(),
                reason: "multi-file series has no DBT evidence".to_string(),
            });
            continue;
        }

        match validate_old_format_dbt_series(&key, items) {
            Ok(finding) => conversion_needed_series.push(finding),
            Err(unsupported) => unsupported_series.push(unsupported),
        }
    }

    conversion_needed_series.sort_by(|a, b| a.series_instance_uid.cmp(&b.series_instance_uid));
    already_multiframe_dbt_series.sort_by(|a, b| a.series_instance_uid.cmp(&b.series_instance_uid));
    copy_through_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    unsupported_series.sort_by(|a, b| a.source_paths.cmp(&b.source_paths));
    skipped_files.sort_by(|a, b| a.path.cmp(&b.path));

    let summary = DbtScanSummary {
        total_files,
        dicom_files,
        conversion_needed_series: conversion_needed_series.len(),
        already_multiframe_dbt_series: already_multiframe_dbt_series.len(),
        copy_through_files: copy_through_files.len(),
        unsupported_series: unsupported_series.len(),
        skipped_files: skipped_files.len(),
    };

    Ok(DbtScanReport {
        input_path: input.display().to_string(),
        summary,
        conversion_needed_series,
        already_multiframe_dbt_series,
        copy_through_files,
        unsupported_series,
        skipped_files,
        warnings: Vec::new(),
    })
}

/// Convert old-format DBT series and copy through non-converted DICOMs.
pub fn convert_dbt_study(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: DbtConvertOptions,
) -> Result<DbtConvertReport> {
    let input = input.as_ref();
    let output = output.as_ref();
    let scan = scan_dbt_study(input, DbtScanOptions)?;

    if !options.dry_run && !scan.unsupported_series.is_empty() {
        return Err(MammocatError::ExtractionError(format!(
            "cannot convert study with {} unsupported series; run check for details",
            scan.unsupported_series.len()
        )));
    }

    let mut converted_series = Vec::new();
    let mut planned_copies = Vec::new();

    for series in &scan.conversion_needed_series {
        let output_path = series_output_path(output, series);
        converted_series.push(DbtConvertedSeries {
            study_instance_uid: series.study_instance_uid.clone(),
            series_instance_uid: series.series_instance_uid.clone(),
            output_path: output_path.display().to_string(),
            frame_count: series.frame_count,
            source_paths: series.source_paths.clone(),
        });
    }

    for file in &scan.copy_through_files {
        planned_copies.push(plan_copy_file(
            input,
            output,
            &file.source_path,
            &file.relative_path,
        ));
    }

    for series in &scan.already_multiframe_dbt_series {
        for source in &series.source_paths {
            planned_copies.push(plan_copy_file(input, output, source, source));
        }
    }

    let copied_files = planned_copies
        .iter()
        .map(|planned| planned.report.clone())
        .collect::<Vec<_>>();

    if !options.dry_run {
        preflight_output_paths(&converted_series, &planned_copies, options.force)?;
        for (series, converted) in scan.conversion_needed_series.iter().zip(&converted_series) {
            combine_series(input, series, Path::new(&converted.output_path))?;
        }
        for planned in &planned_copies {
            copy_file_atomic(&planned.source_path, &planned.output_path)?;
        }
    }

    let summary = DbtConvertSummary {
        total_files: scan.summary.total_files,
        dicom_files: scan.summary.dicom_files,
        conversion_needed_series: scan.summary.conversion_needed_series,
        converted_series: converted_series.len(),
        copied_files: copied_files.len(),
        unsupported_series: scan.unsupported_series.len(),
        skipped_files: scan.skipped_files.len(),
    };

    Ok(DbtConvertReport {
        input_path: input.display().to_string(),
        output_path: output.display().to_string(),
        dry_run: options.dry_run,
        summary,
        converted_series,
        copied_files,
        unsupported_series: scan.unsupported_series,
        skipped_files: scan.skipped_files,
        warnings: scan.warnings,
    })
}

/// Convert one validated old-format DBT series into a multiframe DICOM.
///
/// The `series` value should come from [`scan_dbt_study`]. This writes only the
/// requested series and does not copy through unrelated study files.
pub fn write_combined_dbt_series(
    input: impl AsRef<Path>,
    series: &DbtSeriesFinding,
    output_path: impl AsRef<Path>,
) -> Result<DbtConvertedSeries> {
    let input = input.as_ref();
    let output_path = output_path.as_ref();
    ensure_can_write(output_path, false)?;
    combine_series(input, series, output_path)?;
    Ok(DbtConvertedSeries {
        study_instance_uid: series.study_instance_uid.clone(),
        series_instance_uid: series.series_instance_uid.clone(),
        output_path: output_path.display().to_string(),
        frame_count: series.frame_count,
        source_paths: series.source_paths.clone(),
    })
}

fn collect_files(input: &Path) -> Result<Vec<PathBuf>> {
    fn visit(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if is_dir(&file_type, &path) {
                visit(&path, files)?;
            } else if is_file(&file_type, &path) {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(input, &mut files)?;
    files.sort();
    Ok(files)
}

fn is_dir(file_type: &std::fs::FileType, path: &Path) -> bool {
    file_type.is_dir() || (file_type.is_symlink() && path.is_dir())
}

fn is_file(file_type: &std::fs::FileType, path: &Path) -> bool {
    file_type.is_file() || (file_type.is_symlink() && path.is_file())
}

fn read_dicom_info(input: &Path, path: &Path) -> std::result::Result<DicomFileInfo, String> {
    let dcm = open_dicom_metadata(path)?;
    Ok(dicom_info_from_file(input, path, &dcm))
}

fn read_dicom_info_with_record(
    input: &Path,
    path: &Path,
    record_errors_as_warnings: bool,
) -> std::result::Result<(DicomFileInfo, Option<Result<MammogramRecord>>), String> {
    let dcm = open_dicom_metadata(path)?;
    let info = dicom_info_from_file(input, path, &dcm);
    let record = if let Some(modality) = non_mg_modality(&info) {
        record_errors_as_warnings.then(|| {
            Err(MammocatError::ExtractionError(format!(
                "Expected modality=MG, found {modality}"
            )))
        })
    } else {
        Some(MammogramRecord::from_file_dicom(path.to_path_buf(), &dcm))
    };
    Ok((info, record))
}

fn non_mg_modality(info: &DicomFileInfo) -> Option<&str> {
    info.modality
        .as_deref()
        .filter(|modality| !modality.eq_ignore_ascii_case("MG"))
}

fn open_dicom_metadata(
    path: &Path,
) -> std::result::Result<FileDicomObject<InMemDicomObject>, String> {
    OpenFileOptions::new()
        .read_until(PIXEL_DATA_TAG)
        .open_file(path)
        .map_err(|e| format!("not a readable DICOM file: {}", e))
}

fn dicom_info_from_file(
    input: &Path,
    path: &Path,
    dcm: &FileDicomObject<InMemDicomObject>,
) -> DicomFileInfo {
    let relative_path = path.strip_prefix(input).unwrap_or(path).to_path_buf();

    DicomFileInfo {
        relative_path,
        study_instance_uid: get_string(dcm, tags::STUDY_INSTANCE_UID),
        series_instance_uid: get_string(dcm, tags::SERIES_INSTANCE_UID),
        sop_class_uid: get_string(dcm, tags::SOP_CLASS_UID),
        transfer_syntax_uid: Some(
            dcm.meta()
                .transfer_syntax()
                .trim_end_matches('\0')
                .to_string(),
        ),
        modality: get_string(dcm, tags::MODALITY),
        number_of_frames: get_i32(dcm, tags::NUMBER_OF_FRAMES),
        instance_number: get_i32(dcm, tags::INSTANCE_NUMBER),
        image_position_z: get_image_position_z(dcm),
        rows: get_u16(dcm, tags::ROWS),
        columns: get_u16(dcm, tags::COLUMNS),
        samples_per_pixel: get_u16(dcm, tags::SAMPLES_PER_PIXEL),
        photometric_interpretation: get_string(dcm, tags::PHOTOMETRIC_INTERPRETATION),
        bits_allocated: get_u16(dcm, tags::BITS_ALLOCATED),
        bits_stored: get_u16(dcm, tags::BITS_STORED),
        high_bit: get_u16(dcm, tags::HIGH_BIT),
        pixel_representation: get_u16(dcm, tags::PIXEL_REPRESENTATION),
        image_laterality: get_string(dcm, tags::IMAGE_LATERALITY),
        laterality: get_string(dcm, tags::LATERALITY),
        view_position: get_string(dcm, tags::VIEW_POSITION),
        series_description: get_string(dcm, tags::SERIES_DESCRIPTION),
        image_type: get_multi_string(dcm, tags::IMAGE_TYPE),
    }
}

fn is_multiframe_dbt(info: &DicomFileInfo) -> bool {
    info.number_of_frames.unwrap_or(1) > 1
        && string_eq(&info.modality, "MG")
        && (string_eq(&info.sop_class_uid, BREAST_TOMOSYNTHESIS_SOP_CLASS_UID)
            || has_dbt_text_evidence(info))
}

fn has_dbt_evidence(items: &[DicomFileInfo]) -> bool {
    items.iter().any(has_dbt_text_evidence)
}

fn has_dbt_text_evidence(info: &DicomFileInfo) -> bool {
    string_eq(&info.sop_class_uid, BREAST_TOMOSYNTHESIS_SOP_CLASS_UID)
        || info
            .series_description
            .as_deref()
            .map(|s| s.to_ascii_lowercase().contains("tomo"))
            .unwrap_or(false)
        || info
            .image_type
            .iter()
            .any(|s| s.to_ascii_lowercase().contains("tomo"))
}

fn validate_old_format_dbt_series(
    key: &SeriesKey,
    mut items: Vec<DicomFileInfo>,
) -> std::result::Result<DbtSeriesFinding, DbtUnsupportedSeries> {
    let source_paths = |items: &[DicomFileInfo]| {
        items
            .iter()
            .map(|i| relative_string(&i.relative_path))
            .collect::<Vec<_>>()
    };
    let unsupported = |reason: String, paths: Vec<String>| DbtUnsupportedSeries {
        study_instance_uid: Some(key.study_instance_uid.clone()),
        series_instance_uid: Some(key.series_instance_uid.clone()),
        source_paths: paths,
        reason,
    };

    for item in &items {
        let modality = item.modality.as_deref().unwrap_or("");
        if !modality.eq_ignore_ascii_case("CT") && !modality.eq_ignore_ascii_case("MG") {
            return Err(unsupported(
                format!(
                    "unsupported source modality {}; expected CT or MG",
                    modality
                ),
                source_paths(&items),
            ));
        }

        let frames = item.number_of_frames.unwrap_or(1);
        if frames != 1 {
            return Err(unsupported(
                "old-format DBT slices must be single-frame files".to_string(),
                source_paths(&items),
            ));
        }

        let transfer_syntax = item.transfer_syntax_uid.as_deref().unwrap_or("");
        if !SUPPORTED_TRANSFER_SYNTAXES.contains(&transfer_syntax) {
            return Err(unsupported(
                format!("unsupported transfer syntax {}", transfer_syntax),
                source_paths(&items),
            ));
        }
    }

    let expected_geometry = match geometry(&items[0]) {
        Some(expected_geometry) => expected_geometry,
        None => {
            return Err(unsupported(
                "missing required pixel geometry tags".to_string(),
                source_paths(&items),
            ));
        }
    };
    if items
        .iter()
        .skip(1)
        .any(|item| geometry(item) != Some(expected_geometry.clone()))
    {
        return Err(unsupported(
            "mixed image dimensions or pixel attributes in DBT series".to_string(),
            source_paths(&items),
        ));
    }

    let laterality = consistent_code(items.iter().filter_map(effective_laterality), "laterality")
        .map_err(|reason| unsupported(reason, source_paths(&items)))?;
    let view_position = consistent_view_position(&items)
        .map_err(|reason| unsupported(reason, source_paths(&items)))?;

    sort_frames(&mut items).map_err(|reason| unsupported(reason, source_paths(&items)))?;

    let source_modality = consistent_code(
        items.iter().filter_map(|item| item.modality.as_deref()),
        "modality",
    )
    .map_err(|reason| unsupported(reason, source_paths(&items)))?;
    let relative_parent = common_relative_parent(&items);
    let series_description = items[0].series_description.clone();

    Ok(DbtSeriesFinding {
        study_instance_uid: key.study_instance_uid.clone(),
        series_instance_uid: key.series_instance_uid.clone(),
        source_paths: items
            .iter()
            .map(|i| relative_string(&i.relative_path))
            .collect(),
        relative_parent,
        frame_count: items.len(),
        laterality,
        view_position: view_position.to_string(),
        source_modality,
        series_description,
    })
}

fn validate_contiguous_instance_numbers(
    instance_numbers: &[i32],
) -> std::result::Result<(), String> {
    let unique: BTreeSet<i32> = instance_numbers.iter().copied().collect();
    if unique.len() != instance_numbers.len() {
        return Err("duplicate InstanceNumber values in DBT series".to_string());
    }
    if instance_numbers
        .windows(2)
        .any(|window| window[1] != window[0] + 1)
    {
        return Err("non-contiguous InstanceNumber values in DBT series".to_string());
    }
    Ok(())
}

fn series_finding_from_single(key: &SeriesKey, info: &DicomFileInfo) -> DbtSeriesFinding {
    let relative_parent = info
        .relative_path
        .parent()
        .map(relative_string)
        .unwrap_or_default();
    let view_position = derive_view_position(std::slice::from_ref(info))
        .map(|view| view.to_string())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let laterality = effective_laterality(info).unwrap_or("").to_string();

    DbtSeriesFinding {
        study_instance_uid: key.study_instance_uid.clone(),
        series_instance_uid: key.series_instance_uid.clone(),
        source_paths: vec![relative_string(&info.relative_path)],
        relative_parent,
        frame_count: info.number_of_frames.unwrap_or(1).max(1) as usize,
        laterality,
        view_position,
        source_modality: info.modality.clone().unwrap_or_default(),
        series_description: info.series_description.clone(),
    }
}

fn series_finding_from_multiframe_group(
    key: &SeriesKey,
    items: &[DicomFileInfo],
) -> DbtSeriesFinding {
    let laterality = consistent_code(items.iter().filter_map(effective_laterality), "laterality")
        .unwrap_or_default();
    let view_position = consistent_view_position(items)
        .map(str::to_string)
        .unwrap_or_else(|_| "UNKNOWN".to_string());
    let source_modality = consistent_code(
        items.iter().filter_map(|item| item.modality.as_deref()),
        "modality",
    )
    .unwrap_or_default();

    DbtSeriesFinding {
        study_instance_uid: key.study_instance_uid.clone(),
        series_instance_uid: key.series_instance_uid.clone(),
        source_paths: items
            .iter()
            .map(|i| relative_string(&i.relative_path))
            .collect(),
        relative_parent: common_relative_parent(items),
        frame_count: items
            .iter()
            .map(|item| item.number_of_frames.unwrap_or(1).max(1) as usize)
            .sum(),
        laterality,
        view_position,
        source_modality,
        series_description: items[0].series_description.clone(),
    }
}

fn file_finding(info: &DicomFileInfo) -> DbtFileFinding {
    DbtFileFinding {
        source_path: relative_string(&info.relative_path),
        relative_path: relative_string(&info.relative_path),
        modality: info.modality.clone(),
        sop_class_uid: info.sop_class_uid.clone(),
        series_instance_uid: info.series_instance_uid.clone(),
    }
}

fn geometry(info: &DicomFileInfo) -> Option<Geometry> {
    Some(Geometry {
        rows: info.rows?,
        columns: info.columns?,
        samples_per_pixel: info.samples_per_pixel?,
        photometric_interpretation: info.photometric_interpretation.clone()?,
        bits_allocated: info.bits_allocated?,
        bits_stored: info.bits_stored?,
        high_bit: info.high_bit?,
        pixel_representation: info.pixel_representation?,
    })
}

fn effective_laterality(info: &DicomFileInfo) -> Option<&str> {
    info.image_laterality
        .as_deref()
        .or(info.laterality.as_deref())
        .filter(|s| !s.trim().is_empty())
}

fn derive_view_position(items: &[DicomFileInfo]) -> Option<&'static str> {
    for info in items {
        if let Some(view_position) = first_view_position_candidate(info) {
            return Some(view_position);
        }
    }
    None
}

fn consistent_view_position(items: &[DicomFileInfo]) -> std::result::Result<&'static str, String> {
    let candidates = items
        .iter()
        .flat_map(view_position_candidates)
        .collect::<BTreeSet<_>>();
    match candidates.len() {
        0 => Err("missing or unknown view position".to_string()),
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => Err("mixed view position values".to_string()),
    }
}

fn first_view_position_candidate(info: &DicomFileInfo) -> Option<&'static str> {
    view_position_candidates(info).into_iter().next()
}

fn view_position_candidates(info: &DicomFileInfo) -> Vec<&'static str> {
    [&info.view_position, &info.series_description]
        .into_iter()
        .filter_map(|value| value.as_deref())
        .filter_map(recognized_view_position)
        .collect()
}

fn recognized_view_position(value: &str) -> Option<&'static str> {
    let parsed = parse_view_position(value, false);
    (!parsed.is_unknown()).then(|| view_position_code(parsed))
}

fn view_position_code(view: ViewPosition) -> &'static str {
    match view {
        ViewPosition::Xccl => "XCCL",
        ViewPosition::Xccm => "XCCM",
        ViewPosition::Cc => "CC",
        ViewPosition::Mlo => "MLO",
        ViewPosition::Ml => "ML",
        ViewPosition::Lmo => "LMO",
        ViewPosition::Lm => "LM",
        ViewPosition::At => "AT",
        ViewPosition::Cv => "CV",
        ViewPosition::Unknown => "UNKNOWN",
    }
}

fn consistent_code<'a>(
    values: impl Iterator<Item = &'a str>,
    name: &str,
) -> std::result::Result<String, String> {
    let normalized: BTreeSet<String> = values
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .collect();
    match normalized.len() {
        0 => Err(format!("missing {}", name)),
        1 => Ok(normalized.into_iter().next().unwrap()),
        _ => Err(format!("mixed {} values", name)),
    }
}

fn common_relative_parent(items: &[DicomFileInfo]) -> String {
    let parent_parts: Vec<Vec<String>> = items
        .iter()
        .map(|info| {
            info.relative_path
                .parent()
                .map(|parent| {
                    parent
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_else(Vec::new)
        })
        .collect();

    if parent_parts.is_empty() {
        return String::new();
    }

    let first = &parent_parts[0];
    let mut common_len = first.len();
    for parts in &parent_parts[1..] {
        common_len = common_len.min(parts.len());
        for index in 0..common_len {
            if first[index] != parts[index] {
                common_len = index;
                break;
            }
        }
    }

    first[..common_len].join("/")
}

fn series_output_path(output_root: &Path, series: &DbtSeriesFinding) -> PathBuf {
    let safe_series_uid = series
        .series_instance_uid
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let file_name = format!("dbt_{}.dcm", safe_series_uid);
    if series.relative_parent.is_empty() {
        output_root.join(file_name)
    } else {
        output_root.join(&series.relative_parent).join(file_name)
    }
}

fn plan_copy_file(
    input: &Path,
    output: &Path,
    source_report_path: &str,
    relative_path: &str,
) -> PlannedCopy {
    let source_path = input.join(relative_path);
    let output_path = output.join(relative_path);
    PlannedCopy {
        source_path,
        output_path: output_path.clone(),
        report: DbtCopiedFile {
            source_path: source_report_path.to_string(),
            output_path: output_path.display().to_string(),
        },
    }
}

fn preflight_output_paths(
    converted_series: &[DbtConvertedSeries],
    planned_copies: &[PlannedCopy],
    force: bool,
) -> Result<()> {
    let mut planned_paths = BTreeSet::new();
    for series in converted_series {
        preflight_output_path(Path::new(&series.output_path), force, &mut planned_paths)?;
    }
    for copy in planned_copies {
        preflight_output_path(&copy.output_path, force, &mut planned_paths)?;
    }
    Ok(())
}

fn preflight_output_path(
    path: &Path,
    force: bool,
    planned_paths: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if !planned_paths.insert(path.to_path_buf()) {
        return Err(MammocatError::IoError(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} is planned more than once", path.display()),
        )));
    }
    ensure_can_write(path, force)
}

fn combine_series(input: &Path, series: &DbtSeriesFinding, output_path: &Path) -> Result<()> {
    let mut first_object = None;
    let mut pixel_data = Vec::new();

    for source in &series.source_paths {
        let source_path = input.join(source);
        let dcm = OpenFileOptions::new().open_file(&source_path)?;
        let pixel_bytes = dcm
            .element(tags::PIXEL_DATA)
            .map_err(|_| {
                MammocatError::ExtractionError(format!(
                    "missing PixelData in {}",
                    source_path.display()
                ))
            })?
            .to_bytes()
            .map_err(|e| {
                MammocatError::ExtractionError(format!(
                    "cannot read PixelData in {}: {}",
                    source_path.display(),
                    e
                ))
            })?;
        let expected_len = expected_frame_pixel_bytes(&dcm, &source_path)?;
        if pixel_bytes.len() != expected_len {
            return Err(MammocatError::ExtractionError(format!(
                "PixelData length in {} is {}; expected {} bytes from image geometry",
                source_path.display(),
                pixel_bytes.len(),
                expected_len
            )));
        }
        pixel_data.extend_from_slice(&pixel_bytes);
        drop(pixel_bytes);

        if first_object.is_none() {
            first_object = Some(dcm.into_inner());
        }
    }

    let mut obj = first_object
        .ok_or_else(|| MammocatError::ExtractionError("DBT series has no slices".to_string()))?;
    let sop_instance_uid = generate_uid();

    obj.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        BREAST_TOMOSYNTHESIS_SOP_CLASS_UID,
    ));
    obj.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid.as_str(),
    ));
    obj.put(DataElement::new(tags::MODALITY, VR::CS, "MG"));
    obj.put(DataElement::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        series.frame_count.to_string(),
    ));
    obj.put(DataElement::new(
        tags::VIEW_POSITION,
        VR::CS,
        series.view_position.as_str(),
    ));
    obj.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OW,
        PrimitiveValue::from(pixel_data),
    ));
    obj.remove_element(tags::SLICE_LOCATION);
    obj.remove_element(tags::IMAGE_POSITION_PATIENT);

    let file_obj = obj
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(BREAST_TOMOSYNTHESIS_SOP_CLASS_UID)
                .media_storage_sop_instance_uid(sop_instance_uid),
        )
        .map_err(|e| MammocatError::DicomError(format!("failed to build output meta: {}", e)))?;
    write_dicom_atomic(&file_obj, output_path)
}

fn write_dicom_atomic(
    obj: &dicom_object::FileDicomObject<InMemDicomObject>,
    output_path: &Path,
) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = temp_path_for(output_path);
    obj.write_to_file(&temp_path)
        .map_err(|e| MammocatError::DicomError(format!("failed to write DICOM: {}", e)))?;
    fs::rename(temp_path, output_path)?;
    Ok(())
}

fn copy_file_atomic(source: &Path, output_path: &Path) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = temp_path_for(output_path);
    fs::copy(source, &temp_path)?;
    fs::rename(temp_path, output_path)?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "output".into());
    path.with_file_name(format!(".{}.tmp", file_name))
}

fn ensure_can_write(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(MammocatError::IoError(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; pass --force to overwrite",
                path.display()
            ),
        )));
    }
    Ok(())
}

fn generate_uid() -> String {
    format!("2.25.{}", Uuid::new_v4().as_u128())
}

fn expected_frame_pixel_bytes(dcm: &impl DicomObject, source_path: &Path) -> Result<usize> {
    let rows = required_u16(dcm, tags::ROWS, "Rows", source_path)? as usize;
    let columns = required_u16(dcm, tags::COLUMNS, "Columns", source_path)? as usize;
    let samples_per_pixel =
        required_u16(dcm, tags::SAMPLES_PER_PIXEL, "SamplesPerPixel", source_path)? as usize;
    let bits_allocated =
        required_u16(dcm, tags::BITS_ALLOCATED, "BitsAllocated", source_path)? as usize;

    if !bits_allocated.is_multiple_of(8) {
        return Err(MammocatError::ExtractionError(format!(
            "BitsAllocated in {} is {}; expected whole-byte native pixels",
            source_path.display(),
            bits_allocated
        )));
    }

    rows.checked_mul(columns)
        .and_then(|value| value.checked_mul(samples_per_pixel))
        .and_then(|value| value.checked_mul(bits_allocated / 8))
        .ok_or_else(|| {
            MammocatError::ExtractionError(format!(
                "pixel byte length overflows usize for {}",
                source_path.display()
            ))
        })
}

fn required_u16(dcm: &impl DicomObject, tag: Tag, name: &str, source_path: &Path) -> Result<u16> {
    get_u16(dcm, tag).ok_or_else(|| {
        MammocatError::ExtractionError(format!("missing {} in {}", name, source_path.display()))
    })
}

fn sort_frames(items: &mut [DicomFileInfo]) -> std::result::Result<(), String> {
    if items.iter().all(|item| item.instance_number.is_some()) {
        items.sort_by_key(|item| item.instance_number.unwrap_or(i32::MAX));
        let instance_numbers = items
            .iter()
            .map(|item| {
                item.instance_number
                    .expect("all instance numbers were checked")
            })
            .collect::<Vec<_>>();
        return validate_contiguous_instance_numbers(&instance_numbers);
    }

    if items.iter().all(|item| item.image_position_z.is_some()) {
        items.sort_by(|a, b| {
            a.image_position_z
                .expect("all z positions were checked")
                .total_cmp(&b.image_position_z.expect("all z positions were checked"))
        });
        let positions = items
            .iter()
            .map(|item| item.image_position_z.expect("all z positions were checked"))
            .collect::<Vec<_>>();
        if positions.iter().any(|position| !position.is_finite()) {
            return Err("non-finite ImagePositionPatient z value in DBT series".to_string());
        }
        if positions.windows(2).any(|window| window[0] == window[1]) {
            return Err("duplicate ImagePositionPatient z values in DBT series".to_string());
        }
        return Ok(());
    }

    Err("missing InstanceNumber and unambiguous ImagePositionPatient fallback".to_string())
}

fn get_string(dcm: &impl DicomObject, tag: Tag) -> Option<String> {
    dcm.attr_opt(tag)
        .ok()
        .flatten()
        .and_then(|attr| {
            attr.to_str()
                .ok()
                .map(|value| value.trim_end_matches('\0').trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn get_multi_string(dcm: &impl DicomObject, tag: Tag) -> Vec<String> {
    dcm.attr_opt(tag)
        .ok()
        .flatten()
        .and_then(|attr| {
            let primitive = attr.to_primitive_value().ok()?;
            let values = primitive.to_multi_str();
            Some(
                values
                    .iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn get_i32(dcm: &impl DicomObject, tag: Tag) -> Option<i32> {
    dcm.attr_opt(tag)
        .ok()
        .flatten()
        .and_then(|attr| attr.to_i32().ok())
}

fn get_u16(dcm: &impl DicomObject, tag: Tag) -> Option<u16> {
    dcm.attr_opt(tag)
        .ok()
        .flatten()
        .and_then(|attr| attr.to_u16().ok())
}

fn get_image_position_z(dcm: &impl DicomObject) -> Option<f64> {
    dcm.attr_opt(tags::IMAGE_POSITION_PATIENT)
        .ok()
        .flatten()
        .and_then(|attr| {
            let primitive = attr.to_primitive_value().ok()?;
            let values = primitive.to_multi_float64().ok()?;
            values.get(2).copied()
        })
}

fn string_eq(value: &Option<String>, expected: &str) -> bool {
    value
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

fn relative_string(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_object::open_file;
    use tempfile::tempdir;

    const STUDY_UID: &str = "1.2.826.0.1.3680043.10.100.1";
    const SERIES_UID: &str = "1.2.826.0.1.3680043.10.100.2";
    const OTHER_SERIES_UID: &str = "1.2.826.0.1.3680043.10.100.3";
    const ROWS: u16 = 2;
    const COLUMNS: u16 = 2;
    const BYTES_PER_FRAME: usize = ROWS as usize * COLUMNS as usize * 2;

    #[test]
    fn write_combined_dbt_series_converts_requested_series_in_mammocat_order() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output").join("combined.dcm");
        fs::create_dir_all(input.join("study")).unwrap();

        write_test_slice(
            &input.join("study/z_instance_1.dcm"),
            SERIES_UID,
            1,
            ROWS,
            frame_bytes(1, BYTES_PER_FRAME),
        );
        write_test_slice(
            &input.join("study/a_instance_2.dcm"),
            SERIES_UID,
            2,
            ROWS,
            frame_bytes(2, BYTES_PER_FRAME),
        );
        write_test_slice(
            &input.join("study/unrelated_series.dcm"),
            OTHER_SERIES_UID,
            1,
            ROWS,
            frame_bytes(9, BYTES_PER_FRAME),
        );

        let scan = scan_dbt_study(&input, DbtScanOptions).unwrap();
        let series = scan
            .conversion_needed_series
            .iter()
            .find(|series| series.series_instance_uid == SERIES_UID)
            .unwrap();
        assert_eq!(
            series.source_paths,
            vec!["study/z_instance_1.dcm", "study/a_instance_2.dcm"]
        );

        let converted = write_combined_dbt_series(&input, series, &output).unwrap();

        assert_eq!(converted.study_instance_uid, STUDY_UID);
        assert_eq!(converted.series_instance_uid, SERIES_UID);
        assert_eq!(converted.frame_count, 2);
        assert_eq!(converted.source_paths, series.source_paths);
        assert!(output.exists());
        assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);

        let combined = open_file(&output).unwrap();
        assert_eq!(
            get_string(&combined, tags::SOP_CLASS_UID).as_deref(),
            Some(BREAST_TOMOSYNTHESIS_SOP_CLASS_UID)
        );
        assert_eq!(get_string(&combined, tags::MODALITY).as_deref(), Some("MG"));
        assert_eq!(
            get_string(&combined, tags::VIEW_POSITION).as_deref(),
            Some("MLO")
        );
        assert_eq!(get_i32(&combined, tags::NUMBER_OF_FRAMES), Some(2));
        assert_eq!(get_u16(&combined, tags::ROWS), Some(ROWS));
        assert_eq!(get_u16(&combined, tags::COLUMNS), Some(COLUMNS));

        let pixels = combined
            .element(tags::PIXEL_DATA)
            .unwrap()
            .to_bytes()
            .unwrap();
        assert_eq!(&pixels[..BYTES_PER_FRAME], frame_bytes(1, BYTES_PER_FRAME));
        assert_eq!(&pixels[BYTES_PER_FRAME..], frame_bytes(2, BYTES_PER_FRAME));
    }

    #[test]
    fn write_combined_dbt_series_errors_on_malformed_pixel_data() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        fs::create_dir_all(input.join("study")).unwrap();

        write_test_slice(
            &input.join("study/instance_1.dcm"),
            SERIES_UID,
            1,
            ROWS,
            frame_bytes(1, BYTES_PER_FRAME),
        );
        write_test_slice(
            &input.join("study/instance_2.dcm"),
            SERIES_UID,
            2,
            ROWS,
            frame_bytes(2, BYTES_PER_FRAME - 2),
        );

        let scan = scan_dbt_study(&input, DbtScanOptions).unwrap();
        let series = scan.conversion_needed_series.first().unwrap();
        let error = write_combined_dbt_series(
            &input,
            series,
            temp.path().join("output").join("combined.dcm"),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("PixelData length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn scan_dbt_study_rejects_mixed_geometry_before_series_conversion() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("input");
        fs::create_dir_all(input.join("study")).unwrap();

        write_test_slice(
            &input.join("study/instance_1.dcm"),
            SERIES_UID,
            1,
            ROWS,
            frame_bytes(1, BYTES_PER_FRAME),
        );
        write_test_slice(
            &input.join("study/instance_2.dcm"),
            SERIES_UID,
            2,
            ROWS + 1,
            frame_bytes(2, (ROWS as usize + 1) * COLUMNS as usize * 2),
        );

        let scan = scan_dbt_study(&input, DbtScanOptions).unwrap();

        assert!(scan.conversion_needed_series.is_empty());
        assert_eq!(scan.unsupported_series.len(), 1);
        assert_eq!(
            scan.unsupported_series[0].reason,
            "mixed image dimensions or pixel attributes in DBT series"
        );
    }

    fn write_test_slice(
        path: &Path,
        series_instance_uid: &str,
        instance_number: i32,
        rows: u16,
        pixel_data: Vec<u8>,
    ) {
        let sop_instance_uid = format!("{series_instance_uid}.{instance_number}");
        let mut obj = InMemDicomObject::new_empty();
        put_str(&mut obj, tags::SOP_CLASS_UID, uids::CT_IMAGE_STORAGE);
        put_str(&mut obj, tags::SOP_INSTANCE_UID, &sop_instance_uid);
        put_str(&mut obj, tags::STUDY_INSTANCE_UID, STUDY_UID);
        put_str(&mut obj, tags::SERIES_INSTANCE_UID, series_instance_uid);
        put_str(&mut obj, tags::MODALITY, "CT");
        put_str(&mut obj, tags::IMAGE_LATERALITY, "L");
        put_str(&mut obj, tags::VIEW_POSITION, "MLO");
        put_str(&mut obj, tags::SERIES_DESCRIPTION, "TOMO L MLO");
        obj.put(DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(
                vec![
                    "DERIVED".to_string(),
                    "PRIMARY".to_string(),
                    "TOMO".to_string(),
                ]
                .into(),
            ),
        ));
        obj.put(DataElement::new(
            tags::INSTANCE_NUMBER,
            VR::IS,
            instance_number.to_string(),
        ));
        obj.put(DataElement::new(
            tags::IMAGE_POSITION_PATIENT,
            VR::DS,
            PrimitiveValue::Strs(
                vec![
                    "0".to_string(),
                    "0".to_string(),
                    instance_number.to_string(),
                ]
                .into(),
            ),
        ));
        put_u16(&mut obj, tags::ROWS, rows);
        put_u16(&mut obj, tags::COLUMNS, COLUMNS);
        put_u16(&mut obj, tags::SAMPLES_PER_PIXEL, 1);
        put_str(&mut obj, tags::PHOTOMETRIC_INTERPRETATION, "MONOCHROME2");
        put_u16(&mut obj, tags::BITS_ALLOCATED, 16);
        put_u16(&mut obj, tags::BITS_STORED, 12);
        put_u16(&mut obj, tags::HIGH_BIT, 11);
        put_u16(&mut obj, tags::PIXEL_REPRESENTATION, 0);
        obj.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OW,
            PrimitiveValue::from(pixel_data),
        ));

        let file_obj = obj
            .with_meta(
                FileMetaTableBuilder::new()
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                    .media_storage_sop_class_uid(uids::CT_IMAGE_STORAGE)
                    .media_storage_sop_instance_uid(sop_instance_uid),
            )
            .unwrap();
        file_obj.write_to_file(path).unwrap();
    }

    fn put_str(obj: &mut InMemDicomObject, tag: Tag, value: &str) {
        obj.put(DataElement::new(tag, VR::CS, PrimitiveValue::from(value)));
    }

    fn put_u16(obj: &mut InMemDicomObject, tag: Tag, value: u16) {
        obj.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
    }

    fn frame_bytes(seed: u8, length: usize) -> Vec<u8> {
        vec![seed; length]
    }
}
