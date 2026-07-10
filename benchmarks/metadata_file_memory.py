#!/usr/bin/env python3
"""Benchmark file-based metadata operations in isolated processes.

Fixtures contain only synthetic metadata and sparse zero-filled PixelData payloads.
On Linux, ``ru_maxrss`` and reported RSS values are in KiB.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import resource
import statistics
import struct
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from pydicom.dataset import FileDataset, FileMetaDataset
from pydicom.uid import UID, ExplicitVRLittleEndian

from mammocat import MammogramExtractor, validate_dicom

MIB = 1024 * 1024
DEFAULT_SIZES_MIB = (32, 128, 512)
DEFAULT_RUNS = 5
DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.1.2"
IMPLEMENTATION_CLASS_UID = "1.2.826.0.1.3680043.10.543.74"
PIXEL_DATA_HEADER = struct.Struct("<HH2s2sI")
OPERATIONS = ("extract", "validate")


def create_fixture(path: Path, size_mib: int) -> None:
    """Create a valid synthetic DICOM with sparse native PixelData."""
    payload_size = size_mib * MIB
    pixels = payload_size // 2
    rows = 1 << ((pixels.bit_length() - 1) // 2)
    columns = pixels // rows
    if rows * columns * 2 != payload_size or rows > 65_535 or columns > 65_535:
        message = f"cannot represent {size_mib} MiB as 16-bit DICOM Rows and Columns"
        raise ValueError(message)

    suffix = size_mib + 1
    sop_instance_uid = f"1.2.826.0.1.3680043.10.543.74.{suffix}"
    file_meta = FileMetaDataset()
    file_meta.TransferSyntaxUID = ExplicitVRLittleEndian
    file_meta.MediaStorageSOPClassUID = UID(DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID)
    file_meta.MediaStorageSOPInstanceUID = UID(sop_instance_uid)
    file_meta.ImplementationClassUID = UID(IMPLEMENTATION_CLASS_UID)

    dataset = FileDataset(str(path), {}, file_meta=file_meta, preamble=b"\0" * 128)
    dataset.SOPClassUID = DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID
    dataset.SOPInstanceUID = sop_instance_uid
    dataset.StudyInstanceUID = f"1.2.826.0.1.3680043.10.543.75.{suffix}"
    dataset.SeriesInstanceUID = f"1.2.826.0.1.3680043.10.543.76.{suffix}"
    dataset.Modality = "MG"
    dataset.ImageType = ["ORIGINAL", "PRIMARY"]
    dataset.ImageLaterality = "L"
    dataset.ViewPosition = "MLO"
    dataset.PresentationIntentType = "FOR PRESENTATION"
    dataset.Rows = rows
    dataset.Columns = columns
    dataset.SamplesPerPixel = 1
    dataset.PhotometricInterpretation = "MONOCHROME2"
    dataset.BitsAllocated = 16
    dataset.BitsStored = 16
    dataset.HighBit = 15
    dataset.PixelRepresentation = 0
    dataset.PixelSpacing = ["0.07", "0.07"]
    dataset.LossyImageCompression = "00"
    dataset.save_as(path, enforce_file_format=True)

    with path.open("r+b") as handle:
        handle.seek(0, os.SEEK_END)
        handle.write(PIXEL_DATA_HEADER.pack(0x7FE0, 0x0010, b"OW", b"\0\0", payload_size))
        handle.truncate(handle.tell() + payload_size)


def result_digest(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), default=str).encode()
    return hashlib.sha256(encoded).hexdigest()


def run_worker(operation: str, path: Path) -> dict[str, Any]:
    """Run one measured operation after imports and initialization."""
    before_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    started = time.perf_counter()
    if operation == "extract":
        result = MammogramExtractor.extract_from_file(path).to_dict()
    elif operation == "validate":
        result = validate_dicom(path)
    else:
        message = f"unknown operation: {operation}"
        raise ValueError(message)
    latency_ms = (time.perf_counter() - started) * 1_000
    after_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return {
        "operation": operation,
        "latency_ms": latency_ms,
        "incremental_max_rss_kib": max(0, after_rss - before_rss),
        "output_digest": result_digest(result),
    }


def summarize(samples: list[dict[str, Any]]) -> dict[str, Any]:
    latencies = [float(sample["latency_ms"]) for sample in samples]
    rss_values = [int(sample["incremental_max_rss_kib"]) for sample in samples]
    digests = sorted({str(sample["output_digest"]) for sample in samples})
    return {
        "runs": len(samples),
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
        "output_digests": digests,
    }


def run_benchmark(fixture_dir: Path, sizes_mib: list[int], runs: int) -> dict[str, Any]:
    fixture_dir.mkdir(parents=True, exist_ok=True)
    report: dict[str, Any] = {"runs_per_case": runs, "cases": {}}
    script = Path(__file__).resolve()

    for size_mib in sizes_mib:
        fixture = fixture_dir / f"metadata-{size_mib}mib.dcm"
        expected_size = size_mib * MIB
        if not fixture.exists() or fixture.stat().st_size < expected_size:
            create_fixture(fixture, size_mib)

        for operation in OPERATIONS:
            samples = []
            for _ in range(runs):
                completed = subprocess.run(
                    [sys.executable, str(script), "--worker", operation, str(fixture)],
                    check=True,
                    capture_output=True,
                    text=True,
                )
                samples.append(json.loads(completed.stdout))
            report["cases"][f"{operation}_{size_mib}mib"] = summarize(samples)

    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-dir", type=Path, default=Path("/tmp/mammocat-metadata-bench"))
    parser.add_argument("--sizes-mib", type=int, nargs="+", default=list(DEFAULT_SIZES_MIB))
    parser.add_argument("--runs", type=int, default=DEFAULT_RUNS)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", nargs=2, metavar=("OPERATION", "PATH"))
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.worker:
        operation, raw_path = args.worker
        print(json.dumps(run_worker(operation, Path(raw_path)), sort_keys=True))
        return

    report = run_benchmark(args.fixture_dir, args.sizes_mib, args.runs)
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if args.output:
        args.output.write_text(f"{rendered}\n")
    print(rendered)


if __name__ == "__main__":
    main()
