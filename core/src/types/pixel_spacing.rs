use std::fmt;

/// Pixel spacing in millimeters (row, column)
///
/// Represents the physical spacing between adjacent pixels
/// in the detector/imager, measured in mm.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct PixelSpacing {
    pub row: f64,
    #[cfg_attr(feature = "json", serde(rename = "column"))]
    pub col: f64,
}

impl PixelSpacing {
    /// Creates a new PixelSpacing
    pub fn new(row: f64, col: f64) -> Self {
        Self { row, col }
    }

    /// Parses pixel spacing from a string without image-dimension exceptions.
    ///
    /// Accepts formats like:
    /// - "0.1\\0.1"
    /// - "0.1 0.1"
    /// - "[0.1, 0.1]"
    /// - Exponential notation: "1.5e-4 1.5e-4"
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::parse_with_dimensions(s, None, None)
    }

    /// Parses pixel spacing with the DICOM zero-value exceptions for singleton dimensions.
    ///
    /// A zero row spacing is valid only when `rows` is one, and a zero column spacing is
    /// valid only when `columns` is one. All other values must be finite and positive.
    pub fn parse_with_dimensions(
        s: &str,
        rows: Option<u16>,
        columns: Option<u16>,
    ) -> Result<Self, String> {
        let values = spacing_values(s)?;
        let row = parse_spacing_value(values[0], "row", rows)?;
        let col = parse_spacing_value(values[1], "column", columns)?;

        Ok(Self { row, col })
    }
}

fn spacing_values(s: &str) -> Result<[&str; 2], String> {
    let trimmed = s.trim();
    let contents = match (trimmed.strip_prefix('['), trimmed.strip_suffix(']')) {
        (Some(without_prefix), Some(_)) => without_prefix
            .strip_suffix(']')
            .ok_or_else(|| "PixelSpacing has unmatched brackets".to_string())?
            .trim(),
        (None, None) => trimmed,
        _ => return Err("PixelSpacing has unmatched brackets".to_string()),
    };
    let values: Vec<_> = if contents.contains('\\') {
        contents.split('\\').map(str::trim).collect()
    } else if contents.contains(',') {
        contents.split(',').map(str::trim).collect()
    } else {
        contents.split_whitespace().collect()
    };

    values
        .try_into()
        .map_err(|_| "PixelSpacing must contain exactly two values".to_string())
}

fn parse_spacing_value(
    value: &str,
    component: &str,
    dimension: Option<u16>,
) -> Result<f64, String> {
    let parsed: f64 = value
        .parse()
        .map_err(|_| format!("PixelSpacing {component} value is not numeric"))?;
    if !parsed.is_finite() {
        return Err(format!("PixelSpacing {component} value must be finite"));
    }
    if parsed < 0.0 || (parsed == 0.0 && dimension != Some(1)) {
        return Err(format!(
            "PixelSpacing {component} value must be positive unless its dimension is one"
        ));
    }

    Ok(parsed)
}

impl fmt::Display for PixelSpacing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} mm", self.row, self.col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_backslash_separator() {
        let ps = PixelSpacing::parse("0.1\\0.1").unwrap();
        assert_eq!(ps.row, 0.1);
        assert_eq!(ps.col, 0.1);
    }

    #[test]
    fn test_parse_space_separator() {
        let ps = PixelSpacing::parse("0.194 0.194").unwrap();
        assert_eq!(ps.row, 0.194);
        assert_eq!(ps.col, 0.194);
    }

    #[test]
    fn test_parse_array_format() {
        let ps = PixelSpacing::parse("[0.1, 0.1]").unwrap();
        assert_eq!(ps.row, 0.1);
        assert_eq!(ps.col, 0.1);
    }

    #[test]
    fn test_parse_exponential_notation() {
        let ps = PixelSpacing::parse("1.5e-1\\1.5e-1").unwrap();
        assert_eq!(ps.row, 0.15);
        assert_eq!(ps.col, 0.15);
    }

    #[test]
    fn test_parse_exponential_plus_notation() {
        let ps = PixelSpacing::parse("1.5e+1\\1.5e+1").unwrap();
        assert_eq!(ps.row, 15.0);
        assert_eq!(ps.col, 15.0);
    }

    #[test]
    fn test_parse_different_values() {
        let ps = PixelSpacing::parse("0.194\\0.194").unwrap();
        assert_eq!(ps.row, 0.194);
        assert_eq!(ps.col, 0.194);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(PixelSpacing::parse("invalid").is_err());
        assert!(PixelSpacing::parse("").is_err());
        assert!(PixelSpacing::parse("0.1").is_err());
    }

    #[test]
    fn rejects_invalid_multiplicity_and_malformed_values() {
        assert!(PixelSpacing::parse("0.1\\0.2\\0.3").is_err());
        assert!(PixelSpacing::parse("prefix 0.1\\0.2").is_err());
        assert!(PixelSpacing::parse("0.1\\not-a-number").is_err());
    }

    #[test]
    fn rejects_non_finite_negative_and_zero_values() {
        assert!(PixelSpacing::parse("NaN\\0.1").is_err());
        assert!(PixelSpacing::parse("inf\\0.1").is_err());
        assert!(PixelSpacing::parse("-0.1\\0.1").is_err());
        assert!(PixelSpacing::parse("0.1\\0").is_err());
    }

    #[test]
    fn allows_zero_only_for_the_corresponding_single_dimension() {
        assert_eq!(
            PixelSpacing::parse_with_dimensions("0\\0.2", Some(1), Some(8)).unwrap(),
            PixelSpacing::new(0.0, 0.2)
        );
        assert_eq!(
            PixelSpacing::parse_with_dimensions("0.2\\0", Some(8), Some(1)).unwrap(),
            PixelSpacing::new(0.2, 0.0)
        );
        assert!(PixelSpacing::parse_with_dimensions("0\\0.2", Some(2), Some(8)).is_err());
        assert!(PixelSpacing::parse_with_dimensions("0.2\\0", Some(8), Some(2)).is_err());
    }
}
