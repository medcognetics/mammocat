use clap::Parser;
use dicom_object::open_file;
use log::{error, info};
use mammocat_core::cli::{Cli, OutputFormat};
use mammocat_core::{MammogramExtractor, TextReport};
use std::process;

fn main() {
    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Info)
            .init();
    }

    info!("Reading DICOM file: {}", cli.file.display());

    // Open DICOM file
    let dcm = match open_file(&cli.file) {
        Ok(obj) => obj,
        Err(e) => {
            error!("Failed to read DICOM file: {}", e);
            eprintln!("Error: Failed to read DICOM file: {}", e);
            process::exit(1);
        }
    };

    // Extract metadata
    let metadata = match MammogramExtractor::extract(&dcm) {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to extract metadata: {}", e);
            eprintln!("Error: Failed to extract metadata: {}", e);
            process::exit(1);
        }
    };

    // Output based on format
    match cli.format {
        OutputFormat::Text => {
            let report = TextReport::new(&metadata);
            println!("{}", report);
        }
        OutputFormat::Json => {
            #[cfg(feature = "json")]
            {
                match serde_json::to_string_pretty(&metadata) {
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
