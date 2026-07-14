use std::fs;
use std::io::{self, IsTerminal};
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use mammocat_core::{
    collect_dicom_files_recursively_no_symlinks, complete_file, ensure_no_symlink_components,
    plan_completion, CompletionFileOptions, CompletionOptions, CompletionPlan, CompletionReport,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "mammofill")]
#[command(about = "Fill missing canonical mammography DICOM metadata")]
#[command(version)]
struct Cli {
    /// DICOM file or directory to inspect
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output file or directory for copy mode
    #[arg(value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Replace each input file atomically after verification
    #[arg(long, conflicts_with = "dry_run")]
    in_place: bool,

    /// Report proposed changes without writing files
    #[arg(long)]
    dry_run: bool,

    /// Permit conflict-free heuristic evidence to produce writes
    #[arg(long)]
    allow_heuristic: bool,

    /// Remove signature and MAC structures before writing
    #[arg(long)]
    strip_signatures: bool,

    /// Preserve each original input with this filename suffix
    #[arg(long, requires = "in_place")]
    backup_suffix: Option<String>,

    /// Overwrite existing copy-mode outputs
    #[arg(long)]
    force: bool,

    /// Primary report format written to stdout
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Suppress per-file text report details
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,

    /// Include evidence and inferred-only values in text reports
    #[arg(short, long, conflicts_with = "quiet")]
    verbose: bool,

    /// Control ANSI color in text reports
    #[arg(long, default_value = "auto")]
    color: ColorMode,

    /// Disable ANSI color in text reports
    #[arg(long)]
    no_color: bool,

    /// Control progress output on stderr
    #[arg(long, default_value = "auto")]
    progress: ProgressMode,
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
enum ProgressMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Serialize)]
struct MammofillReport {
    schema_version: u32,
    input: String,
    mode: String,
    summary: ReportSummary,
    files: Vec<FileReport>,
}

#[derive(Debug, Default, Serialize)]
struct ReportSummary {
    discovered: usize,
    changed: usize,
    unchanged: usize,
    issues: usize,
    runtime_errors: usize,
}

#[derive(Debug, Serialize)]
struct FileReport {
    input: String,
    output: Option<String>,
    status: String,
    applied: bool,
    changed: bool,
    supported: bool,
    additions: Vec<mammocat_core::FieldAddition>,
    inferred_only: Vec<mammocat_core::InferredValue>,
    issues: Vec<mammocat_core::CompletionIssue>,
    error: Option<String>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("mammofill failed: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> std::result::Result<u8, String> {
    validate_arguments(&cli)?;
    let inputs = collect_inputs(&cli.input).map_err(|error| error.to_string())?;
    if inputs.is_empty() {
        return Err(format!(
            "no DICOM files found under {}",
            cli.input.display()
        ));
    }
    let progress = progress_bar(inputs.len(), &cli);
    let completion = CompletionOptions {
        allow_heuristic: cli.allow_heuristic,
        strip_signatures: cli.strip_signatures,
    };
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        progress.set_message(input.display().to_string());
        let output = output_for(&cli, &input)?;
        let report = if cli.dry_run {
            preview_file(&input, &completion)
        } else {
            process_file(&input, &output, &cli, &completion)
        };
        if let Some(error) = &report.error {
            eprintln!("mammofill failed for {}: {error}", input.display());
        }
        files.push(report);
        progress.inc(1);
    }
    progress.finish_and_clear();

    let summary = summarize(&files);
    let report = MammofillReport {
        schema_version: 1,
        input: cli.input.display().to_string(),
        mode: if cli.dry_run {
            "dry-run"
        } else if cli.in_place {
            "in-place"
        } else {
            "copy"
        }
        .to_string(),
        summary,
        files,
    };
    match cli.format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        ),
        OutputFormat::Text => print_text_report(&report, &cli),
    }
    Ok(exit_code(&report))
}

