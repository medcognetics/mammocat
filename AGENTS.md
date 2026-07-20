# Repository Guidelines

## Project Overview

Mammocat is a Rust library and CLI suite for extracting, validating, selecting, planning, and conservatively completing mammography DICOM metadata. It is a port of the Python `dicom-utils` library, focusing on performance and type safety while maintaining behavioral compatibility where the DICOM standard does not require a correction.

## Common Commands

### Development Setup
```bash
# Install Python development dependencies
make dev

# Install Node development dependencies
make node-install
```

### Building
```bash
# Build Python bindings (debug)
make build

# Build Python bindings (release, optimized)
make build-release

# Build Rust CLI binaries (standalone, no Python)
cargo build --release

# Build Node/TypeScript bindings
make node-build
```

### Testing
```bash
# Run all tests (Rust + Python, rebuilds bindings automatically)
make test

# Run Python tests only (rebuilds bindings first)
make test-python

# Run Rust tests only
make test-rust

# Run Python tests with coverage
make test-cov

# Run Node/TypeScript binding tests
make node-test

# Test install and clean reinstall from an exact Git commit
make node-test-git-install

# Run specific Rust test
cargo test test_name

# Run Rust tests in a specific module
cargo test laterality
```

### Code Quality
```bash
# Format both Rust and Python code
make format

# Check formatting for both languages (CI-friendly)
make format-check

# Lint both Rust and Python
make lint

# Auto-fix linting issues in both languages
make lint-fix

# Run type checking (cargo check + basedpyright)
make typecheck

# Run all quality checks (format, lint, typecheck)
make quality

# Auto-fix all quality issues
make quality-fix

# Type-check and package-check Node bindings
make node-typecheck
make node-pack

# Build and smoke-test release Rust, Python, and Node outputs
make verify-production

# Generate scheduled dependency-health reports
make security-audit
make deprecation-report
```

### Continuous Integration

- Normal Rust development and CI use Rust 1.97.1 from `rust-toolchain.toml`; Rust 1.88 is
  the workspace MSRV and is checked weekly.
- Python support starts at 3.10. CI tests 3.10 and 3.14. Set
  `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` for Python 3.14 while the project uses PyO3 0.22.
- Node support starts at 22. CI tests Node 22.23.0 and 26.5.0, plus all four native targets.
- Trusted pull requests, `master` pushes, and Linux schedules use the ephemeral Beryl runner.
  Fork pull requests use `ubuntu-24.04`, never receive Beryl access, and cannot read or write
  dependency caches.
- Beryl must run Actions Runner 2.327.1 or newer and belong to a Mammocat-restricted runner
  group with `self-hosted`, `linux`, `x64`, and `beryl` labels.
- The GitHub Actions pull-request gate jobs are `CI / linux-python-min` and
  `CI / linux-full`. Verify the trusted and fork pull-request paths before making those checks
  required in branch protection.
- Weekly security findings fail by design and must not be ignored. Deprecation findings are
  informational; missing, failed, or unparsable report inputs fail the reporting job.

### Running the CLI

#### mammocat - Metadata Extraction
```bash
# After building
./target/release/mammocat path/to/mammogram.dcm

# With verbose logging
./target/release/mammocat --verbose path/to/file.dcm

# JSON output (requires json feature)
./target/release/mammocat --format json path/to/file.dcm
```

#### mammoselect - Preferred View Selection
```bash
# Select preferred views from a directory (uses default ordering)
./target/release/mammoselect /path/to/dicom_directory

# Use different preference ordering
./target/release/mammoselect --preference default /path/to/directory
./target/release/mammoselect --preference tomo-first /path/to/directory

# Output as JSON (requires json feature)
./target/release/mammoselect --format json /path/to/directory

# Output file paths only
./target/release/mammoselect --format paths /path/to/directory

# Error if usable candidates contain multiple studies or missing StudyInstanceUID
./target/release/mammoselect --strict /path/to/directory

# Verbose logging
./target/release/mammoselect --verbose /path/to/directory

# Filtering options
./target/release/mammoselect --allowed-types ffdm /path/to/directory
./target/release/mammoselect --allowed-types ffdm,tomo --exclude-implants /path/to/directory
./target/release/mammoselect --only-standard-views /path/to/directory
./target/release/mammoselect --include-for-processing /path/to/directory
./target/release/mammoselect --include-secondary-capture /path/to/directory
```

