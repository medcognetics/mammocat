use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process;
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use mammocat_core::{
    validate_path, CheckStatus, MammogramType, PreferenceOrder, Severity, ValidationOptions,
    ValidationProfile, ValidationReport, ValidationRuntimeError, ValidationStatus,
};

#[path = "shared/filter_config.rs"]
mod filter_config;

const TOOL_NAME: &str = "mammovalidate";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Validation(#[from] ValidationRuntimeError),

    #[error("failed to write output: {0}")]
    WriteOutput(#[from] std::io::Error),

    #[cfg(not(feature = "json"))]
    #[error("JSON output requires the 'json' feature; rebuild with: cargo build --features json")]
    JsonFeature,

    #[cfg(feature = "json")]
    #[error("failed to serialize JSON output: {0}")]
    SerializeJson(#[from] serde_json::Error),
}

#[derive(Parser, Debug)]
#[command(name = TOOL_NAME)]
#[command(about = "Validate DICOM mammography metadata for mammocat and mammoselect")]
#[command(version)]
struct Args {
    /// DICOM file, directory, or ZIP archive to validate
    #[arg(value_name = "SOURCE")]
    source: PathBuf,

    /// Validation strictness profile
    #[arg(long = "profile", value_enum, default_value_t = ProfileArg::Selection)]
    profile: ProfileArg,

    /// Output format
    #[arg(long = "format", short = 'f', value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Color mode for text output
    #[arg(long = "color", value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,

    /// Disable color output
    #[arg(long = "no-color", default_value_t = false)]
    no_color: bool,

    /// Print summary only
    #[arg(
        long = "quiet",
        short = 'q',
        default_value_t = false,
        conflicts_with = "verbose"
    )]
    quiet: bool,

    /// Print detailed checks
    #[arg(
        long = "verbose",
        short = 'v',
        default_value_t = false,
        conflicts_with = "quiet"
    )]
    verbose: bool,

    /// Preference ordering strategy for directory view selection
    #[arg(long = "preference", short = 'p', value_enum, default_value_t = PreferenceOrderArg::Default)]
    preference: PreferenceOrderArg,

    /// Allowed mammogram types for directory readiness, comma-separated
    #[arg(long, value_delimiter = ',')]
    allowed_types: Option<Vec<MammogramTypeArg>>,

    /// Exclude views with breast implants
    #[arg(long)]
    exclude_implants: bool,

    /// Only include standard views when checking directory coverage
    #[arg(long)]
    only_standard_views: bool,

    /// Include FOR PROCESSING views when checking directory coverage
    #[arg(long)]
    include_for_processing: bool,

    /// Include secondary capture images when checking directory coverage
    #[arg(long)]
    include_secondary_capture: bool,

    /// Include non-MG modality when checking directory coverage
    #[arg(long)]
    include_non_mg: bool,

    /// Require all selected views to come from a common modality group
    #[arg(long)]
    require_common_modality: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProfileArg {
    Extraction,
    Selection,
}

