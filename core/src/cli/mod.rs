pub mod report;

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Command-line arguments for mammocat
#[derive(Parser, Debug)]
#[command(name = "mammocat")]
#[command(about = "DICOM mammography metadata extraction tool")]
#[command(version)]
pub struct Cli {
    /// Path to DICOM file
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "text")]
    pub format: OutputFormat,

    /// Verbose logging
    #[arg(short, long)]
    pub verbose: bool,
}

/// Output format options
#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text format
    Text,
    /// JSON format
    Json,
}