#### mammofill - Canonical Metadata Completion
```bash
# Preview without writing
./target/release/mammofill --dry-run /path/to/file-or-directory

# Copy one file or recursively mirror DICOM files
./target/release/mammofill input.dcm output.dcm
./target/release/mammofill /path/to/input /path/to/output

# Replace atomically with backups
./target/release/mammofill --in-place --backup-suffix .bak /path/to/input
```

Keep completion rules in `core/src/registry.rs`. Every writer rule must name at least one extraction, classification, selection, or validation consumer. Keep legacy aliases shared by extraction and completion; current examples include `SRT`/`SNM3`/`99SDM`, deprecated XCC codes, and the read-only `BILATERAL` laterality alias. `mammofill` must not replace populated values, follow symlinks, retain signature structures without explicit consent, or write heuristic results unless `--allow-heuristic` is set. Do not treat `PositionerType` as SOP-fixed: supported mammography IODs permit both `MAMMOGRAPHIC` and `NONE`.

#### mammoplan - Mammography Input Planning
```bash
# Plan both 2D mammography views and DBT inputs
./target/release/mammoplan /path/to/dicom_directory --format json

# Plan only 2D mammography views
./target/release/mammoplan --include-2d /path/to/directory --format json

# Plan only DBT composition inputs and volume candidates
./target/release/mammoplan --include-dbt /path/to/directory --format json

# Prefer synthetic 2D views over FFDM when both exist for the same view
./target/release/mammoplan --prefer-synthetic-2d /path/to/directory --format json
```

If no `--include-*` flags are supplied, `mammoplan` includes both input groups.
When any include flag is supplied, only the requested groups are included.
`mammoplan` searches recursively so study roots with per-series subdirectories
can be planned directly; `mammoselect` remains non-recursive. Text output
summarizes warnings by default; pass `--verbose` to include per-file warning
details.

#### mammovalidate - DICOM Validation
```bash
# Validate a single DICOM file for mammoselect readiness
./target/release/mammovalidate /path/to/file.dcm

# Validate a directory using the same non-recursive discovery behavior as mammoselect
./target/release/mammovalidate /path/to/dicom_directory

# Validate a ZIP archive as a pseudo-directory
./target/release/mammovalidate /path/to/dicom_archive.zip

# Use the looser extraction profile
./target/release/mammovalidate --profile extraction /path/to/file.dcm

# Machine-readable output
./target/release/mammovalidate --format json /path/to/dicom_archive.zip

# Directory readiness with mammoselect-compatible filters
./target/release/mammovalidate --allowed-types ffdm,tomo --include-for-processing /path/to/directory
```

Exit code `0` means validation passed, `1` means validation completed and found validation problems, and `2` means a runtime/output error occurred.

#### dbt-combine - Old-Format DBT Conversion
```bash
# Check whether a study contains old-format DBT slice series
./target/release/dbt-combine check "/path/to/study"

# Convert old-format DBT slice series and copy through other DICOM files
./target/release/dbt-combine convert "/path/to/study" "/path/to/output"
```

DBT conversion is shared core functionality in `core/src/dbt.rs`; keep the Rust API,
`dbt-combine` CLI, Python bindings, and Python stubs in schema parity when changing report
fields or options. Python DBT APIs return dictionaries generated from the same serde report
structs used by CLI JSON output.