fn validate_arguments(cli: &Cli) -> std::result::Result<(), String> {
    let input_metadata = fs::symlink_metadata(&cli.input).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("input does not exist: {}", cli.input.display())
        } else {
            format!("cannot inspect input {}: {error}", cli.input.display())
        }
    })?;
    ensure_no_symlink_components(&cli.input)
        .map_err(|error| format!("invalid INPUT {}: {error}", cli.input.display()))?;
    if cli
        .backup_suffix
        .as_deref()
        .is_some_and(|suffix| suffix.is_empty() || suffix.contains('/') || suffix.contains('\\'))
    {
        return Err("--backup-suffix must be a non-empty filename suffix".to_string());
    }
    match (cli.in_place, cli.dry_run, cli.output.as_ref()) {
        (true, _, Some(_)) => return Err("OUTPUT cannot be supplied with --in-place".to_string()),
        (false, true, Some(_)) => {
            return Err("OUTPUT cannot be supplied with --dry-run".to_string())
        }
        (false, false, None) => {
            return Err("OUTPUT is required unless --in-place or --dry-run is used".to_string())
        }
        _ => {}
    }
    if !cli.in_place && !cli.dry_run {
        let input = fs::canonicalize(&cli.input)
            .map_err(|error| format!("cannot resolve INPUT {}: {error}", cli.input.display()))?;
        let output = resolve_existing_ancestors(cli.output.as_ref().expect("validated output"))?;
        if input == output {
            return Err("INPUT and OUTPUT are the same path; use --in-place".to_string());
        }
        if input_metadata.is_dir() && output.starts_with(&input) {
            return Err("directory OUTPUT cannot resolve inside INPUT".to_string());
        }
    }
    Ok(())
}

fn collect_inputs(input: &Path) -> io::Result<Vec<PathBuf>> {
    ensure_no_symlink_components(input)?;
    let metadata = fs::symlink_metadata(input)?;
    if metadata.is_file() {
        Ok(vec![input.to_path_buf()])
    } else if metadata.is_dir() {
        collect_dicom_files_recursively_no_symlinks(input)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "INPUT is not a regular file or directory: {}",
                input.display()
            ),
        ))
    }
}

fn output_for(cli: &Cli, input: &Path) -> std::result::Result<PathBuf, String> {
    if cli.in_place || cli.dry_run {
        return Ok(input.to_path_buf());
    }
    let root = cli.output.as_ref().expect("validated output");
    if cli.input.is_file() {
        return Ok(root.clone());
    }
    let relative = input
        .strip_prefix(&cli.input)
        .map_err(|error| error.to_string())?;
    Ok(root.join(relative))
}

fn preview_file(input: &Path, options: &CompletionOptions) -> FileReport {
    match dicom_object::open_file(input) {
        Ok(dcm) => file_report_from_plan(input, None, plan_completion(&dcm, options)),
        Err(error) => runtime_error_report(input, None, error.to_string()),
    }
}

fn process_file(
    input: &Path,
    output: &Path,
    cli: &Cli,
    completion: &CompletionOptions,
) -> FileReport {
    let options = CompletionFileOptions {
        completion: completion.clone(),
        force: cli.force,
        backup_suffix: cli.backup_suffix.clone(),
    };
    match complete_file(input, output, &options) {
        Ok(report) => file_report_from_completion(input, Some(output), report),
        Err(error) => runtime_error_report(input, Some(output), error.to_string()),
    }
}

fn file_report_from_plan(input: &Path, output: Option<&Path>, plan: CompletionPlan) -> FileReport {
    let changed = plan.has_changes();
    let status = if plan.is_blocked() {
        "blocked"
    } else if changed {
        "planned"
    } else if plan.issues.is_empty() {
        "unchanged"
    } else {
        "issues"
    };
    FileReport {
        input: input.display().to_string(),
        output: output.map(|path| path.display().to_string()),
        status: status.to_string(),
        applied: false,
        changed,
        supported: plan.supported,
        additions: plan.additions,
        inferred_only: plan.inferred_only,
        issues: plan.issues,
        error: None,
    }
}

