"""
Mammocat - DICOM Mammography Metadata Extraction

A high-performance library for extracting metadata from mammography DICOM files.

This library provides:
- Mammogram type classification (TOMO, FFDM, SYNTH, SFM)
- Laterality and view position extraction
- Preferred view selection from multiple mammograms
- Fast metadata extraction without loading pixel data

Example:
    >>> from mammocat import MammogramExtractor
    >>> metadata = MammogramExtractor.extract_from_file("mammogram.dcm")
    >>> print(f"{metadata.mammogram_type} {metadata.laterality} {metadata.view_position}")

    >>> from mammocat import MammogramRecord, get_preferred_views
    >>> from pathlib import Path
    >>> records = [MammogramRecord.from_file(f) for f in Path("dicoms").glob("*.dcm")]
    >>> selections = get_preferred_views(records)
"""

from ._mammocat import (
    DicomError,
    ExtractionError,
    # Filter configuration
    FilterConfig,
    # Data structures
    ImageType,
    InvalidValueError,
    Laterality,
    # Exceptions
    MammocatError,
    # Main API
    MammogramExtractor,
    MammogramMetadata,
    MammogramRecord,
    # Enums
    MammogramType,
    MammogramView,
    PhotometricInterpretation,
    PreferenceOrder,
    TagNotFoundError,
    ViewPosition,
    __version__,
    # Selection functions
    get_preferred_views,
    get_preferred_views_filtered,
    get_preferred_views_with_order,
)

__all__ = [
    "DicomError",
    "ExtractionError",
    "FilterConfig",
    "ImageType",
    "InvalidValueError",
    "Laterality",
    "MammocatError",
    "MammogramExtractor",
    "MammogramMetadata",
    "MammogramRecord",
    "MammogramType",
    "MammogramView",
    "PhotometricInterpretation",
    "PreferenceOrder",
    "TagNotFoundError",
    "ViewPosition",
    "__version__",
    "get_preferred_views",
    "get_preferred_views_filtered",
    "get_preferred_views_with_order",
]
