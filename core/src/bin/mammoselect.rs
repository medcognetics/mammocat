use clap::{Parser, ValueEnum};
use log::{error, info, warn};
use mammocat_core::{
    collect_dicom_files, get_preferred_views_filtered_with_study_mode_and_warnings, DbtObjectKind,
    FilterConfig, MammogramRecord, MammogramType, MammogramView, PreferenceOrder,
    PreferredViewSelectionWithWarnings, SelectionWarning, StudySelectionMode, STANDARD_MAMMO_VIEWS,
};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::process;

/// CLI tool for selecting preferred mammogram views from a directory
#[derive(Parser, Debug)]
#[command(name = "mammoselect")]
#[command(about = "Select preferred mammogram views from a directory of DICOM files")]
#[command(version)]
struct Cli {
    /// Directory containing DICOM files
    #[arg(value_name = "DIRECTORY")]
    directory: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Preference ordering strategy for selecting mammogram types
    #[arg(short, long, default_value = "default")]
    preference: PreferenceOrderArg,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Allowed mammogram types (comma-separated: ffdm,tomo,synth,sfm)
    #[arg(long, value_delimiter = ',')]
    allowed_types: Option<Vec<MammogramTypeArg>>,

    /// Allowed DBT object kinds (comma-separated: none,volume,slice,unknown)
    #[arg(long, value_delimiter = ',')]
    allowed_dbt_object_kinds: Option<Vec<DbtObjectKindArg>>,

    /// Exclude views with breast implants
    #[arg(long)]
    exclude_implants: bool,

    /// Only include standard views (CC and MLO)
    #[arg(long)]
    only_standard_views: bool,

    /// Include FOR PROCESSING views (excluded by default)
    #[arg(long)]
    include_for_processing: bool,

    /// Include secondary capture images (excluded by default)
    #[arg(long)]
    include_secondary_capture: bool,

    /// Include non-MG modality (excluded by default)
    #[arg(long)]
    include_non_mg: bool,

    /// Exclude lossy compressed images
    #[arg(long)]
    exclude_lossy: bool,

    /// Do not prefer lossless images over lossy compressed images
    #[arg(long)]
    no_deprioritize_lossy: bool,

    /// Require all selected views to come from a common modality group (2D or DBT)
    #[arg(long)]
    require_common_modality: bool,

    /// Error if usable records contain multiple studies or missing StudyInstanceUID
    #[arg(long)]
    strict: bool,
}

/// Output format options
#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Human-readable text format
    Text,
    /// JSON format
    Json,
    /// File paths only (one per line)
    Paths,
}

/// Preference ordering for mammogram type selection
#[derive(Debug, Clone, ValueEnum)]
enum PreferenceOrderArg {
    /// Default ordering: FFDM > SYNTH > TOMO > SFM (prefers 2D for inference)
    Default,
    /// Tomosynthesis first: TOMO > FFDM > SYNTH > SFM (maximizes 3D imaging)
    TomoFirst,
}

impl From<PreferenceOrderArg> for PreferenceOrder {
    fn from(arg: PreferenceOrderArg) -> Self {
        match arg {
            PreferenceOrderArg::Default => PreferenceOrder::Default,
            PreferenceOrderArg::TomoFirst => PreferenceOrder::TomoFirst,
        }
    }
}

/// Mammogram type argument for filtering
#[derive(Debug, Clone, ValueEnum)]
enum MammogramTypeArg {
    /// Full-field digital mammography
    Ffdm,
    /// Tomosynthesis
    Tomo,
    /// Synthesized 2D from tomosynthesis
    Synth,
    /// Screen-film mammography
    Sfm,
}

impl From<MammogramTypeArg> for MammogramType {
    fn from(arg: MammogramTypeArg) -> Self {
        match arg {
            MammogramTypeArg::Ffdm => MammogramType::Ffdm,
            MammogramTypeArg::Tomo => MammogramType::Tomo,
            MammogramTypeArg::Synth => MammogramType::Synth,
            MammogramTypeArg::Sfm => MammogramType::Sfm,
        }
    }
}

