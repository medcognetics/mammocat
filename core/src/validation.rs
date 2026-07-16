//! Validation reports for mammocat and mammoselect readiness.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use dicom::parser::dataset::lazy_read::LazyDataSetReader;
use dicom::parser::dataset::LazyDataToken;
use dicom::parser::stateful::decode::StatefulDecode;
use dicom::transfer_syntax::{TransferSyntaxIndex, TransferSyntaxRegistry};
use dicom_core::header::HasLength;
use dicom_core::{DataElement, DicomValue, Tag};
use dicom_object::{open_file, FileDicomObject, FileMetaTable, InMemDicomObject, OpenFileOptions};

use crate::api::{MammogramExtractor, MammogramMetadata};
use crate::completion::{plan_completion, CompletionOptions};
use crate::dicom_files::collect_dicom_files;
use crate::extraction::extract_view_descriptor;
use crate::extraction::tags::{
    get_string_value, BITS_ALLOCATED, BITS_STORED, COLUMNS, DICOM_MAGIC_BYTES, HIGH_BIT,
    IMAGE_LATERALITY, IMAGE_TYPE, LOSSY_IMAGE_COMPRESSION, LOSSY_IMAGE_COMPRESSION_METHOD,
    MODALITY, NUMBER_OF_FRAMES, PHOTOMETRIC_INTERPRETATION, PIXEL_DATA_TAG, PIXEL_REPRESENTATION,
    PIXEL_SPACING, ROWS, SAMPLES_PER_PIXEL, SERIES_INSTANCE_UID, SOP_CLASS_UID, SOP_INSTANCE_UID,
    STUDY_INSTANCE_UID, VIEW_POSITION,
};
use crate::selection::{
    get_preferred_views_filtered, refine_dbt_object_classification, MammogramRecord,
};
use crate::types::{
    FilterConfig, Laterality, MammogramType, PreferenceOrder, ViewPosition, STANDARD_MAMMO_VIEWS,
};

const UNKNOWN_TRANSFER_SYNTAX: &str = "unknown transfer syntax";
const MONOCHROME1: &str = "MONOCHROME1";
const MONOCHROME2: &str = "MONOCHROME2";

const LOSSY_TRANSFER_SYNTAX_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.4.50", // JPEG Baseline 8-bit
    "1.2.840.10008.1.2.4.51", // JPEG Extended 12-bit
    "1.2.840.10008.1.2.4.52", // JPEG Extended retired
    "1.2.840.10008.1.2.4.53", // JPEG Spectral Selection retired
    "1.2.840.10008.1.2.4.54", // JPEG Spectral Selection retired
    "1.2.840.10008.1.2.4.55", // JPEG Full Progression retired
    "1.2.840.10008.1.2.4.56", // JPEG Full Progression retired
    "1.2.840.10008.1.2.4.81", // JPEG-LS near-lossless
];

/// Validation strictness profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
#[cfg_attr(
    any(feature = "json", feature = "python"),
    serde(rename_all = "lowercase")
)]
pub enum ValidationProfile {
    /// Validate that `MammogramExtractor` can extract metadata.
    Extraction,
    /// Validate that a file or directory is ready for mammoselect-style selection.
    #[default]
    Selection,
}

impl ValidationProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extraction => "extraction",
            Self::Selection => "selection",
        }
    }
}

/// Validation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
#[cfg_attr(
    any(feature = "json", feature = "python"),
    serde(rename_all = "lowercase")
)]
pub enum ValidationStatus {
    Pass,
    Fail,
}

/// Individual check outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
#[cfg_attr(
    any(feature = "json", feature = "python"),
    serde(rename_all = "lowercase")
)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
    Info,
}

/// Message severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
#[cfg_attr(
    any(feature = "json", feature = "python"),
    serde(rename_all = "lowercase")
)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Error,
    Warning,
    Info,
}

impl MessageKind {
    fn metadata(self) -> (Severity, CheckStatus) {
        match self {
            Self::Error => (Severity::Critical, CheckStatus::Fail),
            Self::Warning => (Severity::Warning, CheckStatus::Warn),
            Self::Info => (Severity::Info, CheckStatus::Info),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CheckDetails<'a> {
    tag: Option<Tag>,
    tag_name: Option<&'a str>,
    value: Option<String>,
}

impl<'a> CheckDetails<'a> {
    fn tag(tag: Tag, tag_name: &'a str, value: Option<String>) -> Self {
        Self {
            tag: Some(tag),
            tag_name: Some(tag_name),
            value,
        }
    }

    fn name(tag_name: &'a str, value: Option<String>) -> Self {
        Self {
            tag_name: Some(tag_name),
            value,
            ..Self::default()
        }
    }
}

/// Runtime error that prevents report construction.
#[derive(Debug, thiserror::Error)]
pub enum ValidationRuntimeError {
    #[error("invalid source path: {path}")]
    InvalidSourcePath { path: PathBuf },

    #[error("failed to read directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to open zip archive {path}: {source}")]
    OpenZip {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read zip archive {path}: {source}")]
    ReadZip {
        path: PathBuf,
        source: zip::result::ZipError,
    },

    #[error("failed to read zip entry {entry} in {path}: {source}")]
    ReadZipEntry {
        path: PathBuf,
        entry: String,
        source: std::io::Error,
    },
}

/// Options shared by the CLI and Python bindings.
#[derive(Debug, Clone)]
pub struct ValidationOptions {
    pub profile: ValidationProfile,
    pub filter_config: FilterConfig,
    pub preference_order: PreferenceOrder,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            profile: ValidationProfile::Selection,
            filter_config: FilterConfig::default(),
            preference_order: PreferenceOrder::Default,
        }
    }
}

/// Validation summary counts.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct ValidationSummary {
    pub valid: bool,
    pub profile: String,
    pub source_type: String,
    pub file_count: usize,
    pub valid_file_count: usize,
    pub invalid_file_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}

impl ValidationSummary {
    fn new(profile: ValidationProfile, source_type: &str, file_count: usize) -> Self {
        Self {
            valid: true,
            profile: profile.as_str().to_string(),
            source_type: source_type.to_string(),
            file_count,
            valid_file_count: 0,
            invalid_file_count: 0,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        }
    }
}

/// Source path metadata.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct SourceReport {
    pub path: String,
    pub source_type: String,
}

/// Validation message.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct ValidationMessage {
    pub code: String,
    pub message: String,
    pub severity: Severity,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub path: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub tag: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub tag_name: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
}

