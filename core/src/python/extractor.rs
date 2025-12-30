//! Python wrapper for MammogramExtractor

use pyo3::prelude::*;

use super::errors::convert_error;
use super::metadata::PyMammogramMetadata;
use super::utils::path_to_pathbuf;

/// Main extractor for mammography metadata from DICOM files
///
/// This class provides static methods to extract metadata from DICOM files
/// by accepting file paths.
#[pyclass(name = "MammogramExtractor", module = "mammocat")]
pub struct PyMammogramExtractor;

#[pymethods]
impl PyMammogramExtractor {
    /// Extract metadata from a DICOM file
    ///
    /// Args:
    ///     path: Path to the DICOM file (str or pathlib.Path)
    ///
    /// Returns:
    ///     MammogramMetadata: Extracted metadata
    ///
    /// Raises:
    ///     DicomError: If the file cannot be read or parsed
    ///     TagNotFoundError: If required DICOM tags are missing
    ///     InvalidValueError: If tag values are invalid
    ///     ExtractionError: For other extraction errors
    ///
    /// Example:
    ///     >>> from mammocat import MammogramExtractor
    ///     >>> metadata = MammogramExtractor.extract_from_file("mammogram.dcm")
    ///     >>> print(metadata.mammogram_type)
    #[staticmethod]
    #[pyo3(signature = (path))]
    fn extract_from_file(path: &Bound<'_, PyAny>) -> PyResult<PyMammogramMetadata> {
        // Convert path to PathBuf
        let path_buf = path_to_pathbuf(path)?;

        // Open DICOM file
        let dcm = dicom_object::open_file(&path_buf).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to open DICOM file: {}", e))
        })?;

        // Extract metadata
        let metadata = crate::api::MammogramExtractor::extract(&dcm).map_err(convert_error)?;

        Ok(metadata.into())
    }

    /// Extract metadata from a DICOM file with options
    ///
    /// Args:
    ///     path: Path to the DICOM file (str or pathlib.Path)
    ///     is_sfm: Whether to treat as SFM instead of FFDM (default: False)
    ///
    /// Returns:
    ///     MammogramMetadata: Extracted metadata
    ///
    /// Raises:
    ///     DicomError: If the file cannot be read or parsed
    ///     TagNotFoundError: If required DICOM tags are missing
    ///     InvalidValueError: If tag values are invalid
    ///     ExtractionError: For other extraction errors
    ///
    /// Example:
    ///     >>> from mammocat import MammogramExtractor
    ///     >>> metadata = MammogramExtractor.extract_from_file_with_options(
    ///     ...     "mammogram.dcm", is_sfm=True
    ///     ... )
    #[staticmethod]
    #[pyo3(signature = (path, is_sfm=false))]
    fn extract_from_file_with_options(
        path: &Bound<'_, PyAny>,
        is_sfm: bool,
    ) -> PyResult<PyMammogramMetadata> {
        // Convert path to PathBuf
        let path_buf = path_to_pathbuf(path)?;

        // Open DICOM file
        let dcm = dicom_object::open_file(&path_buf).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to open DICOM file: {}", e))
        })?;

        // Extract metadata with options
        let metadata = crate::api::MammogramExtractor::extract_with_options(&dcm, is_sfm)
            .map_err(convert_error)?;

        Ok(metadata.into())
    }
}