impl From<ProfileArg> for ValidationProfile {
    fn from(value: ProfileArg) -> Self {
        match value {
            ProfileArg::Extraction => ValidationProfile::Extraction,
            ProfileArg::Selection => ValidationProfile::Selection,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PreferenceOrderArg {
    Default,
    TomoFirst,
}

impl From<PreferenceOrderArg> for PreferenceOrder {
    fn from(value: PreferenceOrderArg) -> Self {
        match value {
            PreferenceOrderArg::Default => PreferenceOrder::Default,
            PreferenceOrderArg::TomoFirst => PreferenceOrder::TomoFirst,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MammogramTypeArg {
    Ffdm,
    Tomo,
    Synth,
    Sfm,
}

impl From<MammogramTypeArg> for MammogramType {
    fn from(value: MammogramTypeArg) -> Self {
        match value {
            MammogramTypeArg::Ffdm => MammogramType::Ffdm,
            MammogramTypeArg::Tomo => MammogramType::Tomo,
            MammogramTypeArg::Synth => MammogramType::Synth,
            MammogramTypeArg::Sfm => MammogramType::Sfm,
        }
    }
}

fn main() {
    let args = Args::parse();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = execute(args, &mut stdout, &mut stderr);
    process::exit(code);
}

fn execute(args: Args, stdout: &mut impl Write, stderr: &mut impl Write) -> i32 {
    match run(args, stdout) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(stderr, "{TOOL_NAME} failed: {error}");
            2
        }
    }
}

fn run(args: Args, stdout: &mut impl Write) -> Result<i32, Error> {
    let options = build_validation_options(&args);
    let start = Instant::now();
    let report = validate_path(&args.source, &options)?;
    let duration = start.elapsed();

    match args.format {
        OutputFormat::Text => {
            let styles = Styles::new(resolve_color(
                args.format,
                args.color,
                args.no_color,
                std::io::stdout().is_terminal(),
            ));
            render_text_report(stdout, &report, duration, &styles, args.quiet, args.verbose)?;
        }
        OutputFormat::Json => {
            #[cfg(feature = "json")]
            {
                serde_json::to_writer_pretty(&mut *stdout, &report)?;
                writeln!(stdout)?;
            }
            #[cfg(not(feature = "json"))]
            {
                return Err(Error::JsonFeature);
            }
        }
    }

    Ok(if report.is_valid() { 0 } else { 1 })
}

fn build_validation_options(args: &Args) -> ValidationOptions {
    ValidationOptions {
        profile: args.profile.into(),
        filter_config: filter_config::build_filter_config(filter_config::FilterConfigArgs {
            allowed_types: args.allowed_types.as_deref(),
            exclude_implants: args.exclude_implants,
            only_standard_views: args.only_standard_views,
            include_for_processing: args.include_for_processing,
            include_secondary_capture: args.include_secondary_capture,
            include_non_mg: args.include_non_mg,
            require_common_modality: args.require_common_modality,
            exclude_lossy_compressed: false,
            deprioritize_lossy_compressed: true,
        }),
        preference_order: args.preference.into(),
    }
}

fn resolve_color(
    format: OutputFormat,
    color: ColorMode,
    no_color: bool,
    stdout_is_terminal: bool,
) -> bool {
    if no_color || format != OutputFormat::Text {
        return false;
    }
    match color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => stdout_is_terminal,
    }
}

struct Styles {
    enabled: bool,
}

impl Styles {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn status(&self, status: ValidationStatus) -> String {
        match status {
            ValidationStatus::Pass => self.paint("PASS", "1;32"),
            ValidationStatus::Fail => self.paint("FAIL", "1;31"),
        }
    }

    fn check_status(&self, status: CheckStatus) -> String {
        match status {
            CheckStatus::Pass => self.paint("PASS", "1;32"),
            CheckStatus::Fail => self.paint("FAIL", "1;31"),
            CheckStatus::Warn => self.paint("WARN", "1;33"),
            CheckStatus::Info => self.paint("INFO", "36"),
        }
    }

    fn section(&self, text: &str) -> String {
        self.paint(text, "1")
    }

    fn paint(&self, text: &str, code: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

fn render_text_report(
    writer: &mut impl Write,
    report: &ValidationReport,
    duration: Duration,
    styles: &Styles,
    quiet: bool,
    verbose: bool,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}  {TOOL_NAME}  {}  ({})",
        styles.status(report.status),
        report.source.path,
        format_duration(duration)
    )?;
    writeln!(writer)?;
    write_section(
        writer,
        &styles.section("Summary"),
        &[
            ("valid", report.summary.valid.to_string()),
            ("profile", report.summary.profile.clone()),
            ("source", report.summary.source_type.clone()),
            ("files", report.summary.file_count.to_string()),
            ("valid_files", report.summary.valid_file_count.to_string()),
            (
                "invalid_files",
                report.summary.invalid_file_count.to_string(),
            ),
            ("errors", report.summary.error_count.to_string()),
            ("warnings", report.summary.warning_count.to_string()),
            ("info", report.summary.info_count.to_string()),
        ],
    )?;

    if quiet {
        return Ok(());
    }

    if let Some(directory) = &report.directory {
        writeln!(writer)?;
        write_section(
            writer,
            &styles.section("Directory"),
            &[
                ("dicom_files", directory.dicom_file_count.to_string()),
                (
                    "missing_views",
                    if directory.missing_views.is_empty() {
                        "none".to_string()
                    } else {
                        directory.missing_views.join(",")
                    },
                ),
            ],
        )?;
        for view in directory.selected_views.values() {
            let value = view
                .file_path
                .as_ref()
                .map(|path| {
                    format!(
                        "{} ({})",
                        path,
                        view.mammogram_type.as_deref().unwrap_or("unknown")
                    )
                })
                .unwrap_or_else(|| "missing".to_string());
            writeln!(writer, "  {:<8}:  {}", view.view, value)?;
        }
    }

    writeln!(writer)?;
    writeln!(writer, "{}", styles.section("Files"))?;
    for file in &report.files {
        writeln!(
            writer,
            "  {}  {}  errors={} warnings={}",
            styles.status(file.status),
            file.file.path,
            file.summary.error_count,
            file.summary.warning_count
        )?;
    }

    write_messages(writer, styles, "Errors", &report.errors)?;
    write_messages(writer, styles, "Warnings", &report.warnings)?;
    for file in &report.files {
        write_messages(writer, styles, "File Errors", &file.errors)?;
        if verbose {
            write_messages(writer, styles, "File Warnings", &file.warnings)?;
        }
    }

    if verbose {
        writeln!(writer)?;
        writeln!(writer, "{}", styles.section("Checks"))?;
        for check in report
            .checks
            .iter()
            .chain(report.files.iter().flat_map(|file| file.checks.iter()))
        {
            writeln!(
                writer,
                "  {}  {}{}",
                styles.check_status(check.status),
                check.message,
                check
                    .path
                    .as_ref()
                    .map(|path| format!(" ({path})"))
                    .unwrap_or_default()
            )?;
        }
    }

    Ok(())
}

fn write_messages(
    writer: &mut impl Write,
    styles: &Styles,
    title: &str,
    messages: &[mammocat_core::ValidationMessage],
) -> std::io::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "{}", styles.section(title))?;
    for message in messages {
        let status = match message.severity {
            Severity::Critical => styles.check_status(CheckStatus::Fail),
            Severity::Warning => styles.check_status(CheckStatus::Warn),
            Severity::Info => styles.check_status(CheckStatus::Info),
        };
        let path = message
            .path
            .as_ref()
            .map(|path| format!(" [{path}]"))
            .unwrap_or_default();
        writeln!(writer, "  {status}  {}{}", message.message, path)?;
    }
    Ok(())
}

fn write_section(
    writer: &mut impl Write,
    title: &str,
    values: &[(&str, String)],
) -> std::io::Result<()> {
    writeln!(writer, "{title}")?;
    let width = values
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    for (label, value) in values {
        writeln!(writer, "  {label:<width$}:  {value}", width = width)?;
    }
    Ok(())
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn base_args(source: PathBuf) -> Args {
        Args {
            source,
            profile: ProfileArg::Selection,
            format: OutputFormat::Text,
            color: ColorMode::Never,
            no_color: false,
            quiet: true,
            verbose: false,
            preference: PreferenceOrderArg::Default,
            allowed_types: None,
            exclude_implants: false,
            only_standard_views: false,
            include_for_processing: false,
            include_secondary_capture: false,
            include_non_mg: false,
            require_common_modality: false,
        }
    }

    #[test]
    fn color_is_disabled_for_json() {
        assert!(!resolve_color(
            OutputFormat::Json,
            ColorMode::Always,
            false,
            true
        ));
    }

    #[test]
    fn no_color_overrides_always() {
        assert!(!resolve_color(
            OutputFormat::Text,
            ColorMode::Always,
            true,
            true
        ));
    }

    #[test]
    fn invalid_dicom_file_exits_one_with_report() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("invalid.dcm");
        fs::write(&path, b"not a dicom").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = execute(base_args(path), &mut stdout, &mut stderr);

        assert_eq!(code, 1);
        assert!(stderr.is_empty());
        assert!(String::from_utf8(stdout).unwrap().contains("FAIL"));
    }

    #[test]
    fn missing_source_exits_two_on_runtime_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("missing.dcm");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = execute(base_args(path), &mut stdout, &mut stderr);

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("mammovalidate failed"));
    }

    #[cfg(feature = "json")]
    #[test]
    fn json_output_is_parseable_and_uncolored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("invalid.dcm");
        fs::write(&path, b"not a dicom").unwrap();
        let mut args = base_args(path);
        args.format = OutputFormat::Json;
        args.color = ColorMode::Always;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = execute(args, &mut stdout, &mut stderr);

        assert_eq!(code, 1);
        assert!(stderr.is_empty());
        let output = String::from_utf8(stdout).unwrap();
        assert!(!output.contains("\u{1b}["));
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["status"], "fail");
    }
}