On this workstation, DBT all-features checks need `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1`.
For DBT changes, run `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make quality` and
`PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make test`. The local Apollo smoke is:
`dbt-combine check "/home/chase/data apollo"` should report 15 conversion-needed DBT series
and 26 copy-through DICOM files.

## Architecture

### Workspace Structure
- This is a Cargo workspace with two members: `core/` and `node/`
- The `core/` crate contains both the library (`lib.rs`) and binary (`main.rs`)
- The `node/` crate builds the NAPI-RS addon used by the local `@medcognetics/mammocat` package

### Module Organization

The codebase follows a clear separation of concerns:

**`types/`** - Core type system and domain models
- `enums.rs`: MammogramType, DbtObjectKind, Laterality, complete CID 4014 `ViewPosition`, complete CID 4015 `MammographyViewModifier`, PhotometricInterpretation, PreferenceOrder
- `filter.rs`: FilterConfig struct for record filtering during view selection
- `image_type.rs`: ImageType struct for decomposed DICOM ImageType field
- `view.rs`: MammogramView combining laterality + view position
- `pixel_spacing.rs`: PixelSpacing parsing

**`extraction/`** - Classification algorithms (mirrors Python dicom-utils behavior)
- `tags.rs`: DICOM tag constants and helper functions:
  - `get_string_value()`, `get_int_value()`: Read tag values from DICOM
  - `get_lowercase_string()`: Get normalized lowercase string (reduces boilerplate)
  - `PIXEL_DATA_TAG`, `DICOM_MAGIC_BYTES`: Shared constants
- `mammo_type.rs`: Type classification logic (TOMO/FFDM/SYNTH/SFM detection) plus DBT object-kind detection
- `laterality.rs`: Laterality extraction with fallback hierarchy
- `view_position.rs`: Shared canonical view descriptor parsing and conflict diagnostics
- `view_modifiers.rs`: Convenience readers derived from the shared descriptor

**`registry.rs`** - Canonical metadata registry
- Records applicability, DICOM paths, tags, VR/VM, current values, legacy aliases, inference sources, confidence, writer representation, and consumers.
- Owns the current CID 4014/CID 4015 tables used by extraction and completion.

**`completion.rs`** - Conservative completion API
- `plan_completion()`: Produces a non-mutating plan with additions, inferred-only values, and issues.
- `apply_completion_plan()`: Applies a source-bound plan in memory, rejects changed completion evidence, and appends the Original Attributes audit item.
- `complete_file()`: Uses a same-directory temporary file, verifies invariants and validation, then renames atomically.

**`selection/`** - Preferred view selection logic
- `record.rs`: MammogramRecord combining file path and metadata, with comparison logic
- `views.rs`: get_preferred_views, get_preferred_views_with_order, and get_preferred_views_filtered for selecting best views

**`planning.rs`** - Collection-level input planning
- `plan_mammography_collection()`: Builds 2D mammography view and/or DBT input plans from one directory.
- `MammographyPlanSelection`: Boolean input-group selection for `include_2d` and `include_dbt`.

**`validation.rs`** - File and collection validation reports
- `validate_path()`: Validates a single DICOM file, non-recursive directory, or ZIP archive.
- `validate_dicom_file()`: File-only validation helper used by Python bindings.
- `validate_directory_path()`: Directory or ZIP validation with mammoselect-compatible filter and preference options.
- `ValidationProfile`: `Selection` is strict and checks preferred-view readiness; `Extraction` only fails when mammocat extraction cannot run.

**`api.rs`** - Public API surface
- `MammogramExtractor`: Main entry point for metadata extraction
- `MammogramMetadata`: Complete extracted metadata structure (includes dbt_object_kind, pixel_spacing, manufacturer, model, number_of_frames, is_secondary_capture, modality, transfer_syntax_uid, transfer_syntax_name, compression_type)

