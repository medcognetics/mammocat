//! Conservative completion of missing mammography DICOM metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use dicom_core::header::Header;
use dicom_core::value::{DataSetSequence, InMemFragment, PrimitiveValue, Value};
use dicom_core::{DataElement, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{open_file, FileDicomObject, InMemDicomObject};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api::MammogramExtractor;
use crate::dicom_files::ensure_no_symlink_components;
use crate::error::{MammocatError, Result};
use crate::extraction::tags::{
    get_string_value, CODE_MEANING, CODE_VALUE, CODING_SCHEME_DESIGNATOR, FRAME_ANATOMY_SEQUENCE,
    FRAME_LATERALITY, IMAGE_LATERALITY, LATERALITY, PHOTOMETRIC_INTERPRETATION,
    SHARED_FUNCTIONAL_GROUPS_SEQUENCE, SOP_CLASS_UID, SOP_INSTANCE_UID, VIEW_CODE_SEQUENCE,
    VIEW_MODIFIER_CODE_SEQUENCE, VIEW_POSITION,
};
use crate::extraction::{
    extract_view_descriptor, view_code_definition, view_modifier_code_definition, Confidence,
    Evidence, MammographyViewDescriptor,
};
use crate::registry::{
    is_retired_snomed_coding_scheme, parse_laterality_value, retired_view_code_matches,
    view_position_value, CanonicalValue, CANONICAL_METADATA_RULES, SUPPORTED_SOP_CLASSES,
};
use crate::types::{Laterality, MammographyViewModifier, ViewPosition};
use crate::validation::{validate_dicom_file, ValidationOptions, ValidationProfile};

const SCHEMA_VERSION: u32 = 1;
const BREAST_CODE_VALUE: &str = "76752008";
const LEGACY_BREAST_CODE_VALUE: &str = "T-04000";
const BREAST_CODE_MEANING: &str = "Breast";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionOptions {
    pub allow_heuristic: bool,
    pub strip_signatures: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionFileOptions {
    pub completion: CompletionOptions,
    pub force: bool,
    pub backup_suffix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldAddition {
    pub path: String,
    pub tag: String,
    pub keyword: String,
    pub value: String,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionIssue {
    pub code: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InferredValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionPlan {
    pub schema_version: u32,
    pub supported: bool,
    pub sop_class_uid: Option<String>,
    pub additions: Vec<FieldAddition>,
    pub inferred_only: Vec<InferredValue>,
    pub issues: Vec<CompletionIssue>,
    #[serde(skip)]
    operations: Vec<PlannedOperation>,
    #[serde(skip)]
    source_identity: CompletionSourceIdentity,
    #[serde(skip)]
    options: CompletionOptions,
}

impl CompletionPlan {
    pub fn is_blocked(&self) -> bool {
        self.issues.iter().any(|issue| issue.blocking)
    }

    pub fn has_changes(&self) -> bool {
        !self.operations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionReport {
    pub schema_version: u32,
    pub supported: bool,
    pub applied: bool,
    pub changed: bool,
    pub additions: Vec<FieldAddition>,
    pub inferred_only: Vec<InferredValue>,
    pub issues: Vec<CompletionIssue>,
    pub stripped_signature_elements: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum PlannedOperation {
    Primitive {
        tag: Tag,
        vr: VR,
        value: PrimitiveValue,
    },
    EnsureViewCode {
        view: ViewPosition,
        modifiers: BTreeSet<MammographyViewModifier>,
    },
    EnsureSharedFrameAnatomy {
        laterality: Laterality,
    },
    StripSignatures,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CompletionSourceIdentity {
    dataset_sop_class_uid: Option<String>,
    dataset_sop_instance_uid: Option<String>,
    media_storage_sop_class_uid: String,
    media_storage_sop_instance_uid: String,
}

impl CompletionSourceIdentity {
    fn capture(dcm: &FileDicomObject<InMemDicomObject>) -> Self {
        Self {
            dataset_sop_class_uid: get_string_value(dcm, SOP_CLASS_UID),
            dataset_sop_instance_uid: get_string_value(dcm, SOP_INSTANCE_UID),
            media_storage_sop_class_uid: dcm.meta().media_storage_sop_class_uid.clone(),
            media_storage_sop_instance_uid: dcm.meta().media_storage_sop_instance_uid.clone(),
        }
    }
}

/// Build a non-mutating completion plan bound to this object's identity and evidence.
pub fn plan_completion(
    dcm: &FileDicomObject<InMemDicomObject>,
    options: &CompletionOptions,
) -> CompletionPlan {
    let sop_class_uid = get_string_value(dcm, SOP_CLASS_UID)
        .or_else(|| nonempty(&dcm.meta().media_storage_sop_class_uid));
    let supported = sop_class_uid
        .as_deref()
        .is_some_and(|uid| SUPPORTED_SOP_CLASSES.contains(&uid));
    let mut plan = CompletionPlan {
        schema_version: SCHEMA_VERSION,
        supported,
        sop_class_uid: sop_class_uid.clone(),
        additions: Vec::new(),
        inferred_only: Vec::new(),
        issues: Vec::new(),
        operations: Vec::new(),
        source_identity: CompletionSourceIdentity::capture(dcm),
        options: options.clone(),
    };

    if !supported {
        plan.issues.push(CompletionIssue {
            code: "unsupported_sop_class".to_string(),
            message: format!(
                "SOP Class {} is not supported by mammofill",
                sop_class_uid.as_deref().unwrap_or("<missing>")
            ),
            blocking: true,
        });
        return plan;
    }

    if contains_signature_structures(dcm) {
        if options.strip_signatures {
            plan.operations.push(PlannedOperation::StripSignatures);
            plan.additions.push(FieldAddition {
                path: "recursive".to_string(),
                tag: "(4FFE,0001)/(FFFA,FFFA)".to_string(),
                keyword: "SignatureStructures".to_string(),
                value: "removed".to_string(),
                confidence: Confidence::Exact,
                evidence: vec!["explicit --strip-signatures request".to_string()],
            });
        } else {
            plan.issues.push(CompletionIssue {
                code: "signed_instance".to_string(),
                message: "instance contains digital signature structures; use --strip-signatures to modify it".to_string(),
                blocking: true,
            });
            return plan;
        }
    }

    let sop_class_uid = sop_class_uid.expect("supported SOP Class is present");
    plan_identity_twins(dcm, &mut plan);
    plan_fixed_iod_values(dcm, &sop_class_uid, &mut plan);
    plan_laterality(dcm, &sop_class_uid, &mut plan);
    plan_view(dcm, options, &mut plan);
    plan_inferred_values(dcm, &mut plan);
    plan
}

fn plan_identity_twins(dcm: &FileDicomObject<InMemDicomObject>, plan: &mut CompletionPlan) {
    plan_primitive(
        dcm,
        plan,
        (SOP_CLASS_UID, VR::UI, "SOPClassUID"),
        PrimitiveValue::from(dcm.meta().media_storage_sop_class_uid.clone()),
        Confidence::Exact,
        "File Meta Information MediaStorageSOPClassUID",
    );
    plan_primitive(
        dcm,
        plan,
        (SOP_INSTANCE_UID, VR::UI, "SOPInstanceUID"),
        PrimitiveValue::from(dcm.meta().media_storage_sop_instance_uid.clone()),
        Confidence::Exact,
        "File Meta Information MediaStorageSOPInstanceUID",
    );
}

fn plan_fixed_iod_values(
    dcm: &FileDicomObject<InMemDicomObject>,
    sop_class_uid: &str,
    plan: &mut CompletionPlan,
) {
    for rule in CANONICAL_METADATA_RULES
        .iter()
        .filter(|rule| rule.applicability.applies(sop_class_uid))
    {
        let value = match rule.canonical_value {
            CanonicalValue::Text(value) => PrimitiveValue::from(value),
            CanonicalValue::UnsignedShort(value) => PrimitiveValue::from(value),
            CanonicalValue::ContextGroup(_) | CanonicalValue::Inferred => continue,
        };
        plan_primitive(
            dcm,
            plan,
            (rule.tag, rule.vr, rule.keyword),
            value,
            rule.confidence,
            &rule.inference_sources.join(", "),
        );
    }

    if let Some(bits_stored) = dcm
        .element(tags::BITS_STORED)
        .ok()
        .and_then(|element| element.to_int::<u16>().ok())
    {
        if bits_stored > 0 {
            plan_primitive(
                dcm,
                plan,
                (tags::HIGH_BIT, VR::US, "HighBit"),
                PrimitiveValue::from(bits_stored - 1),
                Confidence::Structural,
                "BitsStored",
            );
        }
    }

    if sop_class_uid != uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE {
        if let Some(photometric) = get_string_value(dcm, PHOTOMETRIC_INTERPRETATION) {
            let lut_shape = match photometric.to_ascii_uppercase().as_str() {
                "MONOCHROME1" => Some("INVERSE"),
                "MONOCHROME2" => Some("IDENTITY"),
                _ => None,
            };
            if let Some(lut_shape) = lut_shape {
                plan_primitive(
                    dcm,
                    plan,
                    (tags::PRESENTATION_LUT_SHAPE, VR::CS, "PresentationLUTShape"),
                    PrimitiveValue::from(lut_shape),
                    Confidence::Structural,
                    "PhotometricInterpretation",
                );
            }
        } else if let Some(lut_shape) = get_string_value(dcm, tags::PRESENTATION_LUT_SHAPE) {
            let photometric = match lut_shape.to_ascii_uppercase().as_str() {
                "INVERSE" => Some("MONOCHROME1"),
                "IDENTITY" => Some("MONOCHROME2"),
                _ => None,
            };
            if let Some(photometric) = photometric {
                plan_primitive(
                    dcm,
                    plan,
                    (
                        tags::PHOTOMETRIC_INTERPRETATION,
                        VR::CS,
                        "PhotometricInterpretation",
                    ),
                    PrimitiveValue::from(photometric),
                    Confidence::Structural,
                    "PresentationLUTShape",
                );
            }
        }
    }
}

fn plan_laterality(
    dcm: &FileDicomObject<InMemDicomObject>,
    sop_class_uid: &str,
    plan: &mut CompletionPlan,
) {
    if !is_classic_mammography(sop_class_uid) && per_frame_frame_anatomy_present(dcm) {
        plan.issues.push(CompletionIssue {
            code: "per_frame_functional_groups_not_modified".to_string(),
            message: "FrameAnatomySequence is present in PerFrameFunctionalGroupsSequence; mammofill reports per-frame gaps but does not construct or rewrite per-frame functional groups".to_string(),
            blocking: false,
        });
        return;
    }
    let (laterality, evidence) = match resolve_laterality_for_completion(dcm) {
        Ok(Some(resolution)) => resolution,
        Ok(None) => return,
        Err(message) => {
            plan.issues.push(CompletionIssue {
                code: "laterality_evidence_conflict".to_string(),
                message,
                blocking: false,
            });
            return;
        }
    };
    let value = match laterality {
        Laterality::Left => "L",
        Laterality::Right => "R",
        Laterality::Bilateral => "B",
        _ => return,
    };
    if is_classic_mammography(sop_class_uid) {
        if element_is_missing(dcm, IMAGE_LATERALITY) {
            plan_primitive(
                dcm,
                plan,
                (IMAGE_LATERALITY, VR::CS, "ImageLaterality"),
                PrimitiveValue::from(value),
                Confidence::Structural,
                &evidence,
            );
        }
    } else if !shared_frame_anatomy_compatible(dcm, laterality) {
        plan.issues.push(CompletionIssue {
            code: "shared_frame_anatomy_conflict".to_string(),
            message: "existing shared FrameAnatomySequence conflicts with inferred laterality or Breast anatomy".to_string(),
            blocking: false,
        });
    } else if shared_frame_anatomy_needs_completion(dcm, laterality) {
        plan.operations
            .push(PlannedOperation::EnsureSharedFrameAnatomy { laterality });
        plan.additions.push(FieldAddition {
            path: "SharedFunctionalGroupsSequence/FrameAnatomySequence".to_string(),
            tag: tag_string(FRAME_ANATOMY_SEQUENCE),
            keyword: "FrameAnatomySequence".to_string(),
            value: format!("Breast; FrameLaterality={value}"),
            confidence: Confidence::Structural,
            evidence: vec![evidence],
        });
    }
}

fn resolve_laterality_for_completion(
    dcm: &InMemDicomObject,
) -> std::result::Result<Option<(Laterality, String)>, String> {
    let mut evidence = Vec::new();
    for (source, tag) in [
        ("ImageLaterality", IMAGE_LATERALITY),
        ("Laterality", LATERALITY),
    ] {
        if let Some(value) = get_string_value(dcm, tag) {
            if value.is_empty() {
                continue;
            }
            let Some(laterality) = parse_laterality_value(&value) else {
                return Err(format!(
                    "{source} contains unsupported laterality value {value:?}; no laterality field was added"
                ));
            };
            evidence.push((source, value, laterality));
        }
    }
    if let Some(value) = shared_frame_laterality(dcm) {
        if !value.is_empty() {
            let Some(laterality) = parse_laterality_value(&value) else {
                return Err(format!(
                    "SharedFrameLaterality contains unsupported laterality value {value:?}; no laterality field was added"
                ));
            };
            evidence.push(("SharedFrameLaterality", value, laterality));
        }
    }
    let Some((_, _, selected)) = evidence.first() else {
        return Ok(None);
    };
    if evidence.iter().any(|(_, _, value)| value != selected) {
        let detail = evidence
            .iter()
            .map(|(source, value, _)| format!("{source}={value}"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "laterality evidence is not unanimous ({detail}); no laterality field was added"
        ));
    }
    Ok(Some((
        *selected,
        evidence
            .iter()
            .map(|(source, value, _)| format!("{source}={value}"))
            .collect::<Vec<_>>()
            .join(", "),
    )))
}

fn shared_frame_laterality(dcm: &InMemDicomObject) -> Option<String> {
    dcm.get(SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
        .and_then(|shared| shared.get(FRAME_ANATOMY_SEQUENCE))
        .and_then(|element| element.items())
        .and_then(|items| items.first())
        .and_then(|anatomy| get_string_value(anatomy, FRAME_LATERALITY))
}

fn plan_view(
    dcm: &FileDicomObject<InMemDicomObject>,
    options: &CompletionOptions,
    plan: &mut CompletionPlan,
) {
    let descriptor = extract_view_descriptor(dcm);
    for conflict in &descriptor.conflicts {
        plan.issues.push(CompletionIssue {
            code: "view_evidence_conflict".to_string(),
            message: conflict.clone(),
            blocking: false,
        });
    }
    if !descriptor.conflicts.is_empty() {
        return;
    }

    let base_confidence = base_view_confidence(&descriptor);
    if let Some(view_position_value) = view_position_value(descriptor.view_position) {
        if confidence_allowed(base_confidence, options) && element_is_missing(dcm, VIEW_POSITION) {
            plan_primitive(
                dcm,
                plan,
                (VIEW_POSITION, VR::CS, "ViewPosition"),
                PrimitiveValue::from(view_position_value),
                base_confidence,
                "shared mammography view descriptor",
            );
        }
    }

    let writable_modifiers: BTreeSet<_> = descriptor
        .modifiers
        .iter()
        .copied()
        .filter(|modifier| confidence_allowed(modifier_confidence(&descriptor, *modifier), options))
        .collect();
    if descriptor.view_position.is_unknown() {
        if !writable_modifiers.is_empty() {
            plan.inferred_only.push(InferredValue {
                name: "view_modifiers".to_string(),
                value: join_modifiers(&writable_modifiers),
            });
            plan.issues.push(CompletionIssue {
                code: "modifier_without_base_view".to_string(),
                message: "view modifiers were inferred, but no unambiguous CID 4014 base view is available".to_string(),
                blocking: false,
            });
        }
        return;
    }
    if !confidence_allowed(base_confidence, options) {
        plan.inferred_only.push(InferredValue {
            name: "view_position".to_string(),
            value: descriptor.view_position.to_string(),
        });
        if !writable_modifiers.is_empty() {
            plan.inferred_only.push(InferredValue {
                name: "view_modifiers".to_string(),
                value: join_modifiers(&writable_modifiers),
            });
        }
        return;
    }
    if !view_code_compatible(dcm, descriptor.view_position) {
        plan.issues.push(CompletionIssue {
            code: "view_code_not_completable".to_string(),
            message: "existing ViewCodeSequence contains populated components that do not match the inferred base view".to_string(),
            blocking: false,
        });
        return;
    }
    if view_code_needs_completion(dcm, &writable_modifiers) {
        plan.operations.push(PlannedOperation::EnsureViewCode {
            view: descriptor.view_position,
            modifiers: writable_modifiers.clone(),
        });
        plan.additions.push(FieldAddition {
            path: "ViewCodeSequence".to_string(),
            tag: tag_string(VIEW_CODE_SEQUENCE),
            keyword: "ViewCodeSequence".to_string(),
            value: format!(
                "{}{}",
                descriptor.view_position.short_str().to_ascii_uppercase(),
                if writable_modifiers.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", join_modifiers(&writable_modifiers))
                }
            ),
            confidence: base_confidence,
            evidence: descriptor.evidence.iter().map(format_evidence).collect(),
        });
    }
}

fn plan_inferred_values(dcm: &FileDicomObject<InMemDicomObject>, plan: &mut CompletionPlan) {
    if let Ok(metadata) =
        MammogramExtractor::extract_file_with_options_and_modality_policy(dcm, false, true)
    {
        plan.inferred_only.extend([
            InferredValue {
                name: "mammogram_type".to_string(),
                value: metadata.mammogram_type.to_string(),
            },
            InferredValue {
                name: "dbt_object_kind".to_string(),
                value: metadata.dbt_object_kind.to_string(),
            },
        ]);
    }
}

fn plan_primitive(
    dcm: &FileDicomObject<InMemDicomObject>,
    plan: &mut CompletionPlan,
    field: (Tag, VR, &str),
    value: PrimitiveValue,
    confidence: Confidence,
    evidence: &str,
) {
    let (tag, vr, keyword) = field;
    let expected = primitive_display(&value);
    if element_is_missing(dcm, tag) {
        plan.operations
            .push(PlannedOperation::Primitive { tag, vr, value });
        plan.additions.push(FieldAddition {
            path: keyword.to_string(),
            tag: tag_string(tag),
            keyword: keyword.to_string(),
            value: expected,
            confidence,
            evidence: vec![evidence.to_string()],
        });
    } else if let Some(existing) = get_string_value(dcm, tag) {
        if !values_equal(vr, &existing, &expected) {
            plan.issues.push(CompletionIssue {
                code: "populated_value_conflict".to_string(),
                message: format!("{keyword} is populated with {existing:?}; expected {expected:?}; value was not replaced"),
                blocking: false,
            });
        }
    }
}

/// Apply a plan to the unchanged object from which it was created.
///
/// This returns an error if the SOP identity or completion evidence has changed. Call
/// [`plan_completion`] again after changing the object or when targeting another object.
pub fn apply_completion_plan(
    dcm: &mut FileDicomObject<InMemDicomObject>,
    plan: &CompletionPlan,
) -> Result<CompletionReport> {
    apply_completion_plan_at(dcm, plan, &dicom_timestamp())
}

fn apply_completion_plan_at(
    dcm: &mut FileDicomObject<InMemDicomObject>,
    plan: &CompletionPlan,
    timestamp: &str,
) -> Result<CompletionReport> {
    if plan.is_blocked() {
        return Ok(report_from_plan(plan, false, false, 0));
    }
    if plan.operations.is_empty() {
        return Ok(report_from_plan(plan, true, false, 0));
    }
    ensure_plan_matches_target(dcm, plan)?;

    let audit_tags = audit_top_level_tags(dcm, &plan.operations);
    let prior_values = capture_prior_values(dcm, &audit_tags);
    let mut stripped_signature_elements = 0;
    for operation in &plan.operations {
        match operation {
            PlannedOperation::Primitive { tag, vr, value } => {
                if element_is_missing(dcm, *tag) {
                    dcm.put(DataElement::new(*tag, *vr, value.clone()));
                }
            }
            PlannedOperation::EnsureViewCode { view, modifiers } => {
                ensure_view_code(dcm, *view, modifiers);
            }
            PlannedOperation::EnsureSharedFrameAnatomy { laterality } => {
                ensure_shared_frame_anatomy(dcm, *laterality);
            }
            PlannedOperation::StripSignatures => {
                stripped_signature_elements += strip_signature_structures(dcm);
            }
        }
    }
    append_original_attributes(dcm, prior_values, timestamp);
    if element_is_missing(dcm, tags::INSTANCE_COERCION_DATE_TIME) {
        dcm.put(DataElement::new(
            tags::INSTANCE_COERCION_DATE_TIME,
            VR::DT,
            PrimitiveValue::from(timestamp.to_string()),
        ));
    }
    Ok(report_from_plan(
        plan,
        true,
        true,
        stripped_signature_elements,
    ))
}

fn ensure_plan_matches_target(
    dcm: &FileDicomObject<InMemDicomObject>,
    plan: &CompletionPlan,
) -> Result<()> {
    let identity_matches = plan.source_identity == CompletionSourceIdentity::capture(dcm);
    let refreshed = plan_completion(dcm, &plan.options);
    if !identity_matches || refreshed.operations != plan.operations {
        return Err(MammocatError::ExtractionError(
            "completion plan does not match the target object; regenerate the plan".to_string(),
        ));
    }
    Ok(())
}

pub fn complete_file(
    input: &Path,
    output: &Path,
    options: &CompletionFileOptions,
) -> Result<CompletionReport> {
    ensure_no_symlink_components(input)?;
    if input != output && paths_resolve_to_same_file(input, output)? {
        return Err(MammocatError::IoError(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "output resolves to input; use the same path for in-place completion: {}",
                input.display()
            ),
        )));
    }
    let mut dcm = open_file(input)?;
    let plan = plan_completion(&dcm, &options.completion);
    if plan.is_blocked() {
        return Ok(report_from_plan(&plan, false, false, 0));
    }
    if output.exists() && output != input && !options.force {
        return Err(MammocatError::IoError(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("output already exists: {}", output.display()),
        )));
    }
    let before = FileInvariant::capture(&dcm)?;
    if !plan.has_changes() {
        if input != output {
            if options.force {
                copy_file_atomically_replacing(input, output, &before)?;
            } else {
                copy_file_atomically(input, output, &before)?;
            }
        }
        return Ok(report_from_plan(&plan, true, false, 0));
    }

    if input == output {
        if let Some(suffix) = &options.backup_suffix {
            let backup = backup_path(input, suffix);
            copy_backup_no_clobber(input, &backup)?;
        }
    }

    let report = apply_completion_plan(&mut dcm, &plan)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".mammofill-{}.dcm", Uuid::new_v4()));
    if let Err(error) = dcm.write_to_file(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(MammocatError::DicomError(error.to_string()));
    }
    if let Ok(metadata) = fs::metadata(input) {
        if let Err(error) = fs::set_permissions(&temporary, metadata.permissions()) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
    }
    if let Err(error) = verify_output_file(&temporary, &before) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let replace_existing = input == output || options.force;
    if let Err(error) = commit_temporary_file(&temporary, output, replace_existing) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(report)
}

fn paths_resolve_to_same_file(input: &Path, output: &Path) -> Result<bool> {
    let input = fs::canonicalize(input)?;
    match fs::canonicalize(output) {
        Ok(output) => Ok(input == output),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn copy_file_atomically(input: &Path, output: &Path, before: &FileInvariant) -> Result<()> {
    copy_file_atomically_with_mode(input, output, before, false)
}

fn copy_file_atomically_replacing(
    input: &Path,
    output: &Path,
    before: &FileInvariant,
) -> Result<()> {
    copy_file_atomically_with_mode(input, output, before, true)
}

fn copy_file_atomically_with_mode(
    input: &Path,
    output: &Path,
    before: &FileInvariant,
    replace_existing: bool,
) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".mammofill-{}.dcm", Uuid::new_v4()));
    if let Err(error) = fs::copy(input, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    if let Err(error) = verify_output_file(&temporary, before) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = commit_temporary_file(&temporary, output, replace_existing) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn commit_temporary_file(temporary: &Path, output: &Path, replace_existing: bool) -> Result<()> {
    if replace_existing {
        fs::rename(temporary, output)?;
    } else {
        fs::hard_link(temporary, output)?;
        fs::remove_file(temporary)?;
    }
    Ok(())
}

fn copy_backup_no_clobber(input: &Path, backup: &Path) -> Result<()> {
    let mut source = fs::File::open(input)?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup)?;
    if let Err(error) = io::copy(&mut source, &mut destination) {
        drop(destination);
        let _ = fs::remove_file(backup);
        return Err(error.into());
    }
    if let Ok(metadata) = fs::metadata(input) {
        if let Err(error) = fs::set_permissions(backup, metadata.permissions()) {
            drop(destination);
            let _ = fs::remove_file(backup);
            return Err(error.into());
        }
    }
    Ok(())
}

fn verify_output_file(path: &Path, before: &FileInvariant) -> Result<()> {
    let reopened = open_file(path)?;
    before.verify(&reopened)?;
    if MammogramExtractor::extract_file(&reopened).is_err() {
        return Err(MammocatError::ExtractionError(
            "output does not pass mammocat extraction".to_string(),
        ));
    }
    let validation = validate_dicom_file(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Extraction,
            ..ValidationOptions::default()
        },
    );
    if !validation.is_valid() {
        return Err(MammocatError::ExtractionError(
            "output does not pass mammovalidate extraction readiness".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct FileInvariant {
    sop_class_uid: Option<String>,
    sop_instance_uid: Option<String>,
    media_storage_sop_class_uid: String,
    media_storage_sop_instance_uid: String,
    transfer_syntax_uid: String,
    pixel_data: Option<PixelDataFingerprint>,
}

impl FileInvariant {
    fn capture(dcm: &FileDicomObject<InMemDicomObject>) -> Result<Self> {
        Ok(Self {
            sop_class_uid: get_string_value(dcm, SOP_CLASS_UID)
                .or_else(|| nonempty(&dcm.meta().media_storage_sop_class_uid)),
            sop_instance_uid: get_string_value(dcm, SOP_INSTANCE_UID)
                .or_else(|| nonempty(&dcm.meta().media_storage_sop_instance_uid)),
            media_storage_sop_class_uid: dcm.meta().media_storage_sop_class_uid.clone(),
            media_storage_sop_instance_uid: dcm.meta().media_storage_sop_instance_uid.clone(),
            transfer_syntax_uid: dcm.meta().transfer_syntax.clone(),
            pixel_data: dcm
                .get(tags::PIXEL_DATA)
                .map(|element| PixelDataFingerprint::capture(element.vr(), element.value()))
                .transpose()?,
        })
    }

    fn verify(&self, dcm: &FileDicomObject<InMemDicomObject>) -> Result<()> {
        let after = Self::capture(dcm)?;
        if self.sop_class_uid != after.sop_class_uid
            || self.sop_instance_uid != after.sop_instance_uid
            || self.media_storage_sop_class_uid != after.media_storage_sop_class_uid
            || self.media_storage_sop_instance_uid != after.media_storage_sop_instance_uid
            || self.transfer_syntax_uid != after.transfer_syntax_uid
            || self.pixel_data != after.pixel_data
        {
            return Err(MammocatError::ExtractionError(
                "output verification found a changed SOP identity, transfer syntax, or PixelData value".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PixelDataFingerprint([u8; 32]);

impl PixelDataFingerprint {
    fn capture(vr: VR, value: &Value<InMemDicomObject, InMemFragment>) -> Result<Self> {
        let mut digest = Sha256::new();
        digest.update(vr.to_string().as_bytes());
        match value {
            Value::Primitive(PrimitiveValue::Empty) => digest.update(b"empty"),
            Value::Primitive(PrimitiveValue::U8(bytes)) => {
                digest.update(b"native-u8");
                update_digest_length(&mut digest, bytes.len());
                digest.update(bytes);
            }
            Value::Primitive(PrimitiveValue::U16(words)) => {
                digest.update(b"native-u16");
                update_digest_length(&mut digest, words.len());
                for word in words {
                    digest.update(word.to_le_bytes());
                }
            }
            Value::PixelSequence(sequence) => {
                digest.update(b"encapsulated");
                update_digest_length(&mut digest, sequence.offset_table().len());
                for offset in sequence.offset_table() {
                    digest.update(offset.to_le_bytes());
                }
                update_digest_length(&mut digest, sequence.fragments().len());
                for fragment in sequence.fragments() {
                    update_digest_length(&mut digest, fragment.len());
                    digest.update(fragment);
                }
            }
            _ => {
                return Err(MammocatError::ExtractionError(
                    "PixelData has an unsupported in-memory representation".to_string(),
                ));
            }
        }
        Ok(Self(digest.finalize().into()))
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.len()
    }
}

fn update_digest_length(digest: &mut Sha256, length: usize) {
    digest.update((length as u64).to_le_bytes());
}

fn ensure_view_code(
    dcm: &mut InMemDicomObject,
    view: ViewPosition,
    modifiers: &BTreeSet<MammographyViewModifier>,
) {
    let definition = view_code_definition(view).expect("known view has a code definition");
    let mut items = dcm
        .get(VIEW_CODE_SEQUENCE)
        .and_then(|element| element.items())
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    if items.is_empty() {
        items.push(InMemDicomObject::new_empty());
    }
    let item = &mut items[0];
    put_missing_string(item, CODE_VALUE, VR::SH, definition.code_value);
    put_missing_string(item, CODING_SCHEME_DESIGNATOR, VR::SH, "SCT");
    put_missing_string(item, CODE_MEANING, VR::LO, definition.code_meaning);

    let mut modifier_items = item
        .get(VIEW_MODIFIER_CODE_SEQUENCE)
        .and_then(|element| element.items())
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    for modifier in modifiers {
        if modifier_items
            .iter()
            .any(|existing| modifier_item_matches(existing, *modifier))
        {
            continue;
        }
        let definition = view_modifier_code_definition(*modifier);
        modifier_items.push(InMemDicomObject::from_element_iter([
            DataElement::new(
                CODE_VALUE,
                VR::SH,
                PrimitiveValue::from(definition.code_value),
            ),
            DataElement::new(
                CODING_SCHEME_DESIGNATOR,
                VR::SH,
                PrimitiveValue::from("SCT"),
            ),
            DataElement::new(
                CODE_MEANING,
                VR::LO,
                PrimitiveValue::from(definition.code_meaning),
            ),
        ]));
    }
    if !modifier_items.is_empty() {
        item.put(DataElement::new(
            VIEW_MODIFIER_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(modifier_items),
        ));
    }
    dcm.put(DataElement::new(
        VIEW_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(items),
    ));
}

fn ensure_shared_frame_anatomy(dcm: &mut InMemDicomObject, laterality: Laterality) {
    let mut shared_items = dcm
        .get(SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .and_then(|element| element.items())
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    if shared_items.is_empty() {
        shared_items.push(InMemDicomObject::new_empty());
    }
    let shared = &mut shared_items[0];
    let mut anatomy_items = shared
        .get(FRAME_ANATOMY_SEQUENCE)
        .and_then(|element| element.items())
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    if anatomy_items.is_empty() {
        anatomy_items.push(InMemDicomObject::new_empty());
    }
    let anatomy = &mut anatomy_items[0];
    let laterality_value = match laterality {
        Laterality::Left => "L",
        Laterality::Right => "R",
        Laterality::Bilateral => "B",
        _ => "U",
    };
    put_missing_string(anatomy, FRAME_LATERALITY, VR::CS, laterality_value);
    if element_is_missing(anatomy, tags::ANATOMIC_REGION_SEQUENCE) {
        let breast = InMemDicomObject::from_element_iter([
            DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from(BREAST_CODE_VALUE)),
            DataElement::new(
                CODING_SCHEME_DESIGNATOR,
                VR::SH,
                PrimitiveValue::from("SCT"),
            ),
            DataElement::new(
                CODE_MEANING,
                VR::LO,
                PrimitiveValue::from(BREAST_CODE_MEANING),
            ),
        ]);
        anatomy.put(DataElement::new(
            tags::ANATOMIC_REGION_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![breast]),
        ));
    }
    shared.put(DataElement::new(
        FRAME_ANATOMY_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(anatomy_items),
    ));
    dcm.put(DataElement::new(
        SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(shared_items),
    ));
}

fn append_original_attributes(
    dcm: &mut InMemDicomObject,
    prior_values: InMemDicomObject,
    timestamp: &str,
) {
    let mut audit_items = dcm
        .get(tags::ORIGINAL_ATTRIBUTES_SEQUENCE)
        .and_then(|element| element.items())
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    let audit_item = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOURCE_OF_PREVIOUS_VALUES,
            VR::LO,
            PrimitiveValue::Empty,
        ),
        DataElement::new(
            tags::ATTRIBUTE_MODIFICATION_DATE_TIME,
            VR::DT,
            PrimitiveValue::from(timestamp.to_string()),
        ),
        DataElement::new(
            tags::MODIFYING_SYSTEM,
            VR::LO,
            PrimitiveValue::from(format!("mammofill/{}", env!("CARGO_PKG_VERSION"))),
        ),
        DataElement::new(
            tags::REASON_FOR_THE_ATTRIBUTE_MODIFICATION,
            VR::CS,
            PrimitiveValue::from("CORRECT"),
        ),
        DataElement::new(
            tags::MODIFIED_ATTRIBUTES_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![prior_values]),
        ),
    ]);
    audit_items.push(audit_item);
    dcm.put(DataElement::new(
        tags::ORIGINAL_ATTRIBUTES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(audit_items),
    ));
}

fn audit_top_level_tags(
    dcm: &InMemDicomObject,
    operations: &[PlannedOperation],
) -> BTreeMap<Tag, VR> {
    let mut tags_to_audit = BTreeMap::new();
    for operation in operations {
        match operation {
            PlannedOperation::Primitive { tag, vr, .. } => {
                tags_to_audit.insert(*tag, *vr);
            }
            PlannedOperation::EnsureViewCode { .. } => {
                tags_to_audit.insert(VIEW_CODE_SEQUENCE, VR::SQ);
            }
            PlannedOperation::EnsureSharedFrameAnatomy { .. } => {
                tags_to_audit.insert(SHARED_FUNCTIONAL_GROUPS_SEQUENCE, VR::SQ);
            }
            PlannedOperation::StripSignatures => {
                for element in dcm.iter() {
                    if element.tag() == tags::DIGITAL_SIGNATURES_SEQUENCE
                        || element.tag() == tags::MAC_PARAMETERS_SEQUENCE
                        || element
                            .items()
                            .is_some_and(|items| items.iter().any(contains_signature_structures))
                    {
                        tags_to_audit.insert(element.tag(), element.vr());
                    }
                }
            }
        }
    }
    if element_is_missing(dcm, tags::INSTANCE_COERCION_DATE_TIME) {
        tags_to_audit.insert(tags::INSTANCE_COERCION_DATE_TIME, VR::DT);
    }
    tags_to_audit
}

fn capture_prior_values(
    dcm: &InMemDicomObject,
    audit_tags: &BTreeMap<Tag, VR>,
) -> InMemDicomObject {
    let mut prior = InMemDicomObject::new_empty();
    for (tag, vr) in audit_tags {
        if let Some(element) = dcm.get(*tag) {
            prior.put(element.clone());
        } else if *vr == VR::SQ {
            prior.put(DataElement::new(
                *tag,
                *vr,
                DataSetSequence::<InMemDicomObject>::empty(),
            ));
        } else {
            prior.put(DataElement::new(*tag, *vr, PrimitiveValue::Empty));
        }
    }
    // Do not retain signature or MAC structures inside the audit sequence when
    // explicit stripping was requested. The audit item and completion report
    // record the removal without embedding the signed structures elsewhere.
    strip_signature_structures(&mut prior);
    prior
}

fn contains_signature_structures(dcm: &InMemDicomObject) -> bool {
    dcm.get(tags::DIGITAL_SIGNATURES_SEQUENCE).is_some()
        || dcm.get(tags::MAC_PARAMETERS_SEQUENCE).is_some()
        || dcm.iter().any(|element| {
            element
                .items()
                .is_some_and(|items| items.iter().any(contains_signature_structures))
        })
}

fn strip_signature_structures(dcm: &mut InMemDicomObject) -> usize {
    let mut removed = usize::from(dcm.remove_element(tags::DIGITAL_SIGNATURES_SEQUENCE));
    removed += usize::from(dcm.remove_element(tags::MAC_PARAMETERS_SEQUENCE));
    let sequence_tags: Vec<_> = dcm
        .iter()
        .filter(|element| element.items().is_some())
        .map(|element| element.tag())
        .collect();
    for tag in sequence_tags {
        dcm.update_value(tag, |value| {
            if let Some(items) = value.items_mut() {
                for item in items {
                    removed += strip_signature_structures(item);
                }
            }
        });
    }
    removed
}

fn view_code_needs_completion(
    dcm: &InMemDicomObject,
    modifiers: &BTreeSet<MammographyViewModifier>,
) -> bool {
    let Some(item) = dcm
        .get(VIEW_CODE_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
    else {
        return true;
    };
    if [CODE_VALUE, CODING_SCHEME_DESIGNATOR, CODE_MEANING]
        .into_iter()
        .any(|tag| element_is_missing(item, tag))
    {
        return true;
    }
    modifiers.iter().any(|modifier| {
        !item
            .get(VIEW_MODIFIER_CODE_SEQUENCE)
            .and_then(|element| element.items())
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|existing| modifier_item_matches(existing, *modifier))
            })
    })
}

fn view_code_compatible(dcm: &InMemDicomObject, view: ViewPosition) -> bool {
    let Some(item) = dcm
        .get(VIEW_CODE_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
    else {
        return true;
    };
    let definition = view_code_definition(view).expect("known view");
    let scheme = get_string_value(item, CODING_SCHEME_DESIGNATOR).filter(|value| !value.is_empty());
    let code = get_string_value(item, CODE_VALUE).filter(|value| !value.is_empty());
    let tuple_matches = match (scheme.as_deref(), code.as_deref()) {
        (Some(scheme), Some(code)) if scheme.eq_ignore_ascii_case("SCT") => {
            code == definition.code_value
        }
        (Some(scheme), Some(code)) if is_retired_snomed_coding_scheme(scheme) => {
            retired_view_code_matches(definition, code)
        }
        (Some(scheme), None) if scheme.eq_ignore_ascii_case("SCT") => true,
        (None, Some(code)) => code == definition.code_value,
        (None, None) => true,
        _ => false,
    };
    tuple_matches && component_matches_or_missing(item, CODE_MEANING, &[definition.code_meaning])
}

fn modifier_item_matches(item: &InMemDicomObject, modifier: MammographyViewModifier) -> bool {
    let definition = view_modifier_code_definition(modifier);
    let scheme = get_string_value(item, CODING_SCHEME_DESIGNATOR).filter(|value| !value.is_empty());
    let code = get_string_value(item, CODE_VALUE).filter(|value| !value.is_empty());
    match (scheme.as_deref(), code.as_deref()) {
        (Some(scheme), Some(code)) if scheme.eq_ignore_ascii_case("SCT") => {
            code == definition.code_value
        }
        (Some(scheme), Some(code)) if is_retired_snomed_coding_scheme(scheme) => {
            code.eq_ignore_ascii_case(definition.legacy_code_value)
        }
        (None, _) | (_, None) => get_string_value(item, CODE_MEANING)
            .is_some_and(|value| value.eq_ignore_ascii_case(definition.code_meaning)),
        _ => false,
    }
}

fn component_matches_or_missing(item: &InMemDicomObject, tag: Tag, allowed: &[&str]) -> bool {
    element_is_missing(item, tag)
        || get_string_value(item, tag).is_some_and(|value| {
            let value = normalize_code_meaning(&value);
            allowed
                .iter()
                .any(|allowed| value == normalize_code_meaning(allowed))
        })
}

fn normalize_code_meaning(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn per_frame_frame_anatomy_present(dcm: &InMemDicomObject) -> bool {
    dcm.get(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .and_then(|element| element.items())
        .is_some_and(|frames| {
            frames
                .iter()
                .any(|frame| frame.get(FRAME_ANATOMY_SEQUENCE).is_some())
        })
}

fn shared_frame_anatomy_needs_completion(dcm: &InMemDicomObject, laterality: Laterality) -> bool {
    if !matches!(
        laterality,
        Laterality::Left | Laterality::Right | Laterality::Bilateral
    ) {
        return false;
    }
    let anatomy = dcm
        .get(SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
        .and_then(|shared| shared.get(FRAME_ANATOMY_SEQUENCE))
        .and_then(|element| element.items())
        .and_then(|items| items.first());
    anatomy.is_none_or(|anatomy| {
        element_is_missing(anatomy, FRAME_LATERALITY)
            || element_is_missing(anatomy, tags::ANATOMIC_REGION_SEQUENCE)
    })
}

fn shared_frame_anatomy_compatible(dcm: &InMemDicomObject, laterality: Laterality) -> bool {
    let expected = match laterality {
        Laterality::Left => "L",
        Laterality::Right => "R",
        Laterality::Bilateral => "B",
        _ => return true,
    };
    let Some(anatomy) = dcm
        .get(SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
        .and_then(|shared| shared.get(FRAME_ANATOMY_SEQUENCE))
        .and_then(|element| element.items())
        .and_then(|items| items.first())
    else {
        return true;
    };
    let laterality_matches = element_is_missing(anatomy, FRAME_LATERALITY)
        || get_string_value(anatomy, FRAME_LATERALITY).as_deref() == Some(expected);
    let anatomy_matches = anatomy
        .get(tags::ANATOMIC_REGION_SEQUENCE)
        .and_then(|element| element.items())
        .and_then(|items| items.first())
        .is_none_or(anatomic_region_is_breast);
    laterality_matches && anatomy_matches
}

fn anatomic_region_is_breast(region: &InMemDicomObject) -> bool {
    let scheme =
        get_string_value(region, CODING_SCHEME_DESIGNATOR).filter(|value| !value.trim().is_empty());
    let code = get_string_value(region, CODE_VALUE).filter(|value| !value.trim().is_empty());
    match (scheme.as_deref(), code.as_deref()) {
        (Some(scheme), Some(code)) if scheme.eq_ignore_ascii_case("SCT") => {
            code == BREAST_CODE_VALUE
        }
        (Some(scheme), Some(code)) if is_retired_snomed_coding_scheme(scheme) => {
            code.eq_ignore_ascii_case(LEGACY_BREAST_CODE_VALUE)
        }
        _ => get_string_value(region, CODE_MEANING)
            .is_some_and(|meaning| meaning.trim().eq_ignore_ascii_case(BREAST_CODE_MEANING)),
    }
}

fn base_view_confidence(descriptor: &MammographyViewDescriptor) -> Confidence {
    descriptor
        .evidence
        .iter()
        .filter(|evidence| {
            !evidence.source.contains("Modifier") && evidence.source != "PaddleDescription"
        })
        .map(|evidence| evidence.confidence)
        .max()
        .unwrap_or(Confidence::Heuristic)
}

fn modifier_confidence(
    descriptor: &MammographyViewDescriptor,
    modifier: MammographyViewModifier,
) -> Confidence {
    let definition = view_modifier_code_definition(modifier);
    descriptor
        .evidence
        .iter()
        .filter(|evidence| {
            evidence.value == definition.code_value
                || evidence
                    .value
                    .eq_ignore_ascii_case(definition.legacy_code_value)
                || evidence.value.eq_ignore_ascii_case(definition.code_meaning)
                || matches!(
                    (evidence.source.as_str(), modifier),
                    (
                        "ViewPosition",
                        MammographyViewModifier::AxillaryTail | MammographyViewModifier::Cleavage
                    ) | (
                        "PaddleDescription",
                        MammographyViewModifier::SpotCompression
                            | MammographyViewModifier::Magnification
                    )
                )
        })
        .map(|evidence| evidence.confidence)
        .max()
        .unwrap_or(Confidence::Heuristic)
}

fn confidence_allowed(confidence: Confidence, options: &CompletionOptions) -> bool {
    confidence != Confidence::Heuristic || options.allow_heuristic
}

fn element_is_missing(dcm: &InMemDicomObject, tag: Tag) -> bool {
    let Some(element) = dcm.get(tag) else {
        return true;
    };
    if let Some(items) = element.items() {
        return items.is_empty();
    }
    element
        .to_str()
        .map_or(true, |value| value.trim().is_empty())
}

fn put_missing_string(dcm: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    if element_is_missing(dcm, tag) {
        dcm.put(DataElement::new(tag, vr, PrimitiveValue::from(value)));
    }
}

fn primitive_display(value: &PrimitiveValue) -> String {
    value.to_str().to_string()
}

fn values_equal(vr: VR, left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    match vr {
        VR::CS | VR::UI => left.eq_ignore_ascii_case(right),
        VR::DS => match (left.parse::<f64>(), right.parse::<f64>()) {
            (Ok(left), Ok(right)) if left.is_finite() && right.is_finite() => left == right,
            _ => left == right,
        },
        VR::IS => match (left.parse::<i64>(), right.parse::<i64>()) {
            (Ok(left), Ok(right)) => left == right,
            _ => left == right,
        },
        _ => left == right,
    }
}

fn is_classic_mammography(sop_class_uid: &str) -> bool {
    matches!(
        sop_class_uid,
        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION
            | uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PROCESSING
    )
}

fn format_evidence(evidence: &Evidence) -> String {
    format!(
        "{}={} ({:?})",
        evidence.source, evidence.value, evidence.confidence
    )
}

fn join_modifiers(modifiers: &BTreeSet<MammographyViewModifier>) -> String {
    modifiers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn report_from_plan(
    plan: &CompletionPlan,
    applied: bool,
    changed: bool,
    stripped_signature_elements: usize,
) -> CompletionReport {
    CompletionReport {
        schema_version: SCHEMA_VERSION,
        supported: plan.supported,
        applied,
        changed,
        additions: plan.additions.clone(),
        inferred_only: plan.inferred_only.clone(),
        issues: plan.issues.clone(),
        stripped_signature_elements,
    }
}

fn dicom_timestamp() -> String {
    Utc::now().format("%Y%m%d%H%M%S%.6f+0000").to_string()
}

fn tag_string(tag: Tag) -> String {
    format!("({:04X},{:04X})", tag.0, tag.1)
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn backup_path(path: &Path, suffix: &str) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(suffix);
    PathBuf::from(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::value::{PixelFragmentSequence, Value};
    use dicom_object::FileMetaTableBuilder;
    use tempfile::tempdir;

    fn minimal_file_object() -> FileDicomObject<InMemDicomObject> {
        let object = InMemDicomObject::from_element_iter([
            DataElement::new(
                SOP_CLASS_UID,
                VR::UI,
                PrimitiveValue::from(
                    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                ),
            ),
            DataElement::new(
                SOP_INSTANCE_UID,
                VR::UI,
                PrimitiveValue::from("1.2.826.0.1.3680043.10.543.1"),
            ),
            DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                PrimitiveValue::from("ORIGINAL\\PRIMARY"),
            ),
            DataElement::new(LATERALITY, VR::CS, PrimitiveValue::from("L")),
            DataElement::new(VIEW_POSITION, VR::CS, PrimitiveValue::from("CC")),
        ]);
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(
                        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                    )
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.543.1")
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
            )
            .unwrap()
    }

    fn file_object_for_sop(sop_class_uid: &str) -> FileDicomObject<InMemDicomObject> {
        let object = InMemDicomObject::from_element_iter([
            DataElement::new(
                SOP_CLASS_UID,
                VR::UI,
                PrimitiveValue::from(sop_class_uid.to_string()),
            ),
            DataElement::new(
                SOP_INSTANCE_UID,
                VR::UI,
                PrimitiveValue::from("1.2.826.0.1.3680043.10.543.2"),
            ),
            DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                PrimitiveValue::from("ORIGINAL\\PRIMARY"),
            ),
            DataElement::new(LATERALITY, VR::CS, PrimitiveValue::from("L")),
            DataElement::new(VIEW_POSITION, VR::CS, PrimitiveValue::from("CC")),
        ]);
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(sop_class_uid)
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.543.2")
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
            )
            .unwrap()
    }

    #[test]
    fn fills_missing_values_without_replacing_populated_values() {
        let mut dcm = minimal_file_object();
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        assert!(plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "ImageLaterality"));
        let report =
            apply_completion_plan_at(&mut dcm, &plan, "20260713120000.000000+0000").unwrap();
        assert!(report.changed);
        assert_eq!(
            get_string_value(&dcm, IMAGE_LATERALITY).as_deref(),
            Some("L")
        );
        assert_eq!(get_string_value(&dcm, VIEW_POSITION).as_deref(), Some("CC"));
        assert!(dcm.get(tags::ORIGINAL_ATTRIBUTES_SEQUENCE).is_some());
    }

    #[test]
    fn completion_is_idempotent() {
        let mut dcm = minimal_file_object();
        let first = plan_completion(&dcm, &CompletionOptions::default());
        apply_completion_plan_at(&mut dcm, &first, "20260713120000.000000+0000").unwrap();
        let second = plan_completion(&dcm, &CompletionOptions::default());
        assert!(!second.has_changes());
    }

    #[test]
    fn completion_plan_rejects_changed_inference_evidence() {
        let source = minimal_file_object();
        let plan = plan_completion(&source, &CompletionOptions::default());
        let mut target = minimal_file_object();
        target.put(DataElement::new(
            LATERALITY,
            VR::CS,
            PrimitiveValue::from("R"),
        ));

        let result = apply_completion_plan_at(&mut target, &plan, "20260713120000.000000+0000");

        assert!(result.is_err());
        assert!(element_is_missing(&target, IMAGE_LATERALITY));
    }

    #[test]
    fn completion_plan_rejects_a_different_sop_instance() {
        const OTHER_SOP_INSTANCE_UID: &str = "1.2.826.0.1.3680043.10.543.99";
        let source = minimal_file_object();
        let plan = plan_completion(&source, &CompletionOptions::default());
        let mut target = minimal_file_object();
        target.put(DataElement::new(
            SOP_INSTANCE_UID,
            VR::UI,
            PrimitiveValue::from(OTHER_SOP_INSTANCE_UID),
        ));

        let result = apply_completion_plan_at(&mut target, &plan, "20260713120000.000000+0000");

        assert!(result.is_err());
        assert!(element_is_missing(&target, IMAGE_LATERALITY));
    }

    #[test]
    fn successful_prefill_extraction_keeps_selection_metadata_unchanged() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::MODALITY,
            VR::CS,
            PrimitiveValue::from("MG"),
        ));
        let before = MammogramExtractor::extract_file(&dcm).unwrap();

        let plan = plan_completion(&dcm, &CompletionOptions::default());
        apply_completion_plan_at(&mut dcm, &plan, "20260713120000.000000+0000").unwrap();
        let after = MammogramExtractor::extract_file(&dcm).unwrap();

        assert_eq!(after.mammogram_type, before.mammogram_type);
        assert_eq!(after.dbt_object_kind, before.dbt_object_kind);
        assert_eq!(after.laterality, before.laterality);
        assert_eq!(after.view_position, before.view_position);
        assert_eq!(after.view_modifiers, before.view_modifiers);
        assert_eq!(after.is_for_processing, before.is_for_processing);
        assert_eq!(after.has_implant, before.has_implant);
    }

    #[test]
    fn all_supported_sop_classes_are_plannable() {
        for sop_class_uid in SUPPORTED_SOP_CLASSES {
            let dcm = file_object_for_sop(sop_class_uid);
            assert!(
                plan_completion(&dcm, &CompletionOptions::default()).supported,
                "{sop_class_uid}"
            );
        }
    }

    #[test]
    fn zero_length_values_are_missing_but_populated_values_are_not_replaced() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::MODALITY,
            VR::CS,
            PrimitiveValue::Empty,
        ));
        dcm.put(DataElement::new(
            tags::ORGAN_EXPOSED,
            VR::CS,
            PrimitiveValue::from("CHEST"),
        ));
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        assert!(plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "Modality"));
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "OrganExposed"));
        assert!(plan
            .issues
            .iter()
            .any(|issue| issue.code == "populated_value_conflict"));
    }

    #[test]
    fn positioner_type_is_not_guessed_from_sop_class() {
        let dcm = minimal_file_object();
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "PositionerType"));

        let mut none_positioner = minimal_file_object();
        none_positioner.put(DataElement::new(
            tags::POSITIONER_TYPE,
            VR::CS,
            PrimitiveValue::from("NONE"),
        ));
        let plan = plan_completion(&none_positioner, &CompletionOptions::default());
        assert!(!plan
            .issues
            .iter()
            .any(|issue| issue.message.contains("PositionerType")));
    }

    #[test]
    fn structural_pixel_values_are_derived_only_from_unambiguous_inputs() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::BITS_STORED,
            VR::US,
            PrimitiveValue::from(12_u16),
        ));
        dcm.put(DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            PrimitiveValue::from("MONOCHROME1"),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(plan
            .additions
            .iter()
            .any(|addition| { addition.keyword == "HighBit" && addition.value == "11" }));
        assert!(plan.additions.iter().any(|addition| {
            addition.keyword == "PresentationLUTShape" && addition.value == "INVERSE"
        }));
    }

    #[test]
    fn per_frame_functional_groups_are_reported_but_not_modified() {
        let mut dcm = file_object_for_sop(uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE);
        let frame_anatomy = InMemDicomObject::from_element_iter([DataElement::new(
            FRAME_LATERALITY,
            VR::CS,
            PrimitiveValue::from("L"),
        )]);
        let frame = InMemDicomObject::from_element_iter([DataElement::new(
            FRAME_ANATOMY_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame_anatomy]),
        )]);
        dcm.put(DataElement::new(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![frame]),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(plan
            .issues
            .iter()
            .any(|issue| issue.code == "per_frame_functional_groups_not_modified"));
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "FrameAnatomySequence"));
    }

    #[test]
    fn empty_per_frame_groups_allow_shared_frame_anatomy_completion() {
        let mut dcm = file_object_for_sop(uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE);
        dcm.put(DataElement::new(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![InMemDicomObject::new_empty()]),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(!plan
            .issues
            .iter()
            .any(|issue| issue.code == "per_frame_functional_groups_not_modified"));
        assert!(plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "FrameAnatomySequence"));
    }

    #[test]
    fn shared_frame_anatomy_accepts_retired_snomed_breast_codes() {
        for scheme in ["SRT", "SNM3", "99SDM"] {
            let mut dcm = file_object_for_sop(uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE);
            let region = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from(scheme),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from("T-04000")),
                DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from("Breast")),
            ]);
            let anatomy = InMemDicomObject::from_element_iter([
                DataElement::new(FRAME_LATERALITY, VR::CS, PrimitiveValue::from("L")),
                DataElement::new(
                    tags::ANATOMIC_REGION_SEQUENCE,
                    VR::SQ,
                    DataSetSequence::from(vec![region]),
                ),
            ]);
            let mut shared = InMemDicomObject::new_empty();
            shared.put(DataElement::new(
                FRAME_ANATOMY_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![anatomy]),
            ));
            dcm.put(DataElement::new(
                SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![shared]),
            ));

            let plan = plan_completion(&dcm, &CompletionOptions::default());

            assert!(
                !plan
                    .issues
                    .iter()
                    .any(|issue| issue.code == "shared_frame_anatomy_conflict"),
                "{scheme}"
            );
            assert!(!plan
                .additions
                .iter()
                .any(|addition| addition.keyword == "FrameAnatomySequence"));
        }
    }

    #[test]
    fn malformed_populated_view_tuple_is_not_completed() {
        let mut dcm = minimal_file_object();
        let malformed = InMemDicomObject::from_element_iter([
            DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from("399162004")),
            DataElement::new(
                CODING_SCHEME_DESIGNATOR,
                VR::SH,
                PrimitiveValue::from("SRT"),
            ),
            DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from("cranio-caudal")),
        ]);
        dcm.put(DataElement::new(
            VIEW_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![malformed]),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(plan
            .issues
            .iter()
            .any(|issue| issue.code == "view_code_not_completable"));
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "ViewCodeSequence"));
    }

    #[test]
    fn canonical_view_serializer_round_trips_every_base_and_modifier() {
        for definition in crate::registry::VIEW_CODE_DEFINITIONS {
            let mut dcm = minimal_file_object();
            dcm.remove_element(VIEW_POSITION);
            ensure_view_code(&mut dcm, definition.view, &BTreeSet::new());
            let descriptor = extract_view_descriptor(&dcm);
            assert_eq!(descriptor.view_position, definition.view);
            assert!(descriptor.conflicts.is_empty());
        }

        for definition in crate::registry::VIEW_MODIFIER_CODE_DEFINITIONS {
            let mut dcm = minimal_file_object();
            let modifiers = BTreeSet::from([definition.modifier]);
            ensure_view_code(&mut dcm, ViewPosition::Cc, &modifiers);
            let descriptor = extract_view_descriptor(&dcm);
            assert_eq!(descriptor.view_position, ViewPosition::Cc);
            assert_eq!(descriptor.modifiers, modifiers);
            let item = dcm
                .get(VIEW_CODE_SEQUENCE)
                .and_then(|element| element.items())
                .and_then(|items| items.first())
                .unwrap();
            let modifier_item = item
                .get(VIEW_MODIFIER_CODE_SEQUENCE)
                .and_then(|element| element.items())
                .and_then(|items| items.first())
                .unwrap();
            assert_eq!(
                get_string_value(modifier_item, CODING_SCHEME_DESIGNATOR).as_deref(),
                Some("SCT")
            );
            assert_eq!(
                get_string_value(modifier_item, CODE_VALUE).as_deref(),
                Some(definition.code_value)
            );
        }
    }

    #[test]
    fn retired_snomed_schemes_are_compatible_with_nested_completion() {
        for scheme in ["SRT", "SNM3", "99SDM"] {
            let mut dcm = minimal_file_object();
            dcm.put(DataElement::new(
                VIEW_POSITION,
                VR::CS,
                PrimitiveValue::from("MLO"),
            ));
            let view = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from(scheme),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from("R-10226")),
                DataElement::new(
                    CODE_MEANING,
                    VR::LO,
                    PrimitiveValue::from("medio-lateral oblique"),
                ),
            ]);
            dcm.put(DataElement::new(
                VIEW_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![view]),
            ));
            let modifier = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from(scheme),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from("R-102D1")),
                DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from("Axillary Tail")),
            ]);
            dcm.put(DataElement::new(
                VIEW_MODIFIER_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![modifier]),
            ));

            let plan = plan_completion(&dcm, &CompletionOptions::default());

            assert!(
                !plan
                    .issues
                    .iter()
                    .any(|issue| issue.code == "view_code_not_completable"),
                "{scheme}"
            );
            assert!(
                plan.additions.iter().any(|addition| {
                    addition.keyword == "ViewCodeSequence"
                        && addition
                            .value
                            .to_ascii_lowercase()
                            .contains("axillary_tail")
                }),
                "{scheme}"
            );
        }
    }

    #[test]
    fn legacy_modifier_view_positions_do_not_conflict_with_coded_base_views() {
        for (view_position, base_code, base_meaning, modifier_code, modifier_meaning) in [
            (
                "AT",
                "R-10226",
                "medio-lateral oblique",
                "R-102D1",
                "Axillary Tail",
            ),
            ("CV", "R-10242", "cranio-caudal", "R-102D2", "Cleavage"),
            (
                "RL",
                "R-10242",
                "cranio-caudal",
                "R-102D3",
                "Rolled Lateral",
            ),
            ("RM", "R-10242", "cranio-caudal", "R-102D4", "Rolled Medial"),
            ("TAN", "R-10242", "cranio-caudal", "R-102C2", "Tangential"),
            (
                "CCM",
                "R-10242",
                "cranio-caudal",
                "R-102D6",
                "Magnification",
            ),
            (
                "MLOM",
                "R-10226",
                "medio-lateral oblique",
                "R-102D6",
                "Magnification",
            ),
        ] {
            let mut dcm = minimal_file_object();
            dcm.put(DataElement::new(
                VIEW_POSITION,
                VR::CS,
                PrimitiveValue::from(view_position),
            ));
            let modifier = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from("SRT"),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from(modifier_code)),
                DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from(modifier_meaning)),
            ]);
            let mut view = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from("SRT"),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from(base_code)),
                DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from(base_meaning)),
            ]);
            view.put(DataElement::new(
                VIEW_MODIFIER_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![modifier]),
            ));
            dcm.put(DataElement::new(
                VIEW_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![view]),
            ));

            let plan = plan_completion(&dcm, &CompletionOptions::default());

            assert!(
                !plan.issues.iter().any(|issue| {
                    issue.code == "populated_value_conflict"
                        && issue.message.starts_with("ViewPosition")
                }),
                "{view_position}"
            );
            assert!(!plan
                .additions
                .iter()
                .any(|addition| addition.keyword == "ViewPosition"));
        }
    }

    #[test]
    fn legacy_view_code_compatibility_accepts_normalized_meanings_and_deprecated_codes() {
        for (view_position, code, meaning) in [
            ("LMO", "R-10230", "latero-medial-oblique"),
            ("XCCM", "Y-X1771", "cranio-caudal exaggerated medially"),
        ] {
            let mut dcm = minimal_file_object();
            dcm.put(DataElement::new(
                VIEW_POSITION,
                VR::CS,
                PrimitiveValue::from(view_position),
            ));
            let view = InMemDicomObject::from_element_iter([
                DataElement::new(
                    CODING_SCHEME_DESIGNATOR,
                    VR::SH,
                    PrimitiveValue::from("SNM3"),
                ),
                DataElement::new(CODE_VALUE, VR::SH, PrimitiveValue::from(code)),
                DataElement::new(CODE_MEANING, VR::LO, PrimitiveValue::from(meaning)),
            ]);
            dcm.put(DataElement::new(
                VIEW_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![view]),
            ));

            let plan = plan_completion(&dcm, &CompletionOptions::default());

            assert!(
                !plan
                    .issues
                    .iter()
                    .any(|issue| issue.code == "view_code_not_completable"),
                "{view_position}"
            );
        }
    }

    #[test]
    fn numeric_primitive_equality_ignores_dicom_text_formatting() {
        assert!(values_equal(VR::DS, "0.000000", "0"));
        assert!(values_equal(VR::DS, "1.0E0", "1"));
        assert!(values_equal(VR::IS, "001", "1"));
        assert!(!values_equal(VR::DS, "1.01", "1"));
    }

    #[test]
    fn noncanonical_rescale_type_is_reported_without_replacement() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::RESCALE_TYPE,
            VR::LO,
            PrimitiveValue::from("PVAL"),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "RescaleType"));
        assert!(plan.issues.iter().any(|issue| {
            issue.code == "populated_value_conflict"
                && issue.message.starts_with("RescaleType")
                && issue.message.contains("PVAL")
        }));
    }

    #[test]
    fn legacy_bilateral_laterality_is_read_without_replacement() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            IMAGE_LATERALITY,
            VR::CS,
            PrimitiveValue::from("BILATERAL"),
        ));
        dcm.put(DataElement::new(
            LATERALITY,
            VR::CS,
            PrimitiveValue::from("B"),
        ));

        let plan = plan_completion(&dcm, &CompletionOptions::default());

        assert!(!plan
            .issues
            .iter()
            .any(|issue| issue.code == "laterality_evidence_conflict"));
        assert!(!plan
            .issues
            .iter()
            .any(|issue| issue.message.starts_with("ImageLaterality")));
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "ImageLaterality"));
        assert_eq!(
            MammogramExtractor::extract_file(&dcm).unwrap().laterality,
            Laterality::Bilateral
        );
    }

    #[test]
    fn signed_instances_are_refused_by_default() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::DIGITAL_SIGNATURES_SEQUENCE,
            VR::SQ,
            DataSetSequence::<InMemDicomObject>::empty(),
        ));
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        assert!(plan.is_blocked());
        assert!(plan
            .issues
            .iter()
            .any(|issue| issue.code == "signed_instance"));
    }

    #[test]
    fn conflicting_laterality_evidence_is_not_written() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            IMAGE_LATERALITY,
            VR::CS,
            PrimitiveValue::from("R"),
        ));
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        assert!(plan
            .issues
            .iter()
            .any(|issue| issue.code == "laterality_evidence_conflict"));
        assert!(!plan
            .additions
            .iter()
            .any(|addition| addition.keyword == "ImageLaterality"));
    }

    #[test]
    fn pixel_data_invariant_uses_bounded_storage() {
        const PIXEL_BYTES: usize = 4 * 1024;
        const MAX_FINGERPRINT_BYTES: usize = 64;

        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![0_u8; PIXEL_BYTES]),
        ));

        let invariant = FileInvariant::capture(&dcm).unwrap();
        let pixel_data = invariant.pixel_data.as_ref().unwrap();

        assert!(
            pixel_data.len() <= MAX_FINGERPRINT_BYTES,
            "Pixel Data invariant retained {} bytes",
            pixel_data.len()
        );
    }

    #[test]
    fn file_completion_preserves_pixel_data_and_is_idempotent() {
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.dcm");
        let output = directory.path().join("output.dcm");
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![1_u8, 2, 3, 4]),
        ));
        dcm.write_to_file(&input).unwrap();

        let report = complete_file(&input, &output, &CompletionFileOptions::default()).unwrap();
        assert!(report.changed);
        let reopened = open_file(&output).unwrap();
        assert_eq!(
            format!("{:?}", reopened.get(tags::PIXEL_DATA).unwrap().value()),
            format!("{:?}", dcm.get(tags::PIXEL_DATA).unwrap().value())
        );
        assert_eq!(
            MammogramExtractor::extract_file(&reopened)
                .unwrap()
                .view_position,
            ViewPosition::Cc
        );
        assert!(validate_dicom_file(
            &output,
            &ValidationOptions {
                profile: ValidationProfile::Extraction,
                ..ValidationOptions::default()
            }
        )
        .is_valid());

        let second = complete_file(&output, &output, &CompletionFileOptions::default()).unwrap();
        assert!(!second.changed);
    }

    #[cfg(unix)]
    #[test]
    fn file_completion_rejects_output_path_aliasing_input() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let input_directory = directory.path().join("input");
        let output_directory = directory.path().join("output");
        fs::create_dir(&input_directory).unwrap();
        symlink(&input_directory, &output_directory).unwrap();
        let input = input_directory.join("image.dcm");
        let output = output_directory.join("image.dcm");
        minimal_file_object().write_to_file(&input).unwrap();
        let original = fs::read(&input).unwrap();

        let result = complete_file(
            &input,
            &output,
            &CompletionFileOptions {
                force: true,
                ..CompletionFileOptions::default()
            },
        );

        assert!(result.is_err());
        assert_eq!(fs::read(input).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn file_completion_rejects_symlinked_input_ancestor() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target_directory = directory.path().join("target");
        let linked_directory = directory.path().join("linked");
        fs::create_dir(&target_directory).unwrap();
        let target_input = target_directory.join("image.dcm");
        minimal_file_object().write_to_file(&target_input).unwrap();
        let original = fs::read(&target_input).unwrap();
        symlink(&target_directory, &linked_directory).unwrap();
        let linked_input = linked_directory.join("image.dcm");

        let result = complete_file(
            &linked_input,
            &linked_input,
            &CompletionFileOptions {
                backup_suffix: Some(".bak".to_string()),
                ..CompletionFileOptions::default()
            },
        );

        assert!(matches!(
            result,
            Err(MammocatError::IoError(error)) if error.kind() == io::ErrorKind::InvalidInput
        ));
        assert_eq!(fs::read(&target_input).unwrap(), original);
        assert!(!target_directory.join("image.dcm.bak").exists());
    }

    #[test]
    fn unchanged_copy_mode_still_verifies_and_copies_exact_bytes() {
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.dcm");
        let completed = directory.path().join("completed.dcm");
        let copied = directory.path().join("copied.dcm");
        minimal_file_object().write_to_file(&input).unwrap();
        complete_file(&input, &completed, &CompletionFileOptions::default()).unwrap();

        let report = complete_file(&completed, &copied, &CompletionFileOptions::default()).unwrap();

        assert!(!report.changed);
        assert_eq!(fs::read(&copied).unwrap(), fs::read(&completed).unwrap());
        assert!(directory.path().read_dir().unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mammofill-")));
    }

    #[test]
    fn atomic_copy_does_not_replace_an_existing_output() {
        const EXISTING_CONTENT: &[u8] = b"existing output";
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.dcm");
        let output = directory.path().join("output.dcm");
        let mut dcm = minimal_file_object();
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        apply_completion_plan_at(&mut dcm, &plan, "20260713120000.000000+0000").unwrap();
        dcm.write_to_file(&input).unwrap();
        fs::write(&output, EXISTING_CONTENT).unwrap();
        let before = FileInvariant::capture(&dcm).unwrap();

        let result = copy_file_atomically(&input, &output, &before);

        assert!(result.is_err());
        assert_eq!(fs::read(&output).unwrap(), EXISTING_CONTENT);
    }

    #[test]
    fn backup_copy_does_not_replace_an_existing_backup() {
        const ORIGINAL_CONTENT: &[u8] = b"original input";
        const EXISTING_BACKUP_CONTENT: &[u8] = b"existing backup";
        let directory = tempdir().unwrap();
        let input = directory.path().join("input.dcm");
        let backup = directory.path().join("input.dcm.bak");
        fs::write(&input, ORIGINAL_CONTENT).unwrap();
        fs::write(&backup, EXISTING_BACKUP_CONTENT).unwrap();

        let result = copy_backup_no_clobber(&input, &backup);

        assert!(result.is_err());
        assert_eq!(fs::read(&backup).unwrap(), EXISTING_BACKUP_CONTENT);
    }

    #[test]
    fn file_completion_preserves_encapsulated_pixel_fragments() {
        const JPEG_LOSSLESS_TRANSFER_SYNTAX: &str = "1.2.840.10008.1.2.4.70";
        let directory = tempdir().unwrap();
        let input = directory.path().join("compressed-input.dcm");
        let output = directory.path().join("compressed-output.dcm");
        let sop_instance_uid = "1.2.826.0.1.3680043.10.543.3";
        let mut object = InMemDicomObject::from_element_iter([
            DataElement::new(
                SOP_CLASS_UID,
                VR::UI,
                PrimitiveValue::from(
                    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                ),
            ),
            DataElement::new(
                SOP_INSTANCE_UID,
                VR::UI,
                PrimitiveValue::from(sop_instance_uid),
            ),
            DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                PrimitiveValue::from("ORIGINAL\\PRIMARY"),
            ),
            DataElement::new(LATERALITY, VR::CS, PrimitiveValue::from("L")),
            DataElement::new(VIEW_POSITION, VR::CS, PrimitiveValue::from("CC")),
        ]);
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            Value::PixelSequence(PixelFragmentSequence::new_fragments(vec![vec![
                0xFF_u8, 0xD8, 0xFF, 0xD9,
            ]])),
        ));
        let dcm = object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(
                        uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                    )
                    .media_storage_sop_instance_uid(sop_instance_uid)
                    .transfer_syntax(JPEG_LOSSLESS_TRANSFER_SYNTAX),
            )
            .unwrap();
        let before_pixel_data = format!("{:?}", dcm.get(tags::PIXEL_DATA).unwrap().value());
        dcm.write_to_file(&input).unwrap();

        complete_file(&input, &output, &CompletionFileOptions::default()).unwrap();

        let reopened = open_file(&output).unwrap();
        assert_eq!(
            reopened.meta().transfer_syntax(),
            JPEG_LOSSLESS_TRANSFER_SYNTAX
        );
        assert_eq!(
            format!("{:?}", reopened.get(tags::PIXEL_DATA).unwrap().value()),
            before_pixel_data
        );
    }

    #[test]
    fn audit_items_are_appended_and_existing_items_are_preserved() {
        let mut dcm = minimal_file_object();
        let existing = InMemDicomObject::from_element_iter([DataElement::new(
            tags::MODIFYING_SYSTEM,
            VR::LO,
            PrimitiveValue::from("previous-tool"),
        )]);
        dcm.put(DataElement::new(
            tags::ORIGINAL_ATTRIBUTES_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![existing]),
        ));
        let plan = plan_completion(&dcm, &CompletionOptions::default());
        apply_completion_plan_at(&mut dcm, &plan, "20260713120000.000000+0000").unwrap();
        let items = dcm
            .get(tags::ORIGINAL_ATTRIBUTES_SEQUENCE)
            .and_then(|element| element.items())
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            get_string_value(&items[0], tags::MODIFYING_SYSTEM).as_deref(),
            Some("previous-tool")
        );
        assert_eq!(
            get_string_value(&items[1], tags::REASON_FOR_THE_ATTRIBUTE_MODIFICATION).as_deref(),
            Some("CORRECT")
        );
    }

    #[test]
    fn signature_stripping_is_recursive_and_audited() {
        let mut dcm = minimal_file_object();
        let nested = InMemDicomObject::from_element_iter([DataElement::new(
            tags::DIGITAL_SIGNATURES_SEQUENCE,
            VR::SQ,
            DataSetSequence::<InMemDicomObject>::empty(),
        )]);
        dcm.put(DataElement::new(
            tags::SOURCE_IMAGE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![nested]),
        ));
        let plan = plan_completion(
            &dcm,
            &CompletionOptions {
                strip_signatures: true,
                ..CompletionOptions::default()
            },
        );
        let report =
            apply_completion_plan_at(&mut dcm, &plan, "20260713120000.000000+0000").unwrap();
        assert_eq!(report.stripped_signature_elements, 1);
        assert!(!contains_signature_structures(&dcm));
        assert!(dcm.get(tags::ORIGINAL_ATTRIBUTES_SEQUENCE).is_some());
    }

    #[test]
    fn heuristic_modifier_requires_opt_in() {
        let mut dcm = minimal_file_object();
        dcm.put(DataElement::new(
            tags::PADDLE_DESCRIPTION,
            VR::LO,
            PrimitiveValue::from("MAG"),
        ));
        let descriptor = extract_view_descriptor(&dcm);
        assert!(descriptor
            .modifiers
            .contains(&MammographyViewModifier::Magnification));
        assert_eq!(
            modifier_confidence(&descriptor, MammographyViewModifier::Magnification),
            Confidence::Heuristic
        );
        let strict = plan_completion(&dcm, &CompletionOptions::default());
        let permissive = plan_completion(
            &dcm,
            &CompletionOptions {
                allow_heuristic: true,
                strip_signatures: false,
            },
        );
        let strict_view = strict
            .additions
            .iter()
            .find(|addition| addition.keyword == "ViewCodeSequence")
            .unwrap();
        let permissive_view = permissive
            .additions
            .iter()
            .find(|addition| addition.keyword == "ViewCodeSequence")
            .unwrap();
        assert!(!strict_view.value.contains("magnification"));
        assert!(
            permissive_view.value.contains("magnification"),
            "{}",
            permissive_view.value
        );
    }

    #[test]
    fn heuristic_base_view_requires_opt_in() {
        let mut dcm = minimal_file_object();
        dcm.remove_element(VIEW_POSITION);
        dcm.put(DataElement::new(
            tags::SERIES_DESCRIPTION,
            VR::LO,
            PrimitiveValue::from("screening left cc view"),
        ));

        let strict = plan_completion(&dcm, &CompletionOptions::default());
        let permissive = plan_completion(
            &dcm,
            &CompletionOptions {
                allow_heuristic: true,
                strip_signatures: false,
            },
        );

        assert!(!strict
            .additions
            .iter()
            .any(|addition| addition.keyword == "ViewCodeSequence"));
        assert!(permissive
            .additions
            .iter()
            .any(|addition| addition.keyword == "ViewCodeSequence"));
    }
}
