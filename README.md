# Mammocat - DICOM Mammography Metadata Extraction

A Rust library and CLI tool for extracting mammography metadata from DICOM files. Ported from the Python `dicom-utils` library with a focus on performance and type safety.

## Features

- **Mammogram Type Classification**: Automatically determines if a mammogram is TOMO, FFDM, SYNTH, or SFM
- **DBT Object Classification**: Reports whether DBT is stored as a multi-frame volume or split slice object
- **Laterality Detection**: Extracts breast laterality (Left/Right/Bilateral) with fallback hierarchy
- **View Position Parsing**: Identifies view positions (CC, MLO, ML, etc.) with pattern matching
- **Implant Status**: Detects breast implant presence
- **Processing Intent**: Identifies "FOR PROCESSING" images
- **Preferred View Selection**: Automatically selects the best mammogram for each standard view
- **Validation Reports**: Checks whether files or directories are ready for metadata extraction or preferred-view selection
- **Python Bindings**: Full Python API via PyO3 for seamless integration
- **Node/TypeScript Bindings**: Synchronous NAPI-RS package for metadata extraction and preferred-view selection
- **Clean API**: Easy-to-use library and command-line interface
- **Type Safe**: Leverages Rust's type system for correctness
- **Well Tested**: Comprehensive Rust and Python test coverage

## Installation

### From Source

```bash
git clone <repository-url>
cd mammocat
cargo build --release
```

The binaries will be available at `target/release/mammocat`, `target/release/mammoselect`, `target/release/mammoplan`, `target/release/mammovalidate`, and `target/release/dbt-combine`.

Build the local Node/TypeScript package:

```bash
make node-install
make node-build
make node-pack
```

The root npm package uses optional native packages for Linux x64 GNU, macOS x64, macOS arm64, and Windows x64 MSVC. Local repository installs omit those optional packages until the platform packages have been published.

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
# Select preferred views from the most complete study
mammoselect /path/to/dicom_directory

# Use tomo-first ordering (TOMO > FFDM > SYNTH > SFM)
mammoselect --preference tomo-first /path/to/directory

# Error if usable records contain multiple studies or missing StudyInstanceUID
mammoselect --strict /path/to/directory

# Output as JSON
mammoselect --format json /path/to/directory

# Output file paths only (useful for scripting)
mammoselect --format paths /path/to/directory

```

`mammoselect` never mixes studies in its output. After filtering, it groups usable
candidate records by `StudyInstanceUID`, chooses the study with the most true
standard-view slots, then uses MLO-like/CC-like candidate coverage as a
tie-break. If multiple known studies are still tied, the lowest
`StudyInstanceUID` is selected. Records without `StudyInstanceUID` are treated
as singleton fallback groups in default mode and sort after known studies on
equal completeness. When `--require-common-modality` is used, completeness is
scored within the best single modality group for each study. Default mode emits
a warning when usable candidates span multiple study groups so callers know only
the most complete study was selected.

Use `--strict` when a directory must contain exactly one usable study. Strict
mode fails if usable candidates span more than one `StudyInstanceUID` or if any
usable candidate is missing `StudyInstanceUID`.

### mammoplan - Mammography Input Planning

Build a collection-level input plan for 2D mammography views and DBT inputs:

```bash
# Plan both 2D mammography views and DBT inputs
mammoplan /path/to/dicom_directory --format json

# Plan only 2D mammography views
mammoplan --include-2d /path/to/directory --format json

# Plan only DBT composition inputs and volume candidates
mammoplan --include-dbt /path/to/directory --format json

# Prefer synthetic 2D views over FFDM when both exist for the same view
mammoplan --prefer-synthetic-2d /path/to/directory --format json
```

If no `--include-*` flags are supplied, `mammoplan` includes both input groups.
When any include flag is supplied, only the requested groups are included. The
JSON report includes `plan`, `views`, `dbt`, `source_objects`, `warnings`,
and `summary`. Unlike `mammoselect`, `mammoplan` searches recursively so a study
root with per-series subdirectories can be planned in one call. Text output
summarizes warnings by default; pass `--verbose` to include per-file warning
details.

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

Type               : ffdm
DBT Object Kind    : none
Laterality         : left
View Position      : cc
Image Type         : ORIGINAL|PRIMARY
For Processing     : false
Has Implant        : false

Derived Properties
------------------
Standard View      : true
Is 2D              : true
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
    println!("Pixel spacing: {:?}", metadata.pixel_spacing);
    println!("Transfer syntax: {:?}", metadata.transfer_syntax_uid);
    println!("Compression: {:?}", metadata.compression_type);
    println!("Is standard view: {}", metadata.is_standard_view());

    Ok(())
}
```

