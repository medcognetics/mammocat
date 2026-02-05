//! Python wrappers for FilterConfig

use pyo3::prelude::*;
use std::collections::HashSet;

use super::enums::PyMammogramType;
use crate::types::FilterConfig;

#[pyclass(name = "FilterConfig", module = "mammocat")]
#[derive(Clone, Debug)]
pub struct PyFilterConfig {
    pub(crate) inner: FilterConfig,
}

#[pymethods]
impl PyFilterConfig {
    #[new]
    #[pyo3(signature = (
        allowed_types=None,
        exclude_implants=false,
        exclude_non_standard_views=false,
        exclude_for_processing=true,
        exclude_secondary_capture=true,
        exclude_non_mg_modality=true,
        require_common_modality=false
    ))]
    fn new(
        allowed_types: Option<Vec<PyMammogramType>>,
        exclude_implants: bool,
        exclude_non_standard_views: bool,
        exclude_for_processing: bool,
        exclude_secondary_capture: bool,
        exclude_non_mg_modality: bool,
        require_common_modality: bool,
    ) -> Self {
        let rust_allowed =
            allowed_types.map(|types| types.into_iter().map(|t| t.inner).collect::<HashSet<_>>());

        Self {
            inner: FilterConfig {
                allowed_types: rust_allowed,
                exclude_implants,
                exclude_non_standard_views,
                exclude_for_processing,
                exclude_secondary_capture,
                exclude_non_mg_modality,
                require_common_modality,
            },
        }
    }

    #[staticmethod]
    fn default() -> Self {
        Self {
            inner: FilterConfig::default(),
        }
    }

    #[staticmethod]
    fn permissive() -> Self {
        Self {
            inner: FilterConfig::permissive(),
        }
    }

    #[getter]
    fn allowed_types(&self) -> Option<Vec<PyMammogramType>> {
        self.inner
            .allowed_types
            .as_ref()
            .map(|types| types.iter().map(|t| PyMammogramType::from(*t)).collect())
    }

    #[getter]
    fn exclude_implants(&self) -> bool {
        self.inner.exclude_implants
    }

    #[getter]
    fn exclude_non_standard_views(&self) -> bool {
        self.inner.exclude_non_standard_views
    }

    #[getter]
    fn exclude_for_processing(&self) -> bool {
        self.inner.exclude_for_processing
    }

    #[getter]
    fn exclude_secondary_capture(&self) -> bool {
        self.inner.exclude_secondary_capture
    }

    #[getter]
    fn exclude_non_mg_modality(&self) -> bool {
        self.inner.exclude_non_mg_modality
    }

    #[getter]
    fn require_common_modality(&self) -> bool {
        self.inner.require_common_modality
    }

    fn __repr__(&self) -> String {
        format!("FilterConfig({:?})", self.inner)
    }
}

impl From<FilterConfig> for PyFilterConfig {
    fn from(config: FilterConfig) -> Self {
        Self { inner: config }
    }
}

impl From<PyFilterConfig> for FilterConfig {
    fn from(config: PyFilterConfig) -> Self {
        config.inner
    }
}
