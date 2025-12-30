//! Python wrappers for preferred view selection functions

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::enums::{PyMammogramView, PyPreferenceOrder};
use super::filter::PyFilterConfig;
use super::record::PyMammogramRecord;

/// Select preferred views from a collection of mammogram records (using default preference order)
///
/// For each of the 4 standard views (L-MLO, R-MLO, L-CC, R-CC), selects the
/// most preferred mammogram based on comparison logic using the default
/// preference order (FFDM > SYNTH > TOMO > SFM).
///
/// Args:
///     records: List of MammogramRecord objects to select from
///
/// Returns:
///     dict: Dictionary mapping MammogramView to MammogramRecord (or None if not found)
///
/// Example:
///     >>> from mammocat import MammogramRecord, get_preferred_views
///     >>> from pathlib import Path
///     >>> records = [MammogramRecord.from_file(f) for f in Path("dicoms").glob("*.dcm")]
///     >>> selections = get_preferred_views(records)
///     >>> for view, record in selections.items():
///     ...     if record:
///     ...         print(f"{view}: {record.file_path}")
#[pyfunction]
#[pyo3(name = "get_preferred_views")]
pub fn py_get_preferred_views(py: Python, records: Vec<PyMammogramRecord>) -> PyResult<Py<PyDict>> {
    // Convert Python records to Rust records
    let rust_records: Vec<_> = records.into_iter().map(|r| r.inner).collect();

    // Call Rust function
    let result = crate::selection::get_preferred_views(&rust_records);

    // Convert HashMap to Python dict
    hashmap_to_py_dict(py, result)
}

/// Select preferred views using a specific preference order
///
/// For each of the 4 standard views (L-MLO, R-MLO, L-CC, R-CC), selects the
/// most preferred mammogram based on comparison logic using the specified
/// preference order.
///
/// Args:
///     records: List of MammogramRecord objects to select from
///     preference_order: The preference ordering strategy to use
///
/// Returns:
///     dict: Dictionary mapping MammogramView to MammogramRecord (or None if not found)
///
/// Example:
///     >>> from mammocat import (
///     ...     MammogramRecord,
///     ...     get_preferred_views_with_order,
///     ...     PreferenceOrder
///     ... )
///     >>> from pathlib import Path
///     >>> records = [MammogramRecord.from_file(f) for f in Path("dicoms").glob("*.dcm")]
///     >>> selections = get_preferred_views_with_order(
///     ...     records,
///     ...     PreferenceOrder.TOMO_FIRST
///     ... )
///     >>> for view, record in selections.items():
///     ...     if record:
///     ...         print(f"{view}: {record.file_path}")
#[pyfunction]
#[pyo3(name = "get_preferred_views_with_order")]
pub fn py_get_preferred_views_with_order(
    py: Python,
    records: Vec<PyMammogramRecord>,
    preference_order: PyPreferenceOrder,
) -> PyResult<Py<PyDict>> {
    // Convert Python records to Rust records
    let rust_records: Vec<_> = records.into_iter().map(|r| r.inner).collect();

    // Call Rust function
    let result =
        crate::selection::get_preferred_views_with_order(&rust_records, preference_order.inner);

    // Convert HashMap to Python dict
    hashmap_to_py_dict(py, result)
}

/// Select preferred views with filtering
///
/// Applies filters before selecting preferred views. For each of the 4 standard views
/// (L-MLO, R-MLO, L-CC, R-CC), selects the most preferred mammogram from filtered candidates.
///
/// Args:
///     records: List of MammogramRecord objects to select from
///     filter_config: FilterConfig specifying which records to include
///     preference_order: The preference ordering strategy to use
///
/// Returns:
///     dict: Dictionary mapping MammogramView to MammogramRecord (or None if not found)
///
/// Example:
///     >>> from mammocat import (
///     ...     MammogramRecord,
///     ...     FilterConfig,
///     ...     get_preferred_views_filtered,
///     ...     PreferenceOrder,
///     ...     MammogramType
///     ... )
///     >>> from pathlib import Path
///     >>> config = FilterConfig(
///     ...     allowed_types=[MammogramType.FFDM, MammogramType.TOMO],
///     ...     exclude_implants=True
///     ... )
///     >>> records = [MammogramRecord.from_file(f) for f in Path("dicoms").glob("*.dcm")]
///     >>> selections = get_preferred_views_filtered(
///     ...     records,
///     ...     config,
///     ...     PreferenceOrder.DEFAULT
///     ... )
#[pyfunction]
#[pyo3(name = "get_preferred_views_filtered")]
pub fn py_get_preferred_views_filtered(
    py: Python,
    records: Vec<PyMammogramRecord>,
    filter_config: PyFilterConfig,
    preference_order: PyPreferenceOrder,
) -> PyResult<Py<PyDict>> {
    // Convert Python records to Rust records
    let rust_records: Vec<_> = records.into_iter().map(|r| r.inner).collect();

    // Call Rust function
    let result = crate::selection::get_preferred_views_filtered(
        &rust_records,
        &filter_config.inner,
        preference_order.inner,
    );

    // Convert HashMap to Python dict
    hashmap_to_py_dict(py, result)
}

/// Convert HashMap<MammogramView, Option<MammogramRecord>> to Python dict
fn hashmap_to_py_dict(
    py: Python,
    map: std::collections::HashMap<
        crate::types::MammogramView,
        Option<crate::selection::MammogramRecord>,
    >,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new_bound(py);

    for (view, record) in map.into_iter() {
        let py_view = PyMammogramView::from(view).into_py(py);
        let py_record: PyObject = match record {
            Some(r) => PyMammogramRecord::from(r).into_py(py),
            None => py.None(),
        };
        dict.set_item(py_view, py_record)?;
    }

    Ok(dict.unbind())
}