### Node/TypeScript API

The `node/` package builds `@medcognetics/mammocat`, a synchronous NAPI-RS API that returns JSON-safe camelCase objects.

```ts
import {
  extractMetadata,
  selectPreferredViews,
  selectPreferredViewsFromDirectory,
} from "@medcognetics/mammocat"

const metadata = extractMetadata({ path: "study/R_CC.dcm" })
const selection = selectPreferredViewsFromDirectory("study")

const bufferSelection = selectPreferredViews([
  { path: "study/R_CC.dcm" },
  { bytes: new Uint8Array(dicomBytes), filename: "L_CC.dcm" },
])

console.log(metadata.pixelSpacing?.column)
console.log(selection.views.rcc?.source)
console.log(bufferSelection.inputErrors)
```

`PreferredViewSelection.views` always uses the fixed keys `rcc`, `lcc`, `rmlo`, and `lmlo`; missing slots are `null`. Bulk selection reports unreadable or unsupported DICOM inputs in `inputErrors`, while invalid API argument shapes throw. The default selection policy targets annotator-focused 2D standard views, excluding TOMO and DBT objects unless an explicit `preferenceOrder` override is supplied.

The package is prepared for prebuilt native installs on Linux x64 GNU, macOS x64, macOS arm64, and Windows x64 MSVC. The root package stays platform-neutral and resolves the matching native package through optional dependencies.

### Python Validation API

The validation bindings return the same dictionary schema as `mammovalidate --format json`.

```python
from pathlib import Path

from mammocat import plan_mammography_collection, validate_dicom, validate_directory

file_report = validate_dicom("mammogram.dcm")
directory_report = validate_directory(Path("dicoms.zip"), profile="selection")
input_plan = plan_mammography_collection(
    Path("dicoms"),
    include_2d=True,
    include_dbt=True,
    prefer_synthetic_2d=False,
)

if not file_report["summary"]["valid"]:
    print(file_report["files"][0]["errors"])
```

## Classification Algorithms

### Mammogram Type

Mammograms are classified into types:

- **TOMO**: Tomosynthesis/DBT imaging - detected by `NumberOfFrames > 1`, exact `ImageType` component `TOMO`, or collection refinement of ambiguous split-slice DBT series
- **FFDM**: Full Field Digital Mammography - default for "ORIGINAL" images
- **SYNTH**: Synthetic 2D from tomosynthesis - detected by series description, exact `ImageType` component `TOMO_2D`, or `GENERATED_2D` flag
- **SFM**: Screen Film Mammography - manually flagged

`DbtObjectKind` separately reports whether TOMO objects are multi-frame `volume`, single-frame `slice`, or `unknown`; non-DBT images report `none`. Single-file extraction treats Fuji-like `DERIVED\PRIMARY` objects with `VolumetricProperties=VOLUME`, allowed/absent `VolumeBasedCalculationTechnique`, concatenation/source-volume tags, and supporting tomosynthesis evidence as `unknown` because some vendors copy those fields onto singleton synthetic 2D objects. Directory selection and validation refine only large same-series ambiguous groups to `Tomo`/`slice`; ambiguous singleton objects stay `unknown` even when they pair with a split-slice series. Tomosynthesis acquisition tags like `TomoClass`, source-image count, or processing text are supporting evidence only; tomo angle is not used as a classifier by itself.
`ImageType` component matching is exact: `TOMO_PROJ` is not treated as `TOMO`.

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
│   │   │   ├── enums.rs            # MammogramType, DbtObjectKind, Laterality, ViewPosition
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
│   │       ├── dbt-combine.rs      # DBT conversion CLI entry point
│   │       ├── mammoselect.rs      # mammoselect CLI entry point
│   │       ├── mammoplan.rs        # input planning CLI entry point
│   │       └── mammovalidate.rs    # validation CLI entry point
```

## Type System

### Enums

- **`MammogramType`**: Unknown, Tomo, Ffdm, Synth, Sfm
  - Implements preference ordering for deduplication
  - `is_preferred_to()` method for comparison

- **`DbtObjectKind`**: None, Volume, Slice, Unknown
  - Describes DBT storage representation independently from `MammogramType`

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
- **napi/napi-derive** (Node package): NAPI-RS bindings

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

Run Node package checks:

```bash
make node-install
make node-build
make node-test
make node-typecheck
make node-pack
```

Current test coverage includes Rust unit/integration tests and Python tests covering:
- Enum behavior and ordering
- String parsing and pattern matching
- Classification algorithm logic
- Data structure operations
- Preferred view selection
- Python bindings API (via pytest)
- Node/TypeScript bindings API, JSON round trips, file/buffer parity, and directory selection

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
