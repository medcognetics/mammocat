use crate::types::{DbtObjectKind, MammogramType, MammographyViewModifier, ViewPosition};
use std::collections::{BTreeSet, HashSet};

/// Controls which non-standard views may fill standard CC and MLO slots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "json",
    serde(tag = "mode", content = "allowed", rename_all = "snake_case")
)]
pub enum ViewFallbackPolicy {
    /// Allow all recognized views. Selection still considers only CC-like and MLO-like views.
    #[default]
    AllRecognized,
    /// Allow exact CC and MLO views only.
    StandardOnly,
    /// Always allow exact CC/MLO views and allow only the listed non-standard views.
    AllowList(BTreeSet<ViewPosition>),
}

impl ViewFallbackPolicy {
    /// Returns whether a view passes this hard filter.
    pub fn allows(&self, view_position: ViewPosition) -> bool {
        match self {
            Self::AllRecognized => true,
            Self::StandardOnly => view_position.is_standard_view(),
            Self::AllowList(allowed) => {
                view_position.is_standard_view()
                    || (matches!(
                        view_position,
                        ViewPosition::Ml
                            | ViewPosition::Lm
                            | ViewPosition::Lmo
                            | ViewPosition::Xccl
                            | ViewPosition::Xccm
                    ) && allowed.contains(&view_position))
            }
        }
    }
}

/// Controls which recognized CID 4015 view modifiers are eligible for selection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "json",
    serde(tag = "mode", content = "allowed", rename_all = "snake_case")
)]
pub enum ViewModifierPolicy {
    /// Allow every recognized modifier combination.
    #[default]
    AllRecognized,
    /// Allow records without recognized modifiers only.
    UnmodifiedOnly,
    /// Allow unmodified records and records whose modifiers are all listed.
    AllowList(BTreeSet<MammographyViewModifier>),
}

impl ViewModifierPolicy {
    /// Returns whether every recognized modifier passes this hard filter.
    pub fn allows(&self, modifiers: &BTreeSet<MammographyViewModifier>) -> bool {
        match self {
            Self::AllRecognized => true,
            Self::UnmodifiedOnly => modifiers.is_empty(),
            Self::AllowList(allowed) => modifiers.is_subset(allowed),
        }
    }
}

/// Configuration for filtering mammogram records during selection
///
/// Hard-exclusion filters remove records from consideration. Ranking options,
/// such as lossy-compression deprioritization, keep records available as
/// fallbacks while changing their selection priority.
///
/// # Example
///
/// ```
/// use mammocat_core::{DbtObjectKind, FilterConfig, MammogramType};
/// use std::collections::HashSet;
///
/// // Create filter that only allows TOMO slice objects and excludes implants
/// let mut allowed_types = HashSet::new();
/// allowed_types.insert(MammogramType::Tomo);
/// let mut allowed_dbt_object_kinds = HashSet::new();
/// allowed_dbt_object_kinds.insert(DbtObjectKind::Slice);
///
/// let filter = FilterConfig::default()
///     .with_allowed_types(allowed_types)
///     .with_allowed_dbt_object_kinds(allowed_dbt_object_kinds)
///     .exclude_implants(true);
///
/// assert!(filter.exclude_implants);
/// assert_eq!(filter.allowed_types.unwrap().len(), 1);
/// assert_eq!(filter.allowed_dbt_object_kinds.unwrap().len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct FilterConfig {
    /// Allowed mammogram types (whitelist approach)
    /// If None, all types are allowed. If Some, only types in the set are included.
    pub allowed_types: Option<HashSet<MammogramType>>,

    /// Allowed DBT object kinds (whitelist approach)
    /// If None, all kinds are allowed. If Some, only kinds in the set are included.
    #[cfg_attr(feature = "json", serde(default))]
    pub allowed_dbt_object_kinds: Option<HashSet<DbtObjectKind>>,

    /// Exclude records with implants
    pub exclude_implants: bool,

    /// Controls which non-standard views may fill standard CC and MLO slots.
    #[cfg_attr(feature = "json", serde(default))]
    pub view_fallback_policy: ViewFallbackPolicy,

    /// Controls which recognized CID 4015 view modifiers are eligible.
    #[cfg_attr(feature = "json", serde(default))]
    pub view_modifier_policy: ViewModifierPolicy,

    /// Exclude "FOR PROCESSING" views
    pub exclude_for_processing: bool,

    /// Exclude secondary capture images
    pub exclude_secondary_capture: bool,

    /// Exclude non-MG modality
    pub exclude_non_mg_modality: bool,

    /// Exclude records marked as lossy compressed
    pub exclude_lossy_compressed: bool,

    /// Prefer lossless records over lossy compressed records during selection
    pub deprioritize_lossy_compressed: bool,

    /// Require all selected views to come from a common modality group (2D or DBT)
    pub require_common_modality: bool,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            allowed_types: None,            // Allow all types by default
            allowed_dbt_object_kinds: None, // Allow all DBT object kinds by default
            exclude_implants: false,
            view_fallback_policy: ViewFallbackPolicy::AllRecognized,
            view_modifier_policy: ViewModifierPolicy::AllRecognized,
            exclude_for_processing: true, // Default: exclude FOR PROCESSING
            exclude_secondary_capture: true, // Default: exclude secondary capture
            exclude_non_mg_modality: true, // Default: exclude non-MG
            exclude_lossy_compressed: false,
            deprioritize_lossy_compressed: true,
            require_common_modality: false,
        }
    }
}