/// Per-check record.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct ValidationCheck {
    pub name: String,
    pub status: CheckStatus,
    pub severity: Severity,
    pub message: String,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub path: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub tag: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub tag_name: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct DicomFileMetaReport {
    pub path: String,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub transfer_syntax_uid: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub transfer_syntax_name: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub sop_class_uid: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub sop_instance_uid: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub study_instance_uid: Option<String>,
    #[cfg_attr(
        any(feature = "json", feature = "python"),
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub series_instance_uid: Option<String>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct ImageValidationReport {
    pub rows: Option<u16>,
    pub columns: Option<u16>,
    pub number_of_frames: Option<i32>,
    pub number_of_frames_source: String,
    pub samples_per_pixel: Option<u16>,
    pub photometric_interpretation: Option<String>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct PixelValidationReport {
    pub pixel_data_present: bool,
    pub pixel_data_description: Option<String>,
    pub bits_allocated: Option<u16>,
    pub bits_stored: Option<u16>,
    pub high_bit: Option<u16>,
    pub pixel_representation: Option<u16>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct MammographyValidationReport {
    pub modality: Option<String>,
    pub mammogram_type: Option<String>,
    pub dbt_object_kind: Option<String>,
    pub laterality: Option<String>,
    pub view_position: Option<String>,
    pub view_modifiers: Vec<String>,
    pub image_type: Option<String>,
    pub is_for_processing: Option<bool>,
    pub has_implant: Option<bool>,
    pub is_spot_compression: Option<bool>,
    pub is_magnified: Option<bool>,
    pub is_implant_displaced: Option<bool>,
    pub is_secondary_capture: Option<bool>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub concatenation_uid: Option<String>,
    pub sop_instance_uid_of_concatenation_source: Option<String>,
    pub pixel_spacing: Option<String>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct SelectionEligibilityReport {
    pub eligible: bool,
    pub filtered_by: Vec<String>,
    pub mammogram_view: Option<String>,
    pub standard_view: Option<bool>,
}

/// Validation report for one DICOM file.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct FileValidationReport {
    pub status: ValidationStatus,
    pub summary: ValidationSummary,
    pub file: DicomFileMetaReport,
    pub image: ImageValidationReport,
    pub pixel: PixelValidationReport,
    pub mammography: MammographyValidationReport,
    pub selection: SelectionEligibilityReport,
    pub errors: Vec<ValidationMessage>,
    pub warnings: Vec<ValidationMessage>,
    pub info: Vec<ValidationMessage>,
    pub checks: Vec<ValidationCheck>,
}

impl FileValidationReport {
    fn new(path: &Path, profile: ValidationProfile) -> Self {
        Self {
            status: ValidationStatus::Pass,
            summary: ValidationSummary::new(profile, "file", 1),
            file: DicomFileMetaReport {
                path: path.display().to_string(),
                transfer_syntax_uid: None,
                transfer_syntax_name: None,
                sop_class_uid: None,
                sop_instance_uid: None,
                study_instance_uid: None,
                series_instance_uid: None,
            },
            image: ImageValidationReport {
                number_of_frames_source: "unknown".to_string(),
                ..ImageValidationReport::default()
            },
            pixel: PixelValidationReport::default(),
            mammography: MammographyValidationReport::default(),
            selection: SelectionEligibilityReport {
                eligible: false,
                ..SelectionEligibilityReport::default()
            },
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
            checks: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.status == ValidationStatus::Pass
    }

    fn finalize(&mut self) {
        self.status = if self.errors.is_empty() {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        };
        self.summary.valid = self.errors.is_empty();
        self.summary.valid_file_count = usize::from(self.summary.valid);
        self.summary.invalid_file_count = usize::from(!self.summary.valid);
        self.summary.error_count = self.errors.len();
        self.summary.warning_count = self.warnings.len();
        self.summary.info_count = self.info.len();
    }

    fn record(
        &mut self,
        kind: MessageKind,
        code: &str,
        name: &str,
        message: String,
        details: CheckDetails<'_>,
    ) {
        let (severity, status) = kind.metadata();
        let path = Some(self.file.path.clone());
        let validation_message = ValidationMessage {
            code: code.to_string(),
            message: message.clone(),
            severity,
            path: path.clone(),
            tag: details.tag.map(format_tag),
            tag_name: details.tag_name.map(ToOwned::to_owned),
            value: details.value.clone(),
        };
        match kind {
            MessageKind::Error => self.errors.push(validation_message),
            MessageKind::Warning => self.warnings.push(validation_message),
            MessageKind::Info => self.info.push(validation_message),
        }
        self.checks.push(ValidationCheck {
            name: name.to_string(),
            status,
            severity,
            message,
            path,
            tag: details.tag.map(format_tag),
            tag_name: details.tag_name.map(ToOwned::to_owned),
            value: details.value,
        });
    }

    fn record_plain(&mut self, kind: MessageKind, code: &str, name: &str, message: String) {
        self.record(kind, code, name, message, CheckDetails::default());
    }

    fn record_tag(
        &mut self,
        kind: MessageKind,
        code: &str,
        name: &str,
        message: String,
        tag: (Tag, &str),
        value: Option<String>,
    ) {
        self.record(
            kind,
            code,
            name,
            message,
            CheckDetails::tag(tag.0, tag.1, value),
        );
    }

    fn record_name(
        &mut self,
        kind: MessageKind,
        code: &str,
        name: &str,
        message: String,
        tag_name: &str,
        value: Option<String>,
    ) {
        self.record(
            kind,
            code,
            name,
            message,
            CheckDetails::name(tag_name, value),
        );
    }

    fn pass(&mut self, name: &str, message: String, tag: Option<Tag>, value: Option<String>) {
        self.checks.push(ValidationCheck {
            name: name.to_string(),
            status: CheckStatus::Pass,
            severity: Severity::Info,
            message,
            path: Some(self.file.path.clone()),
            tag: tag.map(format_tag),
            tag_name: Some(name.to_string()),
            value,
        });
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct SelectedViewReport {
    pub view: String,
    pub selected: bool,
    pub file_path: Option<String>,
    pub mammogram_type: Option<String>,
    pub dbt_object_kind: Option<String>,
}

#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct DirectoryValidationReport {
    pub dicom_file_count: usize,
    pub selected_views: BTreeMap<String, SelectedViewReport>,
    pub missing_views: Vec<String>,
}

/// Top-level report for a file or directory source.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "json", feature = "python"), derive(serde::Serialize))]
pub struct ValidationReport {
    pub status: ValidationStatus,
    pub summary: ValidationSummary,
    pub source: SourceReport,
    pub files: Vec<FileValidationReport>,
    pub directory: Option<DirectoryValidationReport>,
    pub errors: Vec<ValidationMessage>,
    pub warnings: Vec<ValidationMessage>,
    pub info: Vec<ValidationMessage>,
    pub checks: Vec<ValidationCheck>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.status == ValidationStatus::Pass
    }

    fn new(path: &Path, source_type: &str, profile: ValidationProfile, file_count: usize) -> Self {
        Self {
            status: ValidationStatus::Pass,
            summary: ValidationSummary::new(profile, source_type, file_count),
            source: SourceReport {
                path: path.display().to_string(),
                source_type: source_type.to_string(),
            },
            files: Vec::new(),
            directory: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
            checks: Vec::new(),
        }
    }

    fn record(
        &mut self,
        kind: MessageKind,
        code: &str,
        name: &str,
        message: String,
        value: Option<String>,
    ) {
        let (severity, status) = kind.metadata();
        let validation_message = ValidationMessage {
            code: code.to_string(),
            message: message.clone(),
            severity,
            path: Some(self.source.path.clone()),
            tag: None,
            tag_name: None,
            value: value.clone(),
        };
        match kind {
            MessageKind::Error => self.errors.push(validation_message),
            MessageKind::Warning => self.warnings.push(validation_message),
            MessageKind::Info => self.info.push(validation_message),
        }
        self.checks.push(ValidationCheck {
            name: name.to_string(),
            status,
            severity,
            message,
            path: Some(self.source.path.clone()),
            tag: None,
            tag_name: None,
            value,
        });
    }

    fn finalize(&mut self) {
        let file_error_count: usize = self.files.iter().map(|file| file.errors.len()).sum();
        let file_warning_count: usize = self.files.iter().map(|file| file.warnings.len()).sum();
        let file_info_count: usize = self.files.iter().map(|file| file.info.len()).sum();
        self.summary.valid_file_count = self.files.iter().filter(|file| file.is_valid()).count();
        self.summary.invalid_file_count = self.files.len() - self.summary.valid_file_count;
        self.summary.error_count = self.errors.len() + file_error_count;
        self.summary.warning_count = self.warnings.len() + file_warning_count;
        self.summary.info_count = self.info.len() + file_info_count;
        self.status = if self.summary.error_count == 0 {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        };
        self.summary.valid = self.status == ValidationStatus::Pass;
    }
}

struct FileValidationOutcome {
    report: FileValidationReport,
    record: Option<MammogramRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PixelDataState {
    Present(String),
    Empty,
    Missing,
}

fn probe_pixel_data(path: &Path) -> std::result::Result<PixelDataState, String> {
    let file = File::open(path).map_err(|source| source.to_string())?;
    let file_length = file.metadata().map_err(|source| source.to_string())?.len();
    let mut reader = BufReader::new(file);
    let mut prefix = [0_u8; 132];
    let prefix_len = reader
        .read(&mut prefix)
        .map_err(|source| source.to_string())?;
    let dataset_start = if prefix_len >= prefix.len() && &prefix[128..] == b"DICM" {
        128
    } else {
        0
    };
    reader
        .seek(SeekFrom::Start(dataset_start))
        .map_err(|source| source.to_string())?;

    let meta = FileMetaTable::from_reader(&mut reader).map_err(|source| source.to_string())?;
    let transfer_syntax = TransferSyntaxRegistry
        .get(meta.transfer_syntax())
        .ok_or_else(|| format!("unsupported transfer syntax: {}", meta.transfer_syntax()))?;
    let mut dataset = LazyDataSetReader::new_with_ts(reader, transfer_syntax)
        .map_err(|source| source.to_string())?;

    while let Some(token) = dataset.advance() {
        let token = token.map_err(|source| source.to_string())?;
        match token {
            LazyDataToken::ElementHeader(header) if header.tag == PIXEL_DATA_TAG => {
                let length = header
                    .len
                    .get()
                    .ok_or_else(|| "native PixelData has undefined length".to_string())?;
                let value = dataset
                    .advance()
                    .ok_or_else(|| "native PixelData value is missing".to_string())?
                    .map_err(|source| source.to_string())?;
                match value {
                    LazyDataToken::LazyValue { header, decoder }
                        if header.tag == PIXEL_DATA_TAG =>
                    {
                        let value_end = decoder
                            .position()
                            .checked_add(u64::from(length))
                            .ok_or_else(|| "native PixelData length overflows file".to_string())?;
                        if value_end > file_length {
                            return Err(format!(
                                "native PixelData declares {length} bytes beyond the end of the file"
                            ));
                        }
                    }
                    _ => return Err("native PixelData value is missing".to_string()),
                }
                return if length == 0 {
                    Ok(PixelDataState::Empty)
                } else {
                    Ok(PixelDataState::Present(format!("{length} bytes")))
                };
            }
            LazyDataToken::PixelSequenceStart => return probe_pixel_sequence(&mut dataset),
            lazy @ (LazyDataToken::LazyValue { .. } | LazyDataToken::LazyItemValue { .. }) => {
                lazy.skip().map_err(|source| source.to_string())?;
            }
            _ => {}
        }
    }

    Ok(PixelDataState::Missing)
}

fn probe_pixel_sequence<S>(
    dataset: &mut LazyDataSetReader<S>,
) -> std::result::Result<PixelDataState, String>
where
    S: dicom::parser::stateful::decode::StatefulDecode,
{
    let mut item_count = 0_usize;
    while let Some(token) = dataset.advance() {
        let token = token.map_err(|source| source.to_string())?;
        match token {
            LazyDataToken::ItemStart { .. } => item_count += 1,
            LazyDataToken::SequenceEnd => {
                let fragment_count = item_count.saturating_sub(1);
                return if fragment_count == 0 {
                    Ok(PixelDataState::Empty)
                } else {
                    Ok(PixelDataState::Present(format!(
                        "{fragment_count} fragments"
                    )))
                };
            }
            lazy @ (LazyDataToken::LazyValue { .. } | LazyDataToken::LazyItemValue { .. }) => {
                lazy.skip().map_err(|source| source.to_string())?;
            }
            _ => {}
        }
    }

    Err("encapsulated PixelData ended before its sequence delimiter".to_string())
}

pub fn validate_path(
    path: &Path,
    options: &ValidationOptions,
) -> Result<ValidationReport, ValidationRuntimeError> {
    if path.is_file() && is_zip_file(path) {
        validate_zip_path(path, options)
    } else if path.is_file() {
        Ok(validate_file_source(path, options))
    } else if path.is_dir() {
        validate_directory_path(path, options)
    } else {
        Err(ValidationRuntimeError::InvalidSourcePath {
            path: path.to_path_buf(),
        })
    }
}

pub fn validate_dicom_file(path: &Path, options: &ValidationOptions) -> FileValidationReport {
    validate_file_with_record(path, options).report
}

/// Validate a filesystem directory or `.zip` archive as a DICOM collection.
pub fn validate_directory_path(
    path: &Path,
    options: &ValidationOptions,
) -> Result<ValidationReport, ValidationRuntimeError> {
    if path.is_file() && is_zip_file(path) {
        return validate_zip_path(path, options);
    }

    let dicom_files =
        collect_dicom_files(path).map_err(|source| ValidationRuntimeError::ReadDirectory {
            path: path.to_path_buf(),
            source,
        })?;
    let outcomes = dicom_files
        .into_iter()
        .map(|file_path| validate_file_with_record(&file_path, options))
        .collect();

    Ok(validate_file_collection(
        path,
        "directory",
        options,
        outcomes,
    ))
}

fn validate_zip_path(
    path: &Path,
    options: &ValidationOptions,
) -> Result<ValidationReport, ValidationRuntimeError> {
    let file = File::open(path).map_err(|source| ValidationRuntimeError::OpenZip {
        path: path.to_path_buf(),
        source,
    })?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|source| ValidationRuntimeError::ReadZip {
            path: path.to_path_buf(),
            source,
        })?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry =
            archive
                .by_index(index)
                .map_err(|source| ValidationRuntimeError::ReadZip {
                    path: path.to_path_buf(),
                    source,
                })?;
        if entry.is_dir() {
            continue;
        }

        let entry_name = entry.name().to_string();
        let has_dicom_extension = has_dicom_extension(Path::new(&entry_name));
        if has_dicom_extension {
            entries.push(ZipDicomEntry { index, entry_name });
            continue;
        }
        if Path::new(&entry_name).extension().is_some() {
            continue;
        }

        let mut header = [0_u8; 132];
        match entry.read_exact(&mut header) {
            Ok(()) if has_dicom_magic(&header) => entries.push(ZipDicomEntry { index, entry_name }),
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::UnexpectedEof => {}
            Err(source) => {
                return Err(ValidationRuntimeError::ReadZipEntry {
                    path: path.to_path_buf(),
                    entry: entry_name,
                    source,
                });
            }
        }
    }
    entries.sort_by(|left, right| left.entry_name.cmp(&right.entry_name));

    let mut outcomes = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut zip_entry =
            archive
                .by_index(entry.index)
                .map_err(|source| ValidationRuntimeError::ReadZip {
                    path: path.to_path_buf(),
                    source,
                })?;
        let mut bytes = Vec::new();
        zip_entry.read_to_end(&mut bytes).map_err(|source| {
            ValidationRuntimeError::ReadZipEntry {
                path: path.to_path_buf(),
                entry: entry.entry_name.clone(),
                source,
            }
        })?;
        let source_path = zip_entry_path(path, &entry.entry_name);
        outcomes.push(validate_dicom_bytes_with_record(
            source_path,
            &bytes,
            options,
        ));
    }

    Ok(validate_file_collection(path, "zip", options, outcomes))
}

struct ZipDicomEntry {
    index: usize,
    entry_name: String,
}

fn validate_file_collection(
    path: &Path,
    source_type: &str,
    options: &ValidationOptions,
    outcomes: Vec<FileValidationOutcome>,
) -> ValidationReport {
    let mut report = ValidationReport::new(path, source_type, options.profile, outcomes.len());

    if outcomes.is_empty() {
        report.record(
            MessageKind::Error,
            "no_dicom_files",
            "DICOM file discovery",
            format!("no DICOM files were found in the {source_type}"),
            None,
        );
        report.directory = Some(DirectoryValidationReport {
            dicom_file_count: 0,
            selected_views: BTreeMap::new(),
            missing_views: standard_view_names(),
        });
        report.finalize();
        return report;
    }

    let records_for_refinement: Vec<MammogramRecord> = outcomes
        .iter()
        .filter(|outcome| {
            outcome.report.is_valid() || has_only_unknown_mammogram_type_error(&outcome.report)
        })
        .filter_map(|outcome| outcome.record.clone())
        .collect();
    let refined_records = refine_dbt_object_classification(&records_for_refinement);
    let refined_records_by_path: HashMap<PathBuf, MammogramRecord> = refined_records
        .into_iter()
        .map(|record| (record.file_path.clone(), record))
        .collect();

    let mut valid_records = Vec::new();
    for mut outcome in outcomes {
        if let Some(record) = outcome
            .record
            .as_ref()
            .and_then(|record| refined_records_by_path.get(&record.file_path))
        {
            apply_refined_record_to_file_report(
                &mut outcome.report,
                record,
                &options.filter_config,
            );
            if outcome.report.is_valid() {
                valid_records.push(record.clone());
            }
        }
        report.files.push(outcome.report);
    }

    let selected_views = get_preferred_views_filtered(
        &valid_records,
        &options.filter_config,
        options.preference_order,
    );
    let mut selected_view_reports = BTreeMap::new();
    let mut missing_views = Vec::new();
    for view in &STANDARD_MAMMO_VIEWS {
        let view_name = view.to_string();
        let selected = selected_views.get(view).and_then(Option::as_ref);
        if let Some(record) = selected {
            selected_view_reports.insert(
                view_name.clone(),
                SelectedViewReport {
                    view: view_name,
                    selected: true,
                    file_path: Some(record.file_path.display().to_string()),
                    mammogram_type: Some(record.metadata.mammogram_type.to_string()),
                    dbt_object_kind: Some(record.metadata.dbt_object_kind.to_string()),
                },
            );
        } else {
            missing_views.push(view_name.clone());
            selected_view_reports.insert(
                view_name.clone(),
                SelectedViewReport {
                    view: view_name,
                    selected: false,
                    file_path: None,
                    mammogram_type: None,
                    dbt_object_kind: None,
                },
            );
        }
    }

    if missing_views.is_empty() {
        report.record(
            MessageKind::Info,
            "all_standard_views_selected",
            "Directory view coverage",
            "all four standard mammography views are covered after filtering".to_string(),
            None,
        );
    } else if options.profile == ValidationProfile::Selection {
        report.record(
            MessageKind::Error,
            "missing_standard_views",
            "Directory view coverage",
            "directory is missing one or more standard views after filtering".to_string(),
            Some(missing_views.join(",")),
        );
    } else {
        report.record(
            MessageKind::Warning,
            "missing_standard_views",
            "Directory view coverage",
            "directory is missing one or more standard views after filtering".to_string(),
            Some(missing_views.join(",")),
        );
    }

    report.directory = Some(DirectoryValidationReport {
        dicom_file_count: report.files.len(),
        selected_views: selected_view_reports,
        missing_views,
    });
    report.finalize();
    report
}

fn has_only_unknown_mammogram_type_error(report: &FileValidationReport) -> bool {
    !report.errors.is_empty()
        && report
            .errors
            .iter()
            .all(|message| message.code == "unknown_mammogram_type")
}

fn apply_refined_record_to_file_report(
    report: &mut FileValidationReport,
    record: &MammogramRecord,
    filter_config: &FilterConfig,
) {
    let metadata = &record.metadata;
    let resolved_unknown_type = report
        .mammography
        .mammogram_type
        .as_deref()
        .is_some_and(|mammogram_type| mammogram_type == MammogramType::Unknown.to_string())
        && !metadata.mammogram_type.is_unknown();

    report.mammography.mammogram_type = Some(metadata.mammogram_type.to_string());
    report.mammography.dbt_object_kind = Some(metadata.dbt_object_kind.to_string());
    report.mammography.concatenation_uid = metadata.concatenation_uid.clone();
    report.mammography.sop_instance_uid_of_concatenation_source =
        metadata.sop_instance_uid_of_concatenation_source.clone();

    if resolved_unknown_type {
        report
            .errors
            .retain(|message| message.code != "unknown_mammogram_type");
        report
            .warnings
            .retain(|message| message.code != "unknown_mammogram_type");
        report.checks.retain(|check| check.name != "MammogramType");
        report.pass(
            "MammogramType",
            "mammogram type resolved from collection context".to_string(),
            None,
            Some(metadata.mammogram_type.to_string()),
        );
    }

    reset_selection_eligibility(report);
    validate_selection_eligibility(report, Some(metadata), filter_config);
    report.finalize();
}

fn reset_selection_eligibility(report: &mut FileValidationReport) {
    report
        .warnings
        .retain(|message| message.code != "selection_filter_warning");
    report
        .checks
        .retain(|check| check.name != "Selection eligibility");
    report.selection = SelectionEligibilityReport {
        eligible: false,
        ..SelectionEligibilityReport::default()
    };
}

fn validate_file_source(path: &Path, options: &ValidationOptions) -> ValidationReport {
    let outcome = validate_file_with_record(path, options);
    let mut report = ValidationReport::new(path, "file", options.profile, 1);
    report.files.push(outcome.report);
    report.finalize();
    report
}

fn validate_file_with_record(path: &Path, options: &ValidationOptions) -> FileValidationOutcome {
    if !path.is_file() {
        let mut report = FileValidationReport::new(path, options.profile);
        report.record_plain(
            MessageKind::Error,
            "invalid_source_path",
            "DICOM file",
            "source path is not a file".to_string(),
        );
        report.finalize();
        return FileValidationOutcome {
            report,
            record: None,
        };
    }

    match probe_pixel_data(path) {
        Ok(pixel_data_state) => validate_open_result_with_record(
            path.to_path_buf(),
            OpenFileOptions::new()
                .read_until(PIXEL_DATA_TAG)
                .open_file(path),
            options,
            Some(pixel_data_state),
        ),
        Err(_) => {
            validate_open_result_with_record(path.to_path_buf(), open_file(path), options, None)
        }
    }
}

fn validate_dicom_bytes_with_record(
    source_path: PathBuf,
    bytes: &[u8],
    options: &ValidationOptions,
) -> FileValidationOutcome {
    let cursor = Cursor::new(bytes);
    validate_open_result_with_record(
        source_path,
        FileDicomObject::from_reader(cursor),
        options,
        None,
    )
}

fn validate_open_result_with_record<E>(
    source_path: PathBuf,
    dcm: Result<FileDicomObject<InMemDicomObject>, E>,
    options: &ValidationOptions,
    pixel_data_state: Option<PixelDataState>,
) -> FileValidationOutcome
where
    E: std::fmt::Display,
{
    let mut report = FileValidationReport::new(&source_path, options.profile);
    let dcm = match dcm {
        Ok(dcm) => dcm,
        Err(source) => {
            report.record_plain(
                MessageKind::Error,
                "dicom_read_failed",
                "DICOM readability",
                format!("failed to read DICOM file: {source}"),
            );
            report.finalize();
            return FileValidationOutcome {
                report,
                record: None,
            };
        }
    };

    let allow_non_mg_modality = !options.filter_config.exclude_non_mg_modality;
    collect_file_meta(&mut report, &dcm);
    validate_identity(&mut report, &dcm, options.profile, allow_non_mg_modality);
    validate_image_fields(&mut report, &dcm, options.profile);
    validate_pixel_fields(
        &mut report,
        &dcm,
        options.profile,
        pixel_data_state.as_ref(),
    );
    validate_canonical_completion(&mut report, &dcm);
    let metadata = validate_extraction(&mut report, &dcm, options.profile, allow_non_mg_modality);
    let record = metadata.as_ref().and_then(|metadata| {
        MammogramRecord::from_dicom_with_metadata(source_path, &dcm, metadata.clone()).ok()
    });
    validate_selection_eligibility(&mut report, metadata.as_ref(), &options.filter_config);

    report.finalize();
    FileValidationOutcome { report, record }
}

fn validate_canonical_completion(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
) {
    let plan = plan_completion(dcm, &CompletionOptions::default());
    if !plan.supported {
        return;
    }
    for addition in plan
        .additions
        .iter()
        .filter(|addition| addition.confidence == crate::extraction::Confidence::Exact)
    {
        report.record_plain(
            MessageKind::Info,
            "canonical_metadata_missing",
            &addition.keyword,
            format!(
                "{} is missing; mammofill can add the canonical value {}",
                addition.keyword, addition.value
            ),
        );
    }
    for issue in plan
        .issues
        .iter()
        .filter(|issue| issue.code == "populated_value_conflict")
    {
        report.record_plain(
            MessageKind::Warning,
            "canonical_value_conflict",
            "Canonical mammography metadata",
            issue.message.clone(),
        );
    }
}

fn is_zip_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn has_dicom_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("dcm") || extension.eq_ignore_ascii_case("dicom")
    })
}

