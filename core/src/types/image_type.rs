use std::fmt;

/// DICOM ImageType field decomposed into its components
///
/// The ImageType field contains information about:
/// - `pixels`: First element (e.g., "ORIGINAL", "DERIVED")
/// - `exam`: Second element (e.g., "PRIMARY", "SECONDARY")
/// - `flavor`: Third element (optional)
/// - `extras`: Additional elements beyond the first three
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct ImageType {
    pub pixels: String,
    pub exam: String,
    pub flavor: Option<String>,
    pub extras: Option<Vec<String>>,
}

impl ImageType {
    /// Creates a new ImageType
    pub fn new(
        pixels: String,
        exam: String,
        flavor: Option<String>,
        extras: Option<Vec<String>>,
    ) -> Self {
        Self {
            pixels,
            exam,
            flavor,
            extras,
        }
    }

    /// Returns a simple string representation
    ///
    /// Format: "pixels|exam|flavor|extra1|extra2|..."
    /// Empty flavor is represented as ''
    /// Numeric-only extras are skipped
    pub fn simple_repr(&self) -> String {
        let mut parts = vec![self.pixels.clone(), self.exam.clone()];

        if let Some(ref flavor) = self.flavor {
            parts.push(if flavor.is_empty() {
                "''".to_string()
            } else {
                flavor.clone()
            });
        }

        if let Some(ref extras) = self.extras {
            for extra in extras {
                if !extra.is_empty() && !extra.chars().all(|c| c.is_numeric()) {
                    parts.push(extra.clone());
                }
            }
        }

        parts.join("|")
    }

    /// Checks if the image type contains a specific value
    pub fn contains(&self, val: &str) -> bool {
        self.pixels == val
            || self.exam == val
            || self.flavor.as_ref().is_some_and(|f| f == val)
            || self
                .extras
                .as_ref()
                .is_some_and(|e| e.iter().any(|x| x == val))
    }

    /// Returns true if both pixels and exam are non-empty
    pub fn is_valid(&self) -> bool {
        !self.pixels.is_empty() && !self.exam.is_empty()
    }
}

impl fmt::Display for ImageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.simple_repr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_repr_basic() {
        let img_type = ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None);
        assert_eq!(img_type.simple_repr(), "ORIGINAL|PRIMARY");
    }

    #[test]
    fn test_simple_repr_with_flavor() {
        let img_type = ImageType::new(
            "DERIVED".to_string(),
            "PRIMARY".to_string(),
            Some("POST_CONTRAST".to_string()),
            None,
        );
        assert_eq!(img_type.simple_repr(), "DERIVED|PRIMARY|POST_CONTRAST");
    }

    #[test]
    fn test_simple_repr_empty_flavor() {
        let img_type = ImageType::new(
            "DERIVED".to_string(),
            "PRIMARY".to_string(),
            Some("".to_string()),
            None,
        );
        assert_eq!(img_type.simple_repr(), "DERIVED|PRIMARY|''");
    }

    #[test]
    fn test_simple_repr_with_extras() {
        let img_type = ImageType::new(
            "DERIVED".to_string(),
            "PRIMARY".to_string(),
            Some("TOMO".to_string()),
            Some(vec![
                "GENERATED_2D".to_string(),
                "".to_string(),
                "150000".to_string(), // numeric, should be skipped
            ]),
        );
        assert_eq!(img_type.simple_repr(), "DERIVED|PRIMARY|TOMO|GENERATED_2D");
    }

    #[test]
    fn test_contains() {
        let img_type = ImageType::new(
            "ORIGINAL".to_string(),
            "PRIMARY".to_string(),
            Some("POST_PROCESSED".to_string()),
            Some(vec!["SUBTRACTION".to_string()]),
        );

        assert!(img_type.contains("ORIGINAL"));
        assert!(img_type.contains("PRIMARY"));
        assert!(img_type.contains("POST_PROCESSED"));
        assert!(img_type.contains("SUBTRACTION"));
        assert!(!img_type.contains("DERIVED"));
    }

    #[test]
    fn test_is_valid() {
        assert!(
            ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None).is_valid()
        );
        assert!(!ImageType::new("".to_string(), "PRIMARY".to_string(), None, None).is_valid());
        assert!(!ImageType::new("ORIGINAL".to_string(), "".to_string(), None, None).is_valid());
    }
}
