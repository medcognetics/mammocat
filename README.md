# Mammocat - DICOM Mammography Metadata Extraction

A Rust library and CLI tool for extracting mammography metadata from DICOM files. Ported from the Python `dicom-utils` library with a focus on performance and type safety.

## Features

- **Mammogram Type Classification**: Automatically determines if a mammogram is TOMO, FFDM, SYNTH, or SFM
- **Laterality Detection**: Extracts breast laterality (Left/Right/Bilateral) with fallback hierarchy
- **View Position Parsing**: Identifies view positions (CC, MLO, ML, etc.) with pattern matching
- **Implant Status**: Detects breast implant presence
- **Processing Intent**: Identifies "FOR PROCESSING" images
- **Preferred View Selection**: Automatically selects the best mammogram for each standard view
- **Validation Reports**: Checks whether files or directories are ready for metadata extraction or preferred-view selection
- **Python Bindings**: Full Python API via PyO3 for seamless integration
- **Clean API**: Easy-to-use library and command-line interface
- **Type Safe**: Leverages Rust's type system for correctness
- **Well Tested**: Comprehensive test coverage (60+ Rust tests, 48 Python tests)

## Installation

### From Source

```bash
git clone <repository-url>
cd mammocat
cargo build --release
```

The binaries will be available at `target/release/mammocat`, `target/release/mammoselect`, and `target/release/mammovalidate`.

## Usage

### Command Line

Extract metadata from a DICOM file:

```bash
# Text output (default)
mammocat path/to/mammogram.dcm

# JSON output (requires 'json' feature)
cargo build --release --features json
mammocat --format json path/to/mammogram.dcm

# Verbose logging
mammocat --verbose path/to/mammogram.dcm
```

`mammocat` reports mammography classification fields plus file-meta transfer syntax details, including `transfer_syntax_uid`, `transfer_syntax_name`, and `compression_type` in JSON output.

### mammoselect - Preferred View Selection

Select the best mammogram for each standard view (L-CC, R-CC, L-MLO, R-MLO) from a directory:

```bash
# Select preferred views (default ordering: FFDM > SYNTH > TOMO > SFM)
mammoselect /path/to/dicom_directory

# Use tomo-first ordering (TOMO > FFDM > SYNTH > SFM)
mammoselect --preference tomo-first /path/to/directory

# Output as JSON
mammoselect --format json /path/to/directory

# Output file paths only (useful for scripting)
mammoselect --format paths /path/to/directory
```

### mammovalidate - DICOM Validation

Validate one DICOM file, non-recursive directory, or ZIP archive before running `mammocat` or `mammoselect`:

```bash
# Selection-readiness profile (default)
mammovalidate /path/to/mammogram.dcm
mammovalidate /path/to/dicom_directory
mammovalidate /path/to/dicom_archive.zip

# Looser profile: only require mammocat metadata extraction readiness
mammovalidate --profile extraction /path/to/mammogram.dcm

# JSON report
cargo build --release --features json
mammovalidate --format json /path/to/dicom_archive.zip
```

The selection profile treats missing selection-critical fields as validation failures, including non-`MG` or missing modality, unknown laterality or view, missing key UIDs, invalid dimensions/frames, invalid bit-depth relationships, and missing `PixelData`. It reports likely filtering or ranking issues, such as `FOR PROCESSING`, secondary capture, non-standard views, spot/magnification views, implants, unusual pixel layouts, lossy compression metadata, and optional metadata gaps, as warnings. Directory and ZIP validation also check four-view coverage after applying the same filter options used by `mammoselect`.

Exit code `0` means validation passed, `1` means validation completed and found problems, and `2` means the tool hit a runtime or output error.

Example output:

```
Mammogram Metadata
==================

Type:           ffdm
Laterality:     left
View Position:  cc
Image Type:     ORIGINAL|PRIMARY
For Processing: false
Has Implant:    false

Derived Properties
------------------
Standard View:  true
Is 2D:          true
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
mammocat-core = { path = "core" }
```

Use in your code:

```rust
use mammocat_core::{MammogramExtractor, MammogramType, Laterality, ViewPosition};
use dicom_object::open_file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open DICOM file
    let dcm = open_file("mammogram.dcm")?;

    // Extract metadata
    let metadata = MammogramExtractor::extract_file(&dcm)?;

    // Access extracted information
    println!("Type: {}", metadata.mammogram_type.simple_name());
    println!("Laterality: {}", metadata.laterality);
    println!("View: {}", metadata.view_position);
    println!("Transfer syntax: {:?}", metadata.transfer_syntax_uid);
    println!("Compression: {:?}", metadata.compression_type);
    println!("Is standard view: {}", metadata.is_standard_view());

    Ok(())
}
```

### Python Validation API

The validation bindings return the same dictionary schema as `mammovalidate --format json`.

```python
from pathlib import Path

from mammocat import validate_dicom, validate_directory

file_report = validate_dicom("mammogram.dcm")
directory_report = validate_directory(Path("dicoms.zip"), profile="selection")

if not file_report["summary"]["valid"]:
    print(file_report["files"][0]["errors"])
```

## Classification Algorithms

### Mammogram Type

Mammograms are classified into types with preference ordering (TOMO < FFDM < SYNTH < SFM):

