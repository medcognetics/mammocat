"""Build and smoke-test every Linux production surface without publishing artifacts."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CLI_BINARIES = (
    "mammocat",
    "mammofill",
    "mammoselect",
    "mammoplan",
    "mammovalidate",
    "dbt-combine",
)


def run(command: list[str]) -> None:
    """Run one required production verification command."""

    subprocess.run(command, cwd=ROOT, check=True)


def main() -> int:
    """Build release outputs and verify Rust, Python, and Node consumers."""

    run(["cargo", "build", "--locked", "--workspace", "--all-features", "--release"])
    for binary in CLI_BINARIES:
        run([str(ROOT / "target" / "release" / binary), "--help"])

    with tempfile.TemporaryDirectory(prefix="mammocat-production-") as temporary_directory:
        temporary_root = Path(temporary_directory)
        wheel_directory = temporary_root / "wheels"
        environment = temporary_root / "venv"
        run(
            [
                "uv",
                "run",
                "maturin",
                "build",
                "--locked",
                "--features",
                "python",
                "--release",
                "--out",
                str(wheel_directory),
            ]
        )
        wheels = list(wheel_directory.glob("*.whl"))
        if len(wheels) != 1:
            message = f"expected one release wheel, found {len(wheels)}"
            raise RuntimeError(message)
        run(["uv", "venv", "--python", "3.14", str(environment)])
        python = environment / "bin" / "python"
        run(["uv", "pip", "install", "--python", str(python), str(wheels[0])])
        run(
            [
                str(python),
                "-c",
                "import mammocat; assert callable(mammocat.MammogramExtractor.extract_from_file)",
            ]
        )

    run(["make", "node-install"])
    run(["make", "node-build"])
    run(["make", "node-typecheck"])
    run(["make", "node-test"])
    run(["make", "node-pack"])
    run(["git", "diff", "--exit-code", "--", "node/index.js", "node/index.d.ts"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
