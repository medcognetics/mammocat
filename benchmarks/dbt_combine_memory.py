#!/usr/bin/env python3
"""Benchmark DBT volume composition in isolated processes using synthetic data."""

from __future__ import annotations

import argparse
import json
import math
import os
import resource
import shutil
import statistics
import struct
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, NoReturn

import pydicom

from mammocat import BREAST_TOMOSYNTHESIS_SOP_CLASS_UID, convert_dbt_study

MIB = 1024 * 1024
BYTES_PER_PIXEL = 2
FRAME_COUNT = 32
DEFAULT_CASES_MIB = (64, 256, 1024)
DEFAULT_RUNS = 5
PIXEL_DATA_HEADER = struct.Struct("<HH2s2sI")


def fail_benchmark(message: str) -> NoReturn:
    raise AssertionError(message)


def create_series(root: Path, volume_mib: int) -> Path:
    """Create sparse, synthetic single-frame inputs for one target volume size."""
    tests_dir = Path(__file__).resolve().parents[1] / "tests"
    sys.path.insert(0, str(tests_dir))
    from conftest import create_old_format_dbt_slice  # noqa: I001, PLC0415  # pyright: ignore[reportMissingImports]

    frame_pixels = volume_mib * MIB // FRAME_COUNT // BYTES_PER_PIXEL
    dimension = math.isqrt(frame_pixels)
    if dimension * dimension != frame_pixels:
        fail_benchmark(f"{volume_mib} MiB cannot be represented by square benchmark frames")
    frame_bytes = dimension * dimension * BYTES_PER_PIXEL
    series = root / f"series-{volume_mib}mib"
    series.mkdir(parents=True, exist_ok=True)
    marker = series / ".complete"
    marker_value = f"{FRAME_COUNT}:{dimension}:{dimension}"
    if (
        marker.exists()
        and marker.read_text() == marker_value
        and len(list(series.glob("*.dcm"))) == FRAME_COUNT
    ):
        return series

    shutil.rmtree(series)
    series.mkdir(parents=True)
    study_uid = f"1.2.826.0.1.3680043.10.543.80.{volume_mib}"
    series_uid = f"{study_uid}.1"
    for index in range(FRAME_COUNT):
        path = series / f"slice-{index:04}.dcm"
        dataset = create_old_format_dbt_slice(
            study_uid=study_uid,
            series_uid=series_uid,
            sop_uid=f"{series_uid}.{index + 1}",
            instance_number=index,
            rows=1,
            columns=1,
            pixel_value=index,
        )
        dataset.Rows = dimension
        dataset.Columns = dimension
        del dataset.PixelData
        dataset.save_as(path, enforce_file_format=True)
        with path.open("r+b") as handle:
            handle.seek(0, os.SEEK_END)
            handle.write(PIXEL_DATA_HEADER.pack(0x7FE0, 0x0010, b"OW", b"\0\0", frame_bytes))
            handle.write(struct.pack("<H", index))
            handle.truncate(handle.tell() + frame_bytes - BYTES_PER_PIXEL)
    marker.write_text(marker_value)
    return series