impl FilterConfig {
    /// Creates a new FilterConfig with all filters disabled
    ///
    /// This is a permissive filter that includes everything.
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let permissive = FilterConfig::permissive();
    /// assert!(!permissive.exclude_for_processing);
    /// assert!(!permissive.exclude_secondary_capture);
    /// assert!(!permissive.exclude_non_mg_modality);
    /// ```
    pub fn permissive() -> Self {
        Self {
            allowed_types: None,
            allowed_dbt_object_kinds: None,
            exclude_implants: false,
            view_fallback_policy: ViewFallbackPolicy::AllRecognized,
            view_modifier_policy: ViewModifierPolicy::AllRecognized,
            exclude_for_processing: false,
            exclude_secondary_capture: false,
            exclude_non_mg_modality: false,
            exclude_lossy_compressed: false,
            deprioritize_lossy_compressed: true,
            require_common_modality: false,
        }
    }

    /// Builder: Set allowed mammogram types
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::{FilterConfig, MammogramType};
    /// use std::collections::HashSet;
    ///
    /// let mut allowed = HashSet::new();
    /// allowed.insert(MammogramType::Ffdm);
    ///
    /// let filter = FilterConfig::default().with_allowed_types(allowed);
    /// assert!(filter.allowed_types.is_some());
    /// ```
    pub fn with_allowed_types(mut self, types: HashSet<MammogramType>) -> Self {
        self.allowed_types = Some(types);
        self
    }

    /// Builder: Set allowed DBT object kinds
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::{DbtObjectKind, FilterConfig};
    /// use std::collections::HashSet;
    ///
    /// let mut allowed = HashSet::new();
    /// allowed.insert(DbtObjectKind::Slice);
    ///
    /// let filter = FilterConfig::default().with_allowed_dbt_object_kinds(allowed);
    /// assert!(filter.allowed_dbt_object_kinds.is_some());
    /// ```
    pub fn with_allowed_dbt_object_kinds(mut self, kinds: HashSet<DbtObjectKind>) -> Self {
        self.allowed_dbt_object_kinds = Some(kinds);
        self
    }

    /// Builder: Exclude implants
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_implants(true);
    /// assert!(filter.exclude_implants);
    /// ```
    pub fn exclude_implants(mut self, exclude: bool) -> Self {
        self.exclude_implants = exclude;
        self
    }

    /// Builder: Set the non-standard view fallback policy.
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::{FilterConfig, ViewFallbackPolicy};
    ///
    /// let filter = FilterConfig::default()
    ///     .with_view_fallback_policy(ViewFallbackPolicy::StandardOnly);
    /// assert_eq!(filter.view_fallback_policy, ViewFallbackPolicy::StandardOnly);
    /// ```
    pub fn with_view_fallback_policy(mut self, policy: ViewFallbackPolicy) -> Self {
        self.view_fallback_policy = policy;
        self
    }

    /// Builder: Set the recognized view modifier policy.
    pub fn with_view_modifier_policy(mut self, policy: ViewModifierPolicy) -> Self {
        self.view_modifier_policy = policy;
        self
    }

