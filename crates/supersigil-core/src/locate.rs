//! Filesystem utilities for locating supersigil project files.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The conventional filename for the supersigil project configuration file.
pub const CONFIG_FILENAME: &str = "supersigil.toml";

/// Walk upward from `start` looking for `supersigil.toml`.
///
/// Returns `Ok(Some(path))` when found, `Ok(None)` when the filesystem
/// root is reached without finding one, or `Err` when a non-`NotFound`
/// I/O error is encountered (e.g. permission denied, symlink loop).
///
/// # Errors
///
/// Returns `std::io::Error` for filesystem errors other than `NotFound`.
pub fn find_config(start: &Path) -> Result<Option<PathBuf>, std::io::Error> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(CONFIG_FILENAME);
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(candidate)),
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn finds_config_in_start_dir() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("supersigil.toml"), "").unwrap();

        let result = find_config(tmp.path()).unwrap();
        assert_eq!(result, Some(tmp.path().join("supersigil.toml")));
    }

    #[test]
    fn finds_config_in_ancestor() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("supersigil.toml"), "").unwrap();
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let result = find_config(&nested).unwrap();
        assert_eq!(result, Some(tmp.path().join("supersigil.toml")));
    }

    #[test]
    fn returns_none_when_not_found() {
        let tmp = tempdir().unwrap();
        let result = find_config(tmp.path()).unwrap();
        assert!(result.is_none());
    }
}
