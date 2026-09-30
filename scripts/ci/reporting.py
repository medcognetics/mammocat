"""Shared command and report helpers for scheduled CI checks."""

from __future__ import annotations

import json
import shlex
import subprocess
from collections.abc import Callable
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

MAX_LISTED_FINDINGS = 20
UNKNOWN_TEXT = "unknown"


@dataclass(frozen=True)
class CommandResult:
    """Captured command execution without interpreting its exit status."""

    command: list[str]
    returncode: int
    stdout: str
    stderr: str

    @property
    def display_command(self) -> str:
        """Return a shell-readable command for audit metadata."""

        return shlex.join(self.command)


@dataclass(frozen=True)
class CheckResult:
    """Normalized status for one scanner or report-building step."""

    name: str
    command: str
    returncode: int
    status: str
    finding_count: int
    output: str
    error: str | None = None
    findings: tuple[str, ...] = ()


def report_text(value: Any, *keys: str) -> str:
    """Read one nested scalar for report text without failing on unexpected shapes."""

    for key in keys:
        if not isinstance(value, dict):
            return UNKNOWN_TEXT
        value = value.get(key)
    if value is None or value == "" or isinstance(value, (dict, list)):
        return UNKNOWN_TEXT
    return " ".join(str(value).split())


def limited_findings(findings: list[str] | tuple[str, ...], command: str) -> list[str]:
    """Render at most `MAX_LISTED_FINDINGS` findings as Markdown list items."""

    lines = [f"- {finding}" for finding in findings[:MAX_LISTED_FINDINGS]]
    omitted = len(findings) - MAX_LISTED_FINDINGS
    if omitted > 0:
        lines.append(f"- {omitted} more not shown; run `{command}` locally.")
    return lines


def run_command(command: list[str], cwd: Path) -> CommandResult:
    """Run a command and capture all output so sibling checks can continue."""

    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        return CommandResult(command, 127, "", str(error))
    return CommandResult(command, completed.returncode, completed.stdout, completed.stderr)


def run_json_check(
    *,
    name: str,
    command: list[str],
    cwd: Path,
    output_path: Path,
    count_findings: Callable[[Any], int],
    finding_exit_codes: frozenset[int],
    describe_findings: Callable[[Any], list[str]] | None = None,
) -> CheckResult:
    """Run one JSON scanner and distinguish findings from scanner failures."""

    result = run_command(command, cwd)
    output_path.write_text(result.stdout, encoding="utf-8")
    output_path.with_suffix(".stderr.txt").write_text(result.stderr, encoding="utf-8")

    try:
        payload = json.loads(result.stdout)
        finding_count = count_findings(payload)
        findings = tuple(describe_findings(payload)) if describe_findings else ()
    except (json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
        return CheckResult(
            name=name,
            command=result.display_command,
            returncode=result.returncode,
            status="error",
            finding_count=0,
            output=str(output_path),
            error=f"invalid JSON report: {error}",
        )

    if result.returncode != 0 and result.returncode not in finding_exit_codes:
        return CheckResult(
            name=name,
            command=result.display_command,
            returncode=result.returncode,
            status="error",
            finding_count=finding_count,
            output=str(output_path),
            error=f"scanner returned unexpected exit code {result.returncode}",
        )

    if result.returncode in finding_exit_codes and finding_count == 0:
        return CheckResult(
            name=name,
            command=result.display_command,
            returncode=result.returncode,
            status="error",
            finding_count=0,
            output=str(output_path),
            error="scanner returned a finding exit code without a parsed finding",
        )

    return CheckResult(
        name=name,
        command=result.display_command,
        returncode=result.returncode,
        status="findings" if finding_count else "passed",
        finding_count=finding_count,
        output=str(output_path),
        findings=findings,
    )


def write_json(path: Path, payload: Any) -> None:
    """Write deterministic, human-readable JSON."""

    path.write_text(f"{json.dumps(payload, indent=2, sort_keys=True)}\n", encoding="utf-8")


def serialized_checks(checks: list[CheckResult]) -> list[dict[str, Any]]:
    """Convert immutable check records into JSON-compatible dictionaries."""

    return [asdict(check) for check in checks]
