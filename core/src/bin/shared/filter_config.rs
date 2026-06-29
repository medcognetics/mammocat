use mammocat_core::{FilterConfig, MammogramType};
use std::collections::HashSet;

pub(crate) struct FilterConfigArgs<'a, T> {
    pub(crate) allowed_types: Option<&'a [T]>,
    pub(crate) exclude_implants: bool,
    pub(crate) only_standard_views: bool,
    pub(crate) include_for_processing: bool,
    pub(crate) include_secondary_capture: bool,
    pub(crate) include_non_mg: bool,
    pub(crate) require_common_modality: bool,
    pub(crate) exclude_lossy_compressed: bool,
    pub(crate) deprioritize_lossy_compressed: bool,
}

pub(crate) fn build_filter_config<T>(args: FilterConfigArgs<'_, T>) -> FilterConfig
where
    T: Clone,
    MammogramType: From<T>,
{
    let mut config = FilterConfig::default();

    if let Some(type_args) = args.allowed_types {
        let allowed_types: HashSet<MammogramType> =
            type_args.iter().cloned().map(MammogramType::from).collect();
        config = config.with_allowed_types(allowed_types);
    }

    config = config.exclude_implants(args.exclude_implants);
    config = config.exclude_non_standard_views(args.only_standard_views);
    config = config.exclude_for_processing(!args.include_for_processing);
    config = config.exclude_secondary_capture(!args.include_secondary_capture);
    config = config.exclude_non_mg_modality(!args.include_non_mg);
    config = config.exclude_lossy_compressed(args.exclude_lossy_compressed);
    config = config.deprioritize_lossy_compressed(args.deprioritize_lossy_compressed);
    config.require_common_modality(args.require_common_modality)
}
