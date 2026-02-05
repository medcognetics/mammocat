use crate::types::MammogramType;
use std::collections::HashSet;

/// Configuration for filtering mammogram records during selection
///
/// All filters use hard exclusion - records that don't match are completely removed
/// from consideration, not just deprioritized.
///
/// # Example
///
/// ```
/// use mammocat_core::{FilterConfig, MammogramType};
/// use std::collections::HashSet;
///
/// // Create filter that only allows FFDM and TOMO, excludes implants
/// let mut allowed_types = HashSet::new();
/// allowed_types.insert(MammogramType::Ffdm);
/// allowed_types.insert(MammogramType::Tomo);
///
/// let filter = FilterConfig::default()
///     .with_allowed_types(allowed_types)
///     .exclude_implants(true);
///
/// assert!(filter.exclude_implants);
/// assert_eq!(filter.allowed_types.unwrap().len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct FilterConfig {
    /// Allowed mammogram types (whitelist approach)
    /// If None, all types are allowed. If Some, only types in the set are included.
    pub allowed_types: Option<HashSet<MammogramType>>,

    /// Exclude records with implants
    pub exclude_implants: bool,

    /// Exclude non-standard views (only CC and MLO)
    pub exclude_non_standard_views: bool,

    /// Exclude "FOR PROCESSING" views
    pub exclude_for_processing: bool,

    /// Exclude secondary capture images
    pub exclude_secondary_capture: bool,

    /// Exclude non-MG modality
    pub exclude_non_mg_modality: bool,

    /// Require all selected views to come from a common modality group (2D or DBT)
    pub require_common_modality: bool,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            allowed_types: None, // Allow all types by default
            exclude_implants: false,
            exclude_non_standard_views: false,
            exclude_for_processing: true, // Default: exclude FOR PROCESSING
            exclude_secondary_capture: true, // Default: exclude secondary capture
            exclude_non_mg_modality: true, // Default: exclude non-MG
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
            exclude_implants: false,
            exclude_non_standard_views: false,
            exclude_for_processing: false,
            exclude_secondary_capture: false,
            exclude_non_mg_modality: false,
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

    /// Builder: Exclude non-standard views
    ///
    /// # Example
    ///
    /// ```
    /// use mammocat_core::FilterConfig;
    ///
    /// let filter = FilterConfig::default().exclude_non_standard_views(true);
    /// assert!(filter.exclude_non_standard_views);
    /// ```
    pub fn exclude_non_standard_views(mut self, exclude: bool) -> Self {
        self.exclude_non_standard_views = exclude;
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

    /// Builder: Require common modality across all selected views
    ///
    /// When enabled, enforces that all selected views come from the same
    /// modality group: 2D (FFDM, SYNTH, SFM) or DBT (TOMO).
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

    #[test]
    fn test_default_config() {
        let config = FilterConfig::default();
        assert!(config.allowed_types.is_none());
        assert!(!config.exclude_implants);
        assert!(!config.exclude_non_standard_views);
        assert!(config.exclude_for_processing);
        assert!(config.exclude_secondary_capture);
        assert!(config.exclude_non_mg_modality);
        assert!(!config.require_common_modality);
    }

    #[test]
    fn test_permissive_config() {
        let config = FilterConfig::permissive();
        assert!(config.allowed_types.is_none());
        assert!(!config.exclude_implants);
        assert!(!config.exclude_non_standard_views);
        assert!(!config.exclude_for_processing);
        assert!(!config.exclude_secondary_capture);
        assert!(!config.exclude_non_mg_modality);
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
        assert!(config.exclude_implants);
    }

    #[test]
    fn test_builder_chain() {
        let config = FilterConfig::permissive()
            .exclude_for_processing(true)
            .exclude_secondary_capture(true)
            .exclude_non_mg_modality(true);

        assert!(config.exclude_for_processing);
        assert!(config.exclude_secondary_capture);
        assert!(config.exclude_non_mg_modality);
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
}