**`python/`** - PyO3 bindings (enabled with `--features python`)
- `enums.rs`: Python wrappers for all enum types (PyMammogramType, PyLaterality, etc.)
- `filter.rs`: PyFilterConfig wrapper
- `metadata.rs`: PyMammogramMetadata wrapper
- `record.rs`: PyMammogramRecord wrapper
- `selection.rs`: Python wrappers for selection functions (get_preferred_views_filtered, etc.)
- `planning.rs`: Python wrapper for `plan_mammography_collection()`; returns the same planner schema as `mammoplan --format json`
- `validation.rs`: Python wrappers for `validate_dicom()` and `validate_directory()`; returns the same report schema as `mammovalidate --format json`
- `macros.rs`: Boilerplate reduction macro (`impl_py_from!` for From trait implementations)

**`node/`** - NAPI-RS Node/TypeScript bindings
- `src/lib.rs`: Synchronous public API for `extractMetadata`, `selectPreferredViews`, and `selectPreferredViewsFromDirectory`
- `index.js` and `index.d.ts`: Generated package loader and TypeScript declarations; keep these committed after `npm --prefix node run build`
- `npm/`: Platform-specific optional native package metadata for Linux x64 GNU, macOS x64, macOS arm64, and Windows x64 MSVC
- `test/`: Synthetic non-PHI DICOM fixtures, API tests, and the commit-pinned Git installation integration test
- Native `node/*.node` and `node/npm/**/*.node` artifacts are build outputs and stay ignored

**Root Node package shim**
- Root `package.json` and `package-lock.json` make the repository installable as `@medcognetics/mammocat` from an exact npm Git dependency.
- The root `prepare` script builds the NAPI addon from `node/` with access to the Cargo workspace and `core/` path dependency.
- The installed Git package contains `node/index.js`, `node/index.d.ts`, and the local host `.node` file. It does not declare the unpublished optional platform packages used by the publish-oriented `node/` package.

**`cli/`** - Command-line interface
- `mod.rs`: Argument parsing with clap
- `report.rs`: Text formatting for CLI output

**`dicom_files.rs`** - Shared non-recursive DICOM discovery helpers used by `mammoselect` and `mammovalidate`

**`error.rs`** - Error types using thiserror

### Key Design Patterns

**Configurable Preference Ordering**: The `PreferenceOrder` enum defines different strategies for ranking mammogram types during view selection. Two strategies are available:
- `Default`: FFDM > SYNTH > TOMO > SFM - Prefers 2D images for general inference
- `TomoFirst`: TOMO > FFDM > SYNTH > SFM - Maximizes use of 3D imaging when available

MammogramRecord comparison uses `is_preferred_to_with_order()` to respect the selected preference order. The selection algorithm (`get_preferred_views_with_order`) first chooses one study, then picks the best mammogram for each standard view (L-MLO, R-MLO, L-CC, R-CC) within that study.

**Single-Study Selection**: Preferred-view selection never mixes studies. After filters are applied, usable candidate records are grouped by `StudyInstanceUID`. Default selection chooses the most complete known study by true standard-view coverage first, MLO-like/CC-like candidate coverage second, and lowest `StudyInstanceUID` as the deterministic tie-break. When common-modality selection is required, completeness is scored within the best single modality group for each study. Default mode emits a warning when usable candidates span multiple study groups. Records missing `StudyInstanceUID` are singleton fallback groups in default mode and sort after known studies on equal completeness. `StudySelectionMode::StrictSingleStudy`, Python `strict=True`, and CLI `--strict` fail if usable candidates contain multiple studies or any missing `StudyInstanceUID`.

**Fallback Hierarchy**: Laterality extraction attempts multiple DICOM tags in order:
1. ImageLaterality
2. Laterality
3. FrameLaterality in SharedFunctionalGroupsSequence