fn file_report_from_completion(
    input: &Path,
    output: Option<&Path>,
    report: CompletionReport,
) -> FileReport {
    let status = if !report.applied {
        "blocked"
    } else if report.changed {
        "changed"
    } else if report.issues.is_empty() {
        "unchanged"
    } else {
        "issues"
    };
    FileReport {
        input: input.display().to_string(),
        output: output.map(|path| path.display().to_string()),
        status: status.to_string(),
        applied: report.applied,
        changed: report.changed,
        supported: report.supported,
        additions: report.additions,
        inferred_only: report.inferred_only,
        issues: report.issues,
        error: None,
    }
}

fn runtime_error_report(input: &Path, output: Option<&Path>, error: String) -> FileReport {
    FileReport {
        input: input.display().to_string(),
        output: output.map(|path| path.display().to_string()),
        status: "error".to_string(),
        applied: false,
        changed: false,
        supported: false,
        additions: Vec::new(),
        inferred_only: Vec::new(),
        issues: Vec::new(),
        error: Some(error),
    }
}

fn summarize(files: &[FileReport]) -> ReportSummary {
    ReportSummary {
        discovered: files.len(),
        changed: files.iter().filter(|file| file.changed).count(),
        unchanged: files
            .iter()
            .filter(|file| !file.changed && file.error.is_none() && file.issues.is_empty())
            .count(),
        issues: files.iter().map(|file| file.issues.len()).sum(),
        runtime_errors: files.iter().filter(|file| file.error.is_some()).count(),
    }
}

fn exit_code(report: &MammofillReport) -> u8 {
    if report.summary.runtime_errors > 0 {
        2
    } else if report.summary.issues > 0 {
        1
    } else {
        0
    }
}

fn print_text_report(report: &MammofillReport, cli: &Cli) {
    let color = resolve_color(cli);
    let status = if report.summary.runtime_errors > 0 {
        style("ERROR", "1;31", color)
    } else if report.summary.issues > 0 {
        style("WARN", "1;33", color)
    } else {
        style("OK", "1;32", color)
    };
    println!("{status}  mammofill  {}", report.input);
    println!();
    println!("Summary");
    println!("  Discovered:      {}", report.summary.discovered);
    println!("  Changed/planned: {}", report.summary.changed);
    println!("  Unchanged:       {}", report.summary.unchanged);
    println!("  Issues:          {}", report.summary.issues);
    println!("  Runtime errors:  {}", report.summary.runtime_errors);
    if cli.quiet {
        return;
    }
    for file in &report.files {
        println!();
        println!("{}  {}", file.status.to_ascii_uppercase(), file.input);
        for addition in &file.additions {
            println!(
                "  add {} {} = {} ({:?})",
                addition.tag, addition.keyword, addition.value, addition.confidence
            );
            if cli.verbose {
                for evidence in &addition.evidence {
                    println!("    evidence: {evidence}");
                }
            }
        }
        for issue in &file.issues {
            println!("  issue {}: {}", issue.code, issue.message);
        }
        if cli.verbose {
            for inferred in &file.inferred_only {
                println!("  inferred {} = {}", inferred.name, inferred.value);
            }
        }
        if let Some(error) = &file.error {
            println!("  error: {error}");
        }
    }
}

fn progress_bar(length: usize, cli: &Cli) -> ProgressBar {
    let enabled = match cli.progress {
        ProgressMode::Always => true,
        ProgressMode::Never => false,
        ProgressMode::Auto => io::stderr().is_terminal(),
    };
    if !enabled {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::with_draw_target(Some(length as u64), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("Processing [{bar:30.cyan/blue}] {pos}/{len} {msg}")
            .expect("valid progress template"),
    );
    bar
}

fn resolve_color(cli: &Cli) -> bool {
    if cli.no_color || cli.format != OutputFormat::Text {
        return false;
    }
    match cli.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal(),
    }
}

