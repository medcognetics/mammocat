.PHONY: help dev build build-release install test test-python test-rust test-cov
.PHONY: node-install node-build node-test node-test-git-install node-typecheck node-pack
.PHONY: format format-check lint lint-fix typecheck quality quality-fix clean all
.PHONY: verify-production security-audit deprecation-report

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

install:  ## Install CLI binaries to ~/.cargo/bin
	cargo install --path core

# Testing commands
test: build test-rust test-python  ## Run all tests (rebuilds Python bindings first)

test-python:  ## Run Python tests
	uv run pytest tests/ -v

test-rust:  ## Run Rust tests only
	# Exercise every supported runtime surface, including Python and JSON contracts.
	cargo test --all-features

test-cov:  ## Run Python tests with coverage
	uv run pytest tests/ --cov=mammocat --cov-report=html --cov-report=term

# Node/TypeScript bindings
node-install:  ## Install Node development dependencies
	npm ci --omit=optional --ignore-scripts
	npm --prefix node ci --omit=optional

node-build:  ## Build Node/TypeScript native bindings
	npm --prefix node run build

node-test:  ## Run Node/TypeScript binding tests
	npm --prefix node test

node-test-git-install:  ## Test commit-pinned npm Git installation and clean reinstall
	npm --prefix node run test:git-install

node-typecheck:  ## Type-check generated Node/TypeScript declarations
	npm --prefix node run typecheck

node-pack:  ## Verify Node package contents without publishing
	npm --prefix node run pack:dry-run
	npm pack --dry-run

# CI verification and reports
verify-production:  ## Build and smoke-test production Rust, Python, and Node surfaces
	python -m scripts.ci.verify_production

security-audit:  ## Aggregate security scans and fail on findings or incomplete results
	python -m scripts.ci.security_audit

deprecation-report:  ## Report deprecations and fail only when the report is incomplete
	PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv run --no-project --python 3.14 python -m scripts.ci.deprecation_report

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
	rm -rf node/node_modules/
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name "*.egg-info" -exec rm -rf {} + 2>/dev/null || true

# Full workflow
all: dev build test quality  ## Install deps, build, test, and check quality

.DEFAULT_GOAL := help
