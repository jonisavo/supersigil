use std::fs;
use std::io::Write;

use crate::{ImportError, OutputFile, PlannedDocument};

/// Write generated spec document files to disk.
///
/// Uses best-effort semantics: files are written sequentially and a failure
/// on a later file does not roll back previously written files.
///
/// When `force` is false, uses `create_new(true)` to atomically fail if a
/// file already exists (avoiding a TOCTOU race with a separate existence check).
///
/// # Errors
///
/// Returns `ImportError::FileExists` if a target file exists and `force` is false.
/// Returns `ImportError::Io` on I/O failures.
pub fn write_files(
    documents: &[PlannedDocument],
    force: bool,
) -> Result<Vec<OutputFile>, ImportError> {
    let mut written = Vec::with_capacity(documents.len());

    for doc in documents {
        if let Some(parent) = doc.output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if force {
            fs::write(&doc.output_path, &doc.content)?;
        } else {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&doc.output_path)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        ImportError::FileExists {
                            path: doc.output_path.clone(),
                        }
                    } else {
                        ImportError::Io { source: e }
                    }
                })?;
            file.write_all(doc.content.as_bytes())?;
        }

        written.push(OutputFile {
            path: doc.output_path.clone(),
            document_id: doc.document_id.clone(),
        });
    }

    Ok(written)
}
