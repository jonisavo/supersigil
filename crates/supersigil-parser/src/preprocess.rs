//! Stage 1a: UTF-8 decode, BOM strip, CRLF normalization.

use std::path::Path;

use supersigil_core::ParseError;

/// UTF-8 BOM character.
const BOM: char = '\u{FEFF}';

/// Stage 1: Preprocess raw bytes — decode UTF-8, strip BOM, normalize CRLF to LF.
///
/// # Errors
///
/// Returns `ParseError::IoError` if the input is not valid UTF-8.
pub fn preprocess(raw: &[u8], path: &Path) -> Result<String, ParseError> {
    let text = std::str::from_utf8(raw).map_err(|e| ParseError::IoError {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
    })?;

    let text = text.strip_prefix(BOM).unwrap_or(text);

    // Fast path: no \r means no CRLF normalization needed.
    if !text.as_bytes().contains(&b'\r') {
        return Ok(text.to_owned());
    }

    // Normalize CRLF → LF without creating new CRLF from bare \r + replacement \n.
    // \r and \n are single-byte ASCII, so we can safely scan bytes and reconstruct
    // valid UTF-8 by copying non-CRLF spans verbatim.
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            // Flush the span before this \r\n, then emit \n
            out.push_str(&text[start..i]);
            out.push('\n');
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[start..]);
    Ok(out)
}