fn has_dicom_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 132 && &bytes[128..132] == DICOM_MAGIC_BYTES
}

fn zip_entry_path(zip_path: &Path, entry_name: &str) -> PathBuf {
    PathBuf::from(format!("{}::{entry_name}", zip_path.display()))
}

fn collect_file_meta(report: &mut FileValidationReport, dcm: &FileDicomObject<InMemDicomObject>) {
    let transfer_syntax_uid = dcm
        .meta()
        .transfer_syntax
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string();
    let transfer_syntax_name = TransferSyntaxRegistry
        .get(&transfer_syntax_uid)
        .map(|syntax| syntax.name().to_string())
        .unwrap_or_else(|| UNKNOWN_TRANSFER_SYNTAX.to_string());
    report.file.transfer_syntax_uid = Some(transfer_syntax_uid.clone());
    report.file.transfer_syntax_name = Some(transfer_syntax_name.clone());
    report.pass(
        "TransferSyntaxUID",
        "TransferSyntaxUID is available".to_string(),
        None,
        Some(format!("{transfer_syntax_uid} ({transfer_syntax_name})")),
    );
    validate_lossy_compression(report, dcm, &transfer_syntax_uid, &transfer_syntax_name);
}

fn validate_lossy_compression(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    transfer_syntax_uid: &str,
    transfer_syntax_name: &str,
) {
    let lossy_indicator = get_string_value(dcm, LOSSY_IMAGE_COMPRESSION);
    let lossy_method = get_string_value(dcm, LOSSY_IMAGE_COMPRESSION_METHOD)
        .filter(|value| !value.trim().is_empty());
    let lossy_transfer_syntax = is_lossy_transfer_syntax(transfer_syntax_uid, transfer_syntax_name);

    if !lossy_indicator_is_enabled(lossy_indicator.as_deref())
        && lossy_method.is_none()
        && !lossy_transfer_syntax
    {
        return;
    }

    let mut details = Vec::new();
    if let Some(indicator) = lossy_indicator {
        details.push(format!("LossyImageCompression={indicator}"));
    }
    if let Some(method) = lossy_method {
        details.push(format!("LossyImageCompressionMethod={method}"));
    }
    if lossy_transfer_syntax {
        details.push(format!(
            "TransferSyntaxUID={transfer_syntax_uid} ({transfer_syntax_name})"
        ));
    }
    let value = details.join("; ");

    report.record_name(
        MessageKind::Warning,
        "lossy_compression",
        "Lossy compression",
        format!("lossy compression metadata is present: {value}"),
        "LossyImageCompression",
        Some(value),
    );
}

