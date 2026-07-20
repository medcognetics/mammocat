"""Install a pinned Rustup binary when a CI runner does not provide one."""

from __future__ import annotations

import hashlib
import hmac
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

RUSTUP_VERSION = "1.29.0"
RUSTUP_TARGET = "x86_64-unknown-linux-gnu"
RUSTUP_INIT_SHA256 = "4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10"
RUSTUP_INIT_URL = (
    f"https://static.rust-lang.org/rustup/archive/{RUSTUP_VERSION}/{RUSTUP_TARGET}/rustup-init"
)
SUPPORTED_MACHINES = frozenset({"amd64", "x86_64"})
DOWNLOAD_TIMEOUT_SECONDS = 60
HASH_CHUNK_BYTES = 1024 * 1024
EXECUTABLE_MODE = 0o755


def cargo_home() -> Path:
    """Return the Cargo home used by Rustup."""

    configured_home = os.environ.get("CARGO_HOME")
    if configured_home:
        return Path(configured_home).expanduser().resolve()
    return Path.home() / ".cargo"


def verify_rustup_checksum(installer: Path, expected_sha256: str) -> None:
    """Reject a downloaded Rustup installer unless its SHA-256 matches."""

    digest = hashlib.sha256()
    with installer.open("rb") as installer_file:
        for chunk in iter(lambda: installer_file.read(HASH_CHUNK_BYTES), b""):
            digest.update(chunk)
    actual_sha256 = digest.hexdigest()
    if not hmac.compare_digest(actual_sha256, expected_sha256):
        message = (
            "Rustup installer checksum mismatch: "
            f"expected {expected_sha256}, received {actual_sha256}"
        )
        raise RuntimeError(message)


def github_path_file() -> Path:
    """Return the GitHub Actions path-command file or fail with context."""

    configured_path = os.environ.get("GITHUB_PATH")
    if not configured_path:
        message = "GITHUB_PATH is required when Rustup is not already on PATH"
        raise RuntimeError(message)
    return Path(configured_path)


def export_rustup_path(rustup: Path) -> None:
    """Expose Rustup and Cargo binaries to subsequent workflow steps."""

    with github_path_file().open("a", encoding="utf-8") as path_file:
        path_file.write(f"{rustup.parent}\n")


def existing_rustup() -> Path | None:
    """Find Rustup either on PATH or in the configured Cargo home."""

    path_rustup = shutil.which("rustup")
    if path_rustup:
        return Path(path_rustup)
    cargo_rustup = cargo_home() / "bin" / "rustup"
    return cargo_rustup if cargo_rustup.is_file() else None


def download_rustup(installer: Path) -> None:
    """Download the fixed Rustup installer and verify its published checksum."""

    request = urllib.request.Request(
        RUSTUP_INIT_URL,
        headers={"User-Agent": "mammocat-ci-rustup-bootstrap/1"},
    )
    try:
        with (
            urllib.request.urlopen(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response,
            installer.open("wb") as installer_file,
        ):
            shutil.copyfileobj(response, installer_file)
    except (OSError, urllib.error.URLError) as error:
        message = f"Rustup installer download failed: {error}"
        raise RuntimeError(message) from error
    verify_rustup_checksum(installer, RUSTUP_INIT_SHA256)
    installer.chmod(EXECUTABLE_MODE)


def install_rustup() -> Path:
    """Install Rustup without selecting a default toolchain or editing shell files."""

    if platform.system() != "Linux" or platform.machine().lower() not in SUPPORTED_MACHINES:
        message = f"unsupported Rustup bootstrap platform: {platform.system()} {platform.machine()}"
        raise RuntimeError(message)

    with tempfile.TemporaryDirectory(prefix="mammocat-rustup-") as temporary_directory:
        installer = Path(temporary_directory) / "rustup-init"
        download_rustup(installer)
        subprocess.run(
            [
                str(installer),
                "-y",
                "--profile",
                "minimal",
                "--default-toolchain",
                "none",
                "--no-modify-path",
            ],
            check=True,
        )

    rustup = cargo_home() / "bin" / "rustup"
    if not rustup.is_file():
        message = f"Rustup installation did not create {rustup}"
        raise RuntimeError(message)
    return rustup


def ensure_rustup() -> Path:
    """Make Rustup available to later workflow steps, installing it if needed."""

    rustup = existing_rustup()
    if rustup is None:
        rustup = install_rustup()
        print(f"Installed Rustup {RUSTUP_VERSION} at {rustup}")
    if shutil.which("rustup") is None:
        export_rustup_path(rustup)
    return rustup


if __name__ == "__main__":
    try:
        ensure_rustup()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