    /// Builder: Exclude FOR PROCESSING
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_for_processing(false);
    /// assert!(!filter.exclude_for_processing);
    /// ```
    pub fn exclude_for_processing(mut self, exclude: bool) -> Self {
        self.exclude_for_processing = exclude;
        self
    }

    /// Builder: Exclude secondary capture
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_secondary_capture(false);
    /// assert!(!filter.exclude_secondary_capture);
    /// ```
    pub fn exclude_secondary_capture(mut self, exclude: bool) -> Self {
        self.exclude_secondary_capture = exclude;
        self
    }

    /// Builder: Exclude non-MG modality
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_non_mg_modality(false);
    /// assert!(!filter.exclude_non_mg_modality);
    /// ```
    pub fn exclude_non_mg_modality(mut self, exclude: bool) -> Self {
        self.exclude_non_mg_modality = exclude;
        self
    }

    /// Builder: Exclude lossy compressed images
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_lossy_compressed(true);
    /// assert!(filter.exclude_lossy_compressed);
    /// ```
    pub fn exclude_lossy_compressed(mut self, exclude: bool) -> Self {
        self.exclude_lossy_compressed = exclude;
        self
    }

    /// Builder: Prefer lossless images over lossy compressed images
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().deprioritize_lossy_compressed(false);
    /// assert!(!filter.deprioritize_lossy_compressed);
    /// ```
    pub fn deprioritize_lossy_compressed(mut self, deprioritize: bool) -> Self {
        self.deprioritize_lossy_compressed = deprioritize;
        self
    }

    /// Builder: Require common modality across all selected views
    ///
    /// When enabled, enforces that all selected views come from the same
    /// modality group: 2D (FFDM, SYNTH, DBT MIP, SFM) or DBT (TOMO).
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().require_common_modality(true);
    /// assert!(filter.require_common_modality);
    /// ```
    pub fn require_common_modality(mut self, require: bool) -> Self {
        self.require_common_modality = require;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MammographyViewModifier, ViewPosition};
    use std::collections::BTreeSet;

    #[test]
    fn test_default_config() {
        let config = FilterConfig::default();
        assert!(config.allowed_types.is_none());
        assert!(config.allowed_dbt_object_kinds.is_none());
        assert!(!config.exclude_implants);
        assert_eq!(
            config.view_fallback_policy,
            ViewFallbackPolicy::AllRecognized
        );
        assert_eq!(
            config.view_modifier_policy,
            ViewModifierPolicy::AllRecognized
        );
        assert!(config.exclude_for_processing);
        assert!(config.exclude_secondary_capture);
        assert!(config.exclude_non_mg_modality);
        assert!(!config.exclude_lossy_compressed);
        assert!(config.deprioritize_lossy_compressed);
        assert!(!config.require_common_modality);
    }

    #[test]
    fn test_permissive_config() {
        let config = FilterConfig::permissive();
        assert!(config.allowed_types.is_none());
        assert!(config.allowed_dbt_object_kinds.is_none());
        assert!(!config.exclude_implants);
        assert_eq!(
            config.view_fallback_policy,
            ViewFallbackPolicy::AllRecognized
        );
        assert_eq!(
            config.view_modifier_policy,
            ViewModifierPolicy::AllRecognized
        );
        assert!(!config.exclude_for_processing);
        assert!(!config.exclude_secondary_capture);
        assert!(!config.exclude_non_mg_modality);
        assert!(!config.exclude_lossy_compressed);
        assert!(config.deprioritize_lossy_compressed);
        assert!(!config.require_common_modality);
    }

    #[test]
    fn test_builder_pattern() {
        let mut allowed = HashSet::new();
        allowed.insert(MammogramType::Ffdm);
        allowed.insert(MammogramType::Tomo);

        let config = FilterConfig::default()
            .with_allowed_types(allowed.clone())
            .exclude_implants(true);

        assert_eq!(config.allowed_types, Some(allowed));
        assert!(config.allowed_dbt_object_kinds.is_none());
        assert!(config.exclude_implants);
    }

    #[test]
    fn test_builder_chain() {
        let config = FilterConfig::permissive()
            .exclude_for_processing(true)
            .exclude_secondary_capture(true)
            .exclude_non_mg_modality(true)
            .exclude_lossy_compressed(true)
            .deprioritize_lossy_compressed(false);

        assert!(config.exclude_for_processing);
        assert!(config.exclude_secondary_capture);
        assert!(config.exclude_non_mg_modality);
        assert!(config.exclude_lossy_compressed);
        assert!(!config.deprioritize_lossy_compressed);
        assert!(!config.exclude_implants);
    }

