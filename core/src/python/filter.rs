//! Python wrappers for FilterConfig

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::collections::{BTreeSet, HashSet};

use super::enums::{PyDbtObjectKind, PyMammogramType, PyMammographyViewModifier, PyViewPosition};
use crate::types::{FilterConfig, ViewFallbackPolicy, ViewModifierPolicy, ViewPosition};

#[pyclass(name = "ViewFallbackPolicy", module = "mammocat", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyViewFallbackPolicy {
    pub(crate) inner: ViewFallbackPolicy,
}

#[pymethods]
impl PyViewFallbackPolicy {
    #[staticmethod]
    fn all_recognized() -> Self {
        Self {
            inner: ViewFallbackPolicy::AllRecognized,
        }
    }

    #[staticmethod]
    fn standard_only() -> Self {
        Self {
            inner: ViewFallbackPolicy::StandardOnly,
        }
    }

    #[staticmethod]
    fn allow_list(views: Vec<PyViewPosition>) -> PyResult<Self> {
        let allowed = views
            .into_iter()
            .map(|view| view.inner)
            .collect::<BTreeSet<_>>();
        if let Some(invalid) = allowed
            .iter()
            .find(|view| !is_supported_fallback_view(**view))
        {
            return Err(PyValueError::new_err(format!(
                "{invalid} is not a supported fallback view; expected ml, lm, lmo, xccl, or xccm"
            )));
        }
        Ok(Self {
            inner: ViewFallbackPolicy::AllowList(allowed),
        })
    }

    #[getter]
    fn mode(&self) -> &'static str {
        match self.inner {
            ViewFallbackPolicy::AllRecognized => "all_recognized",
            ViewFallbackPolicy::StandardOnly => "standard_only",
            ViewFallbackPolicy::AllowList(_) => "allow_list",
        }
    }

    #[getter]
    fn allowed_views(&self) -> Option<Vec<PyViewPosition>> {
        match &self.inner {
            ViewFallbackPolicy::AllowList(allowed) => {
                Some(allowed.iter().copied().map(PyViewPosition::from).collect())
            }
            ViewFallbackPolicy::AllRecognized | ViewFallbackPolicy::StandardOnly => None,
        }
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    fn __repr__(&self) -> String {
        format!("ViewFallbackPolicy({:?})", self.inner)
    }
}

impl From<ViewFallbackPolicy> for PyViewFallbackPolicy {
    fn from(inner: ViewFallbackPolicy) -> Self {
        Self { inner }
    }
}

#[pyclass(name = "ViewModifierPolicy", module = "mammocat", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyViewModifierPolicy {
    pub(crate) inner: ViewModifierPolicy,
}

#[pymethods]
impl PyViewModifierPolicy {
    #[staticmethod]
    fn all_recognized() -> Self {
        Self {
            inner: ViewModifierPolicy::AllRecognized,
        }
    }

    #[staticmethod]
    fn unmodified_only() -> Self {
        Self {
            inner: ViewModifierPolicy::UnmodifiedOnly,
        }
    }

    #[staticmethod]
    fn allow_list(modifiers: Vec<PyMammographyViewModifier>) -> Self {
        Self {
            inner: ViewModifierPolicy::AllowList(
                modifiers
                    .into_iter()
                    .map(|modifier| modifier.inner)
                    .collect(),
            ),
        }
    }

    #[getter]
    fn mode(&self) -> &'static str {
        match self.inner {
            ViewModifierPolicy::AllRecognized => "all_recognized",
            ViewModifierPolicy::UnmodifiedOnly => "unmodified_only",
            ViewModifierPolicy::AllowList(_) => "allow_list",
        }
    }

    #[getter]
    fn allowed_modifiers(&self) -> Option<Vec<PyMammographyViewModifier>> {
        match &self.inner {
            ViewModifierPolicy::AllowList(allowed) => Some(
                allowed
                    .iter()
                    .copied()
                    .map(PyMammographyViewModifier::from)
                    .collect(),
            ),
            ViewModifierPolicy::AllRecognized | ViewModifierPolicy::UnmodifiedOnly => None,
        }
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    fn __repr__(&self) -> String {
        format!("ViewModifierPolicy({:?})", self.inner)
    }
}

impl From<ViewModifierPolicy> for PyViewModifierPolicy {
    fn from(inner: ViewModifierPolicy) -> Self {
        Self { inner }
    }
}

fn is_supported_fallback_view(view: ViewPosition) -> bool {
    matches!(
        view,
        ViewPosition::Ml
            | ViewPosition::Lm
            | ViewPosition::Lmo
            | ViewPosition::Xccl
            | ViewPosition::Xccm
    )
}

#[pyclass(name = "FilterConfig", module = "mammocat", from_py_object)]
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
        view_fallback_policy=None,
        view_modifier_policy=None,
        exclude_for_processing=true,
        exclude_secondary_capture=true,
        exclude_non_mg_modality=true,
        require_common_modality=false,
        exclude_lossy_compressed=false,
        deprioritize_lossy_compressed=true,
        allowed_dbt_object_kinds=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        allowed_types: Option<Vec<PyMammogramType>>,
        exclude_implants: bool,
        view_fallback_policy: Option<PyViewFallbackPolicy>,
        view_modifier_policy: Option<PyViewModifierPolicy>,
        exclude_for_processing: bool,
        exclude_secondary_capture: bool,
        exclude_non_mg_modality: bool,
        require_common_modality: bool,
        exclude_lossy_compressed: bool,
        deprioritize_lossy_compressed: bool,
        allowed_dbt_object_kinds: Option<Vec<PyDbtObjectKind>>,
    ) -> Self {
        let rust_allowed =
            allowed_types.map(|types| types.into_iter().map(|t| t.inner).collect::<HashSet<_>>());
        let rust_allowed_dbt_object_kinds = allowed_dbt_object_kinds.map(|kinds| {
            kinds
                .into_iter()
                .map(|kind| kind.inner)
                .collect::<HashSet<_>>()
        });

        Self {
            inner: FilterConfig {
                allowed_types: rust_allowed,
                allowed_dbt_object_kinds: rust_allowed_dbt_object_kinds,
                exclude_implants,
                view_fallback_policy: view_fallback_policy
                    .map(|policy| policy.inner)
                    .unwrap_or_default(),
                view_modifier_policy: view_modifier_policy
                    .map(|policy| policy.inner)
                    .unwrap_or_default(),
                exclude_for_processing,
                exclude_secondary_capture,
                exclude_non_mg_modality,
                exclude_lossy_compressed,
                deprioritize_lossy_compressed,
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
    fn allowed_dbt_object_kinds(&self) -> Option<Vec<PyDbtObjectKind>> {
        self.inner.allowed_dbt_object_kinds.as_ref().map(|kinds| {
            kinds
                .iter()
                .map(|kind| PyDbtObjectKind::from(*kind))
                .collect()
        })
    }

    #[getter]
    fn exclude_implants(&self) -> bool {
        self.inner.exclude_implants
    }

    #[getter]
    fn view_fallback_policy(&self) -> PyViewFallbackPolicy {
        self.inner.view_fallback_policy.clone().into()
    }

    #[getter]
    fn view_modifier_policy(&self) -> PyViewModifierPolicy {
        self.inner.view_modifier_policy.clone().into()
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

    #[getter]
    fn exclude_lossy_compressed(&self) -> bool {
        self.inner.exclude_lossy_compressed
    }

    #[getter]
    fn deprioritize_lossy_compressed(&self) -> bool {
        self.inner.deprioritize_lossy_compressed
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