**Rule-Based Classification**: Mammogram type classification follows the ordered algorithm documented in `core/src/extraction/mammo_type.rs`. Rules are applied from strongest evidence to fallback rules, preserving Python-compatible behavior where applicable. Defaults to FFDM when ImageType fields are missing.
Exact `ImageType` component `TOMO_2D` remains `Synth`; exact component `TOMO` is `Tomo` even for single-frame slice-per-file DBT. `TOMO_PROJ` is not treated as `TOMO`. Fuji-like single-frame `DERIVED\PRIMARY` objects with `VolumetricProperties=VOLUME`, allowed/absent `VolumeBasedCalculationTechnique`, concatenation/source-volume tags, and supporting tomosynthesis evidence are ambiguous in single-file extraction because vendors may copy those fields onto singleton synthetic 2D objects. Single-file `mammocat` reports those as `Unknown`/`DbtObjectKind::Unknown`; collection-aware selection and directory validation refine only large same-series ambiguous groups to `Tomo`/`Slice`, and leave ambiguous singleton objects unknown even when paired with a split-slice series. Tomosynthesis acquisition tags alone are not enough because Fuji FFDM and synthetic objects can carry them. `DbtObjectKind` records whether DBT is a multi-frame volume, single-frame slice, unknown DBT representation, or non-DBT.

**Enum Combinators**: Laterality has a `reduce()` method for combining lateralities (e.g., LEFT + RIGHT → BILATERAL). ViewPosition has helper methods like `is_standard_view()`, `is_mlo_like()`, `is_cc_like()`.

**Filtering Architecture**: The `FilterConfig` struct bundles all filtering options for view selection:
- `allowed_types`: Whitelist approach - only specified types included (None = allow all)
- Boolean exclusion flags: `exclude_implants`, `exclude_non_standard_views`, etc.
- Default behavior: Excludes FOR PROCESSING, secondary capture, and non-MG modality
- Permissive mode: `FilterConfig::permissive()` disables all filters

Hard filtering is used - records that don't pass filters are completely excluded from the candidate pool before view selection runs. This ensures filtered records never appear in results.

Filtering flow:
1. Load all DICOM files into MammogramRecord collection
2. Refine ambiguous DBT/SYN2D classifications using collection context
3. Apply FilterConfig to remove unwanted records via `apply_filters()`
4. Choose one study from filtered usable candidates, or fail in strict study mode
5. Run view selection algorithm (`get_preferred_views_with_order`) on the chosen study
6. Return best views from remaining candidates

**Node Selection Defaults**: The Node API is annotator-focused by default. It selects only FFDM, synthesized 2D, and SFM records with `DbtObjectKind::None` for the standard CC/MLO slots, uses recursive directory discovery for `selectPreferredViewsFromDirectory()`, and returns JSON-safe camelCase DTOs with fixed `rcc`, `lcc`, `rmlo`, and `lmlo` keys. Unreadable inputs in bulk selection go to `inputErrors`; only invalid API argument shapes should throw.

### Validation Architecture

`mammovalidate` and the Python validation functions use the same Rust report model. File validation records critical errors, warnings, info messages, and check details. Directory and ZIP validation aggregate per-file reports and run `get_preferred_views_filtered()` on valid records to verify standard-view coverage.

The default `Selection` profile is strict: it fails files with missing/invalid selection-critical tags such as `Modality`, `SOPInstanceUID`, `StudyInstanceUID`, `SeriesInstanceUID`, laterality, view position, dimensions, bit-depth fields, or `PixelData`. It warns about metadata that can cause default filtering or deprioritization, including `FOR PROCESSING`, secondary capture, non-standard views, spot/magnification views, implants, and optional manufacturer/model/spacing gaps.

The `Extraction` profile is looser: it fails only when DICOM reading or `MammogramExtractor` metadata extraction fails. Selection-specific gaps are warnings or info.

New metadata fields for filtering:
- `is_secondary_capture`: Detected via SOP Class UID (checks if starts with "1.2.840.10008.5.1.4.1.1.7")
- `modality`: DICOM Modality tag value (should be "MG" for mammography)

### Python Compatibility

