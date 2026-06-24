# AGENTS.md

- DBT conversion is shared core functionality in `core/src/dbt.rs`; keep the Rust API, `dbt-combine` CLI, Python bindings, and Python stubs in schema parity when changing report fields or options.
- Python DBT APIs return dictionaries generated from the same serde report structs used by CLI JSON output.
- On this workstation, all-features Rust checks need `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1`.
- For DBT changes, run `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make quality` and `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make test`.
- The local Apollo smoke is: `dbt-combine check "/home/chase/data apollo"` should report 15 conversion-needed DBT series and 26 copy-through DICOM files.
