//! Shared DICOM file discovery helpers for CLI tools.

use std::path::{Path, PathBuf};

use crate::extraction::tags::DICOM_MAGIC_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecursiveFileInventory {
    pub all_files: Vec<PathBuf>,
    pub dicom_files: Vec<PathBuf>,
    pub dbt_files: Vec<PathBuf>,
    pub dbt_skipped_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InventoryFileKind {
    mammogram_candidate: bool,
    dbt_scan_candidate: bool,
}

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
        let file_type = entry.file_type()?;
        let path = entry.path();

        if is_file(&file_type, &path) && is_dicom_candidate(&path) {
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
            let file_type = entry.file_type()?;
            let path = entry.path();
            if is_dir(&file_type, &path) {
                visit(&path, files)?;
            } else if is_file(&file_type, &path) && is_dicom_candidate(&path) {
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

pub(crate) fn collect_recursive_file_inventory(
    directory: &Path,
) -> std::io::Result<RecursiveFileInventory> {
    fn visit(directory: &Path, inventory: &mut RecursiveFileInventory) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if is_dir(&file_type, &path) {
                visit(&path, inventory)?;
            } else if is_file(&file_type, &path) {
                let file_kind = inventory_file_kind(&path);
                if file_kind.mammogram_candidate {
                    inventory.dicom_files.push(path.clone());
                }
                if file_kind.dbt_scan_candidate {
                    inventory.dbt_files.push(path.clone());
                } else {
                    inventory.dbt_skipped_files.push(path.clone());
                }
                inventory.all_files.push(path);
            }
        }
        Ok(())
    }

    let mut inventory = RecursiveFileInventory {
        all_files: Vec::new(),
        dicom_files: Vec::new(),
        dbt_files: Vec::new(),
        dbt_skipped_files: Vec::new(),
    };
    visit(directory, &mut inventory)?;
    inventory.all_files.sort();
    inventory.dicom_files.sort();
    inventory.dbt_files.sort();
    inventory.dbt_skipped_files.sort();
    Ok(inventory)
}

fn is_dir(file_type: &std::fs::FileType, path: &Path) -> bool {
    file_type.is_dir() || (file_type.is_symlink() && path.is_dir())
}

fn is_file(file_type: &std::fs::FileType, path: &Path) -> bool {
    file_type.is_file() || (file_type.is_symlink() && path.is_file())
}

fn is_dicom_candidate(path: &Path) -> bool {
    if has_dicom_candidate_extension(path) {
        return true;
    }
    path.extension().is_none() && is_dicom_file(path)
}

fn inventory_file_kind(path: &Path) -> InventoryFileKind {
    let has_dicom_candidate_extension = has_dicom_candidate_extension(path);
    let has_magic = if has_dicom_candidate_extension {
        false
    } else {
        is_dicom_file(path)
    };
    InventoryFileKind {
        mammogram_candidate: has_dicom_candidate_extension
            || (path.extension().is_none() && has_magic),
        dbt_scan_candidate: has_dicom_candidate_extension || has_magic,
    }
}

fn has_dicom_candidate_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm") || ext.eq_ignore_ascii_case("dicom"))
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
