use super::{Laterality, ViewPosition};
use std::fmt;

/// Mammogram view combining laterality and view position
///
/// Represents a complete mammogram view specification,
/// such as "left MLO" or "right CC".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MammogramView {
    pub laterality: Laterality,
    pub view: ViewPosition,
}

impl MammogramView {
    /// Creates a new MammogramView
    pub fn new(laterality: Laterality, view: ViewPosition) -> Self {
        Self { laterality, view }
    }

    /// Checks if this is a standard mammography view (CC or MLO)
    pub fn is_standard_mammo_view(&self) -> bool {
        self.view.is_standard_view()
    }

    /// Checks if this is an MLO-like view
    pub fn is_mlo_like(&self) -> bool {
        self.view.is_mlo_like()
    }

    /// Checks if this is a CC-like view
    pub fn is_cc_like(&self) -> bool {
        self.view.is_cc_like()
    }
}

impl fmt::Display for MammogramView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            self.laterality.simple_name(),
            self.view.simple_name()
        )
    }
}

/// Standard mammogram views (4 views for complete bilateral study)
#[allow(dead_code)]
pub const STANDARD_MAMMO_VIEWS: [MammogramView; 4] = [
    MammogramView {
        laterality: Laterality::Left,
        view: ViewPosition::Mlo,
    },
    MammogramView {
        laterality: Laterality::Right,
        view: ViewPosition::Mlo,
    },
    MammogramView {
        laterality: Laterality::Left,
        view: ViewPosition::Cc,
    },
    MammogramView {
        laterality: Laterality::Right,
        view: ViewPosition::Cc,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_view() {
        let view = MammogramView::new(Laterality::Left, ViewPosition::Cc);
        assert!(view.is_standard_mammo_view());
        assert!(view.is_cc_like());
        assert!(!view.is_mlo_like());

        let view = MammogramView::new(Laterality::Right, ViewPosition::Mlo);
        assert!(view.is_standard_mammo_view());
        assert!(view.is_mlo_like());
        assert!(!view.is_cc_like());
    }

    #[test]
    fn test_non_standard_view() {
        let view = MammogramView::new(Laterality::Left, ViewPosition::Ml);
        assert!(!view.is_standard_mammo_view());
        assert!(view.is_mlo_like());
        assert!(!view.is_cc_like());
    }

    #[test]
    fn test_standard_views_constant() {
        assert_eq!(STANDARD_MAMMO_VIEWS.len(), 4);
        assert!(
            STANDARD_MAMMO_VIEWS.contains(&MammogramView::new(Laterality::Left, ViewPosition::Cc))
        );
        assert!(
            STANDARD_MAMMO_VIEWS.contains(&MammogramView::new(Laterality::Right, ViewPosition::Cc))
        );
        assert!(
            STANDARD_MAMMO_VIEWS.contains(&MammogramView::new(Laterality::Left, ViewPosition::Mlo))
        );
        assert!(STANDARD_MAMMO_VIEWS
            .contains(&MammogramView::new(Laterality::Right, ViewPosition::Mlo)));
    }
}
