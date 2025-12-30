.PHONY: help dev build build-release test test-python test-rust test-cov
.PHONY: format format-check lint lint-fix typecheck quality quality-fix clean all

help:  ## Show this help message
	@echo "Available targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# Development setup
dev:  ## Install Python development dependencies
	uv sync --extra dev

# Build commands
build:  ## Build Python bindings with maturin (debug)
	uv run maturin develop --features python

build-release:  ## Build Python bindings (release, optimized)
	uv run maturin develop --features python --release

# Testing commands
test: build test-rust test-python  ## Run all tests (rebuilds Python bindings first)

test-python:  ## Run Python tests
	uv run pytest tests/ -v

test-rust:  ## Run Rust tests only
	cargo test --features python

test-cov:  ## Run Python tests with coverage
	uv run pytest tests/ --cov=mammocat --cov-report=html --cov-report=term

# Code formatting
format:  ## Format both Rust and Python code
	cargo fmt
	uv run ruff check --select I --fix
	uv run ruff format

format-check:  ## Check formatting for both Rust and Python
	cargo fmt -- --check
	uv run ruff check --select I
	uv run ruff format --check

# Linting
lint:  ## Lint both Rust and Python code
	cargo clippy --all-features -- -D warnings
	uv run ruff check

lint-fix:  ## Fix linting issues in both Rust and Python
	cargo fix --allow-dirty --all-features
	cargo clippy --all-features --fix --allow-dirty
	uv run ruff check --fix

# Type checking
typecheck:  ## Run type checker (basedpyright for Python, cargo check for Rust)
	cargo check --all-features
	uv run basedpyright

# Quality checks
quality: format-check lint typecheck  ## Run all quality checks (format, lint, typecheck)

quality-fix: format lint-fix  ## Auto-fix all quality issues

# Cleanup
clean:  ## Clean build artifacts
	rm -rf target/
	rm -rf .pytest_cache/
	rm -rf .ruff_cache/
	rm -rf .basedpyright/
	rm -rf htmlcov/
	rm -rf .coverage
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name "*.egg-info" -exec rm -rf {} + 2>/dev/null || true

# Full workflow
all: dev build test quality  ## Install deps, build, test, and check quality

.DEFAULT_GOAL := help
