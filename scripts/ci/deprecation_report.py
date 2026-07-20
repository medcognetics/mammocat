"""Build a non-failing dependency deprecation and runtime lifecycle report."""

from __future__ import annotations

import importlib
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Any

from scripts.ci.reporting import run_command, write_json

ROOT = Path(__file__).resolve().parents[2]
REPORT_DIRECTORY = ROOT / "reports" / "deprecation"
NPM_VERSION = "12.0.1"
PACKAGE_NAME_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+")
MISSION_CRITICAL_FAMILIES = ["dicom-rs 0.9", "PyO3 0.22", "N-API 3.x"]
CARGO_AUDIT_FINDING_EXIT_CODES = frozenset({1})
SUCCESSFUL_CARGO_AUDIT_EXIT_CODES = frozenset({0, *CARGO_AUDIT_FINDING_EXIT_CODES})
RUNTIME_POLICIES = (
    {
        "runtime": "Python",
        "minimum": "3.10",
        "end_of_life": "2026-10-31",
        "source": "https://devguide.python.org/versions/",
    },
    {
        "runtime": "Node.js",
        "minimum": "22",
        "end_of_life": "2027-04-30",
        "source": "https://nodejs.org/en/about/previous-releases",
    },
)


def npm_command(*arguments: str) -> list[str]:
    """Build an npm metadata command pinned to the audited npm version."""

    return ["npx", "--yes", f"npm@{NPM_VERSION}", *arguments]


def requirement_name(requirement: str) -> str:
    """Extract and normalize a package name from a PEP 508 requirement."""

    match = PACKAGE_NAME_PATTERN.match(requirement.strip())
    if match is None:
        message = f"cannot parse requirement: {requirement}"
        raise ValueError(message)
    return re.sub(r"[-_.]+", "-", match.group(0)).lower()


def direct_python_requirements(pyproject: dict[str, Any]) -> dict[str, set[str]]:
    """Collect direct Python requirements by declared dependency group."""

    groups: dict[str, set[str]] = {}
    build_requirements = pyproject.get("build-system", {}).get("requires", [])
    groups["build-system"] = {requirement_name(item) for item in build_requirements}

    project = pyproject.get("project", {})
    groups["runtime"] = {requirement_name(item) for item in project.get("dependencies", [])}
    for group, requirements in project.get("optional-dependencies", {}).items():
        groups[group] = {requirement_name(item) for item in requirements}
    return groups


def locked_python_versions(uv_lock: dict[str, Any]) -> dict[str, str]:
    """Map normalized PyPI names to their locked versions."""

    versions: dict[str, str] = {}
    for package in uv_lock.get("package", []):
        name = package.get("name")
        version = package.get("version")
        if isinstance(name, str) and isinstance(version, str):
            versions[requirement_name(name)] = version
    return versions


