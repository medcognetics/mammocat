use clap::{Parser, Subcommand, ValueEnum};
use mammocat_core::{
    convert_dbt_study, scan_dbt_study, DbtConvertOptions, DbtConvertReport, DbtScanOptions,
    DbtScanReport,
};
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(name = "dbt-combine")]
#[command(about = "Check and convert old-format DBT slice series")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output format
    #[arg(long, value_enum, default_value = "text", global = true)]
    format: OutputFormat,

    /// Suppress human-readable output
    #[arg(long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Emit more details in human-readable output
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check whether a study contains old-format DBT series needing conversion
    Check {
        /// Input study directory
        input_dir: PathBuf,
    },
    /// Convert old-format DBT series and copy through other DICOM files
    Convert {
        /// Input study directory
        input_dir: PathBuf,
        /// Output study directory
        output_dir: PathBuf,
        /// Report planned writes without mutating the filesystem
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing output files
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Check { input_dir } => run_check(&cli, input_dir),
        Command::Convert {
            input_dir,
            output_dir,
            dry_run,
            force,
        } => run_convert(&cli, input_dir, output_dir, *dry_run, *force),
    };

    match result {
        Ok(exit_code) => process::exit(exit_code),
        Err(error) => {
            eprintln!("dbt-combine failed: {}", error);
            process::exit(2);
        }
    }
}

fn run_check(cli: &Cli, input_dir: &PathBuf) -> Result<i32, Box<dyn std::error::Error>> {
    let report = scan_dbt_study(input_dir, DbtScanOptions)?;
    match cli.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Text => {
            if !cli.quiet {
                print_scan_report(&report, cli.verbose);
            }
        }
    }

    Ok(if report.summary.conversion_needed_series > 0 {
        1
    } else {
        0
    })
}

fn run_convert(
    cli: &Cli,
    input_dir: &PathBuf,
    output_dir: &PathBuf,
    dry_run: bool,
    force: bool,
) -> Result<i32, Box<dyn std::error::Error>> {
    let report = convert_dbt_study(input_dir, output_dir, DbtConvertOptions { dry_run, force })?;
    match cli.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Text => {
            if !cli.quiet {
                print_convert_report(&report, cli.verbose);
            }
        }
    }

    Ok(0)
}

fn print_scan_report(report: &DbtScanReport, verbose: bool) {
    println!("Input: {}", report.input_path);
    println!(
        "DICOM files: {} of {}",
        report.summary.dicom_files, report.summary.total_files
    );
    println!(
        "Conversion-needed DBT series: {}",
        report.summary.conversion_needed_series
    );
    println!(
        "Already multi-frame DBT series: {}",
        report.summary.already_multiframe_dbt_series
    );
    println!(
        "Copy-through DICOM files: {}",
        report.summary.copy_through_files
    );
    println!("Unsupported series: {}", report.summary.unsupported_series);
    println!("Skipped files: {}", report.summary.skipped_files);

    if verbose {
        print_scan_details(report);
    }
}

fn print_scan_details(report: &DbtScanReport) {
    for series in &report.conversion_needed_series {
        println!(
            "convert: series={} frames={} {}-{}",
            series.series_instance_uid, series.frame_count, series.laterality, series.view_position
        );
    }
    for series in &report.already_multiframe_dbt_series {
        println!(
            "already-dbt: series={} frames={}",
            series.series_instance_uid, series.frame_count
        );
    }
    for unsupported in &report.unsupported_series {
        println!(
            "unsupported: series={} reason={}",
            unsupported
                .series_instance_uid
                .as_deref()
                .unwrap_or("<missing>"),
            unsupported.reason
        );
    }
    for skipped in &report.skipped_files {
        println!("skipped: {} reason={}", skipped.path, skipped.reason);
    }
}

fn print_convert_report(report: &DbtConvertReport, verbose: bool) {
    println!("Input: {}", report.input_path);
    println!("Output: {}", report.output_path);
    println!("Dry run: {}", report.dry_run);
    println!(
        "Converted DBT series: {} of {}",
        report.summary.converted_series, report.summary.conversion_needed_series
    );
    println!("Copied DICOM files: {}", report.summary.copied_files);
    println!("Unsupported series: {}", report.summary.unsupported_series);
    println!("Skipped files: {}", report.summary.skipped_files);

    if verbose {
        for series in &report.converted_series {
            println!(
                "write: {} frames={} from_series={}",
                series.output_path, series.frame_count, series.series_instance_uid
            );
        }
        for copied in &report.copied_files {
            println!("copy: {} -> {}", copied.source_path, copied.output_path);
        }
        for unsupported in &report.unsupported_series {
            println!(
                "unsupported: series={} reason={}",
                unsupported
                    .series_instance_uid
                    .as_deref()
                    .unwrap_or("<missing>"),
                unsupported.reason
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn rejects_unsupported_output_control_options() {
        for option in ["--color", "--no-color", "--progress"] {
            let arguments = if option == "--progress" {
                vec!["dbt-combine", option, "always", "check", "input"]
            } else {
                vec!["dbt-combine", option, "check", "input"]
            };

            assert!(
                Cli::try_parse_from(arguments).is_err(),
                "{option} should not be accepted"
            );
        }
    }

    #[test]
    fn accepts_supported_automation_and_verbosity_options() {
        assert!(Cli::try_parse_from([
            "dbt-combine",
            "--format",
            "json",
            "--quiet",
            "check",
            "input",
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["dbt-combine", "--verbose", "check", "input"]).is_ok());
    }

    #[test]
    fn help_only_advertises_supported_output_controls() {
        let help = Cli::command().render_long_help().to_string();

        for unsupported_option in ["--color", "--no-color", "--progress"] {
            assert!(!help.contains(unsupported_option));
        }
        for supported_option in ["--format", "--quiet", "--verbose"] {
            assert!(help.contains(supported_option));
        }
    }
}
