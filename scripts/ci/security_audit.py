"""Run all scheduled security scanners and aggregate their results."""

from __future__ import annotations

import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from scripts.ci.reporting import (
    CheckResult,
    run_command,
    run_json_check,
    serialized_checks,
    write_json,
)

ROOT = Path(__file__).resolve().parents[2]
REPORT_DIRECTORY = ROOT / "reports" / "security"
NPM_VERSION = "12.0.1"
PIP_AUDIT_VERSION = "2.10.1"
ZIZMOR_VERSION = "1.27.0"
AUDITED_GROUPS = {
    "python": ["build-system", "dev", "test"],
    "npm": ["prod", "dev", "optional", "peer"],
    "rust": ["Cargo.lock", "all workspace packages and features"],
}
MISSION_CRITICAL_FAMILIES = ["dicom-rs 0.9", "PyO3 0.22", "N-API 3.x"]
STANDARD_FINDING_EXIT_CODES = frozenset({1})
NO_FINDING_EXIT_CODES: frozenset[int] = frozenset()


def npm_command(*arguments: str) -> list[str]:
    """Build an npm command pinned to the scheduled scanner version."""

    return ["npx", "--yes", f"npm@{NPM_VERSION}", *arguments]


def count_cargo_findings(payload: Any) -> int:
    """Count Cargo Audit vulnerabilities and warning records."""

    if not isinstance(payload, dict):
        message = "Cargo Audit report must be an object"
        raise TypeError(message)
    vulnerabilities = payload["vulnerabilities"]["list"]
    warnings = payload.get("warnings", {})
    if not isinstance(vulnerabilities, list) or not isinstance(warnings, dict):
        message = "Cargo Audit findings have an unexpected shape"
        raise TypeError(message)
    warning_count = sum(len(items) for items in warnings.values() if isinstance(items, list))
    return len(vulnerabilities) + warning_count


def count_pip_findings(payload: Any) -> int:
    """Count vulnerabilities in Pip Audit JSON output."""

    if not isinstance(payload, dict) or not isinstance(payload.get("dependencies"), list):
        message = "Pip Audit report must contain a dependencies list"
        raise TypeError(message)
    return sum(len(dependency.get("vulns", [])) for dependency in payload["dependencies"])


def count_npm_findings(payload: Any) -> int:
    """Read npm's total vulnerability count."""

    if not isinstance(payload, dict):
        message = "npm audit report must be an object"
        raise TypeError(message)
    total = payload["metadata"]["vulnerabilities"]["total"]
    if not isinstance(total, int):
        message = "npm vulnerability total must be an integer"
        raise TypeError(message)
    return total


def count_zizmor_findings(payload: Any) -> int:
    """Count Zizmor diagnostics in either supported JSON envelope."""

    if isinstance(payload, list):
        return len(payload)
    if isinstance(payload, dict) and isinstance(payload.get("diagnostics"), list):
        return len(payload["diagnostics"])
    message = "Zizmor report must be a list or contain diagnostics"
    raise TypeError(message)


def command_version(command: list[str]) -> tuple[str | None, str | None]:
    """Capture a scanner version without aborting the report."""

    result = run_command(command, ROOT)
    if result.returncode != 0:
        return None, result.stderr.strip() or "version command failed"
    return result.stdout.strip() or result.stderr.strip(), None


def advisory_database_revision() -> tuple[str | None, str | None]:
    """Read the RustSec database revision populated by Cargo Audit."""

    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    result = run_command(["git", "-C", str(cargo_home / "advisory-db"), "rev-parse", "HEAD"], ROOT)
    if result.returncode != 0:
        return None, result.stderr.strip() or "RustSec advisory database revision unavailable"
    return result.stdout.strip(), None


def export_python_requirements() -> CheckResult:
    """Export every locked Python dependency group before scanning it."""

    output_path = REPORT_DIRECTORY / "python-requirements.txt"
    command = [
        "uv",
        "export",
        "--frozen",
        "--all-groups",
        "--all-extras",
        "--no-emit-project",
        "--format",
        "requirements-txt",
        "--output-file",
        str(output_path),
    ]
    result = run_command(command, ROOT)
    (REPORT_DIRECTORY / "python-export.stderr.txt").write_text(result.stderr, encoding="utf-8")
    complete = result.returncode == 0 and output_path.is_file() and output_path.stat().st_size > 0
    return CheckResult(
        name="python-export",
        command=result.display_command,
        returncode=result.returncode,
        status="passed" if complete else "error",
        finding_count=0,
        output=str(output_path),
        error=None if complete else "uv export did not produce a complete requirements file",
    )