/// DBT object kind argument for filtering
#[derive(Debug, Clone, ValueEnum)]
enum DbtObjectKindArg {
    /// Not a DBT object
    None,
    /// Multi-frame DBT volume object
    Volume,
    /// Single-frame DBT slice object
    Slice,
    /// Ambiguous DBT object kind
    Unknown,
}

impl From<DbtObjectKindArg> for DbtObjectKind {
    fn from(arg: DbtObjectKindArg) -> Self {
        match arg {
            DbtObjectKindArg::None => DbtObjectKind::None,
            DbtObjectKindArg::Volume => DbtObjectKind::Volume,
            DbtObjectKindArg::Slice => DbtObjectKind::Slice,
            DbtObjectKindArg::Unknown => DbtObjectKind::Unknown,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // Setup logging
    setup_logging(cli.verbose);

    // Verify directory exists
    if !cli.directory.is_dir() {
        eprintln!("Error: {} is not a directory", cli.directory.display());
        process::exit(1);
    }

    info!("Processing directory: {}", cli.directory.display());

    let preference_order: PreferenceOrder = cli.preference.clone().into();

    // Collect all .dcm files
    let dicom_files = match collect_dicom_files(&cli.directory) {
        Ok(files) => files,
        Err(e) => {
            error!("Failed to read directory: {}", e);
            eprintln!("Error: Failed to read directory: {}", e);
            process::exit(1);
        }
    };

    if dicom_files.is_empty() {
        eprintln!("Error: No DICOM files (.dcm) found in directory");
        process::exit(1);
    }

    info!("Found {} DICOM files", dicom_files.len());

    // Create records from files
    let mut records = Vec::new();
    for file_path in dicom_files {
        match MammogramRecord::from_file(file_path.clone()) {
            Ok(record) => {
                info!("Processed: {}", file_path.display());
                records.push(record);
            }
            Err(e) => {
                warn!("Skipping {}: {}", file_path.display(), e);
            }
        }
    }

    if records.is_empty() {
        eprintln!("Error: No valid mammogram files could be processed");
        process::exit(1);
    }

    info!("Successfully processed {} files", records.len());

    // Build filter configuration
    let filter_config = build_filter_config(&cli);
    info!("Filter config: {:?}", filter_config);

    info!("Using preference order: {:?}", preference_order);

    // Select preferred views with filtering
    let (selections, warnings) =
        match select_preferred_views(&records, &filter_config, preference_order, cli.strict) {
            Ok(selection_result) => selection_result,
            Err(e) => {
                error!("Selection failed: {}", e);
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        };
    output_selection_warnings(&warnings);
    output_selected_lossy_warnings(&selections, &filter_config);

    // Output results
    output_selections(&selections, cli.format);
}

fn setup_logging(verbose: bool) {
    if verbose {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Info)
            .init();
    }
}

/// Builds FilterConfig from CLI arguments
fn build_filter_config(cli: &Cli) -> FilterConfig {
    let mut config = FilterConfig::default();

    // Handle allowed types (whitelist)
    if let Some(type_args) = &cli.allowed_types {
        let allowed: HashSet<MammogramType> = type_args
            .iter()
            .map(|arg| MammogramType::from(arg.clone()))
            .collect();
        config = config.with_allowed_types(allowed);
    }

    if let Some(kind_args) = &cli.allowed_dbt_object_kinds {
        let allowed: HashSet<DbtObjectKind> = kind_args
            .iter()
            .map(|arg| DbtObjectKind::from(arg.clone()))
            .collect();
        config = config.with_allowed_dbt_object_kinds(allowed);
    }

    // Handle exclude flags
    config = config.exclude_implants(cli.exclude_implants);
    config = config.exclude_non_standard_views(cli.only_standard_views);

    // Handle include flags (inverted logic)
    config = config.exclude_for_processing(!cli.include_for_processing);
    config = config.exclude_secondary_capture(!cli.include_secondary_capture);
    config = config.exclude_non_mg_modality(!cli.include_non_mg);
    config = config.exclude_lossy_compressed(cli.exclude_lossy);
    config = config.deprioritize_lossy_compressed(!cli.no_deprioritize_lossy);
    config = config.require_common_modality(cli.require_common_modality);

    config
}

fn output_selected_lossy_warnings(
    selections: &HashMap<MammogramView, Option<MammogramRecord>>,
    filter_config: &FilterConfig,
) {
    for warning in selected_lossy_warning_messages(selections, filter_config) {
        warn!("{}", warning);
    }
}

fn selected_lossy_warning_messages(
    selections: &HashMap<MammogramView, Option<MammogramRecord>>,
    filter_config: &FilterConfig,
) -> Vec<String> {
    if filter_config.exclude_lossy_compressed {
        return Vec::new();
    }

    STANDARD_MAMMO_VIEWS
        .iter()
        .filter_map(|view| {
            let record = selections.get(view).and_then(Option::as_ref)?;
            if !record.is_lossy_compressed {
                return None;
            }

            let transfer_syntax_uid = record.transfer_syntax_uid.as_deref().unwrap_or("unknown");
            Some(format!(
                "lossy compressed image selected for {view}: {} \
                 (transfer syntax UID: {transfer_syntax_uid}; use --exclude-lossy to remove lossy images)",
                record.file_path.display()
            ))
        })
        .collect()
}

fn select_preferred_views(
    records: &[MammogramRecord],
    filter_config: &FilterConfig,
    preference_order: PreferenceOrder,
    strict: bool,
) -> mammocat_core::Result<PreferredViewSelectionWithWarnings> {
    get_preferred_views_filtered_with_study_mode_and_warnings(
        records,
        filter_config,
        preference_order,
        StudySelectionMode::from_strict(strict),
    )
}

fn output_selection_warnings(warnings: &[SelectionWarning]) {
    for warning in warnings {
        warn!("{}", warning.message());
    }
}

fn output_selections(
    selections: &HashMap<MammogramView, Option<MammogramRecord>>,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Text => {
            let report = TextReport::new(selections);
            println!("{}", report);
        }
        OutputFormat::Paths => {
            output_paths(selections);
        }
        OutputFormat::Json => {
            #[cfg(feature = "json")]
            {
                match output_json(selections) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        error!("Failed to serialize to JSON: {}", e);
                        eprintln!("Error: Failed to serialize to JSON: {}", e);
                        process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "json"))]
            {
                eprintln!("Error: JSON output requires the 'json' feature");
                eprintln!("Rebuild with: cargo build --features json");
                process::exit(1);
            }
        }
    }
}

