"""Regression tests for CI report parsing and policy classification."""

import json
from datetime import date
from pathlib import Path

import pytest
from scripts.ci import deprecation_report, reporting
from scripts.ci.bootstrap_rustup import ensure_rustup, verify_rustup_checksum
from scripts.ci.deprecation_report import (
    direct_npm_dependencies,
    direct_python_requirements,
    locked_npm_version,
    parse_npm_deprecated,
    requirement_name,
    runtime_status,
)
from scripts.ci.reporting import CommandResult, run_command, run_json_check
from scripts.ci.security_audit import (
    count_cargo_findings,
    count_npm_findings,
    count_pip_findings,
    count_zizmor_findings,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
EXPECTED_FINDING_EXIT_CODE = 1
UNEXPECTED_SCANNER_EXIT_CODE = 2
INVALID_SHA256 = "0" * 64
BERYL_WORKFLOWS = ("ci.yml", "production-build.yml", "slow-linux.yml")
HOSTED_CACHE_WORKFLOWS = ("platforms.yml", "dependency-health.yml")
CARGO_DOWNLOAD_CACHE_PATHS = {
    "~/.cargo/git/db",
    "~/.cargo/registry/cache",
    "~/.cargo/registry/index",
}


def test_security_parsers_count_each_scanner_report() -> None:
    cargo_report = {
        "vulnerabilities": {"list": [{"advisory": {"id": "RUSTSEC-test"}}]},
        "warnings": {"unmaintained": [{"package": {"name": "old"}}], "yanked": []},
    }
    pip_report = {"dependencies": [{"name": "pkg", "vulns": [{"id": "PYSEC-test"}]}]}
    npm_report = {"metadata": {"vulnerabilities": {"total": 3}}}

    assert count_cargo_findings(cargo_report) == 2
    assert count_pip_findings(pip_report) == 1
    assert count_npm_findings(npm_report) == 3
    assert count_zizmor_findings([{"ident": "unpinned-uses"}]) == 1


def test_security_parsers_reject_incomplete_reports() -> None:
    with pytest.raises((KeyError, TypeError)):
        count_cargo_findings({})
    with pytest.raises((KeyError, TypeError)):
        count_npm_findings({})
    with pytest.raises(TypeError):
        count_zizmor_findings({})


def test_direct_dependency_parsers_preserve_declared_groups() -> None:
    pyproject = {
        "build-system": {"requires": ["maturin>=1,<2"]},
        "project": {
            "dependencies": [],
            "optional-dependencies": {"dev": ["pytest-cov>=4", "pydicom>=2.4"]},
        },
    }
    package = {
        "dependencies": {"runtime-package": "1.0.0"},
        "devDependencies": {"dev-package": "2.0.0"},
    }

    assert requirement_name("pytest_cov[tests]>=4") == "pytest-cov"
    assert direct_python_requirements(pyproject) == {
        "build-system": {"maturin"},
        "runtime": set(),
        "dev": {"pytest-cov", "pydicom"},
    }
    assert direct_npm_dependencies(package) == {
        "prod": {"runtime-package"},
        "dev": {"dev-package"},
    }


def test_npm_metadata_parsing_uses_exact_locked_version() -> None:
    lockfile = {"packages": {"node_modules/pkg": {"version": "1.2.3"}}}

    assert locked_npm_version(lockfile, "pkg") == "1.2.3"
    assert parse_npm_deprecated(json.dumps("replace with maintained-pkg")) == (
        "replace with maintained-pkg"
    )
    assert parse_npm_deprecated("null") is None


def test_runtime_status_warns_before_and_after_end_of_life() -> None:
    end_of_life = date(2026, 10, 31)

    assert runtime_status(end_of_life, date(2026, 1, 1)) == "supported"
    assert runtime_status(end_of_life, date(2026, 7, 20)) == "approaching-end-of-life"
    assert runtime_status(end_of_life, date(2026, 11, 1)) == "end-of-life"


def test_missing_scanner_is_recorded_instead_of_aborting(tmp_path) -> None:
    result = run_command(["mammocat-scanner-that-does-not-exist"], tmp_path)

    assert result.returncode == 127
    assert result.stdout == ""
    assert result.stderr


def test_unexpected_scanner_exit_with_findings_is_incomplete(monkeypatch, tmp_path) -> None:
    payload = {"findings": 1}

    def unexpected_scanner_result(command: list[str], _cwd: Path) -> CommandResult:
        return CommandResult(
            command=command,
            returncode=UNEXPECTED_SCANNER_EXIT_CODE,
            stdout=json.dumps(payload),
            stderr="scanner failed after producing partial output",
        )

    monkeypatch.setattr(reporting, "run_command", unexpected_scanner_result)

    result = run_json_check(
        name="test-scanner",
        command=["test-scanner", "--json"],
        cwd=tmp_path,
        output_path=tmp_path / "report.json",
        count_findings=lambda report: report["findings"],
        finding_exit_codes=frozenset({EXPECTED_FINDING_EXIT_CODE}),
    )

    assert result.status == "error"
    assert result.finding_count == 1
    assert result.error == "scanner returned unexpected exit code 2"


def test_rustsec_notices_rejects_unexpected_exit_with_findings(monkeypatch, tmp_path) -> None:
    payload = {
        "vulnerabilities": {"list": [{"advisory": {"id": "RUSTSEC-test"}}]},
        "warnings": {"unmaintained": [], "unsound": [], "yanked": []},
    }

    def unexpected_cargo_audit_result(command: list[str], _cwd: Path) -> CommandResult:
        return CommandResult(
            command=command,
            returncode=UNEXPECTED_SCANNER_EXIT_CODE,
            stdout=json.dumps(payload),
            stderr="Cargo Audit failed after producing partial output",
        )

    monkeypatch.setattr(deprecation_report, "REPORT_DIRECTORY", tmp_path)
    monkeypatch.setattr(deprecation_report, "run_command", unexpected_cargo_audit_result)

    _notices, errors, _metadata = deprecation_report.rustsec_notices()

    assert errors == ["Cargo Audit returned unexpected exit code 2"]


def test_native_package_dry_run_uses_a_shell_independent_matrix_path() -> None:
    workflow = (REPOSITORY_ROOT / ".github" / "workflows" / "platforms.yml").read_text(
        encoding="utf-8"
    )

    assert "working-directory: node/npm/${{ matrix.package-directory }}" in workflow
    assert "$NATIVE_PACKAGE_DIRECTORY" not in workflow


def test_deprecation_report_target_selects_python_314() -> None:
    makefile = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")

    assert "uv run --no-project --python 3.14 python -m scripts.ci.deprecation_report" in makefile


def test_existing_rustup_is_exported_without_downloading(monkeypatch, tmp_path) -> None:
    cargo_home = tmp_path / "cargo"
    rustup = cargo_home / "bin" / "rustup"
    rustup.parent.mkdir(parents=True)
    rustup.write_text("#!/bin/sh\n", encoding="utf-8")
    rustup.chmod(0o755)
    github_path = tmp_path / "github-path"

    monkeypatch.setenv("CARGO_HOME", str(cargo_home))
    monkeypatch.setenv("GITHUB_PATH", str(github_path))
    monkeypatch.setenv("PATH", "")

    assert ensure_rustup() == rustup
    assert github_path.read_text(encoding="utf-8") == f"{rustup.parent}\n"


def test_rustup_checksum_mismatch_is_rejected(tmp_path) -> None:
    rustup_init = tmp_path / "rustup-init"
    rustup_init.write_bytes(b"unexpected installer")

    with pytest.raises(RuntimeError, match="Rustup installer checksum mismatch"):
        verify_rustup_checksum(rustup_init, INVALID_SHA256)


def test_beryl_workflows_bootstrap_rustup_after_python_setup() -> None:
    workflow_directory = REPOSITORY_ROOT / ".github" / "workflows"

    for workflow_name in BERYL_WORKFLOWS:
        workflow = (workflow_directory / workflow_name).read_text(encoding="utf-8")
        python_setup = workflow.index("actions/setup-python@")
        rustup_bootstrap = workflow.index("python scripts/ci/bootstrap_rustup.py")
        toolchain_install = workflow.index("rustup toolchain install")

        assert python_setup < rustup_bootstrap < toolchain_install


def test_frozen_uv_workflows_have_a_committed_lockfile() -> None:
    uv_lock = REPOSITORY_ROOT / "uv.lock"
    ignored_paths = {
        line.strip()
        for line in (REPOSITORY_ROOT / ".gitignore").read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }

    assert uv_lock.is_file()
    assert uv_lock.name not in ignored_paths


def test_beryl_workflows_do_not_transfer_dependency_caches() -> None:
    workflow_directory = REPOSITORY_ROOT / ".github" / "workflows"

    for workflow_name in BERYL_WORKFLOWS:
        workflow = (workflow_directory / workflow_name).read_text(encoding="utf-8")

        assert "uses: actions/cache@" not in workflow


def test_explicit_binding_builds_skip_project_install_during_uv_sync() -> None:
    workflow_directory = REPOSITORY_ROOT / ".github" / "workflows"

    for workflow_name in BERYL_WORKFLOWS:
        workflow = (workflow_directory / workflow_name).read_text(encoding="utf-8")
        sync_commands = [line.strip() for line in workflow.splitlines() if "uv sync" in line]

        assert 'UV_NO_SYNC: "1"' in workflow
        assert sync_commands
        assert all("--no-install-project" in command for command in sync_commands)


def test_hosted_cargo_caches_only_store_download_archives() -> None:
    workflow_directory = REPOSITORY_ROOT / ".github" / "workflows"

    for workflow_name in HOSTED_CACHE_WORKFLOWS:
        workflow = (workflow_directory / workflow_name).read_text(encoding="utf-8")
        configured_paths = {line.strip() for line in workflow.splitlines()}

        assert "~/.cargo/registry" not in configured_paths
        assert "~/.cargo/git" not in configured_paths
        assert configured_paths >= CARGO_DOWNLOAD_CACHE_PATHS


def test_dependency_health_caches_are_scoped_by_ecosystem() -> None:
    workflow = (REPOSITORY_ROOT / ".github" / "workflows" / "dependency-health.yml").read_text(
        encoding="utf-8"
    )

    assert "Restore scanner downloads" not in workflow
    assert "Restore report downloads" not in workflow
    assert workflow.count("- name: Restore Cargo downloads") == 2
    assert workflow.count("- name: Restore uv downloads") == 1
    assert workflow.count("- name: Restore npm downloads") == 2
    assert "hashFiles('Cargo.lock', 'uv.lock'" not in workflow
