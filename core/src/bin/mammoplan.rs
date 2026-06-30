use clap::{Parser, ValueEnum};
use log::{error, info};
use mammocat_core::{
    plan_mammography_collection, MammographyPlan, MammographyPlanOptions, MammographyPlanSelection,
    PreferenceOrder, StudySelectionMode,
};
use std::path::PathBuf;
use std::process;

/// CLI tool for planning mammography inputs from a DICOM directory.
#[derive(Parser, Debug)]
#[command(name = "mammoplan")]
#[command(about = "Plan 2D mammography view and DBT inputs from a DICOM directory")]
#[command(version)]
struct Cli {
    /// Directory containing DICOM files
    #[arg(value_name = "DIRECTORY")]
    directory: PathBuf,

    /// Output format
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Include selected 2D mammography views. If no include flags are set, all input groups are included.
    #[arg(long = "include-2d")]
    include_2d: bool,

    /// Include DBT composition inputs and volume candidates. If no include flags are set, all input groups are included.
    #[arg(long = "include-dbt")]
    include_dbt: bool,

    /// Preference ordering strategy for selecting 2D mammography views
    #[arg(short, long, default_value = "default")]
    preference: PreferenceOrderArg,

    /// Error if usable 2D records contain multiple studies or missing StudyInstanceUID
    #[arg(long)]
    strict: bool,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// Output format options.
#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Human-readable text format
    Text,
    /// JSON format
    Json,
}

/// Preference ordering for 2D mammography view selection.
#[derive(Debug, Clone, ValueEnum)]
enum PreferenceOrderArg {
    /// Default ordering: FFDM > SYNTH > TOMO > SFM
    Default,
    /// Tomosynthesis first: TOMO > FFDM > SYNTH > SFM
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
    setup_logging(cli.verbose);

    if !cli.directory.is_dir() {
        eprintln!(
            "mammoplan failed: {} is not a directory",
            cli.directory.display()
        );
        process::exit(2);
    }

    info!("Planning directory: {}", cli.directory.display());
    let options = MammographyPlanOptions {
        selection: selection_from_cli(&cli),
        preference_order: cli.preference.clone().into(),
        study_selection_mode: StudySelectionMode::from_strict(cli.strict),
    };

    let report = match plan_mammography_collection(&cli.directory, options) {
        Ok(report) => report,
        Err(error) => {
            error!("Planning failed: {}", error);
            eprintln!("mammoplan failed: {error}");
            process::exit(2);
        }
    };

    if let Err(message) = output_plan(&report, &cli.format) {
        eprintln!("mammoplan failed: {message}");
        process::exit(2);
    }
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

fn selection_from_cli(cli: &Cli) -> MammographyPlanSelection {
    if cli.include_2d || cli.include_dbt {
        MammographyPlanSelection::new(cli.include_2d, cli.include_dbt)
    } else {
        MammographyPlanSelection::all()
    }
}

fn output_plan(plan: &MammographyPlan, format: &OutputFormat) -> Result<(), String> {
    match format {
        OutputFormat::Text => {
            print_plan_text(plan);
            Ok(())
        }
        OutputFormat::Json => {
            #[cfg(feature = "json")]
            {
                let json = serde_json::to_string_pretty(plan)
                    .map_err(|error| format!("failed to serialize plan JSON: {error}"))?;
                println!("{json}");
                Ok(())
            }
            #[cfg(not(feature = "json"))]
            {
                Err("JSON output requires the 'json' feature; rebuild with: cargo build --features json".to_string())
            }
        }
    }
}

fn print_plan_text(plan: &MammographyPlan) {
    println!("Mammography Input Plan");
    println!("======================");
    println!();
    println!("Input: {}", plan.input_path);
    println!("Plan inputs: {}", plan_inputs(plan.plan));
    println!("DICOM files: {}", plan.summary.input_dicom_files);
    println!("Mammogram records: {}", plan.summary.mammogram_records);
    println!("Selected views: {}", plan.summary.views_selected);
    println!(
        "DBT composition inputs: {}",
        plan.summary.dbt_composition_inputs
    );
    println!(
        "DBT volume candidates: {}",
        plan.summary.dbt_multiframe_volume_candidates
    );

    if let Some(views) = &plan.views {
        println!();
        println!("Views");
        println!("-----");
        for selection in views.selected_views.values() {
            let source = selection.source_path.as_deref().unwrap_or("not found");
            println!("{}: {}", selection.view, source);
        }
    }

    if let Some(dbt) = &plan.dbt {
        println!();
        println!("DBT");
        println!("---");
        for series in &dbt.composition_inputs {
            println!(
                "compose: series={} frames={} sources={}",
                series.series_instance_uid,
                series.frame_count,
                series.source_paths.len()
            );
        }
        for candidate in &dbt.multiframe_volume_candidates {
            println!(
                "volume: series={} frames={} sources={}",
                candidate
                    .series_instance_uid
                    .as_deref()
                    .unwrap_or("<missing>"),
                candidate.frame_count,
                candidate.source_paths.len()
            );
        }
    }

    if !plan.warnings.is_empty() {
        println!();
        println!("Warnings");
        println!("--------");
        for warning in &plan.warnings {
            println!("{warning}");
        }
    }
}

fn plan_inputs(selection: MammographyPlanSelection) -> String {
    let mut inputs = Vec::new();
    if selection.include_2d {
        inputs.push("2d");
    }
    if selection.include_dbt {
        inputs.push("dbt");
    }
    inputs.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_defaults_to_all_inputs() {
        let cli = Cli::try_parse_from(["mammoplan", "/tmp"]).unwrap();

        assert_eq!(selection_from_cli(&cli), MammographyPlanSelection::all());
    }

    #[test]
    fn selection_uses_explicit_include_flags_when_any_are_present() {
        let cli = Cli::try_parse_from(["mammoplan", "--include-dbt", "/tmp"]).unwrap();

        assert_eq!(
            selection_from_cli(&cli),
            MammographyPlanSelection::dbt_only()
        );
    }
}
