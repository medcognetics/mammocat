# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mammocat is a Rust library and CLI tool for extracting mammography metadata from DICOM files. It's a port of the Python `dicom-utils` library, focusing on performance and type safety while maintaining behavioral compatibility.

## Common Commands

### Building
```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Build with JSON output support
cargo build --release --features json
```

### Testing
```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with all features enabled
cargo test --all-features

# Run tests in a specific module
cargo test laterality
```

### Code Quality
```bash
# Check for common mistakes and style issues
cargo clippy

# Format code
cargo fmt

# Check if code is formatted (CI-friendly)
cargo fmt --check

# Run clippy with all features
cargo clippy --all-features
```

### Running the CLI
```bash
# After building
./target/release/mammocat path/to/mammogram.dcm

# With verbose logging
./target/release/mammocat --verbose path/to/file.dcm

# JSON output (requires json feature)
./target/release/mammocat --format json path/to/file.dcm
```

## Architecture

### Workspace Structure
- This is a Cargo workspace with a single member: `core/`
- The `core/` crate contains both the library (`lib.rs`) and binary (`main.rs`)

### Module Organization

The codebase follows a clear separation of concerns:

**`types/`** - Core type system and domain models
- `enums.rs`: MammogramType, Laterality, ViewPosition, PhotometricInterpretation
- `image_type.rs`: ImageType struct for decomposed DICOM ImageType field
- `view.rs`: MammogramView combining laterality + view position
- `pixel_spacing.rs`: PixelSpacing parsing

**`extraction/`** - Classification algorithms (mirrors Python dicom-utils behavior)
- `tags.rs`: DICOM tag constants and helper functions for reading tag values
- `mammo_type.rs`: Type classification logic (TOMO/FFDM/SYNTH/SFM detection)
- `laterality.rs`: Laterality extraction with fallback hierarchy
- `view_position.rs`: View position parsing from multiple DICOM fields

**`api.rs`** - Public API surface
- `MammogramExtractor`: Main entry point for metadata extraction
- `MammogramMetadata`: Complete extracted metadata structure (includes manufacturer, model, number_of_frames)

**`cli/`** - Command-line interface
- `mod.rs`: Argument parsing with clap
- `report.rs`: Text formatting for CLI output

**`error.rs`** - Error types using thiserror

### Key Design Patterns

**Preference Ordering**: MammogramType implements Ord/PartialOrd based on preference (TOMO < FFDM < SYNTH < SFM). This allows using `.min()` to select the best type from a collection.

**Fallback Hierarchy**: Laterality extraction attempts multiple DICOM tags in order:
1. ImageLaterality
2. Laterality
3. FrameLaterality in SharedFunctionalGroupsSequence

**Rule-Based Classification**: Mammogram type classification follows a strict order of rules (see core/src/extraction/mammo_type.rs:26-50 for algorithm). Rules are categorized as "very solid", "ok", and "not good" matching Python implementation. Defaults to FFDM when ImageType fields are missing.

**Enum Combinators**: Laterality has a `reduce()` method for combining lateralities (e.g., LEFT + RIGHT → BILATERAL). ViewPosition has helper methods like `is_standard_view()`, `is_mlo_like()`, `is_cc_like()`.

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

Tests are embedded in module files using `#[cfg(test)]`. Current coverage: 34+ unit tests.

Test categories:
- Enum behavior and ordering (types/enums.rs)
- String parsing (ViewPosition, Laterality from strings)
- Data structure operations (Laterality::reduce, ImageType decomposition)
- Classification algorithm logic

When adding features that affect metadata extraction, add corresponding unit tests in the relevant module file.

## Binary Location

The CLI binary is defined in core/Cargo.toml:
```toml
[[bin]]
name = "mammocat"
path = "src/main.rs"
```

After building, the binary is at `./target/release/mammocat` (release) or `./target/debug/mammocat` (debug).