fn output_paths(selections: &HashMap<MammogramView, Option<MammogramRecord>>) {
    for view in &STANDARD_MAMMO_VIEWS {
        if let Some(Some(record)) = selections.get(view) {
            println!("{}", record.file_path.display());
        }
    }
}

#[cfg(feature = "json")]
fn output_json(
    selections: &HashMap<MammogramView, Option<MammogramRecord>>,
) -> Result<String, serde_json::Error> {
    use serde::Serialize;

    #[derive(Serialize)]
    struct SelectionJson {
        selections: HashMap<String, Option<RecordJson>>,
    }

    #[derive(Serialize)]
    struct RecordJson {
        file_path: String,
        metadata: mammocat_core::MammogramMetadata,
        rows: Option<u16>,
        columns: Option<u16>,
        image_area: Option<u32>,
        transfer_syntax_uid: Option<String>,
        is_lossy_compressed: bool,
        is_implant_displaced: bool,
    }

    let json_selections: HashMap<String, Option<RecordJson>> = selections
        .iter()
        .map(|(view, record)| {
            let key = format!("{}", view);
            let value = record.as_ref().map(|r| RecordJson {
                file_path: r.file_path.display().to_string(),
                metadata: r.metadata.clone(),
                rows: r.rows,
                columns: r.columns,
                image_area: r.image_area(),
                transfer_syntax_uid: r.transfer_syntax_uid.clone(),
                is_lossy_compressed: r.is_lossy_compressed,
                is_implant_displaced: r.is_implant_displaced,
            });
            (key, value)
        })
        .collect();

    let output = SelectionJson {
        selections: json_selections,
    };

    serde_json::to_string_pretty(&output)
}

