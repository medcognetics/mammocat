"""Tests for collection-level mammography input planning."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from mammocat import plan_mammography_collection

from .conftest import create_mammogram_dicom, create_old_format_dbt_series


def _write_ffdm(path: Path) -> Path:
    ds = create_mammogram_dicom(
        mammogram_type="FFDM",
        laterality="L",
        view_position="MLO",
        study_instance_uid="1.2.826.0.1.3680043.10.700.1",
        series_instance_uid="1.2.826.0.1.3680043.10.700.1.1",
        sop_instance_uid="1.2.826.0.1.3680043.10.700.1.1.1",
    )
    ds.PresentationIntentType = "FOR PRESENTATION"
    ds.save_as(path, enforce_file_format=True)
    return path


def _run_cli(binary: str, *args: str) -> subprocess.CompletedProcess[str]:
    command = [
        "cargo",
        "run",
        "--quiet",
        "--all-features",
        "--bin",
        binary,
        "--",
        *args,
    ]
    return subprocess.run(command, check=False, capture_output=True, text=True)


def test_plan_mammography_collection_combines_2d_views_and_dbt(tmp_path: Path) -> None:
    _write_ffdm(tmp_path / "l_mlo.dcm")
    create_old_format_dbt_series(tmp_path)

    report = plan_mammography_collection(tmp_path)

    assert report["plan"] == {"include_2d": True, "include_dbt": True}
    assert report["summary"]["views_selected"] == 1
    assert report["summary"]["dbt_composition_inputs"] == 1
    selected_views = report["views"]["selected_views"].values()
    assert any(view["selected"] for view in selected_views)
    composition = report["dbt"]["composition_inputs"][0]
    assert composition["frame_count"] == 3
    assert len(composition["source_paths"]) == 3
    assert any(
        "dbt_composition_source" in role
        for source in report["source_objects"]
        for role in source["selected_as"]
    )


def test_plan_mammography_collection_can_select_only_dbt(tmp_path: Path) -> None:
    _write_ffdm(tmp_path / "l_mlo.dcm")
    create_old_format_dbt_series(tmp_path)

    report = plan_mammography_collection(
        tmp_path,
        include_2d=False,
        include_dbt=True,
    )

    assert report["plan"] == {"include_2d": False, "include_dbt": True}
    assert report["views"] is None
    assert report["dbt"]["composition_inputs"]


def test_mammoplan_json_output(tmp_path: Path) -> None:
    _write_ffdm(tmp_path / "l_mlo.dcm")
    create_old_format_dbt_series(tmp_path)

    result = _run_cli(
        "mammoplan",
        str(tmp_path),
        "--format",
        "json",
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["plan"] == {"include_2d": True, "include_dbt": True}
    assert report["summary"]["dbt_composition_inputs"] == 1


def test_mammoplan_include_flags_select_exact_input_groups(tmp_path: Path) -> None:
    _write_ffdm(tmp_path / "l_mlo.dcm")
    create_old_format_dbt_series(tmp_path)

    result = _run_cli(
        "mammoplan",
        str(tmp_path),
        "--include-dbt",
        "--format",
        "json",
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["plan"] == {"include_2d": False, "include_dbt": True}
    assert report["views"] is None
    assert report["dbt"]["composition_inputs"]


def test_mammoselect_no_longer_accepts_plan_flag(tmp_path: Path) -> None:
    _write_ffdm(tmp_path / "l_mlo.dcm")

    result = _run_cli("mammoselect", str(tmp_path), "--plan", "2d")

    assert result.returncode != 0
    assert "unexpected argument '--plan'" in result.stderr