- **TOMO**: Tomosynthesis (3D imaging) - detected by `NumberOfFrames > 1`
- **FFDM**: Full Field Digital Mammography - default for "ORIGINAL" images
- **SYNTH**: Synthetic 2D from tomosynthesis - detected by series description or `GENERATED_2D` flag
- **SFM**: Screen Film Mammography - manually flagged

The classification follows a hierarchical rule system matching the Python implementation.

### Laterality

Laterality is extracted using a fallback hierarchy:

1. `ImageLaterality` tag
2. `Laterality` tag
3. `FrameLaterality` in `SharedFunctionalGroupsSequence`

Values are parsed as: `"L"` → Left, `"R"` → Right

### View Position

View positions are detected using pattern matching on:

- `ViewPosition` tag
- `ViewCodeSequence` → `CodeMeaning`
- `ViewModifierCodeSequence` → `CodeMeaning`

Supports standard views (CC, MLO) and specialized views (XCCL, XCCM, ML, LM, LMO, AT, CV).

## Architecture

```
mammocat/
├── core/                           # Library and binary
│   ├── src/
│   │   ├── types/                  # Core type system
│   │   │   ├── enums.rs            # MammogramType, Laterality, ViewPosition
│   │   │   ├── image_type.rs       # ImageType struct
│   │   │   ├── pixel_spacing.rs
│   │   │   └── view.rs             # MammogramView
│   │   ├── extraction/             # Classification algorithms
│   │   │   ├── tags.rs             # DICOM tag constants and helpers
│   │   │   ├── mammo_type.rs       # Type classification
│   │   │   ├── laterality.rs       # Laterality extraction
│   │   │   ├── view_position.rs    # View parsing
│   │   │   └── view_modifiers.rs   # Spot/mag/implant displaced
│   │   ├── selection/              # Preferred view selection
│   │   │   ├── record.rs           # MammogramRecord with comparison
│   │   │   └── views.rs            # get_preferred_views functions
│   │   ├── python/                 # PyO3 bindings
│   │   │   ├── enums.rs            # Python enum wrappers
│   │   │   ├── metadata.rs         # PyMammogramMetadata
│   │   │   ├── record.rs           # PyMammogramRecord
│   │   │   └── macros.rs           # Boilerplate reduction macros
│   │   ├── api.rs                  # Public API
│   │   ├── cli/                    # Command-line interface
│   │   │   ├── mod.rs
│   │   │   └── report.rs           # Text formatting
│   │   ├── validation.rs           # File/directory validation reports
│   │   ├── error.rs                # Error types
│   │   ├── main.rs                 # mammocat CLI entry point
│   │   └── bin/
│   │       ├── mammoselect.rs      # mammoselect CLI entry point
│   │       └── mammovalidate.rs    # validation CLI entry point
```

## Type System

### Enums

- **`MammogramType`**: Unknown, Tomo, Ffdm, Synth, Sfm
  - Implements preference ordering for deduplication
  - `is_preferred_to()` method for comparison

- **`Laterality`**: Unknown, None, Left, Right, Bilateral
  - `reduce()` method for combining lateralities
  - `opposite()` for getting contralateral side

- **`ViewPosition`**: Unknown, Xccl, Xccm, Cc, Mlo, Ml, Lmo, Lm, At, Cv
  - `is_standard_view()`, `is_mlo_like()`, `is_cc_like()` properties

### Data Structures

- **`ImageType`**: Decomposed DICOM ImageType field (pixels, exam, flavor, extras)
- **`PixelSpacing`**: Pixel spacing in mm with regex parsing
- **`MammogramView`**: Combination of laterality + view position
- **`MammogramMetadata`**: Complete extracted metadata

## Dependencies

- **dicom-rs** (0.9): DICOM reading and parsing
- **clap** (4.5): Command-line argument parsing
- **thiserror** (1.0): Error handling
- **regex** (1.10): Pattern matching
- **serde/serde_json** (optional): JSON serialization

## Testing

Run all tests (Rust + Python):

```bash
make test
```

Run Python tests only:

```bash
make test-python
```

Run Rust tests only:

```bash
make test-rust
```

Run Python tests with coverage:

```bash
make test-cov
```

Run specific Rust test:

```bash
cargo test test_name
```

Current test coverage: 60+ Rust unit tests and 48 Python tests covering:
- Enum behavior and ordering
- String parsing and pattern matching
- Classification algorithm logic
- Data structure operations
- Preferred view selection
- Python bindings API (via pytest)

## Future Enhancements

- [ ] Additional metadata fields (PatientAge, StudyDate, etc.)
- [ ] Sequence navigation for nested DICOM tags (FrameLaterality, ViewCodeSequence)
- [ ] Performance optimization with rayon for batch processing

## Python Compatibility

This implementation maintains behavioral compatibility with the Python `dicom-utils` library while providing:

- 10-100x faster performance (Rust vs Python)
- Type safety at compile time
- Zero-cost abstractions
- Memory safety without garbage collection

Reference Python files:
- `dicom-utils/dicom_utils/types.py` - Core algorithms
- `dicom-utils/dicom_utils/container/record.py` - Metadata extraction patterns

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE.md](LICENSE.md) file for details.

## Contributing

Contributions welcome! Please ensure:

1. All tests pass (`make test`)
2. Code is formatted (`make format`)
3. All quality checks pass (`make quality`)
4. Add tests for new features

### Development Workflow

```bash
# Install dependencies
make dev

# Build the project
make build

# Run all tests
make test

# Check and fix code quality
make quality-fix

# Run everything (install, build, test, check quality)
make all
```