def verify_output(path: Path, frame_count: int) -> None:
    metadata = pydicom.dcmread(path, stop_before_pixels=True)
    if int(metadata.NumberOfFrames) != frame_count:
        fail_benchmark("output frame count does not match source ordering")
    if str(metadata.SOPClassUID) != BREAST_TOMOSYNTHESIS_SOP_CLASS_UID:
        fail_benchmark("output SOP class is not Breast Tomosynthesis Image Storage")
    if len(metadata.PerFrameFunctionalGroupsSequence) != frame_count:
        fail_benchmark("per-frame metadata count does not match frame count")

    frame_bytes = (
        int(metadata.Rows)
        * int(metadata.Columns)
        * int(metadata.SamplesPerPixel)
        * (int(metadata.BitsAllocated) // 8)
    )
    pixel_header_offset = path.stat().st_size - frame_count * frame_bytes - PIXEL_DATA_HEADER.size
    sample_frames = sorted({0, frame_count // 2, frame_count - 1})
    with path.open("rb") as handle:
        handle.seek(pixel_header_offset)
        group, element, vr, reserved, length = PIXEL_DATA_HEADER.unpack(handle.read(12))
        if (group, element, vr, reserved, length) != (
            0x7FE0,
            0x0010,
            b"OW",
            b"\0\0",
            frame_count * frame_bytes,
        ):
            fail_benchmark("output PixelData header is not conformant")
        for frame_index in sample_frames:
            handle.seek(pixel_header_offset + 12 + frame_index * frame_bytes)
            value = struct.unpack("<H", handle.read(BYTES_PER_PIXEL))[0]
            if value != frame_index:
                fail_benchmark("output pixels are not in source frame order")


def run_worker(input_path: Path, output_path: Path) -> dict[str, Any]:
    if output_path.exists():
        shutil.rmtree(output_path)
    before_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    started = time.perf_counter()
    report = convert_dbt_study(input_path, output_path)
    latency_ms = (time.perf_counter() - started) * 1_000
    after_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss

    converted = report["converted_series"]
    if len(converted) != 1:
        fail_benchmark("benchmark expected exactly one converted series")
    frame_count = int(converted[0]["frame_count"])
    verify_output(Path(converted[0]["output_path"]), frame_count)
    return {
        "latency_ms": latency_ms,
        "incremental_max_rss_kib": max(0, after_rss - before_rss),
        "frame_count": frame_count,
        "verified": True,
    }


def summarize(samples: list[dict[str, Any]]) -> dict[str, Any]:
    latencies = [float(sample["latency_ms"]) for sample in samples]
    rss_values = [int(sample["incremental_max_rss_kib"]) for sample in samples]
    return {
        "runs": len(samples),
        "frame_count": samples[0]["frame_count"],
        "verified": all(bool(sample["verified"]) for sample in samples),
        "latency_ms": {
            "median": statistics.median(latencies),
            "mean": statistics.mean(latencies),
            "stdev": statistics.stdev(latencies) if len(latencies) > 1 else 0.0,
        },
        "incremental_max_rss_kib": {
            "median": statistics.median(rss_values),
            "mean": statistics.mean(rss_values),
            "stdev": statistics.stdev(rss_values) if len(rss_values) > 1 else 0.0,
        },
    }


def run_benchmark(fixture_dir: Path, cases_mib: list[int], runs: int) -> dict[str, Any]:
    fixture_dir.mkdir(parents=True, exist_ok=True)
    script = Path(__file__).resolve()
    report: dict[str, Any] = {"runs_per_case": runs, "cases": {}}
    for volume_mib in cases_mib:
        input_path = create_series(fixture_dir, volume_mib)
        output_path = fixture_dir / f"output-{volume_mib}mib"
        samples = []
        for _ in range(runs):
            completed = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "--worker",
                    str(input_path),
                    str(output_path),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            samples.append(json.loads(completed.stdout))
        report["cases"][f"combine_{volume_mib}mib"] = summarize(samples)
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-dir", type=Path, default=Path("/tmp/mammocat-dbt-bench"))
    parser.add_argument("--cases-mib", type=int, nargs="+", default=list(DEFAULT_CASES_MIB))
    parser.add_argument("--runs", type=int, default=DEFAULT_RUNS)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", nargs=2, metavar=("INPUT", "OUTPUT"))
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.worker:
        input_path, output_path = args.worker
        print(json.dumps(run_worker(Path(input_path), Path(output_path)), sort_keys=True))
        return
    report = run_benchmark(args.fixture_dir, args.cases_mib, args.runs)
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if args.output:
        args.output.write_text(f"{rendered}\n")
    print(rendered)


if __name__ == "__main__":
    main()
