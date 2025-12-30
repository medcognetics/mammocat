use std::cmp::Ordering;
use std::fmt;

/// Sentinel value for unknown enums
#[allow(dead_code)]
pub const UNKNOWN: i32 = -1;

/// Preference ordering strategy for selecting preferred mammogram types
///
/// Defines different strategies for ranking mammogram types during view selection.
/// Lower preference values indicate MORE preferred types (will be selected by .min()).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "kebab-case"))]
pub enum PreferenceOrder {
    /// Default ordering: FFDM > SYNTH > TOMO > SFM
    /// Prefers 2D images over tomosynthesis for general inference
    #[default]
    Default,

    /// Tomosynthesis first: TOMO > FFDM > SYNTH > SFM
    /// Maximizes use of 3D imaging when available
    TomoFirst,
}

impl PreferenceOrder {
    /// Returns the preference value for a given mammogram type under this ordering
    ///
    /// Lower values are MORE preferred (will be selected by .min())
    pub fn preference_value(&self, mammo_type: &MammogramType) -> i32 {
        match self {
            PreferenceOrder::Default => match mammo_type {
                MammogramType::Unknown => 5,
                MammogramType::Ffdm => 1,
                MammogramType::Synth => 2,
                MammogramType::Tomo => 3,
                MammogramType::Sfm => 4,
            },
            PreferenceOrder::TomoFirst => match mammo_type {
                MammogramType::Unknown => 5,
                MammogramType::Tomo => 1,
                MammogramType::Ffdm => 2,
                MammogramType::Synth => 3,
                MammogramType::Sfm => 4,
            },
        }
    }
}

/// Mammogram type classification with preference ordering
///
/// Preference order: TOMO < FFDM < SYNTH < SFM < UNKNOWN
/// (Lower values are MORE preferred for deduplication/selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
pub enum MammogramType {
    Unknown,
    Tomo,
    Ffdm,
    Synth,
    Sfm,
}

impl MammogramType {
    /// Returns whether this type is unknown
    pub fn is_unknown(&self) -> bool {
        matches!(self, MammogramType::Unknown)
    }

    /// Returns simple name for display
    pub fn simple_name(&self) -> &'static str {
        match self {
            MammogramType::Unknown => "unknown",
            MammogramType::Tomo => "tomo",
            MammogramType::Ffdm => "ffdm",
            MammogramType::Synth => "s-view",
            MammogramType::Sfm => "sfm",
        }
    }

    /// Returns numeric value for preference ordering
    fn value(&self) -> i32 {
        match self {
            MammogramType::Unknown => 0,
            MammogramType::Tomo => 1,
            MammogramType::Ffdm => 2,
            MammogramType::Synth => 3,
            MammogramType::Sfm => 4,
        }
    }

    /// Checks if this type is preferred over another
    ///
    /// Returns `true` if this type should be preferred over `other`
    /// when selecting the best mammogram from a collection.
    pub fn is_preferred_to(&self, other: &MammogramType) -> bool {
        if self.is_unknown() {
            false
        } else if other.is_unknown() {
            true
        } else {
            self.value() < other.value()
        }
    }

    /// Returns the best mammogram type from a list
    ///
    /// # Panics
    ///
    /// Panics if the slice is empty
    pub fn get_best(types: &[MammogramType]) -> MammogramType {
        assert!(!types.is_empty(), "types must not be empty");
        *types.iter().min().unwrap()
    }

    /// Parses mammogram type from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let s_lower = s.to_lowercase();
        if s_lower.contains("tomo") {
            MammogramType::Tomo
        } else if s_lower.contains("view") || s_lower.contains("synth") {
            MammogramType::Synth
        } else if s_lower.contains("2d") || s_lower.contains("ffdm") {
            MammogramType::Ffdm
        } else if s_lower.contains("sfm") {
            MammogramType::Sfm
        } else {
            MammogramType::Unknown
        }
    }
}

// Implement ordering by preference
impl PartialOrd for MammogramType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MammogramType {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.is_preferred_to(other) {
            Ordering::Less
        } else if other.is_preferred_to(self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl fmt::Display for MammogramType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.simple_name())
    }
}

/// Laterality specification (left/right/bilateral)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
pub enum Laterality {
    Unknown,
    None,
    Left,
    Right,
    Bilateral,
}

impl Laterality {
    /// Returns whether this laterality is unknown
    pub fn is_unknown(&self) -> bool {
        matches!(self, Laterality::Unknown)
    }

    /// Returns whether this is a unilateral (left or right) laterality
    pub fn is_unilateral(&self) -> bool {
        matches!(self, Laterality::Left | Laterality::Right)
    }

