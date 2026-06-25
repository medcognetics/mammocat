//! Shared DICOM file discovery helpers for CLI tools.

use std::path::{Path, PathBuf};

use crate::extraction::tags::DICOM_MAGIC_BYTES;

/// Collect DICOM file candidates from a directory.
///
/// The scan is intentionally non-recursive to match `mammoselect` behavior.
/// Files with `.dcm` or `.dicom` extensions are accepted directly. Files
/// without an extension are accepted only when they contain the standard DICM
/// magic bytes at offset 128.
pub fn collect_dicom_files(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.eq_ignore_ascii_case("dcm") || ext.eq_ignore_ascii_case("dicom") {
                    files.push(path);
                }
            } else if is_dicom_file(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

/// Check whether a file has the standard DICOM preamble and DICM magic bytes.
pub fn is_dicom_file(path: &Path) -> bool {
    use std::fs::File;
    use std::io::Read;

    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };

    let mut buffer = [0_u8; 132];
    matches!(file.read(&mut buffer), Ok(n) if n >= 132 && &buffer[128..132] == DICOM_MAGIC_BYTES)
}
