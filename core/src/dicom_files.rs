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

        if path.is_file() && is_dicom_candidate(&path) {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

/// Collect DICOM file candidates recursively from a directory.
///
/// This is used by collection-level planning, where callers commonly pass a
/// study root containing per-series subdirectories.
pub fn collect_dicom_files_recursively(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn visit(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.is_file() && is_dicom_candidate(&path) {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(directory, &mut files)?;
    files.sort();
    Ok(files)
}

fn is_dicom_candidate(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        ext.eq_ignore_ascii_case("dcm") || ext.eq_ignore_ascii_case("dicom")
    } else {
        is_dicom_file(path)
    }
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