This implementation maintains behavioral compatibility with the Python `dicom-utils` library:
- Classification algorithm in `mammo_type.rs` matches `dicom-utils/dicom_utils/types.py:159-195`
- Type preference ordering preserved
- Pattern matching behavior identical
- When making changes to classification logic, verify against Python reference

## Dependencies

- **dicom-rs (0.9)**: DICOM file parsing and tag reading
- **clap (4.5)**: CLI argument parsing with derive macros
- **thiserror (1.0)**: Error type definitions
- **regex (1.10)**: Pattern matching for view positions and metadata
- **serde/serde_json** (optional): JSON serialization behind `json` feature flag
- **napi/napi-derive**: Node addon bindings under `node/`

## Testing Strategy

Tests are embedded in module files using `#[cfg(test)]`, with additional Python tests under `tests/`.

Test categories:
- Enum behavior and ordering (types/enums.rs)
- String parsing (ViewPosition, Laterality from strings)
- Data structure operations (Laterality::reduce, ImageType decomposition)
- Classification algorithm logic
- Preferred view selection (selection/record.rs, selection/views.rs)
- Canonical parser, completion planning, safe file writes, audit, and CLI behavior
- Python bindings API (tests/test_enums.py, tests/test_api.py)
- Node/TypeScript bindings API, generated declarations, file/buffer parity, selection diagnostics, package dry-run, and commit-pinned Git installation

When adding features that affect metadata extraction, add corresponding unit tests in the relevant module file.

## Binary Locations

Six CLI binaries are defined in core/Cargo.toml:

**mammocat** - Metadata extraction from individual DICOM files
```toml
[[bin]]
name = "mammocat"
path = "src/main.rs"
```

**mammoselect** - Preferred view selection from directories
```toml
[[bin]]
name = "mammoselect"
path = "src/bin/mammoselect.rs"
```

**mammofill** - Conservative canonical metadata completion
```toml
[[bin]]
name = "mammofill"
path = "src/bin/mammofill.rs"
```

**mammoplan** - 2D mammography view and DBT input planning from directories
```toml
[[bin]]
name = "mammoplan"
path = "src/bin/mammoplan.rs"
```

**mammovalidate** - Validation for files, directories, or ZIP archives
```toml
[[bin]]
name = "mammovalidate"
path = "src/bin/mammovalidate.rs"
```

**dbt-combine** - Old-format DBT conversion
```toml
[[bin]]
name = "dbt-combine"
path = "src/bin/dbt-combine.rs"
```

After building, binaries are at:
- `./target/release/mammocat`, `./target/release/mammofill`, `./target/release/mammoselect`, `./target/release/mammoplan`, `./target/release/mammovalidate`, and `./target/release/dbt-combine` (release)
- `./target/debug/mammocat`, `./target/debug/mammofill`, `./target/debug/mammoselect`, `./target/debug/mammoplan`, `./target/debug/mammovalidate`, and `./target/debug/dbt-combine` (debug)

## Contribution Guidelines

Use `rustfmt` and `clippy --all-features -- -D warnings` for Rust; keep Rust module,
function, and test names in `snake_case`, and type names in `PascalCase`. Python targets
3.10+, uses Ruff with 100-character lines and double quotes, and is type-checked with
`basedpyright`. Node targets 22+. Keep public Python exports and `_mammocat.pyi` stubs aligned
with their PyO3 bindings.

Add Rust unit tests next to core logic and Python tests under `tests/test_*.py`; use
integration tests for cross-module or I/O behavior. Run focused tests first, then
`PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make quality` and
`PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make test` before handoff.

Keep commits focused and use short imperative subjects. Pull requests should summarize
behavioral changes, list validation commands, describe CLI and Python API effects, and note
DICOM fixture or data assumptions. Do not commit DICOM PHI, tokens, or local data paths;
treat file paths, archive handling, DICOM parsing, JSON output, and Python bindings as trust
boundaries.