def render_markdown(checks: list[CheckResult]) -> str:
    """Render a compact report suitable for artifacts and job summaries."""

    lines = [
        "# Security audit",
        "",
        "| Check | Status | Findings | Exit code |",
        "|---|---:|---:|---:|",
    ]
    lines.extend(
        f"| {check.name} | {check.status} | {check.finding_count} | {check.returncode} |"
        for check in checks
    )
    errors = [check for check in checks if check.error]
    if errors:
        lines.extend(["", "## Incomplete scans", ""])
        lines.extend(f"- `{check.name}`: {check.error}" for check in errors)
    return "\n".join(lines) + "\n"


def main() -> int:
    """Run every scanner, persist all reports, then return the aggregate status."""

    REPORT_DIRECTORY.mkdir(parents=True, exist_ok=True)
    checks = [export_python_requirements()]
    checks.extend(
        [
            run_json_check(
                name="cargo-audit",
                command=[
                    "cargo",
                    "audit",
                    "--json",
                    "--deny",
                    "warnings",
                    "--file",
                    "Cargo.lock",
                ],
                cwd=ROOT,
                output_path=REPORT_DIRECTORY / "cargo-audit.json",
                count_findings=count_cargo_findings,
                finding_exit_codes=STANDARD_FINDING_EXIT_CODES,
            ),
            run_json_check(
                name="pip-audit",
                command=[
                    "uvx",
                    "--from",
                    f"pip-audit=={PIP_AUDIT_VERSION}",
                    "pip-audit",
                    "--requirement",
                    str(REPORT_DIRECTORY / "python-requirements.txt"),
                    "--format",
                    "json",
                ],
                cwd=ROOT,
                output_path=REPORT_DIRECTORY / "pip-audit.json",
                count_findings=count_pip_findings,
                finding_exit_codes=STANDARD_FINDING_EXIT_CODES,
            ),
            run_json_check(
                name="npm-audit-root",
                command=npm_command(
                    "audit",
                    "--package-lock-only",
                    "--json",
                    "--audit-level=low",
                    "--include=prod",
                    "--include=dev",
                    "--include=optional",
                    "--include=peer",
                ),
                cwd=ROOT,
                output_path=REPORT_DIRECTORY / "npm-root.json",
                count_findings=count_npm_findings,
                finding_exit_codes=STANDARD_FINDING_EXIT_CODES,
            ),
            run_json_check(
                name="npm-audit-node",
                command=npm_command(
                    "audit",
                    "--package-lock-only",
                    "--json",
                    "--audit-level=low",
                    "--include=prod",
                    "--include=dev",
                    "--include=optional",
                    "--include=peer",
                ),
                cwd=ROOT / "node",
                output_path=REPORT_DIRECTORY / "npm-node.json",
                count_findings=count_npm_findings,
                finding_exit_codes=STANDARD_FINDING_EXIT_CODES,
            ),
            run_json_check(
                name="zizmor",
                command=[
                    "uvx",
                    "--from",
                    f"zizmor=={ZIZMOR_VERSION}",
                    "zizmor",
                    "--pedantic",
                    "--strict-collection",
                    "--no-exit-codes",
                    "--format=json",
                    ".github/workflows",
                ],
                cwd=ROOT,
                output_path=REPORT_DIRECTORY / "zizmor.json",
                count_findings=count_zizmor_findings,
                finding_exit_codes=NO_FINDING_EXIT_CODES,
            ),
        ]
    )

    scanner_commands = {
        "cargo-audit": ["cargo", "audit", "--version"],
        "npm": npm_command("--version"),
        "pip-audit": [
            "uvx",
            "--from",
            f"pip-audit=={PIP_AUDIT_VERSION}",
            "pip-audit",
            "--version",
        ],
        "uv": ["uv", "--version"],
        "zizmor": [
            "uvx",
            "--from",
            f"zizmor=={ZIZMOR_VERSION}",
            "zizmor",
            "--version",
        ],
    }
    versions: dict[str, str | None] = {}
    version_errors: dict[str, str] = {}
    for scanner, command in scanner_commands.items():
        version, error = command_version(command)
        versions[scanner] = version
        if error:
            version_errors[scanner] = error

    database_revision, database_error = advisory_database_revision()
    metadata = {
        "advisory_source": "https://github.com/RustSec/advisory-db",
        "audited_at": datetime.now(timezone.utc).isoformat(),
        "audited_lockfiles": [
            "Cargo.lock",
            "uv.lock",
            "package-lock.json",
            "node/package-lock.json",
        ],
        "database_revision": database_revision,
        "database_revision_error": database_error,
        "included_dependency_groups": AUDITED_GROUPS,
        "mission_critical_dependency_families": MISSION_CRITICAL_FAMILIES,
        "scanner_version_errors": version_errors,
        "scanner_versions": versions,
        "checks": serialized_checks(checks),
    }
    write_json(REPORT_DIRECTORY / "metadata.json", metadata)
    (REPORT_DIRECTORY / "report.md").write_text(render_markdown(checks), encoding="utf-8")

    if database_error or version_errors or any(check.status == "error" for check in checks):
        return 2
    if any(check.finding_count for check in checks):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