    /// Returns whether this is unknown or none
    pub fn is_unknown_or_none(&self) -> bool {
        matches!(self, Laterality::Unknown | Laterality::None)
    }

    /// Returns the opposite laterality
    pub fn opposite(&self) -> Self {
        match self {
            Laterality::Left => Laterality::Right,
            Laterality::Right => Laterality::Left,
            _ => Laterality::Unknown,
        }
    }

    /// Returns short string representation
    pub fn short_str(&self) -> &'static str {
        match self {
            Laterality::Left => "l",
            Laterality::Right => "r",
            Laterality::Bilateral => "bilateral",
            Laterality::None => "none",
            Laterality::Unknown => "",
        }
    }

    /// Returns simple name for display
    pub fn simple_name(&self) -> &'static str {
        match self {
            Laterality::Left => "left",
            Laterality::Right => "right",
            Laterality::Bilateral => "bilateral",
            Laterality::None => "none",
            Laterality::Unknown => "unknown",
        }
    }

    /// Parses laterality from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let s_lower = s.trim().to_lowercase();
        if s_lower == "none" {
            Laterality::None
        } else if s_lower.contains("bi") {
            Laterality::Bilateral
        } else if s_lower.contains('r') || s_lower.contains('d') {
            Laterality::Right
        } else if s_lower.contains('l') || s_lower.contains('e') {
            Laterality::Left
        } else {
            Laterality::Unknown
        }
    }

    /// Reduces two lateralities according to combination rules
    ///
    /// Rules:
    /// - ANY + BILATERAL -> BILATERAL
    /// - LEFT + RIGHT -> BILATERAL
    /// - LEFT + (UNKNOWN/NONE) -> LEFT
    /// - RIGHT + (UNKNOWN/NONE) -> RIGHT
    /// - NONE + NONE -> NONE
    /// - UNKNOWN + UNKNOWN -> UNKNOWN
    pub fn reduce(self, other: Self) -> Self {
        if self.is_unknown() {
            return other;
        }
        if other.is_unknown() {
            return self;
        }
        if matches!(self, Laterality::Bilateral) || matches!(other, Laterality::Bilateral) {
            return Laterality::Bilateral;
        }
        if self.is_unilateral() && other.is_unilateral() {
            if self != other {
                return Laterality::Bilateral;
            }
            return self;
        }
        if self.is_unilateral() {
            return self;
        }
        if other.is_unilateral() {
            return other;
        }
        Laterality::None
    }
}

impl fmt::Display for Laterality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.simple_name())
    }
}

/// View position enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
pub enum ViewPosition {
    Unknown,
    Xccl, // Cranio-caudal exaggerated laterally
    Xccm, // Cranio-caudal exaggerated medially
    Cc,   // Cranio-caudal
    Mlo,  // Medio-lateral oblique
    Ml,   // Medio-lateral
    Lmo,  // Latero-medial oblique
    Lm,   // Latero-medial
    At,   // Axillary tail
    Cv,   // Cleavage view
}

impl ViewPosition {
    /// Returns whether this view position is unknown
    pub fn is_unknown(&self) -> bool {
        matches!(self, ViewPosition::Unknown)
    }

    /// Returns whether this is a standard view (CC or MLO)
    pub fn is_standard_view(&self) -> bool {
        matches!(self, ViewPosition::Cc | ViewPosition::Mlo)
    }

    /// Returns whether this is an MLO-like view
    pub fn is_mlo_like(&self) -> bool {
        matches!(
            self,
            ViewPosition::Mlo | ViewPosition::Ml | ViewPosition::Lmo | ViewPosition::Lm
        )
    }

    /// Returns whether this is a CC-like view
    pub fn is_cc_like(&self) -> bool {
        matches!(
            self,
            ViewPosition::Cc | ViewPosition::Xccl | ViewPosition::Xccm
        )
    }

    /// Returns short string representation
    pub fn short_str(&self) -> &'static str {
        match self {
            ViewPosition::Unknown => "",
            ViewPosition::Xccl => "xccl",
            ViewPosition::Xccm => "xccm",
            ViewPosition::Cc => "cc",
            ViewPosition::Mlo => "mlo",
            ViewPosition::Ml => "ml",
            ViewPosition::Lmo => "lmo",
            ViewPosition::Lm => "lm",
            ViewPosition::At => "at",
            ViewPosition::Cv => "cv",
        }
    }

    /// Returns simple name for display
    pub fn simple_name(&self) -> &'static str {
        self.short_str()
    }
}

impl fmt::Display for ViewPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.simple_name())
    }
}

/// Photometric interpretation enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhotometricInterpretation {
    Unknown,
    Monochrome1,
    Monochrome2,
    PaletteColor,
    Rgb,
    Hsv,
    Argb,
    Cmyk,
    YbrFull,
    YbrFull422,
    YbrPartial422,
    YbrPartial420,
    YbrIct,
    YbrRct,
}