    #[test]
    fn test_allowed_types_whitelist() {
        let mut allowed = HashSet::new();
        allowed.insert(MammogramType::Ffdm);

        let config = FilterConfig::default().with_allowed_types(allowed.clone());

        assert!(config.allowed_types.is_some());
        assert_eq!(config.allowed_types.unwrap().len(), 1);
    }

    #[test]
    fn test_allowed_dbt_object_kinds_whitelist() {
        let mut allowed = HashSet::new();
        allowed.insert(DbtObjectKind::Slice);

        let config = FilterConfig::default().with_allowed_dbt_object_kinds(allowed.clone());

        assert_eq!(config.allowed_dbt_object_kinds, Some(allowed));
    }

    #[test]
    fn fallback_policy_always_allows_standard_views() {
        let policy =
            ViewFallbackPolicy::AllowList(BTreeSet::from([ViewPosition::Ml, ViewPosition::Fb]));

        assert!(policy.allows(ViewPosition::Mlo));
        assert!(policy.allows(ViewPosition::Cc));
        assert!(policy.allows(ViewPosition::Ml));
        assert!(!policy.allows(ViewPosition::Lm));
        assert!(!policy.allows(ViewPosition::Xccl));
        assert!(!policy.allows(ViewPosition::Fb));
    }

    #[test]
    fn standard_only_rejects_every_supported_fallback() {
        let policy = ViewFallbackPolicy::StandardOnly;

        for fallback in [
            ViewPosition::Ml,
            ViewPosition::Lm,
            ViewPosition::Lmo,
            ViewPosition::Xccl,
            ViewPosition::Xccm,
        ] {
            assert!(!policy.allows(fallback));
        }
    }

    #[test]
    fn modifier_allow_list_requires_every_modifier() {
        let policy = ViewModifierPolicy::AllowList(BTreeSet::from([
            MammographyViewModifier::ImplantDisplaced,
            MammographyViewModifier::SpotCompression,
        ]));

        assert!(policy.allows(&BTreeSet::new()));
        assert!(policy.allows(&BTreeSet::from([
            MammographyViewModifier::ImplantDisplaced,
            MammographyViewModifier::SpotCompression,
        ])));
        assert!(!policy.allows(&BTreeSet::from([
            MammographyViewModifier::ImplantDisplaced,
            MammographyViewModifier::Magnification,
        ])));
    }

    #[test]
    fn unmodified_only_rejects_any_recognized_modifier() {
        let policy = ViewModifierPolicy::UnmodifiedOnly;

        assert!(policy.allows(&BTreeSet::new()));
        assert!(!policy.allows(&BTreeSet::from(
            [MammographyViewModifier::ImplantDisplaced,]
        )));
    }

    #[cfg(feature = "json")]
    #[test]
    fn policies_use_stable_tagged_json() {
        let fallback = ViewFallbackPolicy::AllowList(BTreeSet::from([ViewPosition::Ml]));
        let modifier = ViewModifierPolicy::AllowList(BTreeSet::from([
            MammographyViewModifier::ImplantDisplaced,
        ]));

        let fallback_json = serde_json::json!({"mode": "allow_list", "allowed": ["ml"]});
        let modifier_json = serde_json::json!({
            "mode": "allow_list",
            "allowed": ["implant_displaced"]
        });

        assert_eq!(serde_json::to_value(&fallback).unwrap(), fallback_json);
        assert_eq!(serde_json::to_value(&modifier).unwrap(), modifier_json);
        assert_eq!(
            serde_json::from_value::<ViewFallbackPolicy>(fallback_json).unwrap(),
            fallback
        );
        assert_eq!(
            serde_json::from_value::<ViewModifierPolicy>(modifier_json).unwrap(),
            modifier
        );
    }

    #[cfg(feature = "json")]
    #[test]
    fn obsolete_non_standard_filter_field_is_rejected() {
        let mut value = serde_json::to_value(FilterConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("exclude_non_standard_views".to_string(), true.into());

        let error = serde_json::from_value::<FilterConfig>(value).unwrap_err();
        assert!(error.to_string().contains("exclude_non_standard_views"));
    }
}
