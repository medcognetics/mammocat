use crate::api::MammogramMetadata;
use std::fmt;

const FIELD_LABEL_WIDTH: usize = "Transfer Syntax UID".len();

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
        write_field(f, "Type", self.metadata.mammogram_type.simple_name())?;
        write_field(f, "Laterality", self.metadata.laterality.simple_name())?;
        write_field(
            f,
            "View Position",
            self.metadata.view_position.simple_name(),
        )?;
        write_field(f, "Image Type", &self.metadata.image_type)?;
        write_field(
            f,
            "Manufacturer",
            self.metadata.manufacturer.as_deref().unwrap_or("unknown"),
        )?;
        write_field(
            f,
            "Model",
            self.metadata.model.as_deref().unwrap_or("unknown"),
        )?;
        write_field(f, "Frames", self.metadata.number_of_frames)?;
        write_field(f, "For Processing", self.metadata.is_for_processing)?;
        write_field(f, "Has Implant", self.metadata.has_implant)?;
        write_field(f, "Implant Displaced", self.metadata.is_implant_displaced)?;
        write_field(f, "Spot Compression", self.metadata.is_spot_compression)?;
        write_field(f, "Magnification", self.metadata.is_magnified)?;
        write_field(f, "Secondary Capture", self.metadata.is_secondary_capture)?;
        write_field(
            f,
            "Modality",
            self.metadata.modality.as_deref().unwrap_or("unknown"),
        )?;
        write_field(
            f,
            "Transfer Syntax UID",
            self.metadata
                .transfer_syntax_uid
                .as_deref()
                .unwrap_or("unknown"),
        )?;
        write_field(
            f,
            "Transfer Syntax",
            self.metadata
                .transfer_syntax_name
                .as_deref()
                .unwrap_or("unknown"),
        )?;
        write_field(
            f,
            "Compression",
            self.metadata
                .compression_type
                .as_deref()
                .unwrap_or("unknown"),
        )?;
        writeln!(f)?;

        // Additional derived information
        writeln!(f, "Derived Properties")?;
        writeln!(f, "------------------")?;
        write_field(f, "Standard View", self.metadata.is_standard_view())?;
        write_field(f, "Is 2D", self.metadata.is_2d())?;

        Ok(())
    }
}

fn write_field<T: fmt::Display>(f: &mut fmt::Formatter<'_>, label: &str, value: T) -> fmt::Result {
    writeln!(f, "{label:<FIELD_LABEL_WIDTH$}: {value}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageType, Laterality, MammogramType, ViewPosition};

    fn test_metadata() -> MammogramMetadata {
        MammogramMetadata {
            mammogram_type: MammogramType::Ffdm,
            laterality: Laterality::Left,
            view_position: ViewPosition::Cc,
            image_type: ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            is_spot_compression: false,
            is_magnified: false,
            is_implant_displaced: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 1,
            is_secondary_capture: false,
            modality: Some("MG".to_string()),
            transfer_syntax_uid: Some("1.2.840.10008.1.2.1".to_string()),
            transfer_syntax_name: Some("Explicit VR Little Endian".to_string()),
            compression_type: Some("uncompressed".to_string()),
        }
    }

    #[test]
    fn test_text_report_format() {
        let metadata = test_metadata();
        let report = TextReport::new(&metadata);
        let output = format!("{}", report);

        assert!(output.contains("Mammogram Metadata"));
        assert!(output.contains("Type"));
        assert!(output.contains("Laterality"));
        assert!(output.contains("View Position"));
        assert!(output.contains("Manufacturer"));
        assert!(output.contains("Model"));
        assert!(output.contains("Frames"));
        assert!(output.contains("Transfer Syntax UID"));
        assert!(output.contains("Transfer Syntax"));
        assert!(output.contains("Compression"));
    }

    #[test]
    fn text_report_fields_have_aligned_columns() {
        let metadata = test_metadata();
        let output = TextReport::new(&metadata).to_string();
        let field_lines: Vec<&str> = output.lines().filter(|line| line.contains(": ")).collect();

        let colon_columns: Vec<usize> = field_lines
            .iter()
            .map(|line| line.find(':').expect("field line has colon"))
            .collect();
        let value_columns: Vec<usize> = field_lines
            .iter()
            .map(|line| line.find(": ").expect("field line has separator") + 2)
            .collect();

        assert!(
            colon_columns
                .windows(2)
                .all(|columns| columns[0] == columns[1]),
            "field labels should align on one colon column:\n{output}"
        );
        assert!(
            value_columns
                .windows(2)
                .all(|columns| columns[0] == columns[1]),
            "field values should align on one value column:\n{output}"
        );
        assert!(output.contains("Spot Compression"));
        assert!(output.contains("Magnification"));
        assert!(output.contains("Secondary Capture"));
        assert!(output.contains("Spot Compression   : false"));
        assert!(output.contains("Magnification      : false"));
        assert!(output.contains("Secondary Capture  : false"));
    }
}