impl PhotometricInterpretation {
    /// Returns whether this is a monochrome interpretation
    pub fn is_monochrome(&self) -> bool {
        matches!(
            self,
            PhotometricInterpretation::Monochrome1 | PhotometricInterpretation::Monochrome2
        )
    }

    /// Returns whether this is inverted (MONOCHROME1)
    pub fn is_inverted(&self) -> bool {
        matches!(self, PhotometricInterpretation::Monochrome1)
    }

    /// Returns the number of color channels
    pub fn num_channels(&self) -> usize {
        if self.is_monochrome() {
            1
        } else {
            3
        }
    }

    /// Parses photometric interpretation from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "MONOCHROME1" => PhotometricInterpretation::Monochrome1,
            "MONOCHROME2" => PhotometricInterpretation::Monochrome2,
            "PALETTE COLOR" => PhotometricInterpretation::PaletteColor,
            "RGB" => PhotometricInterpretation::Rgb,
            "HSV" => PhotometricInterpretation::Hsv,
            "ARGB" => PhotometricInterpretation::Argb,
            "CMYK" => PhotometricInterpretation::Cmyk,
            "YBR_FULL" => PhotometricInterpretation::YbrFull,
            "YBR_FULL_422" => PhotometricInterpretation::YbrFull422,
            "YBR_PARTIAL_422" => PhotometricInterpretation::YbrPartial422,
            "YBR_PARTIAL_420" => PhotometricInterpretation::YbrPartial420,
            "YBR_ICT" => PhotometricInterpretation::YbrIct,
            "YBR_RCT" => PhotometricInterpretation::YbrRct,
            _ => PhotometricInterpretation::Unknown,
        }
    }
}

impl fmt::Display for PhotometricInterpretation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            PhotometricInterpretation::Unknown => "UNKNOWN",
            PhotometricInterpretation::Monochrome1 => "MONOCHROME1",
            PhotometricInterpretation::Monochrome2 => "MONOCHROME2",
            PhotometricInterpretation::PaletteColor => "PALETTE COLOR",
            PhotometricInterpretation::Rgb => "RGB",
            PhotometricInterpretation::Hsv => "HSV",
            PhotometricInterpretation::Argb => "ARGB",
            PhotometricInterpretation::Cmyk => "CMYK",
            PhotometricInterpretation::YbrFull => "YBR_FULL",
            PhotometricInterpretation::YbrFull422 => "YBR_FULL_422",
            PhotometricInterpretation::YbrPartial422 => "YBR_PARTIAL_422",
            PhotometricInterpretation::YbrPartial420 => "YBR_PARTIAL_420",
            PhotometricInterpretation::YbrIct => "YBR_ICT",
            PhotometricInterpretation::YbrRct => "YBR_RCT",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mammogram_type_ordering() {
        assert!(MammogramType::Tomo < MammogramType::Ffdm);
        assert!(MammogramType::Ffdm < MammogramType::Synth);
        assert!(MammogramType::Synth < MammogramType::Sfm);
        assert!(MammogramType::Sfm < MammogramType::Unknown);
    }

    #[test]
    fn test_mammogram_type_preference() {
        assert!(MammogramType::Tomo.is_preferred_to(&MammogramType::Ffdm));
        assert!(MammogramType::Ffdm.is_preferred_to(&MammogramType::Synth));
        assert!(!MammogramType::Synth.is_preferred_to(&MammogramType::Ffdm));
        assert!(!MammogramType::Unknown.is_preferred_to(&MammogramType::Ffdm));
    }

    #[test]
    fn test_laterality_reduce() {
        assert_eq!(
            Laterality::Left.reduce(Laterality::Right),
            Laterality::Bilateral
        );
        assert_eq!(Laterality::Left.reduce(Laterality::Left), Laterality::Left);
        assert_eq!(
            Laterality::Unknown.reduce(Laterality::Left),
            Laterality::Left
        );
        assert_eq!(Laterality::None.reduce(Laterality::None), Laterality::None);
    }

    #[test]
    fn test_view_position_properties() {
        assert!(ViewPosition::Cc.is_standard_view());
        assert!(ViewPosition::Mlo.is_standard_view());
        assert!(!ViewPosition::Ml.is_standard_view());

        assert!(ViewPosition::Mlo.is_mlo_like());
        assert!(ViewPosition::Ml.is_mlo_like());
        assert!(!ViewPosition::Cc.is_mlo_like());

        assert!(ViewPosition::Cc.is_cc_like());
        assert!(ViewPosition::Xccl.is_cc_like());
        assert!(!ViewPosition::Mlo.is_cc_like());
    }
}