fn style(value: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

fn absolute_lexical(path: &Path) -> std::result::Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| error.to_string())?
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn resolve_existing_ancestors(path: &Path) -> std::result::Result<PathBuf, String> {
    let absolute = absolute_lexical(path)?;
    let mut existing = absolute.clone();
    let mut missing_components = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let component = existing.file_name().ok_or_else(|| {
                    format!("cannot find an existing ancestor for {}", path.display())
                })?;
                missing_components.push(component.to_os_string());
                if !existing.pop() {
                    return Err(format!(
                        "cannot find an existing ancestor for {}",
                        path.display()
                    ));
                }
            }
            Err(error) => return Err(format!("cannot inspect OUTPUT {}: {error}", path.display())),
        }
    }
    let mut resolved = fs::canonicalize(&existing)
        .map_err(|error| format!("cannot resolve OUTPUT {}: {error}", path.display()))?;
    for component in missing_components.into_iter().rev() {
        resolved.push(component);
    }
    absolute_lexical(&resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn exit_codes_distinguish_issues_and_runtime_errors() {
        let mut report = MammofillReport {
            schema_version: 1,
            input: "x".to_string(),
            mode: "dry-run".to_string(),
            summary: ReportSummary::default(),
            files: Vec::new(),
        };
        assert_eq!(exit_code(&report), 0);
        report.summary.issues = 1;
        assert_eq!(exit_code(&report), 1);
        report.summary.runtime_errors = 1;
        assert_eq!(exit_code(&report), 2);
    }

    #[test]
    fn command_shapes_parse_as_documented() {
        let copy = Cli::try_parse_from(["mammofill", "input", "output"]).unwrap();
        assert_eq!(copy.output.as_deref(), Some(Path::new("output")));

        let in_place = Cli::try_parse_from(["mammofill", "--in-place", "input"]).unwrap();
        assert!(in_place.in_place);

        let dry_run = Cli::try_parse_from(["mammofill", "--dry-run", "input"]).unwrap();
        assert!(dry_run.dry_run);

        assert!(Cli::try_parse_from(["mammofill", "--in-place", "--dry-run", "input"]).is_err());
        assert!(
            Cli::try_parse_from(["mammofill", "--backup-suffix", ".bak", "input", "output"])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_mode_rejects_output_directory_aliasing_input() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let input = directory.path().join("input");
        let output = directory.path().join("output");
        std::fs::create_dir(&input).unwrap();
        symlink(&input, &output).unwrap();
        let cli = Cli {
            input,
            output: Some(output),
            in_place: false,
            dry_run: false,
            allow_heuristic: false,
            strip_signatures: false,
            backup_suffix: None,
            force: true,
            format: OutputFormat::Text,
            quiet: false,
            verbose: false,
            color: ColorMode::Never,
            no_color: false,
            progress: ProgressMode::Never,
        };

        let error = validate_arguments(&cli).unwrap_err();

        assert!(error.contains("OUTPUT"), "{error}");
        assert!(error.contains("INPUT"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn explicit_symlink_input_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target = directory.path().join("target");
        let input = directory.path().join("input-link");
        std::fs::create_dir(&target).unwrap();
        symlink(target, &input).unwrap();
        let cli = Cli {
            input,
            output: None,
            in_place: false,
            dry_run: true,
            allow_heuristic: false,
            strip_signatures: false,
            backup_suffix: None,
            force: false,
            format: OutputFormat::Text,
            quiet: false,
            verbose: false,
            color: ColorMode::Never,
            no_color: false,
            progress: ProgressMode::Never,
        };

        let error = validate_arguments(&cli).unwrap_err();

        assert!(error.contains("symbolic link"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn explicit_input_with_symlinked_ancestor_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target_directory = directory.path().join("target");
        let linked_directory = directory.path().join("linked");
        std::fs::create_dir(&target_directory).unwrap();
        std::fs::write(target_directory.join("image.dcm"), b"synthetic DICOM").unwrap();
        symlink(&target_directory, &linked_directory).unwrap();
        let cli = Cli {
            input: linked_directory.join("image.dcm"),
            output: None,
            in_place: false,
            dry_run: true,
            allow_heuristic: false,
            strip_signatures: false,
            backup_suffix: None,
            force: false,
            format: OutputFormat::Text,
            quiet: false,
            verbose: false,
            color: ColorMode::Never,
            no_color: false,
            progress: ProgressMode::Never,
        };

        let error = validate_arguments(&cli).unwrap_err();

        assert!(error.contains("symbolic link"), "{error}");
    }
}
