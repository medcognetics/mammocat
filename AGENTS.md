# Repository Guidelines

## Project Structure & Module Organization

Mammocat is a Rust workspace with one crate in `core/`. The library entry point is `core/src/lib.rs`; CLI binaries live in `core/src/main.rs` and `core/src/bin/` (`mammoselect`, `mammovalidate`, `dbt-combine`). Domain modules are split across `types/`, `extraction/`, `selection/`, `validation.rs`, `cli/`, and `python/`. Python packaging and stubs live in `python/mammocat/`. Python tests are in `tests/test_*.py`; Rust integration tests are in `core/tests/`, with unit tests colocated in Rust modules.

## Build, Test, and Development Commands

- `make dev`: install Python development dependencies with `uv`.
- `make build`: build debug PyO3 bindings through `maturin develop`.
- `make build-release`: build optimized Python bindings.
- `cargo build --release --all-features`: build release CLI binaries and all feature-gated code.
- `make test`: rebuild bindings, then run Rust and Python tests.
- `make test-rust` / `make test-python`: run one test surface.
- `make test-cov`: run Python tests with coverage output.
- `make quality`: run format checks, linting, and type checks.

On this workstation, all-feature PyO3 checks may need `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make quality` and `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 make test`.

## Coding Style & Naming Conventions

Use Rust 2021 style with `rustfmt` and `clippy --all-features -- -D warnings`. Keep Rust module, function, and test names in `snake_case`; enum and type names in `PascalCase`. Python targets 3.10+, uses Ruff formatting with 100-character lines, double quotes, and space indentation, and is type-checked with `basedpyright`. Keep public Python exports and `_mammocat.pyi` stubs in sync with PyO3 bindings.

## Testing Guidelines

Use `pytest` for Python and Cargo tests for Rust. Name Python files `test_*.py` and functions `test_*`, matching `pyproject.toml`. Add Rust unit tests near core logic and integration tests in `core/tests/` for cross-module or CLI-adjacent behavior. Run focused tests first, then `make test` and `make quality` before handoff.

## Commit & Pull Request Guidelines

Recent history uses short imperative subjects such as `Add mammography validation reports`; `chore:` is used sparingly for maintenance. Keep commits focused, mention issue or PR numbers when relevant, and commit lockfile changes with dependency changes. Pull requests should summarize behavior changes, list validation commands run, describe CLI/Python API effects, and note DICOM fixture or data assumptions.

## Security & Configuration Tips

Do not commit DICOM PHI, tokens, or local data paths. Treat file paths, archive handling, DICOM parsing, JSON output, and Python bindings as trust boundaries; validate external inputs and keep user-facing errors concise.