fn lossy_indicator_is_enabled(value: Option<&str>) -> bool {
    value
        .map(|value| {
            let value = value.trim();
            value == "01" || value == "1" || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn is_lossy_transfer_syntax(uid: &str, name: &str) -> bool {
    let normalized_name = name.to_ascii_lowercase();
    LOSSY_TRANSFER_SYNTAX_UIDS.contains(&uid)
        || (normalized_name.contains("lossy") && !normalized_name.contains("lossless"))
}

fn validate_identity(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
    allow_non_mg_modality: bool,
) {
    report.file.sop_class_uid = validate_optional_string(
        report,
        dcm,
        SOP_CLASS_UID,
        "SOPClassUID",
        "missing_sop_class_uid",
        profile == ValidationProfile::Selection,
    );
    report.file.sop_instance_uid = validate_optional_string(
        report,
        dcm,
        SOP_INSTANCE_UID,
        "SOPInstanceUID",
        "missing_sop_instance_uid",
        profile == ValidationProfile::Selection,
    );
    report.file.study_instance_uid = validate_optional_string(
        report,
        dcm,
        STUDY_INSTANCE_UID,
        "StudyInstanceUID",
        "missing_study_instance_uid",
        profile == ValidationProfile::Selection,
    );
    report.file.series_instance_uid = validate_optional_string(
        report,
        dcm,
        SERIES_INSTANCE_UID,
        "SeriesInstanceUID",
        "missing_series_instance_uid",
        profile == ValidationProfile::Selection,
    );

    report.mammography.modality = get_string_value(dcm, MODALITY);
    match report.mammography.modality.clone() {
        Some(modality) if modality.eq_ignore_ascii_case("MG") => report.pass(
            "Modality",
            "Modality is MG".to_string(),
            Some(MODALITY),
            Some(modality),
        ),
        Some(modality) if profile == ValidationProfile::Selection && !allow_non_mg_modality => {
            report.record_tag(
                MessageKind::Error,
                "non_mg_modality",
                "Modality",
                "Modality must be MG for mammography selection".to_string(),
                (MODALITY, "Modality"),
                Some(modality),
            )
        }
        Some(modality) if profile == ValidationProfile::Selection => report.record_tag(
            MessageKind::Warning,
            "non_mg_modality",
            "Modality",
            "Modality is not MG, but this filter configuration allows non-MG files".to_string(),
            (MODALITY, "Modality"),
            Some(modality),
        ),
        Some(modality) => report.record_tag(
            MessageKind::Warning,
            "non_mg_modality",
            "Modality",
            "Modality is not MG; extraction may still be useful for diagnostics".to_string(),
            (MODALITY, "Modality"),
            Some(modality),
        ),
        None if profile == ValidationProfile::Selection && !allow_non_mg_modality => report
            .record_tag(
                MessageKind::Error,
                "missing_modality",
                "Modality",
                "Modality is required for mammography selection".to_string(),
                (MODALITY, "Modality"),
                None,
            ),
        None if profile == ValidationProfile::Selection => report.record_tag(
            MessageKind::Warning,
            "missing_modality",
            "Modality",
            "Modality is absent, but this filter configuration allows non-MG files".to_string(),
            (MODALITY, "Modality"),
            None,
        ),
        None => report.record_tag(
            MessageKind::Warning,
            "missing_modality",
            "Modality",
            "Modality is absent; extraction will default mammogram type rules when possible"
                .to_string(),
            (MODALITY, "Modality"),
            None,
        ),
    }
}

fn validate_image_fields(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
) {
    let strict = profile == ValidationProfile::Selection;
    report.image.rows = validate_positive_u16(report, dcm, ROWS, "Rows", "missing_rows", strict);
    report.image.columns =
        validate_positive_u16(report, dcm, COLUMNS, "Columns", "missing_columns", strict);
    report.image.samples_per_pixel = validate_positive_u16(
        report,
        dcm,
        SAMPLES_PER_PIXEL,
        "SamplesPerPixel",
        "missing_samples_per_pixel",
        strict,
    );
    report.image.photometric_interpretation = validate_optional_string(
        report,
        dcm,
        PHOTOMETRIC_INTERPRETATION,
        "PhotometricInterpretation",
        "missing_photometric_interpretation",
        strict,
    );
    validate_pixel_layout_expectations(report);

    match read_optional_i32(dcm, NUMBER_OF_FRAMES) {
        Some(Ok(value)) if value > 0 => {
            report.image.number_of_frames = Some(value);
            report.image.number_of_frames_source = "explicit".to_string();
            report.pass(
                "NumberOfFrames",
                "NumberOfFrames is present and positive".to_string(),
                Some(NUMBER_OF_FRAMES),
                Some(value.to_string()),
            );
        }
        Some(Ok(value)) if strict => report.record_tag(
            MessageKind::Error,
            "invalid_number_of_frames",
            "NumberOfFrames",
            "NumberOfFrames must be positive when present".to_string(),
            (NUMBER_OF_FRAMES, "NumberOfFrames"),
            Some(value.to_string()),
        ),
        Some(Ok(value)) => report.record_tag(
            MessageKind::Warning,
            "invalid_number_of_frames",
            "NumberOfFrames",
            "NumberOfFrames is invalid and will not be useful for classification".to_string(),
            (NUMBER_OF_FRAMES, "NumberOfFrames"),
            Some(value.to_string()),
        ),
        Some(Err(value)) if strict => report.record_tag(
            MessageKind::Error,
            "invalid_number_of_frames",
            "NumberOfFrames",
            "NumberOfFrames cannot be parsed as an integer".to_string(),
            (NUMBER_OF_FRAMES, "NumberOfFrames"),
            value,
        ),
        Some(Err(value)) => report.record_tag(
            MessageKind::Warning,
            "invalid_number_of_frames",
            "NumberOfFrames",
            "NumberOfFrames cannot be parsed as an integer".to_string(),
            (NUMBER_OF_FRAMES, "NumberOfFrames"),
            value,
        ),
        None => {
            report.image.number_of_frames = Some(1);
            report.image.number_of_frames_source = "default".to_string();
            report.record_tag(
                MessageKind::Info,
                "default_number_of_frames",
                "NumberOfFrames",
                "NumberOfFrames is absent; mammocat treats this as a single-frame image"
                    .to_string(),
                (NUMBER_OF_FRAMES, "NumberOfFrames"),
                Some("1".to_string()),
            );
        }
    }
}

fn validate_pixel_fields(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
    pixel_data_state: Option<&PixelDataState>,
) {
    let strict = profile == ValidationProfile::Selection;
    report.pixel.bits_allocated = validate_positive_u16(
        report,
        dcm,
        BITS_ALLOCATED,
        "BitsAllocated",
        "missing_bits_allocated",
        strict,
    );
    report.pixel.bits_stored = validate_positive_u16(
        report,
        dcm,
        BITS_STORED,
        "BitsStored",
        "missing_bits_stored",
        strict,
    );
    report.pixel.high_bit =
        validate_u16(report, dcm, HIGH_BIT, "HighBit", "missing_high_bit", strict);
    report.pixel.pixel_representation = validate_u16(
        report,
        dcm,
        PIXEL_REPRESENTATION,
        "PixelRepresentation",
        "missing_pixel_representation",
        strict,
    );
    validate_bit_relationships(report, strict);
    validate_pixel_data(report, dcm, strict, pixel_data_state);
}

fn validate_extraction(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
    allow_non_mg_modality: bool,
) -> Option<MammogramMetadata> {
    match MammogramExtractor::extract_file_with_options_and_modality_policy(
        dcm,
        false,
        allow_non_mg_modality,
    ) {
        Ok(metadata) => {
            report.pass(
                "MammogramExtractor",
                "mammocat metadata extraction succeeded".to_string(),
                None,
                None,
            );
            collect_mammography_metadata(report, &metadata, dcm, profile);
            Some(metadata)
        }
        Err(source) => {
            report.record_plain(
                MessageKind::Error,
                "mammocat_extraction_failed",
                "MammogramExtractor",
                format!("mammocat metadata extraction failed: {source}"),
            );
            None
        }
    }
}

fn collect_mammography_metadata(
    report: &mut FileValidationReport,
    metadata: &MammogramMetadata,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
) {
    report.mammography.mammogram_type = Some(metadata.mammogram_type.to_string());
    report.mammography.dbt_object_kind = Some(metadata.dbt_object_kind.to_string());
    report.mammography.laterality = Some(metadata.laterality.to_string());
    report.mammography.view_position = Some(metadata.view_position.to_string());
    report.mammography.view_modifiers = metadata
        .view_modifiers
        .iter()
        .map(ToString::to_string)
        .collect();
    report.mammography.image_type = Some(metadata.image_type.to_string());
    report.mammography.is_for_processing = Some(metadata.is_for_processing);
    report.mammography.has_implant = Some(metadata.has_implant);
    report.mammography.is_spot_compression = Some(metadata.is_spot_compression());
    report.mammography.is_magnified = Some(metadata.is_magnified());
    report.mammography.is_implant_displaced = Some(metadata.is_implant_displaced());
    report.mammography.is_secondary_capture = Some(metadata.is_secondary_capture);
    report.mammography.manufacturer = metadata.manufacturer.clone();
    report.mammography.model = metadata.model.clone();
    report.mammography.concatenation_uid = metadata.concatenation_uid.clone();
    report.mammography.sop_instance_uid_of_concatenation_source =
        metadata.sop_instance_uid_of_concatenation_source.clone();
    report.mammography.pixel_spacing = get_string_value(dcm, PIXEL_SPACING);

    validate_image_type(report, dcm, profile);
    validate_laterality_value(report, metadata.laterality, profile);
    validate_view_value(report, metadata.view_position, profile);
    let descriptor = extract_view_descriptor(dcm);
    for conflict in descriptor.conflicts {
        report.record_plain(
            if profile == ValidationProfile::Selection {
                MessageKind::Error
            } else {
                MessageKind::Warning
            },
            "view_evidence_conflict",
            "Mammography view evidence",
            conflict,
        );
    }
    validate_mammogram_type_value(report, metadata.mammogram_type, profile);
    optional_metadata_warning(
        report,
        metadata.manufacturer.as_deref(),
        "Manufacturer",
        "missing_manufacturer",
    );
    optional_metadata_warning(
        report,
        metadata.model.as_deref(),
        "ManufacturerModelName",
        "missing_model",
    );
    optional_metadata_warning(
        report,
        report.mammography.pixel_spacing.clone().as_deref(),
        "PixelSpacing",
        "missing_pixel_spacing",
    );
}

fn validate_selection_eligibility(
    report: &mut FileValidationReport,
    metadata: Option<&MammogramMetadata>,
    filter_config: &FilterConfig,
) {
    let Some(metadata) = metadata else {
        report.selection.eligible = false;
        report
            .selection
            .filtered_by
            .push("extraction_failed".to_string());
        return;
    };

    let view = metadata.mammogram_view();
    report.selection.mammogram_view = Some(view.to_string());
    report.selection.standard_view = Some(metadata.is_standard_view());
    let mut filtered_by = Vec::new();

    if let Some(allowed_types) = &filter_config.allowed_types {
        if !allowed_types.contains(&metadata.mammogram_type) {
            filtered_by.push("allowed_types".to_string());
        }
    }
    if let Some(allowed_dbt_object_kinds) = &filter_config.allowed_dbt_object_kinds {
        if !allowed_dbt_object_kinds.contains(&metadata.dbt_object_kind) {
            filtered_by.push("allowed_dbt_object_kinds".to_string());
        }
    }
    if filter_config.exclude_implants && metadata.has_implant {
        filtered_by.push("exclude_implants".to_string());
    }
    if filter_config.exclude_non_standard_views && !metadata.is_standard_view() {
        filtered_by.push("only_standard_views".to_string());
    }
    if filter_config.exclude_for_processing && metadata.is_for_processing {
        filtered_by.push("exclude_for_processing".to_string());
    }
    if filter_config.exclude_secondary_capture && metadata.is_secondary_capture {
        filtered_by.push("exclude_secondary_capture".to_string());
    }
    if filter_config.exclude_non_mg_modality {
        match &metadata.modality {
            Some(modality) if modality.eq_ignore_ascii_case("MG") => {}
            Some(_) => filtered_by.push("exclude_non_mg".to_string()),
            None => filtered_by.push("missing_modality".to_string()),
        }
    }
    for modifier in metadata
        .view_modifiers
        .iter()
        .filter(|modifier| modifier.affects_selection())
    {
        report.record_plain(
            MessageKind::Warning,
            "selection_ranking_warning",
            "Selection ranking",
            format!(
                "mammoselect will rank an otherwise equivalent unmodified view ahead of this {modifier} view"
            ),
        );
    }

    for reason in &filtered_by {
        report.record_plain(
            MessageKind::Warning,
            "selection_filter_warning",
            "Selection eligibility",
            format!("file may be skipped or deprioritized by mammoselect: {reason}"),
        );
    }

    report.selection.eligible = report.errors.is_empty() && filtered_by.is_empty();
    report.selection.filtered_by = filtered_by;
    if report.selection.eligible {
        report.pass(
            "Selection eligibility",
            "file is eligible under the selected filter configuration".to_string(),
            None,
            Some(view.to_string()),
        );
    }
}

fn validate_image_type(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    profile: ValidationProfile,
) {
    match get_string_value(dcm, IMAGE_TYPE) {
        Some(value) if !value.trim().is_empty() => report.pass(
            "ImageType",
            "ImageType is present".to_string(),
            Some(IMAGE_TYPE),
            Some(value),
        ),
        _ if profile == ValidationProfile::Selection => report.record_tag(
            MessageKind::Error,
            "missing_image_type",
            "ImageType",
            "ImageType is required for reliable selection classification".to_string(),
            (IMAGE_TYPE, "ImageType"),
            None,
        ),
        _ => report.record_tag(
            MessageKind::Warning,
            "missing_image_type",
            "ImageType",
            "ImageType is absent; mammocat will use default classification rules".to_string(),
            (IMAGE_TYPE, "ImageType"),
            None,
        ),
    }
}

fn validate_laterality_value(
    report: &mut FileValidationReport,
    laterality: Laterality,
    profile: ValidationProfile,
) {
    if !laterality.is_unknown_or_none() {
        report.pass(
            "Laterality",
            "laterality is known".to_string(),
            None,
            Some(laterality.to_string()),
        );
    } else if profile == ValidationProfile::Selection {
        report.record_tag(
            MessageKind::Error,
            "unknown_laterality",
            "Laterality",
            "laterality must be known for preferred-view selection".to_string(),
            (IMAGE_LATERALITY, "ImageLaterality"),
            Some(laterality.to_string()),
        );
    } else {
        report.record_tag(
            MessageKind::Warning,
            "unknown_laterality",
            "Laterality",
            "laterality could not be resolved from DICOM fallback fields".to_string(),
            (IMAGE_LATERALITY, "ImageLaterality"),
            Some(laterality.to_string()),
        );
    }
}

fn validate_view_value(
    report: &mut FileValidationReport,
    view_position: ViewPosition,
    profile: ValidationProfile,
) {
    if !view_position.is_unknown() {
        report.pass(
            "ViewPosition",
            "view position is known".to_string(),
            None,
            Some(view_position.to_string()),
        );
    } else if profile == ValidationProfile::Selection {
        report.record_tag(
            MessageKind::Error,
            "unknown_view_position",
            "ViewPosition",
            "view position must be known for preferred-view selection".to_string(),
            (VIEW_POSITION, "ViewPosition"),
            None,
        );
    } else {
        report.record_tag(
            MessageKind::Warning,
            "unknown_view_position",
            "ViewPosition",
            "view position could not be resolved from DICOM fallback fields".to_string(),
            (VIEW_POSITION, "ViewPosition"),
            None,
        );
    }
}

fn validate_pixel_layout_expectations(report: &mut FileValidationReport) {
    if let Some(samples_per_pixel) = report.image.samples_per_pixel {
        if samples_per_pixel != 1 {
            report.record_tag(
                MessageKind::Warning,
                "unexpected_samples_per_pixel",
                "SamplesPerPixel",
                "SamplesPerPixel is not the usual single-channel mammography layout".to_string(),
                (SAMPLES_PER_PIXEL, "SamplesPerPixel"),
                Some(samples_per_pixel.to_string()),
            );
        }
    }

    if let Some(photometric_interpretation) = report.image.photometric_interpretation.clone() {
        let normalized = photometric_interpretation.trim().to_ascii_uppercase();
        if normalized != MONOCHROME1 && normalized != MONOCHROME2 {
            report.record_tag(
                MessageKind::Warning,
                "unexpected_photometric_interpretation",
                "PhotometricInterpretation",
                "PhotometricInterpretation is not a usual monochrome mammography layout"
                    .to_string(),
                (PHOTOMETRIC_INTERPRETATION, "PhotometricInterpretation"),
                Some(photometric_interpretation),
            );
        }
    }
}

fn validate_mammogram_type_value(
    report: &mut FileValidationReport,
    mammogram_type: MammogramType,
    profile: ValidationProfile,
) {
    if !mammogram_type.is_unknown() {
        report.pass(
            "MammogramType",
            "mammogram type is known".to_string(),
            None,
            Some(mammogram_type.to_string()),
        );
    } else if profile == ValidationProfile::Selection {
        report.record_plain(
            MessageKind::Error,
            "unknown_mammogram_type",
            "MammogramType",
            "mammogram type must be known for preferred-view selection".to_string(),
        );
    } else {
        report.record_plain(
            MessageKind::Warning,
            "unknown_mammogram_type",
            "MammogramType",
            "mammogram type is unknown".to_string(),
        );
    }
}

fn validate_optional_string(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    tag: Tag,
    name: &'static str,
    missing_code: &'static str,
    critical_if_missing: bool,
) -> Option<String> {
    match get_string_value(dcm, tag).filter(|value| !value.trim().is_empty()) {
        Some(value) => {
            report.pass(
                name,
                format!("{name} is present"),
                Some(tag),
                Some(value.clone()),
            );
            Some(value)
        }
        None if critical_if_missing => {
            report.record_tag(
                MessageKind::Error,
                missing_code,
                name,
                format!("{name} is required for selection validation"),
                (tag, name),
                None,
            );
            None
        }
        None => {
            report.record_tag(
                MessageKind::Warning,
                missing_code,
                name,
                format!("{name} is absent"),
                (tag, name),
                None,
            );
            None
        }
    }
}

fn validate_u16(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    tag: Tag,
    name: &'static str,
    missing_code: &'static str,
    critical_if_missing: bool,
) -> Option<u16> {
    match read_optional_u16(dcm, tag) {
        Some(Ok(value)) => {
            report.pass(
                name,
                format!("{name} is present"),
                Some(tag),
                Some(value.to_string()),
            );
            Some(value)
        }
        Some(Err(value)) if critical_if_missing => {
            report.record_tag(
                MessageKind::Error,
                "invalid_u16_value",
                name,
                format!("{name} cannot be read as u16"),
                (tag, name),
                value,
            );
            None
        }
        Some(Err(value)) => {
            report.record_tag(
                MessageKind::Warning,
                "invalid_u16_value",
                name,
                format!("{name} cannot be read as u16"),
                (tag, name),
                value,
            );
            None
        }
        None if critical_if_missing => {
            report.record_tag(
                MessageKind::Error,
                missing_code,
                name,
                format!("{name} is required for selection validation"),
                (tag, name),
                None,
            );
            None
        }
        None => {
            report.record_tag(
                MessageKind::Warning,
                missing_code,
                name,
                format!("{name} is absent"),
                (tag, name),
                None,
            );
            None
        }
    }
}

fn validate_positive_u16(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    tag: Tag,
    name: &'static str,
    missing_code: &'static str,
    critical_if_missing: bool,
) -> Option<u16> {
    let value = validate_u16(report, dcm, tag, name, missing_code, critical_if_missing);
    if let Some(0) = value {
        if critical_if_missing {
            report.record_tag(
                MessageKind::Error,
                "invalid_positive_u16",
                name,
                format!("{name} must be greater than zero"),
                (tag, name),
                Some("0".to_string()),
            );
        } else {
            report.record_tag(
                MessageKind::Warning,
                "invalid_positive_u16",
                name,
                format!("{name} should be greater than zero"),
                (tag, name),
                Some("0".to_string()),
            );
        }
    }
    value.filter(|value| *value > 0)
}

fn validate_bit_relationships(report: &mut FileValidationReport, strict: bool) {
    if let Some(bits_allocated) = report.pixel.bits_allocated {
        if matches!(bits_allocated, 8 | 16 | 32) {
            report.pass(
                "BitsAllocated support",
                "BitsAllocated is supported for metadata validation".to_string(),
                Some(BITS_ALLOCATED),
                Some(bits_allocated.to_string()),
            );
        } else if strict {
            report.record_tag(
                MessageKind::Error,
                "unsupported_bits_allocated",
                "BitsAllocated support",
                "BitsAllocated should be 8, 16, or 32".to_string(),
                (BITS_ALLOCATED, "BitsAllocated"),
                Some(bits_allocated.to_string()),
            );
        } else {
            report.record_tag(
                MessageKind::Warning,
                "unsupported_bits_allocated",
                "BitsAllocated support",
                "BitsAllocated is unusual for mammography".to_string(),
                (BITS_ALLOCATED, "BitsAllocated"),
                Some(bits_allocated.to_string()),
            );
        }
    }

    if let (Some(bits_stored), Some(bits_allocated)) =
        (report.pixel.bits_stored, report.pixel.bits_allocated)
    {
        if (1..=bits_allocated).contains(&bits_stored) {
            report.pass(
                "BitsStored relationship",
                "BitsStored is within BitsAllocated".to_string(),
                Some(BITS_STORED),
                Some(format!("{bits_stored}/{bits_allocated}")),
            );
        } else {
            let kind = if strict {
                MessageKind::Error
            } else {
                MessageKind::Warning
            };
            report.record_tag(
                kind,
                "invalid_bits_stored",
                "BitsStored relationship",
                "BitsStored must be in the range 1..=BitsAllocated".to_string(),
                (BITS_STORED, "BitsStored"),
                Some(bits_stored.to_string()),
            );
        }
    }

    if let (Some(high_bit), Some(bits_stored), Some(bits_allocated)) = (
        report.pixel.high_bit,
        report.pixel.bits_stored,
        report.pixel.bits_allocated,
    ) {
        let expected = bits_stored.saturating_sub(1);
        if bits_stored > 0 && high_bit == expected && high_bit < bits_allocated {
            report.pass(
                "HighBit relationship",
                "HighBit equals BitsStored - 1 and is less than BitsAllocated".to_string(),
                Some(HIGH_BIT),
                Some(high_bit.to_string()),
            );
        } else {
            let kind = if strict {
                MessageKind::Error
            } else {
                MessageKind::Warning
            };
            report.record_tag(
                kind,
                "invalid_high_bit",
                "HighBit relationship",
                format!(
                    "HighBit must equal BitsStored - 1 ({expected}) and be less than BitsAllocated"
                ),
                (HIGH_BIT, "HighBit"),
                Some(high_bit.to_string()),
            );
        }
    }
}

fn validate_pixel_data(
    report: &mut FileValidationReport,
    dcm: &FileDicomObject<InMemDicomObject>,
    strict: bool,
    pixel_data_state: Option<&PixelDataState>,
) {
    if let Some(pixel_data_state) = pixel_data_state {
        validate_pixel_data_state(report, strict, pixel_data_state);
        return;
    }

    match dcm.element(PIXEL_DATA_TAG) {
        Ok(element) if !element.is_empty() => {
            let description = pixel_data_description(element);
            validate_pixel_data_state(report, strict, &PixelDataState::Present(description));
        }
        Ok(_) => validate_pixel_data_state(report, strict, &PixelDataState::Empty),
        Err(_) => validate_pixel_data_state(report, strict, &PixelDataState::Missing),
    }
}

fn validate_pixel_data_state(
    report: &mut FileValidationReport,
    strict: bool,
    pixel_data_state: &PixelDataState,
) {
    match pixel_data_state {
        PixelDataState::Present(description) => {
            report.pixel.pixel_data_present = true;
            report.pixel.pixel_data_description = Some(description.clone());
            report.pass(
                "PixelData",
                "PixelData is present".to_string(),
                Some(PIXEL_DATA_TAG),
                Some(description.clone()),
            );
        }
        PixelDataState::Empty => report.record_tag(
            if strict {
                MessageKind::Error
            } else {
                MessageKind::Warning
            },
            "empty_pixel_data",
            "PixelData",
            "PixelData is present but empty".to_string(),
            (PIXEL_DATA_TAG, "PixelData"),
            None,
        ),
        PixelDataState::Missing => report.record_tag(
            if strict {
                MessageKind::Error
            } else {
                MessageKind::Warning
            },
            "missing_pixel_data",
            "PixelData",
            if strict {
                "PixelData is required for selection readiness".to_string()
            } else {
                "PixelData is absent; metadata extraction can still succeed".to_string()
            },
            (PIXEL_DATA_TAG, "PixelData"),
            None,
        ),
    }
}

fn optional_metadata_warning(
    report: &mut FileValidationReport,
    value: Option<&str>,
    name: &'static str,
    code: &'static str,
) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        report.record_name(
            MessageKind::Info,
            "optional_metadata_present",
            name,
            format!("{name} is present"),
            name,
            Some(value.to_string()),
        );
    } else {
        report.record_name(
            MessageKind::Warning,
            code,
            name,
            format!("{name} is absent"),
            name,
            None,
        );
    }
}

fn read_optional_u16(
    dcm: &FileDicomObject<InMemDicomObject>,
    tag: Tag,
) -> Option<Result<u16, Option<String>>> {
    let element = dcm.element(tag).ok()?;
    if element.is_empty() {
        return None;
    }
    Some(element.to_int::<u16>().map_err(|_| element_value(element)))
}

fn read_optional_i32(
    dcm: &FileDicomObject<InMemDicomObject>,
    tag: Tag,
) -> Option<Result<i32, Option<String>>> {
    let element = dcm.element(tag).ok()?;
    if element.is_empty() {
        return None;
    }
    Some(element.to_int::<i32>().map_err(|_| element_value(element)))
}

fn element_value(element: &DataElement<InMemDicomObject>) -> Option<String> {
    element
        .to_str()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn pixel_data_description(element: &DataElement<InMemDicomObject>) -> String {
    match element.value() {
        DicomValue::Primitive(value) => format!("{} bytes", value.to_bytes().len()),
        DicomValue::PixelSequence(sequence) => format!("{} fragments", sequence.fragments().len()),
        DicomValue::Sequence(_) => "sequence value".to_string(),
    }
}

fn format_tag(tag: Tag) -> String {
    format!("({:04X},{:04X})", tag.0, tag.1)
}

fn standard_view_names() -> Vec<String> {
    STANDARD_MAMMO_VIEWS
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::fs::File;
    use std::io::Write;

    use crate::extraction::tags::{
        ACQUISITION_DEVICE_PROCESSING_DESCRIPTION, CONCATENATION_UID, LATERALITY,
        NUMBER_OF_TOMOSYNTHESIS_SOURCE_IMAGES, PRESENTATION_INTENT_TYPE,
        SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE, TOMO_CLASS, VIEW_CODE_SEQUENCE,
        VIEW_MODIFIER_CODE_SEQUENCE, VOLUMETRIC_PROPERTIES, VOLUME_BASED_CALCULATION_TECHNIQUE,
    };
    use dicom_core::value::DataSetSequence;
    #[cfg(feature = "json")]
    use dicom_core::value::PixelFragmentSequence;
    #[cfg(feature = "json")]
    use dicom_core::DicomValue;
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_object::InMemDicomObject;

    fn put_str(dcm: &mut InMemDicomObject, tag: Tag, value: &str) {
        dcm.put(DataElement::new(tag, VR::CS, PrimitiveValue::from(value)));
    }

    fn put_str_with_vr(dcm: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
        dcm.put(DataElement::new(tag, vr, PrimitiveValue::from(value)));
    }

    fn put_u16(dcm: &mut InMemDicomObject, tag: Tag, value: u16) {
        dcm.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
    }

    fn valid_metadata_object() -> FileDicomObject<InMemDicomObject> {
        valid_metadata_object_with("L", "MLO")
    }

    fn valid_metadata_object_with(
        laterality: &str,
        view_position: &str,
    ) -> FileDicomObject<InMemDicomObject> {
        let mut dcm = InMemDicomObject::new_empty();
        put_str(&mut dcm, MODALITY, "MG");
        put_str(&mut dcm, SOP_CLASS_UID, "1.2.840.10008.5.1.4.1.1.1.2");
        let laterality_uid = if laterality == "L" { 1 } else { 2 };
        let view_uid = if view_position == "MLO" { 1 } else { 2 };
        put_str(
            &mut dcm,
            SOP_INSTANCE_UID,
            &format!("1.2.3.4.5.6.7.8.{laterality_uid}.{view_uid}"),
        );
        put_str(&mut dcm, STUDY_INSTANCE_UID, "1.2.3.4.5");
        put_str(&mut dcm, SERIES_INSTANCE_UID, "1.2.3.4.5.6");
        dcm.put(DataElement::new(
            IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(vec!["ORIGINAL".to_string(), "PRIMARY".to_string()].into()),
        ));
        put_str(&mut dcm, IMAGE_LATERALITY, laterality);
        put_str(&mut dcm, VIEW_POSITION, view_position);
        put_str(&mut dcm, PRESENTATION_INTENT_TYPE, "FOR PRESENTATION");
        put_u16(&mut dcm, ROWS, 8);
        put_u16(&mut dcm, COLUMNS, 8);
        put_u16(&mut dcm, SAMPLES_PER_PIXEL, 1);
        put_str(&mut dcm, PHOTOMETRIC_INTERPRETATION, "MONOCHROME2");
        put_u16(&mut dcm, BITS_ALLOCATED, 16);
        put_u16(&mut dcm, BITS_STORED, 16);
        put_u16(&mut dcm, HIGH_BIT, 15);
        put_u16(&mut dcm, PIXEL_REPRESENTATION, 0);
        dcm.put(DataElement::new(
            PIXEL_DATA_TAG,
            VR::OW,
            PrimitiveValue::U16(vec![0_u16; 64].into()),
        ));

        dcm.with_meta(
            dicom_object::FileMetaTableBuilder::new()
                .transfer_syntax("1.2.840.10008.1.2.1")
                .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.1.2")
                .media_storage_sop_instance_uid("1.2.3.4.5.6.7.8.9"),
        )
        .unwrap()
    }

    fn ambiguous_dbt_object_with(
        study_uid: &str,
        series_uid: &str,
        sop_uid: &str,
        source_sop_uid: Option<&str>,
        laterality: &str,
        view_position: &str,
    ) -> FileDicomObject<InMemDicomObject> {
        let mut dcm = valid_metadata_object_with(laterality, view_position);
        put_str_with_vr(&mut dcm, STUDY_INSTANCE_UID, VR::UI, study_uid);
        put_str_with_vr(&mut dcm, SERIES_INSTANCE_UID, VR::UI, series_uid);
        put_str_with_vr(&mut dcm, SOP_INSTANCE_UID, VR::UI, sop_uid);
        dcm.put(DataElement::new(
            IMAGE_TYPE,
            VR::CS,
            PrimitiveValue::Strs(vec!["DERIVED".to_string(), "PRIMARY".to_string()].into()),
        ));
        put_str(&mut dcm, VOLUMETRIC_PROPERTIES, "VOLUME");
        put_str(&mut dcm, VOLUME_BASED_CALCULATION_TECHNIQUE, "NONE");
        put_str_with_vr(
            &mut dcm,
            CONCATENATION_UID,
            VR::UI,
            &format!("{series_uid}.1"),
        );
        if let Some(source_sop_uid) = source_sop_uid {
            put_str_with_vr(
                &mut dcm,
                SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
                VR::UI,
                source_sop_uid,
            );
        }
        put_str(&mut dcm, TOMO_CLASS, "TOMOSYNTHESIS");
        put_str_with_vr(
            &mut dcm,
            NUMBER_OF_TOMOSYNTHESIS_SOURCE_IMAGES,
            VR::IS,
            "15",
        );
        put_str_with_vr(
            &mut dcm,
            ACQUISITION_DEVICE_PROCESSING_DESCRIPTION,
            VR::LO,
            "TOMO R MAMMOGRAPHY,CC",
        );
        dcm
    }

    fn validate_object(
        dcm: &mut FileDicomObject<InMemDicomObject>,
        profile: ValidationProfile,
    ) -> FileValidationReport {
        let mut report = FileValidationReport::new(Path::new("test.dcm"), profile);
        collect_file_meta(&mut report, dcm);
        validate_identity(&mut report, dcm, profile, false);
        validate_image_fields(&mut report, dcm, profile);
        validate_pixel_fields(&mut report, dcm, profile, None);
        validate_canonical_completion(&mut report, dcm);
        let metadata = validate_extraction(&mut report, dcm, profile, false);
        validate_selection_eligibility(&mut report, metadata.as_ref(), &FilterConfig::default());
        report.finalize();
        report
    }

    fn error_codes(report: &FileValidationReport) -> HashSet<&str> {
        report
            .errors
            .iter()
            .map(|message| message.code.as_str())
            .collect()
    }

    fn warning_codes(report: &FileValidationReport) -> HashSet<&str> {
        report
            .warnings
            .iter()
            .map(|message| message.code.as_str())
            .collect()
    }

    #[test]
    fn selection_profile_accepts_complete_object() {
        let mut dcm = valid_metadata_object();
        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(report.is_valid(), "{:?}", report.errors);
        assert_eq!(report.status, ValidationStatus::Pass);
        assert!(report.pixel.pixel_data_present);
    }

    #[cfg(feature = "json")]
    #[test]
    fn metadata_only_file_validation_matches_eager_report() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("synthetic.dcm");
        valid_metadata_object().write_to_file(&path).unwrap();
        let options = ValidationOptions::default();

        let eager =
            validate_open_result_with_record(path.clone(), open_file(&path), &options, None).report;
        let metadata_only = validate_file_with_record(&path, &options).report;

        assert_eq!(
            serde_json::to_value(metadata_only).unwrap(),
            serde_json::to_value(eager).unwrap()
        );
    }

    #[test]
    fn metadata_only_validation_rejects_truncated_native_pixel_data() {
        const PIXEL_VALUE_LENGTH: u64 = 64 * 2;

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("truncated-pixel-data.dcm");
        valid_metadata_object().write_to_file(&path).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let file_length = file.metadata().unwrap().len();
        file.set_len(file_length - PIXEL_VALUE_LENGTH).unwrap();

        let report = validate_file_with_record(&path, &ValidationOptions::default()).report;

        assert!(error_codes(&report).contains("dicom_read_failed"));
        assert!(!report.pixel.pixel_data_present);
    }

    #[cfg(feature = "json")]
    #[test]
    fn metadata_only_validation_preserves_encapsulated_fragment_count() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("synthetic-encapsulated.dcm");
        let mut inner = valid_metadata_object().into_inner();
        inner.put(DataElement::new(
            PIXEL_DATA_TAG,
            VR::OB,
            DicomValue::PixelSequence(PixelFragmentSequence::new_fragments(vec![
                vec![1_u8, 2],
                vec![3_u8, 4],
            ])),
        ));
        inner
            .with_meta(
                dicom_object::FileMetaTableBuilder::new()
                    .transfer_syntax("1.2.840.10008.1.2.4.50")
                    .media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.1.2")
                    .media_storage_sop_instance_uid("1.2.3.4.5.6.7.8.9"),
            )
            .unwrap()
            .write_to_file(&path)
            .unwrap();
        let options = ValidationOptions::default();

        let eager =
            validate_open_result_with_record(path.clone(), open_file(&path), &options, None).report;
        let metadata_only = validate_file_with_record(&path, &options).report;

        assert_eq!(
            metadata_only.pixel.pixel_data_description.as_deref(),
            Some("2 fragments")
        );
        assert_eq!(
            serde_json::to_value(metadata_only).unwrap(),
            serde_json::to_value(eager).unwrap()
        );
    }

    #[test]
    fn view_modifiers_affect_ranking_without_filtering() {
        let mut dcm = valid_metadata_object();
        let modifier = InMemDicomObject::from_element_iter([DataElement::new(
            crate::extraction::tags::CODE_MEANING,
            VR::LO,
            PrimitiveValue::from("Tangential"),
        )]);
        dcm.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![modifier]),
        ));

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(report.selection.eligible);
        assert!(report.selection.filtered_by.is_empty());
        assert!(warning_codes(&report).contains("selection_ranking_warning"));
    }

    #[test]
    fn fixed_iod_conflicts_are_checked_through_completion_registry() {
        let mut dcm = valid_metadata_object();
        put_str(&mut dcm, dicom_dictionary_std::tags::ORGAN_EXPOSED, "CHEST");

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(warning_codes(&report).contains("canonical_value_conflict"));
    }

    #[test]
    fn selection_profile_fails_missing_pixel_data() {
        let mut dcm = valid_metadata_object();
        dcm.remove_element(PIXEL_DATA_TAG);

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(!report.is_valid());
        assert!(error_codes(&report).contains("missing_pixel_data"));
    }

    #[test]
    fn extraction_profile_warns_missing_pixel_data() {
        let mut dcm = valid_metadata_object();
        dcm.remove_element(PIXEL_DATA_TAG);

        let report = validate_object(&mut dcm, ValidationProfile::Extraction);

        assert!(report.is_valid(), "{:?}", report.errors);
        assert!(report
            .warnings
            .iter()
            .any(|message| message.code == "missing_pixel_data"));
    }

    #[test]
    fn selection_profile_fails_invalid_high_bit() {
        let mut dcm = valid_metadata_object();
        dcm.put(DataElement::new(
            HIGH_BIT,
            VR::US,
            PrimitiveValue::from(14_u16),
        ));

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(!report.is_valid());
        assert!(error_codes(&report).contains("invalid_high_bit"));
    }

    #[test]
    fn selection_profile_warns_unexpected_pixel_layout_without_failing() {
        let mut dcm = valid_metadata_object();
        put_u16(&mut dcm, SAMPLES_PER_PIXEL, 3);
        put_str(&mut dcm, PHOTOMETRIC_INTERPRETATION, "RGB");

        let report = validate_object(&mut dcm, ValidationProfile::Selection);
        let warning_codes = warning_codes(&report);

        assert!(report.is_valid(), "{:?}", report.errors);
        assert!(warning_codes.contains("unexpected_samples_per_pixel"));
        assert!(warning_codes.contains("unexpected_photometric_interpretation"));
    }

    #[test]
    fn selection_profile_warns_lossy_compression_without_failing() {
        let mut dcm = valid_metadata_object();
        put_str(&mut dcm, LOSSY_IMAGE_COMPRESSION, "01");
        put_str(&mut dcm, LOSSY_IMAGE_COMPRESSION_METHOD, "ISO_10918_1");

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(report.is_valid(), "{:?}", report.errors);
        assert!(warning_codes(&report).contains("lossy_compression"));
    }

    #[test]
    fn selection_profile_fails_unknown_laterality() {
        let mut dcm = valid_metadata_object();
        dcm.remove_element(IMAGE_LATERALITY);
        dcm.remove_element(LATERALITY);

        let report = validate_object(&mut dcm, ValidationProfile::Selection);

        assert!(!report.is_valid());
        assert!(error_codes(&report).contains("unknown_laterality"));
    }

    #[test]
    fn extraction_profile_warns_unknown_view() {
        let mut dcm = valid_metadata_object();
        dcm.remove_element(VIEW_POSITION);
        dcm.remove_element(VIEW_CODE_SEQUENCE);
        dcm.remove_element(VIEW_MODIFIER_CODE_SEQUENCE);

        let report = validate_object(&mut dcm, ValidationProfile::Extraction);

        assert!(report.is_valid(), "{:?}", report.errors);
        assert!(report
            .warnings
            .iter()
            .any(|message| message.code == "unknown_view_position"));
    }

    #[test]
    fn directory_validation_passes_with_all_standard_views() {
        let temp_dir = tempfile::tempdir().unwrap();
        for (laterality, view) in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")] {
            let path = temp_dir.path().join(format!(
                "{}_{}.dcm",
                laterality.to_lowercase(),
                view.to_lowercase()
            ));
            valid_metadata_object_with(laterality, view)
                .write_to_file(path)
                .unwrap();
        }

        let report =
            validate_directory_path(temp_dir.path(), &ValidationOptions::default()).unwrap();

        assert!(report.is_valid(), "{:?}", report.errors);
        assert_eq!(report.summary.file_count, 4);
        assert!(report
            .directory
            .as_ref()
            .expect("directory report")
            .missing_views
            .is_empty());
    }

    #[test]
    fn directory_validation_allows_non_mg_when_filter_allows() {
        let temp_dir = tempfile::tempdir().unwrap();
        for (laterality, view) in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")] {
            let path = temp_dir.path().join(format!(
                "{}_{}.dcm",
                laterality.to_lowercase(),
                view.to_lowercase()
            ));
            let mut dcm = valid_metadata_object_with(laterality, view);
            put_str(&mut dcm, MODALITY, "CT");
            dcm.write_to_file(path).unwrap();
        }
        let options = ValidationOptions {
            filter_config: FilterConfig::default().exclude_non_mg_modality(false),
            ..ValidationOptions::default()
        };

        let report = validate_directory_path(temp_dir.path(), &options).unwrap();

        assert!(report.is_valid(), "{:?}", report.errors);
        assert_eq!(report.summary.file_count, 4);
        assert!(report
            .directory
            .as_ref()
            .expect("directory report")
            .missing_views
            .is_empty());
        assert!(report
            .files
            .iter()
            .all(|file| error_codes(file).get("non_mg_modality").is_none()));
        assert!(report
            .files
            .iter()
            .all(|file| warning_codes(file).contains("non_mg_modality")));
    }

    #[test]
    fn directory_validation_uses_collection_refined_dbt_classification() {
        let temp_dir = tempfile::tempdir().unwrap();
        const STUDY_UID: &str = "1.2.826.0.1";
        const SPLIT_SLICE_SERIES_UID: &str = "1.2.826.0.1.10";
        const SYNTH_SINGLETON_SERIES_UID: &str = "1.2.826.0.1.20";
        const SOURCE_SOP_UID: &str = "1.2.826.0.1.30";
        const SYNTH_SINGLETON_SOP_UID: &str = "1.2.826.0.1.40";
        for index in 0..13 {
            let path = temp_dir.path().join(format!("slice_{index}.dcm"));
            ambiguous_dbt_object_with(
                STUDY_UID,
                SPLIT_SLICE_SERIES_UID,
                &format!("{SPLIT_SLICE_SERIES_UID}.{index}"),
                Some(SOURCE_SOP_UID),
                "R",
                "CC",
            )
            .write_to_file(path)
            .unwrap();
        }
        let syn2d_path = temp_dir.path().join("syn2d.dcm");
        ambiguous_dbt_object_with(
            STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID),
            "R",
            "CC",
        )
        .write_to_file(&syn2d_path)
        .unwrap();

        let mut allowed_types = HashSet::new();
        allowed_types.insert(MammogramType::Ffdm);
        allowed_types.insert(MammogramType::Synth);
        allowed_types.insert(MammogramType::Sfm);
        let options = ValidationOptions {
            filter_config: FilterConfig::permissive().with_allowed_types(allowed_types),
            ..ValidationOptions::default()
        };

        let report = validate_directory_path(temp_dir.path(), &options).unwrap();
        let selected_views = &report.directory.as_ref().unwrap().selected_views;
        let selected_rcc = selected_views.get("rcc").expect("rcc report");

        assert!(selected_rcc.file_path.is_none());
        assert!(selected_rcc.mammogram_type.is_none());
        assert!(selected_rcc.dbt_object_kind.is_none());
        let singleton_report = report
            .files
            .iter()
            .find(|file| file.file.path == syn2d_path.display().to_string())
            .expect("singleton report");
        assert_eq!(
            singleton_report.mammography.mammogram_type.as_deref(),
            Some("unknown")
        );
        assert_eq!(
            singleton_report.mammography.dbt_object_kind.as_deref(),
            Some("unknown")
        );
        assert!(report
            .files
            .iter()
            .filter(|file| file.file.path.contains("slice_"))
            .all(|file| !error_codes(file).contains("unknown_mammogram_type")));
        assert!(error_codes(singleton_report).contains("unknown_mammogram_type"));
    }

    #[test]
    fn extraction_directory_validation_clears_refined_unknown_type_warnings() {
        let temp_dir = tempfile::tempdir().unwrap();
        const STUDY_UID: &str = "1.2.826.0.2";
        const SPLIT_SLICE_SERIES_UID: &str = "1.2.826.0.2.10";
        const SYNTH_SINGLETON_SERIES_UID: &str = "1.2.826.0.2.20";
        const SOURCE_SOP_UID: &str = "1.2.826.0.2.30";
        const SYNTH_SINGLETON_SOP_UID: &str = "1.2.826.0.2.40";
        for index in 0..13 {
            let path = temp_dir.path().join(format!("slice_{index}.dcm"));
            ambiguous_dbt_object_with(
                STUDY_UID,
                SPLIT_SLICE_SERIES_UID,
                &format!("{SPLIT_SLICE_SERIES_UID}.{index}"),
                Some(SOURCE_SOP_UID),
                "R",
                "CC",
            )
            .write_to_file(path)
            .unwrap();
        }
        ambiguous_dbt_object_with(
            STUDY_UID,
            SYNTH_SINGLETON_SERIES_UID,
            SYNTH_SINGLETON_SOP_UID,
            Some(SOURCE_SOP_UID),
            "R",
            "CC",
        )
        .write_to_file(temp_dir.path().join("syn2d.dcm"))
        .unwrap();

        let options = ValidationOptions {
            profile: ValidationProfile::Extraction,
            ..ValidationOptions::default()
        };

        let report = validate_directory_path(temp_dir.path(), &options).unwrap();

        assert!(report
            .files
            .iter()
            .filter(|file| file.file.path.contains("slice_"))
            .all(|file| {
                !warning_codes(file).contains("unknown_mammogram_type")
                    && !error_codes(file).contains("unknown_mammogram_type")
            }));
        let singleton_report = report
            .files
            .iter()
            .find(|file| file.file.path.ends_with("syn2d.dcm"))
            .expect("singleton report");
        assert!(warning_codes(singleton_report).contains("unknown_mammogram_type"));
        assert!(!error_codes(singleton_report).contains("unknown_mammogram_type"));
        assert_eq!(
            singleton_report.mammography.mammogram_type.as_deref(),
            Some("unknown")
        );
        assert!(report
            .files
            .iter()
            .any(|file| file.mammography.mammogram_type.as_deref() == Some("unknown")));
    }

    #[test]
    fn zip_validation_passes_with_all_standard_views() {
        let temp_dir = tempfile::tempdir().unwrap();
        let zip_path = temp_dir.path().join("dicoms.zip");
        let zip_file = File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default();

        for (laterality, view) in [("L", "MLO"), ("R", "MLO"), ("L", "CC"), ("R", "CC")] {
            let dicom_path = temp_dir.path().join(format!(
                "{}_{}.dcm",
                laterality.to_lowercase(),
                view.to_lowercase()
            ));
            valid_metadata_object_with(laterality, view)
                .write_to_file(&dicom_path)
                .unwrap();
            let bytes = std::fs::read(&dicom_path).unwrap();
            zip_writer
                .start_file(
                    format!("nested/{}", dicom_path.file_name().unwrap().display()),
                    options,
                )
                .unwrap();
            zip_writer.write_all(&bytes).unwrap();
        }
        zip_writer.start_file("notes.txt", options).unwrap();
        zip_writer.write_all(b"not a dicom").unwrap();
        zip_writer.finish().unwrap();

        let report = validate_path(&zip_path, &ValidationOptions::default()).unwrap();

        assert!(report.is_valid(), "{:?}", report.errors);
        assert_eq!(report.summary.source_type, "zip");
        assert_eq!(report.summary.file_count, 4);
        assert!(report
            .directory
            .as_ref()
            .expect("zip directory report")
            .missing_views
            .is_empty());
        assert!(report
            .files
            .iter()
            .all(|file| file.file.path.contains("dicoms.zip::nested/")));
    }
}