def pypi_yanked_files(name: str, version: str) -> list[dict[str, str | None]]:
    """Return yanked files for one exact locked PyPI release."""

    encoded_name = urllib.parse.quote(name, safe="")
    encoded_version = urllib.parse.quote(version, safe="")
    request = urllib.request.Request(
        f"https://pypi.org/pypi/{encoded_name}/{encoded_version}/json",
        headers={"Accept": "application/json", "User-Agent": "mammocat-deprecation-report/1"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = json.load(response)
    files = payload.get("urls")
    if not isinstance(files, list):
        message = f"PyPI response for {name} {version} has no file list"
        raise TypeError(message)
    return [
        {
            "filename": file.get("filename"),
            "reason": file.get("yanked_reason"),
        }
        for file in files
        if file.get("yanked") is True
    ]


def direct_npm_dependencies(package: dict[str, Any]) -> dict[str, set[str]]:
    """Collect direct npm dependency specifications by package group."""

    dependency_fields = {
        "prod": "dependencies",
        "dev": "devDependencies",
        "optional": "optionalDependencies",
        "peer": "peerDependencies",
    }
    return {
        group: set(package.get(field, {}))
        for group, field in dependency_fields.items()
        if package.get(field)
    }


def locked_npm_version(lockfile: dict[str, Any], package_name: str) -> str:
    """Read one direct npm dependency version from a v3 lockfile."""

    package = lockfile.get("packages", {}).get(f"node_modules/{package_name}")
    if not isinstance(package, dict) or not isinstance(package.get("version"), str):
        message = f"no locked version for npm package {package_name}"
        raise KeyError(message)
    return package["version"]


def parse_npm_deprecated(stdout: str) -> str | None:
    """Parse the `npm view ... deprecated --json` response."""

    if not stdout.strip():
        return None
    payload = json.loads(stdout)
    if payload is None or payload == "":
        return None
    if not isinstance(payload, str):
        message = "npm deprecated field must be a string or null"
        raise TypeError(message)
    return payload


def runtime_status(end_of_life: date, today: date) -> str:
    """Classify lifecycle state, warning six months before end of life."""

    days_remaining = (end_of_life - today).days
    if days_remaining < 0:
        return "end-of-life"
    if days_remaining <= 180:
        return "approaching-end-of-life"
    return "supported"


def rustsec_notices() -> tuple[list[dict[str, Any]], list[str], dict[str, Any]]:
    """Collect RustSec unmaintained, yanked, and unsound notices."""

    command = ["cargo", "audit", "--json", "--file", "Cargo.lock"]
    result = run_command(command, ROOT)
    (REPORT_DIRECTORY / "cargo-audit.json").write_text(result.stdout, encoding="utf-8")
    (REPORT_DIRECTORY / "cargo-audit.stderr.txt").write_text(result.stderr, encoding="utf-8")
    errors: list[str] = []
    notices: list[dict[str, Any]] = []
    vulnerabilities: list[dict[str, Any]] = []
    try:
        payload = json.loads(result.stdout)
        if not isinstance(payload, dict):
            message = "Cargo Audit report must be an object"
            raise TypeError(message)
        warnings = payload.get("warnings")
        if not isinstance(warnings, dict):
            message = "Cargo Audit report has no warnings object"
            raise TypeError(message)
        vulnerability_records = payload.get("vulnerabilities", {}).get("list")
        if not isinstance(vulnerability_records, list):
            message = "Cargo Audit report has no vulnerabilities list"
            raise TypeError(message)
        vulnerabilities.extend(vulnerability_records)
        for notice_type in ("unmaintained", "yanked", "unsound"):
            records = warnings.get(notice_type, [])
            if not isinstance(records, list):
                message = f"Cargo Audit {notice_type} notices are not a list"
                raise TypeError(message)
            notices.extend({"type": notice_type, **record} for record in records)
    except (json.JSONDecodeError, TypeError) as error:
        errors.append(f"Cargo Audit report could not be parsed: {error}")
    if result.returncode not in SUCCESSFUL_CARGO_AUDIT_EXIT_CODES:
        errors.append(f"Cargo Audit returned unexpected exit code {result.returncode}")
    elif result.returncode in CARGO_AUDIT_FINDING_EXIT_CODES and not (notices or vulnerabilities):
        errors.append("Cargo Audit returned a finding exit code without a parsed finding")
    return (
        notices,
        errors,
        {
            "command": result.display_command,
            "returncode": result.returncode,
        },
    )


def future_incompatibility_report() -> tuple[list[str], dict[str, Any]]:
    """Run Cargo's future-incompatibility check and retain its full output."""

    command = [
        "cargo",
        "check",
        "--future-incompat-report",
        "--locked",
        "--workspace",
        "--all-targets",
        "--all-features",
    ]
    result = run_command(command, ROOT)
    output_path = REPORT_DIRECTORY / "cargo-future-incompat.txt"
    output_path.write_text(f"{result.stdout}{result.stderr}", encoding="utf-8")
    errors = [] if result.returncode == 0 else ["cargo check --future-incompat-report failed"]
    return errors, {
        "command": result.display_command,
        "returncode": result.returncode,
        "output": str(output_path),
    }


def python_yanked_report() -> tuple[list[dict[str, Any]], list[str]]:
    """Check every locked direct Python package for yanked release files."""

    tomllib = importlib.import_module("tomllib")
    pyproject = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    uv_lock = tomllib.loads((ROOT / "uv.lock").read_text(encoding="utf-8"))
    groups = direct_python_requirements(pyproject)
    versions = locked_python_versions(uv_lock)
    package_groups: dict[str, set[str]] = {}
    for group, packages in groups.items():
        for package in packages:
            package_groups.setdefault(package, set()).add(group)

    findings: list[dict[str, Any]] = []
    errors: list[str] = []
    for package, declared_groups in sorted(package_groups.items()):
        version = versions.get(package)
        if version is None:
            errors.append(f"no uv.lock version for direct Python package {package}")
            continue
        try:
            yanked_files = pypi_yanked_files(package, version)
        except (OSError, TypeError, ValueError, urllib.error.URLError) as error:
            errors.append(f"PyPI metadata failed for {package} {version}: {error}")
            continue
        if yanked_files:
            findings.append(
                {
                    "package": package,
                    "version": version,
                    "groups": sorted(declared_groups),
                    "yanked_files": yanked_files,
                }
            )
    return findings, errors


def local_npm_packages() -> dict[tuple[str, str], tuple[dict[str, Any], Path]]:
    """Index repository-owned optional native packages by exact name and version."""

    packages: dict[tuple[str, str], tuple[dict[str, Any], Path]] = {}
    for package_path in sorted((ROOT / "node" / "npm").glob("*/package.json")):
        package = json.loads(package_path.read_text(encoding="utf-8"))
        name = package.get("name")
        version = package.get("version")
        if isinstance(name, str) and isinstance(version, str):
            packages[(name, version)] = (package, package_path)
    return packages


def inspect_local_npm_deprecation(
    *,
    package: str,
    version: str,
    groups: set[str],
    local_packages_by_version: dict[tuple[str, str], tuple[dict[str, Any], Path]],
) -> tuple[dict[str, Any] | None, str | None, dict[str, Any]] | None:
    """Read a repository-owned package instead of querying an unpublished package."""

    local_package = local_packages_by_version.get((package, version))
    if local_package is None:
        return None
    metadata, package_path = local_package
    message = metadata.get("deprecated")
    command = {"command": f"read {package_path.relative_to(ROOT)}", "returncode": 0}
    if message is not None and not isinstance(message, str):
        error = f"local npm deprecated field is invalid for {package} {version}"
        return None, error, command
    finding = None
    if message:
        finding = {
            "package": package,
            "version": version,
            "groups": sorted(groups),
            "message": message,
        }
    return finding, None, command


def npm_deprecation_report() -> tuple[list[dict[str, Any]], list[str], list[dict[str, Any]]]:
    """Read npm's deprecation field for every exact direct dependency."""

    findings: list[dict[str, Any]] = []
    errors: list[str] = []
    commands: list[dict[str, Any]] = []
    packages: dict[tuple[str, str], set[str]] = {}
    local_packages_by_version = local_npm_packages()
    for package_path, lock_path, surface in (
        (ROOT / "package.json", ROOT / "package-lock.json", "root"),
        (ROOT / "node" / "package.json", ROOT / "node" / "package-lock.json", "node"),
    ):
        package = json.loads(package_path.read_text(encoding="utf-8"))
        lockfile = json.loads(lock_path.read_text(encoding="utf-8"))
        for group, dependencies in direct_npm_dependencies(package).items():
            for dependency in dependencies:
                try:
                    version = locked_npm_version(lockfile, dependency)
                except KeyError as error:
                    errors.append(str(error))
                    continue
                packages.setdefault((dependency, version), set()).add(f"{surface}:{group}")

    for (package, version), groups in sorted(packages.items()):
        local_result = inspect_local_npm_deprecation(
            package=package,
            version=version,
            groups=groups,
            local_packages_by_version=local_packages_by_version,
        )
        if local_result is not None:
            finding, error, command = local_result
            commands.append(command)
            if error:
                errors.append(error)
            if finding:
                findings.append(finding)
            continue

        command = npm_command("view", f"{package}@{version}", "deprecated", "--json")
        result = run_command(command, ROOT)
        commands.append({"command": result.display_command, "returncode": result.returncode})
        if result.returncode != 0:
            errors.append(
                f"npm metadata failed for {package} {version}: "
                f"{result.stderr.strip() or 'unknown error'}"
            )
            continue
        try:
            message = parse_npm_deprecated(result.stdout)
        except (json.JSONDecodeError, TypeError) as error:
            errors.append(f"npm metadata could not be parsed for {package} {version}: {error}")
            continue
        if message:
            findings.append(
                {
                    "package": package,
                    "version": version,
                    "groups": sorted(groups),
                    "message": message,
                }
            )
    return findings, errors, commands


def lifecycle_report(today: date) -> list[dict[str, str]]:
    """Report lifecycle status for every declared public runtime floor."""

    report = []
    for policy in RUNTIME_POLICIES:
        end_of_life = date.fromisoformat(policy["end_of_life"])
        report.append({**policy, "status": runtime_status(end_of_life, today)})
    report.append(
        {
            "runtime": "Rust",
            "minimum": "1.88",
            "normal_toolchain": "1.97.1",
            "status": "project-supported-msrv",
            "source": "rust-toolchain.toml and workspace Cargo.toml files",
        }
    )
    return report


def render_markdown(report: dict[str, Any]) -> str:
    """Render findings without making deprecation notices job-fatal."""

    findings = report["findings"]
    lines = [
        "# Dependency deprecation report",
        "",
        "Deprecation findings are informational. Missing or unparsable inputs fail this job.",
        "",
        "| Category | Findings |",
        "|---|---:|",
        f"| RustSec notices | {len(findings['rustsec'])} |",
        f"| Yanked PyPI releases | {len(findings['pypi_yanked'])} |",
        f"| Deprecated npm dependencies | {len(findings['npm_deprecated'])} |",
        "",
        "## Runtime lifecycle",
        "",
        "| Runtime | Minimum | Status | End of life |",
        "|---|---:|---|---:|",
    ]
    for runtime in report["runtime_lifecycle"]:
        lines.append(
            f"| {runtime['runtime']} | {runtime['minimum']} | {runtime['status']} | "
            f"{runtime.get('end_of_life', 'project policy')} |"
        )
    if report["errors"]:
        lines.extend(["", "## Incomplete report", ""])
        lines.extend(f"- {error}" for error in report["errors"])
    return "\n".join(lines) + "\n"


def main() -> int:
    """Generate JSON and Markdown reports, failing only incomplete results."""

    REPORT_DIRECTORY.mkdir(parents=True, exist_ok=True)
    rustsec, rustsec_errors, cargo_audit_command = rustsec_notices()
    future_errors, future_command = future_incompatibility_report()
    try:
        pypi_yanked, pypi_errors = python_yanked_report()
    except (KeyError, OSError, TypeError, ValueError) as error:
        pypi_yanked, pypi_errors = [], [f"Python deprecation report failed: {error}"]
    try:
        npm_deprecated, npm_errors, npm_commands = npm_deprecation_report()
    except (KeyError, OSError, TypeError, ValueError) as error:
        npm_deprecated = []
        npm_errors = [f"npm deprecation report failed: {error}"]
        npm_commands = []
    errors = [*rustsec_errors, *future_errors, *pypi_errors, *npm_errors]
    report = {
        "audited_at": datetime.now(timezone.utc).isoformat(),
        "commands": {
            "cargo_audit": cargo_audit_command,
            "cargo_future_incompat": future_command,
            "npm_metadata": npm_commands,
        },
        "errors": errors,
        "findings": {
            "npm_deprecated": npm_deprecated,
            "pypi_yanked": pypi_yanked,
            "rustsec": rustsec,
        },
        "mission_critical_dependency_families": MISSION_CRITICAL_FAMILIES,
        "runtime_lifecycle": lifecycle_report(datetime.now(timezone.utc).date()),
    }
    write_json(REPORT_DIRECTORY / "report.json", report)
    (REPORT_DIRECTORY / "report.md").write_text(render_markdown(report), encoding="utf-8")
    return 2 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
