use regex::Regex;
use std::fmt;
use std::sync::OnceLock;

/// Pixel spacing in millimeters (row, column)
///
/// Represents the physical spacing between adjacent pixels
/// in the detector/imager, measured in mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelSpacing {
    pub row: f64,
    pub col: f64,
}

impl PixelSpacing {
    /// Creates a new PixelSpacing
    pub fn new(row: f64, col: f64) -> Self {
        Self { row, col }
    }

    /// Parses pixel spacing from string
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
        static REGEX: OnceLock<Regex> = OnceLock::new();
        let re = REGEX.get_or_init(|| {
            Regex::new(r"(\d+\.?\d*(?:[e\-\d]+)?)[^\d.]+(\d+\.?\d*(?:[e\-\d]+)?)")
                .expect("Failed to compile regex")
        });

        let caps = re
            .captures(s)
            .ok_or_else(|| format!("Failed to parse PixelSpacing from '{}'", s))?;

        let row: f64 = caps
            .get(1)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|e| format!("Failed to parse row value: {}", e))?;

        let col: f64 = caps
            .get(2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|e| format!("Failed to parse col value: {}", e))?;

        Ok(PixelSpacing { row, col })
    }
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
}
