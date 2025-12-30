# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mammocat is a Rust library and CLI tool for extracting mammography metadata from DICOM files. It's a port of the Python `dicom-utils` library, focusing on performance and type safety while maintaining behavioral compatibility.

## Common Commands

### Development Setup
```bash
# Install Python development dependencies
make dev
```

### Building
```bash
# Build Python bindings (debug)
make build

# Build Python bindings (release, optimized)
make build-release

# Build Rust CLI binaries (standalone, no Python)
cargo build --release
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
```

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

# Verbose logging
./target/release/mammoselect --verbose /path/to/directory

# Filtering options
./target/release/mammoselect --allowed-types ffdm /path/to/directory
./target/release/mammoselect --allowed-types ffdm,tomo --exclude-implants /path/to/directory
./target/release/mammoselect --only-standard-views /path/to/directory
./target/release/mammoselect --include-for-processing /path/to/directory
./target/release/mammoselect --include-secondary-capture /path/to/directory
```

## Architecture

### Workspace Structure
- This is a Cargo workspace with a single member: `core/`
- The `core/` crate contains both the library (`lib.rs`) and binary (`main.rs`)

### Module Organization

The codebase follows a clear separation of concerns:

**`types/`** - Core type system and domain models
- `enums.rs`: MammogramType, Laterality, ViewPosition, PhotometricInterpretation, PreferenceOrder
- `filter.rs`: FilterConfig struct for record filtering during view selection
- `image_type.rs`: ImageType struct for decomposed DICOM ImageType field
- `view.rs`: MammogramView combining laterality + view position
- `pixel_spacing.rs`: PixelSpacing parsing

**`extraction/`** - Classification algorithms (mirrors Python dicom-utils behavior)
- `tags.rs`: DICOM tag constants and helper functions:
  - `get_string_value()`, `get_int_value()`: Read tag values from DICOM
  - `get_lowercase_string()`: Get normalized lowercase string (reduces boilerplate)
  - `PIXEL_DATA_TAG`, `DICOM_MAGIC_BYTES`: Shared constants
- `mammo_type.rs`: Type classification logic (TOMO/FFDM/SYNTH/SFM detection)
- `laterality.rs`: Laterality extraction with fallback hierarchy
- `view_position.rs`: View position parsing with helper functions (`match_strict_patterns`, `match_loose_patterns`)
- `view_modifiers.rs`: Spot compression, magnification, implant displaced detection

**`selection/`** - Preferred view selection logic
- `record.rs`: MammogramRecord combining file path and metadata, with comparison logic
- `views.rs`: get_preferred_views, get_preferred_views_with_order, and get_preferred_views_filtered for selecting best views

**`api.rs`** - Public API surface
- `MammogramExtractor`: Main entry point for metadata extraction
- `MammogramMetadata`: Complete extracted metadata structure (includes manufacturer, model, number_of_frames, is_secondary_capture, modality)

**`python/`** - PyO3 bindings (enabled with `--features python`)
- `enums.rs`: Python wrappers for all enum types (PyMammogramType, PyLaterality, etc.)
- `filter.rs`: PyFilterConfig wrapper
- `metadata.rs`: PyMammogramMetadata wrapper
- `record.rs`: PyMammogramRecord wrapper
- `selection.rs`: Python wrappers for selection functions (get_preferred_views_filtered, etc.)
- `macros.rs`: Boilerplate reduction macro (`impl_py_from!` for From trait implementations)

**`cli/`** - Command-line interface
- `mod.rs`: Argument parsing with clap
- `report.rs`: Text formatting for CLI output

**`error.rs`** - Error types using thiserror

### Key Design Patterns

**Configurable Preference Ordering**: The `PreferenceOrder` enum defines different strategies for ranking mammogram types during view selection. Two strategies are available:
- `Default`: FFDM > SYNTH > TOMO > SFM - Prefers 2D images for general inference
- `TomoFirst`: TOMO > FFDM > SYNTH > SFM - Maximizes use of 3D imaging when available

MammogramRecord comparison uses `is_preferred_to_with_order()` to respect the selected preference order. The selection algorithm (`get_preferred_views_with_order`) uses this to pick the best mammogram for each standard view (L-MLO, R-MLO, L-CC, R-CC).

**Fallback Hierarchy**: Laterality extraction attempts multiple DICOM tags in order:
1. ImageLaterality
2. Laterality
3. FrameLaterality in SharedFunctionalGroupsSequence

**Rule-Based Classification**: Mammogram type classification follows a strict order of rules (see core/src/extraction/mammo_type.rs:26-50 for algorithm). Rules are categorized as "very solid", "ok", and "not good" matching Python implementation. Defaults to FFDM when ImageType fields are missing.

**Enum Combinators**: Laterality has a `reduce()` method for combining lateralities (e.g., LEFT + RIGHT → BILATERAL). ViewPosition has helper methods like `is_standard_view()`, `is_mlo_like()`, `is_cc_like()`.

**Filtering Architecture**: The `FilterConfig` struct bundles all filtering options for view selection:
- `allowed_types`: Whitelist approach - only specified types included (None = allow all)
- Boolean exclusion flags: `exclude_implants`, `exclude_non_standard_views`, etc.
- Default behavior: Excludes FOR PROCESSING, secondary capture, and non-MG modality
- Permissive mode: `FilterConfig::permissive()` disables all filters

Hard filtering is used - records that don't pass filters are completely excluded from the candidate pool before view selection runs. This ensures filtered records never appear in results.

Filtering flow:
1. Load all DICOM files into MammogramRecord collection
2. Apply FilterConfig to remove unwanted records via `apply_filters()`
3. Run view selection algorithm (`get_preferred_views_with_order`) on filtered set
4. Return best views from remaining candidates

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

- **dicom-rs (0.7)**: DICOM file parsing and tag reading
- **clap (4.5)**: CLI argument parsing with derive macros
- **thiserror (1.0)**: Error type definitions
- **regex (1.10)**: Pattern matching for view positions and metadata
- **serde/serde_json** (optional): JSON serialization behind `json` feature flag

## Testing Strategy

Tests are embedded in module files using `#[cfg(test)]`. Current coverage: 60+ Rust unit tests + 48 Python tests.

Test categories:
- Enum behavior and ordering (types/enums.rs)
- String parsing (ViewPosition, Laterality from strings)
- Data structure operations (Laterality::reduce, ImageType decomposition)
- Classification algorithm logic
- Preferred view selection (selection/record.rs, selection/views.rs)
- Python bindings API (tests/test_enums.py, tests/test_api.py)

When adding features that affect metadata extraction, add corresponding unit tests in the relevant module file.

## Binary Locations

Two CLI binaries are defined in core/Cargo.toml:

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

After building, binaries are at:
- `./target/release/mammocat` and `./target/release/mammoselect` (release)
- `./target/debug/mammocat` and `./target/debug/mammoselect` (debug)
