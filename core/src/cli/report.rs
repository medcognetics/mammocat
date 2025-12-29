use crate::api::MammogramMetadata;
use std::fmt;

/// Text report formatter for mammogram metadata
pub struct TextReport<'a> {
    metadata: &'a MammogramMetadata,
}

impl<'a> TextReport<'a> {
    /// Creates a new text report
    pub fn new(metadata: &'a MammogramMetadata) -> Self {
        Self { metadata }
    }
}

impl<'a> fmt::Display for TextReport<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Mammogram Metadata")?;
        writeln!(f, "==================")?;
        writeln!(f)?;
        writeln!(
            f,
            "Type:           {}",
            self.metadata.mammogram_type.simple_name()
        )?;
        writeln!(
            f,
            "Laterality:     {}",
            self.metadata.laterality.simple_name()
        )?;
        writeln!(
            f,
            "View Position:  {}",
            self.metadata.view_position.simple_name()
        )?;
        writeln!(f, "Image Type:     {}", self.metadata.image_type)?;
        writeln!(f, "For Processing: {}", self.metadata.is_for_processing)?;
        writeln!(f, "Has Implant:    {}", self.metadata.has_implant)?;
        writeln!(f)?;

        // Additional derived information
        writeln!(f, "Derived Properties")?;
        writeln!(f, "------------------")?;
        writeln!(f, "Standard View:  {}", self.metadata.is_standard_view())?;
        writeln!(f, "Is 2D:          {}", self.metadata.is_2d())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageType, Laterality, MammogramType, ViewPosition};

    #[test]
    fn test_text_report_format() {
        let metadata = MammogramMetadata {
            mammogram_type: MammogramType::Ffdm,
            laterality: Laterality::Left,
            view_position: ViewPosition::Cc,
            image_type: ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
        };

        let report = TextReport::new(&metadata);
        let output = format!("{}", report);

        assert!(output.contains("Mammogram Metadata"));
        assert!(output.contains("Type:           ffdm"));
        assert!(output.contains("Laterality:     left"));
        assert!(output.contains("View Position:  cc"));
    }
}
