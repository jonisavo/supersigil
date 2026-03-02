use std::io::Write;

/// Write TOML content to a temp file and return the path.
pub fn write_temp_toml(content: &str) -> tempfile::TempPath {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.into_temp_path()
}
