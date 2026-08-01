use crate::api::MammogramMetadata;
use std::fmt;

const FIELD_LABEL_WIDTH: usize = "Horizontal Flip Required".len();

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
        write_field(f, "DBT Object Kind", self.metadata.dbt_object_kind)?;
        write_field(f, "Laterality", self.metadata.laterality.simple_name())?;
        write_field(
            f,
            "View Position",
            self.metadata.view_position.simple_name(),
        )?;
        write_field(
            f,
            "Orientation Status",
            self.metadata.conventional_orientation.status,
        )?;
        write_field(
            f,
            "Expected Orientation",
            orientation_components(
                self.metadata
                    .conventional_orientation
                    .expected_components
                    .as_deref(),
            ),
        )?;
        write_field(
            f,
            "Observed Orientation",
            orientation_components(
                self.metadata
                    .conventional_orientation
                    .observed_components
                    .as_deref(),
            ),
        )?;
        write_field(
            f,
            "Horizontal Flip Required",
            optional_bool(
                self.metadata
                    .conventional_orientation
                    .horizontal_flip_required,
            ),
        )?;
        write_field(
            f,
            "Vertical Flip Required",
            optional_bool(
                self.metadata
                    .conventional_orientation
                    .vertical_flip_required,
            ),
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
        match self.metadata.pixel_spacing {
            Some(pixel_spacing) => write_field(f, "Pixel Spacing", pixel_spacing)?,
            None => write_field(f, "Pixel Spacing", "unknown")?,
        }
        write_field(
            f,
            "Concatenation UID",
            self.metadata
                .concatenation_uid
                .as_deref()
                .unwrap_or("unknown"),
        )?;
        write_field(
            f,
            "Concat Source SOP UID",
            self.metadata
                .sop_instance_uid_of_concatenation_source
                .as_deref()
                .unwrap_or("unknown"),
        )?;
        write_field(f, "For Processing", self.metadata.is_for_processing)?;
        write_field(f, "Has Implant", self.metadata.has_implant)?;
        write_field(f, "Implant Displaced", self.metadata.is_implant_displaced())?;
        write_field(f, "Spot Compression", self.metadata.is_spot_compression())?;
        write_field(f, "Magnification", self.metadata.is_magnified())?;
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

fn orientation_components(components: Option<&[String]>) -> String {
    components
        .map(|components| components.join("\\"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DbtObjectKind, ImageType, Laterality, MammogramType, ViewPosition};

    fn test_metadata() -> MammogramMetadata {
        MammogramMetadata {
            mammogram_type: MammogramType::Ffdm,
            dbt_object_kind: DbtObjectKind::None,
            laterality: Laterality::Left,
            view_position: ViewPosition::Cc,
            view_modifiers: Default::default(),
            conventional_orientation: Default::default(),
            image_type: ImageType::new("ORIGINAL".to_string(), "PRIMARY".to_string(), None, None),
            is_for_processing: false,
            has_implant: false,
            manufacturer: Some("Test Manufacturer".to_string()),
            model: Some("Test Model".to_string()),
            number_of_frames: 1,
            pixel_spacing: None,
            concatenation_uid: None,
            sop_instance_uid_of_concatenation_source: None,
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
        assert!(output.contains("DBT Object Kind"));
        assert!(output.contains("Laterality"));
        assert!(output.contains("View Position"));
        assert!(output.contains("Orientation Status"));
        assert!(output.contains("Expected Orientation"));
        assert!(output.contains("Observed Orientation"));
        assert!(output.contains("Horizontal Flip Required"));
        assert!(output.contains("Vertical Flip Required"));
        assert!(output.contains("Manufacturer"));
        assert!(output.contains("Model"));
        assert!(output.contains("Frames"));
        assert!(output.contains("Pixel Spacing"));
        assert!(output.contains("Concatenation UID"));
        assert!(output.contains("Concat Source SOP UID"));
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
        assert!(output
            .lines()
            .any(|line| line.starts_with("Spot Compression") && line.ends_with(": false")));
        assert!(output
            .lines()
            .any(|line| line.starts_with("Magnification") && line.ends_with(": false")));
        assert!(output
            .lines()
            .any(|line| line.starts_with("Secondary Capture") && line.ends_with(": false")));
    }

    #[test]
    fn text_report_includes_slice_dbt_object_kind() {
        let mut metadata = test_metadata();
        metadata.mammogram_type = MammogramType::Tomo;
        metadata.dbt_object_kind = DbtObjectKind::Slice;

        let output = TextReport::new(&metadata).to_string();

        assert!(output
            .lines()
            .any(|line| line.starts_with("Type") && line.ends_with(": tomo")));
        assert!(output
            .lines()
            .any(|line| line.starts_with("DBT Object Kind") && line.ends_with(": slice")));
    }
}
