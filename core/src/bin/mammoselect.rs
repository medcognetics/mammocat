use clap::{Parser, ValueEnum};
use log::{error, info, warn};
use mammocat_core::{
    get_preferred_views_with_order, MammogramRecord, MammogramView, PreferenceOrder,
    STANDARD_MAMMO_VIEWS,
};
use std::collections::HashMap;
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
}

/// Output format options
#[derive(Debug, Clone, ValueEnum)]
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

    // Convert preference order argument to core type
    let preference_order: PreferenceOrder = cli.preference.into();
    info!("Using preference order: {:?}", preference_order);

    // Select preferred views
    let selections = get_preferred_views_with_order(&records, preference_order);

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

fn collect_dicom_files(directory: &PathBuf) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                // Accept .dcm and .dicom extensions
                if ext.eq_ignore_ascii_case("dcm") || ext.eq_ignore_ascii_case("dicom") {
                    files.push(path);
                }
            } else {
                // For files without extension, check for DICOM header
                if is_dicom_file(&path) {
                    info!("Found headerless DICOM file: {}", path.display());
                    files.push(path);
                }
            }
        }
    }

    Ok(files)
}

/// Checks if a file has a DICOM header
///
/// DICOM files typically have:
/// - 128-byte preamble
/// - 4-byte "DICM" magic string at offset 128
///
/// Some DICOM files may not have the preamble and start directly with
/// DICOM data elements, but we primarily check for the standard header.
fn is_dicom_file(path: &PathBuf) -> bool {
    use std::fs::File;
    use std::io::Read;

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    // Read first 132 bytes (128-byte preamble + 4-byte "DICM" magic)
    let mut buffer = [0u8; 132];
    match file.read(&mut buffer) {
        Ok(n) if n >= 132 => {
            // Check for "DICM" magic bytes at offset 128
            &buffer[128..132] == b"DICM"
        }
        _ => false,
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
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

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

        assert!(is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_without_valid_header() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not_dicom");

        // Create a file without DICOM header
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"This is not a DICOM file").unwrap();

        assert!(!is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_too_small() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("small_file");

        // Create a file smaller than 132 bytes
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"small").unwrap();

        assert!(!is_dicom_file(&file_path));
    }

    #[test]
    fn test_is_dicom_file_wrong_magic() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("wrong_magic");

        // Create a file with 128-byte preamble but wrong magic
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&[0u8; 128]).unwrap();
        file.write_all(b"NOTM").unwrap(); // Wrong magic

        assert!(!is_dicom_file(&file_path));
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
}
