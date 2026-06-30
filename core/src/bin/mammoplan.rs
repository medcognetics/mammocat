use clap::{Parser, ValueEnum};
use log::{error, info};
use mammocat_core::{
    plan_mammography_collection, DbtCompositionInput, DbtVolumeCandidate, MammographyPlan,
    MammographyPlanConfig, MammographyPlanOptions, MammographyPlanSelection, StudySelectionMode,
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

    /// Prefer synthetic 2D views over FFDM when both are available for the same view.
    #[arg(long = "prefer-synthetic-2d")]
    prefer_synthetic_2d: bool,

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
    let options = options_from_cli(&cli);

    let report = match plan_mammography_collection(&cli.directory, options) {
        Ok(report) => report,
        Err(error) => {
            error!("Planning failed: {}", error);
            eprintln!("mammoplan failed: {error}");
            process::exit(2);
        }
    };

    if let Err(message) = output_plan(&report, &cli.format, cli.verbose) {
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
            .filter_level(log::LevelFilter::Warn)
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

fn options_from_cli(cli: &Cli) -> MammographyPlanOptions {
    MammographyPlanOptions {
        selection: selection_from_cli(cli),
        prefer_synthetic_2d: cli.prefer_synthetic_2d,
        study_selection_mode: StudySelectionMode::from_strict(cli.strict),
    }
}

fn output_plan(plan: &MammographyPlan, format: &OutputFormat, verbose: bool) -> Result<(), String> {
    match format {
        OutputFormat::Text => {
            print_plan_text(plan, verbose);
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

fn print_plan_text(plan: &MammographyPlan, verbose: bool) {
    println!("Mammography Input Plan");
    println!("======================");
    println!();
    println!("Input: {}", plan.input_path);
    println!("Plan inputs: {}", plan_inputs(plan.plan));
    println!("Prefer synthetic 2D: {}", plan.plan.prefer_synthetic_2d);
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
            println!("{}", format_composition_input(series));
        }
        for candidate in &dbt.multiframe_volume_candidates {
            println!("{}", format_volume_candidate(candidate));
        }
    }

    if !plan.warnings.is_empty() {
        println!();
        println!("Warnings");
        println!("--------");
        for warning in warning_lines(&plan.warnings, verbose) {
            println!("{warning}");
        }
    }
}

fn format_composition_input(series: &DbtCompositionInput) -> String {
    format!(
        "compose {}: frames={} sources={} series={}",
        dbt_view_label(Some(&series.laterality), Some(&series.view_position)),
        series.frame_count,
        series.source_paths.len(),
        series.series_instance_uid
    )
}

fn format_volume_candidate(candidate: &DbtVolumeCandidate) -> String {
    format!(
        "volume {}: frames={} sources={} series={}",
        dbt_view_label(
            candidate.laterality.as_deref(),
            candidate.view_position.as_deref()
        ),
        candidate.frame_count,
        candidate.source_paths.len(),
        candidate
            .series_instance_uid
            .as_deref()
            .unwrap_or("<missing>")
    )
}

fn dbt_view_label(laterality: Option<&str>, view_position: Option<&str>) -> String {
    match (
        view_label_component(laterality),
        view_label_component(view_position),
    ) {
        (Some(laterality), Some(view_position)) => format!("{laterality}-{view_position}"),
        (Some(laterality), None) => format!("{laterality}-unknown"),
        (None, Some(view_position)) => format!("unknown-{view_position}"),
        (None, None) => "unknown-view".to_string(),
    }
}

fn view_label_component(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"))
}

fn warning_lines(warnings: &[String], verbose: bool) -> Vec<String> {
    if verbose {
        return warnings.to_vec();
    }

    vec![format!(
        "{} {}; rerun with --verbose to show details.",
        warnings.len(),
        pluralize(warnings.len(), "warning", "warnings")
    )]
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn plan_inputs(selection: MammographyPlanConfig) -> String {
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

    #[test]
    fn options_include_synthetic_2d_preference() {
        let cli = Cli::try_parse_from(["mammoplan", "--prefer-synthetic-2d", "/tmp"]).unwrap();

        assert!(options_from_cli(&cli).prefer_synthetic_2d);
    }

    #[test]
    fn dbt_composition_line_starts_with_view_label() {
        let series = DbtCompositionInput {
            study_instance_uid: "study-1".to_string(),
            series_instance_uid: "series-1".to_string(),
            source_paths: vec!["slice-1.dcm".to_string(), "slice-2.dcm".to_string()],
            relative_parent: "series".to_string(),
            frame_count: 2,
            laterality: "R".to_string(),
            view_position: "CC".to_string(),
            source_modality: "MG".to_string(),
            series_description: None,
            reason: "split_slice_series_needs_composition".to_string(),
        };

        assert_eq!(
            format_composition_input(&series),
            "compose R-CC: frames=2 sources=2 series=series-1"
        );
    }

    #[test]
    fn dbt_volume_line_starts_with_view_label() {
        let candidate = DbtVolumeCandidate {
            study_instance_uid: Some("study-1".to_string()),
            series_instance_uid: Some("series-1".to_string()),
            source_paths: vec!["volume.dcm".to_string()],
            frame_count: 64,
            laterality: Some("L".to_string()),
            view_position: Some("MLO".to_string()),
            reason: "already_multiframe_dbt_series".to_string(),
        };

        assert_eq!(
            format_volume_candidate(&candidate),
            "volume L-MLO: frames=64 sources=1 series=series-1"
        );
    }

    #[test]
    fn dbt_view_label_collapses_unknown_placeholders() {
        assert_eq!(dbt_view_label(None, Some("UNKNOWN")), "unknown-view");
        assert_eq!(dbt_view_label(Some("UNKNOWN"), Some("MLO")), "unknown-MLO");
    }

    #[test]
    fn warning_lines_are_compact_unless_verbose() {
        let warnings = vec![
            "skipping first.dcm: missing pixel data".to_string(),
            "skipping second.dcm: invalid DICOM".to_string(),
        ];

        assert_eq!(
            warning_lines(&warnings, false),
            vec!["2 warnings; rerun with --verbose to show details.".to_string()]
        );
        assert_eq!(warning_lines(&warnings, true), warnings);
    }
}