/// Text report for preferred view selection
struct TextReport<'a> {
    selections: &'a HashMap<MammogramView, Option<MammogramRecord>>,
}

impl<'a> TextReport<'a> {
    fn new(selections: &'a HashMap<MammogramView, Option<MammogramRecord>>) -> Self {
        Self { selections }
    }
}

impl<'a> fmt::Display for TextReport<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Preferred View Selection")?;
        writeln!(f, "========================")?;
        writeln!(f)?;

        for view in &STANDARD_MAMMO_VIEWS {
            write!(f, "{}: ", view)?;

            if let Some(Some(record)) = self.selections.get(view) {
                writeln!(f, "{}", record.file_path.display())?;
                writeln!(
                    f,
                    "  Type: {}",
                    record.metadata.mammogram_type.simple_name()
                )?;
                writeln!(
                    f,
                    "  Manufacturer: {}",
                    record.metadata.manufacturer.as_deref().unwrap_or("unknown")
                )?;
                writeln!(
                    f,
                    "  Model: {}",
                    record.metadata.model.as_deref().unwrap_or("unknown")
                )?;
                writeln!(f, "  Frames: {}", record.metadata.number_of_frames)?;
                if let Some(area) = record.image_area() {
                    writeln!(
                        f,
                        "  Resolution: {}x{} ({} pixels)",
                        record.rows.unwrap(),
                        record.columns.unwrap(),
                        area
                    )?;
                }
                if record.is_implant_displaced {
                    writeln!(f, "  Implant Displaced: yes")?;
                }
                if record.is_lossy_compressed {
                    writeln!(f, "  Lossy Compressed: yes")?;
                }
                if let Some(transfer_syntax_uid) = &record.transfer_syntax_uid {
                    writeln!(f, "  Transfer Syntax UID: {}", transfer_syntax_uid)?;
                }
            } else {
                writeln!(f, "Not found")?;
            }
            writeln!(f)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mammocat_core::{DbtObjectKind, ImageType, Laterality, MammogramMetadata, ViewPosition};
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_cli_test_record(
        laterality: Laterality,
        view_position: ViewPosition,
        mammo_type: MammogramType,
        study_uid: &str,
    ) -> MammogramRecord {
        make_cli_test_record_with_lossy(laterality, view_position, mammo_type, study_uid, false)
    }

    fn make_cli_test_record_with_lossy(
        laterality: Laterality,
        view_position: ViewPosition,
        mammo_type: MammogramType,
        study_uid: &str,
        is_lossy_compressed: bool,
    ) -> MammogramRecord {
        let transfer_syntax_uid = if is_lossy_compressed {
            "1.2.840.10008.1.2.4.50"
        } else {
            "1.2.840.10008.1.2.1"
        };
        MammogramRecord {
            file_path: PathBuf::from(format!("{study_uid}_{laterality:?}_{view_position:?}.dcm")),
            metadata: MammogramMetadata {
                mammogram_type: mammo_type,
                dbt_object_kind: default_dbt_object_kind(mammo_type),
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
                number_of_frames: 1,
                concatenation_uid: None,
                sop_instance_uid_of_concatenation_source: None,
                is_secondary_capture: false,
                modality: Some("MG".to_string()),
                transfer_syntax_uid: Some(transfer_syntax_uid.to_string()),
                transfer_syntax_name: None,
                compression_type: None,
            },
            study_instance_uid: Some(study_uid.to_string()),
            sop_instance_uid: Some(format!(
                "{}.{}.{}",
                study_uid,
                laterality.short_str(),
                view_position.short_str()
            )),
            rows: Some(2560),
            columns: Some(3328),
            transfer_syntax_uid: Some(transfer_syntax_uid.to_string()),
            is_lossy_compressed,
            is_implant_displaced: false,
            is_spot_compression: false,
            is_magnified: false,
            series_instance_uid: Some(format!("{study_uid}.series")),
        }
    }

    fn default_dbt_object_kind(mammo_type: MammogramType) -> DbtObjectKind {
        match mammo_type {
            MammogramType::Tomo => DbtObjectKind::Unknown,
            _ => DbtObjectKind::None,
        }
    }

    fn make_cli_test_record_with_path(
        view: MammogramView,
        file_name: &str,
        is_lossy_compressed: bool,
    ) -> MammogramRecord {
        let mut record = make_cli_test_record_with_lossy(
            view.laterality,
            view.view,
            MammogramType::Ffdm,
            "1.2.826.0.99",
            is_lossy_compressed,
        );
        record.file_path = PathBuf::from(file_name);
        record
    }

    #[test]
    fn test_is_dicom_file_with_valid_header() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_dicom");

        // Create a file with DICOM header
        let mut file = File::create(&file_path).unwrap();

        // Write 128-byte preamble (zeros)
        file.write_all(&[0u8; 128]).unwrap();

        // Write "DICM" magic bytes
        file.write_all(b"DICM").unwrap();

        // Write some additional data
        file.write_all(b"additional data").unwrap();

        assert!(mammocat_core::is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_without_valid_header() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not_dicom");

        // Create a file without DICOM header
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"This is not a DICOM file").unwrap();

        assert!(!mammocat_core::is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_too_small() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("small_file");

        // Create a file smaller than 132 bytes
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"small").unwrap();

        assert!(!mammocat_core::is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_wrong_magic() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("wrong_magic");

        // Create a file with 128-byte preamble but wrong magic
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&[0u8; 128]).unwrap();
        file.write_all(b"NOTM").unwrap(); // Wrong magic

        assert!(!mammocat_core::is_dicom_file(&file_path));
    }

    #[test]
    fn test_collect_dicom_files_with_extensions() {
        let temp_dir = TempDir::new().unwrap();

        // Create files with different extensions
        File::create(temp_dir.path().join("file1.dcm")).unwrap();
        File::create(temp_dir.path().join("file2.DCM")).unwrap(); // uppercase
        File::create(temp_dir.path().join("file3.dicom")).unwrap();
        File::create(temp_dir.path().join("file4.DICOM")).unwrap(); // uppercase
        File::create(temp_dir.path().join("file5.txt")).unwrap();

        let files = collect_dicom_files(&temp_dir.path().to_path_buf()).unwrap();

        // Should find 4 files (.dcm and .dicom, case-insensitive)
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn test_collect_dicom_files_with_headerless() {
        let temp_dir = TempDir::new().unwrap();

        // Create a headerless DICOM file
        let dicom_file = temp_dir.path().join("headerless_dicom");
        let mut file = File::create(&dicom_file).unwrap();
        file.write_all(&[0u8; 128]).unwrap();
        file.write_all(b"DICM").unwrap();

        // Create a headerless non-DICOM file
        let non_dicom = temp_dir.path().join("headerless_other");
        File::create(&non_dicom)
            .unwrap()
            .write_all(b"not dicom")
            .unwrap();

        let files = collect_dicom_files(&temp_dir.path().to_path_buf()).unwrap();

        // Should find only the valid DICOM file
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], dicom_file);
    }

    #[test]
    fn test_build_filter_config_deprioritizes_lossy_by_default() {
        let cli = Cli::try_parse_from(["mammoselect", "/tmp"]).unwrap();
        let config = build_filter_config(&cli);

        assert!(!config.exclude_lossy_compressed);
        assert!(config.deprioritize_lossy_compressed);
    }

    #[test]
    fn test_build_filter_config_excludes_lossy_when_flag_enabled() {
        let cli = Cli::try_parse_from(["mammoselect", "--exclude-lossy", "/tmp"]).unwrap();
        let config = build_filter_config(&cli);

        assert!(config.exclude_lossy_compressed);
        assert!(config.deprioritize_lossy_compressed);
    }

    #[test]
    fn test_build_filter_config_allows_dbt_object_kinds() {
        let cli = Cli::try_parse_from([
            "mammoselect",
            "--allowed-dbt-object-kinds",
            "volume,slice",
            "/tmp",
        ])
        .unwrap();
        let config = build_filter_config(&cli);
        let allowed = config
            .allowed_dbt_object_kinds
            .expect("allowed DBT object kinds");

        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&DbtObjectKind::Volume));
        assert!(allowed.contains(&DbtObjectKind::Slice));
    }

    #[test]
    fn test_build_filter_config_can_disable_lossy_deprioritization() {
        let cli = Cli::try_parse_from(["mammoselect", "--no-deprioritize-lossy", "/tmp"]).unwrap();
        let config = build_filter_config(&cli);

        assert!(!config.exclude_lossy_compressed);
        assert!(!config.deprioritize_lossy_compressed);
    }

    #[test]
    fn test_selected_lossy_warning_messages_warns_when_lossy_selected() {
        let view = MammogramView::new(Laterality::Left, ViewPosition::Mlo);
        let mut selections = HashMap::new();
        selections.insert(
            view,
            Some(make_cli_test_record_with_path(view, "/tmp/lossy.dcm", true)),
        );

        let warnings = selected_lossy_warning_messages(&selections, &FilterConfig::default());

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("lossy compressed image selected"));
        assert!(warnings[0].contains(&view.to_string()));
        assert!(warnings[0].contains("/tmp/lossy.dcm"));
        assert!(warnings[0].contains("--exclude-lossy"));
    }

    #[test]
    fn test_selected_lossy_warning_messages_suppressed_when_lossy_excluded() {
        let view = MammogramView::new(Laterality::Left, ViewPosition::Mlo);
        let mut selections = HashMap::new();
        selections.insert(
            view,
            Some(make_cli_test_record_with_path(view, "/tmp/lossy.dcm", true)),
        );
        let config = FilterConfig::default().exclude_lossy_compressed(true);

        let warnings = selected_lossy_warning_messages(&selections, &config);

        assert!(warnings.is_empty());
    }

    #[test]
    fn test_selected_lossy_warning_messages_ignores_lossless_selected() {
        let view = MammogramView::new(Laterality::Left, ViewPosition::Mlo);
        let mut selections = HashMap::new();
        selections.insert(
            view,
            Some(make_cli_test_record_with_path(
                view,
                "/tmp/lossless.dcm",
                false,
            )),
        );

        let warnings = selected_lossy_warning_messages(&selections, &FilterConfig::default());

        assert!(warnings.is_empty());
    }

    #[test]
    fn test_select_preferred_views_default_uses_most_complete_study() {
        let incomplete_study = "1.2.826.0.10";
        let complete_study = "1.2.826.0.20";
        let records = vec![
            make_cli_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                incomplete_study,
            ),
            make_cli_test_record(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                incomplete_study,
            ),
            make_cli_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                complete_study,
            ),
            make_cli_test_record(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Tomo,
                complete_study,
            ),
            make_cli_test_record(
                Laterality::Left,
                ViewPosition::Cc,
                MammogramType::Tomo,
                complete_study,
            ),
        ];

        let (selections, warnings) = select_preferred_views(
            &records,
            &FilterConfig::permissive(),
            PreferenceOrder::Default,
            false,
        )
        .unwrap();

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message().contains("mixed study input detected"));
        assert!(warnings[0]
            .message()
            .contains("selecting only the most complete study"));
        for record in selections.values().flatten() {
            assert_eq!(record.study_instance_uid.as_deref(), Some(complete_study));
        }
    }

    #[test]
    fn test_select_preferred_views_strict_errors_for_multiple_studies() {
        let records = vec![
            make_cli_test_record(
                Laterality::Left,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                "1.2.826.0.10",
            ),
            make_cli_test_record(
                Laterality::Right,
                ViewPosition::Mlo,
                MammogramType::Ffdm,
                "1.2.826.0.20",
            ),
        ];

        let error = select_preferred_views(
            &records,
            &FilterConfig::permissive(),
            PreferenceOrder::Default,
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains("strict study selection"));
        assert!(error.to_string().contains("1.2.826.0.10"));
        assert!(error.to_string().contains("1.2.826.0.20"));
    }
}
