use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use regex::Regex;

/// A supersigil tag occurrence found in a source file.
#[derive(Debug, Clone)]
pub struct TagMatch {
    /// Path to the file containing the tag.
    pub file: PathBuf,
    /// Line number (1-based) where the tag was found.
    pub line: usize,
    /// The tag value (e.g. `"prop:auth-login"`).
    pub tag: String,
}

/// Scan files for a specific tag. Returns all matches.
///
/// # Panics
///
/// Panics if the escaped tag produces an invalid regex (should not happen in practice).
#[must_use]
pub fn scan_for_tag(tag: &str, files: &[PathBuf]) -> Vec<TagMatch> {
    let escaped = regex::escape(tag);
    let pattern = format!(r"(?:///?|#|--|/\*)\s*supersigil:\s+{escaped}(?:\s|$|\*/)");
    let re = Regex::new(&pattern).expect("valid tag regex");
    scan_files(&re, Some(tag), files)
}

/// Scan files for ALL supersigil tags. Returns all matches with their tag values.
///
/// # Panics
///
/// Panics if the internal regex fails to compile (should not happen in practice).
#[must_use]
pub fn scan_all_tags(files: &[PathBuf]) -> Vec<TagMatch> {
    let pattern = r"(?:///?|#|--|/\*)\s*supersigil:\s+(\S+)";
    let re = Regex::new(pattern).expect("valid all-tags regex");
    scan_files(&re, None, files)
}

fn scan_files(re: &Regex, fixed_tag: Option<&str>, files: &[PathBuf]) -> Vec<TagMatch> {
    let mut matches = Vec::new();
    for file in files {
        scan_file(re, fixed_tag, file, &mut matches);
    }
    matches
}

fn scan_file(re: &Regex, fixed_tag: Option<&str>, path: &Path, matches: &mut Vec<TagMatch>) {
    let Ok(f) = fs::File::open(path) else { return };
    let reader = BufReader::new(f);
    for (line_idx, line_result) in reader.lines().enumerate() {
        let Ok(line) = line_result else { return }; // non-UTF-8 → stop reading this file
        if let Some(tag) = extract_tag(re, fixed_tag, &line) {
            matches.push(TagMatch {
                file: path.to_owned(),
                line: line_idx + 1,
                tag,
            });
        }
    }
}

/// Extract a tag from a line using a single regex pass.
fn extract_tag(re: &Regex, fixed_tag: Option<&str>, line: &str) -> Option<String> {
    if let Some(t) = fixed_tag {
        re.is_match(line).then(|| t.to_owned())
    } else {
        re.captures(line)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::write_test_file;
    use tempfile::TempDir;

    #[test]
    fn matches_all_comment_styles() {
        let cases = [
            (
                "test.rs",
                "// supersigil: prop:auth-login\n",
                "prop:auth-login",
            ),
            ("test.rs", "/// supersigil: prop:docs\n", "prop:docs"),
            ("test.py", "# supersigil: prop:login\n", "prop:login"),
            ("test.sql", "-- supersigil: prop:query\n", "prop:query"),
            ("test.c", "/* supersigil: prop:alloc */\n", "prop:alloc"),
        ];
        for (file, content, tag) in cases {
            let dir = TempDir::new().unwrap();
            let path = write_test_file(&dir, file, content);
            let matches = scan_for_tag(tag, &[path]);
            assert_eq!(matches.len(), 1, "expected 1 match for {file}: {content}");
            assert_eq!(matches[0].tag, tag);
        }
    }

    #[test]
    fn no_match_for_different_tag() {
        let dir = TempDir::new().unwrap();
        let path = write_test_file(&dir, "test.rs", "// supersigil: prop:other\n");
        let matches = scan_for_tag("prop:login", &[path]);
        assert!(matches.is_empty());
    }

    #[test]
    fn multiple_matches_in_one_file() {
        let dir = TempDir::new().unwrap();
        let path = write_test_file(
            &dir,
            "test.rs",
            "// supersigil: prop:x\nfn test() {}\n// supersigil: prop:x\n",
        );
        let matches = scan_for_tag("prop:x", &[path]);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 1);
        assert_eq!(matches[1].line, 3);
    }

    #[test]
    fn skips_non_utf8_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("binary.bin");
        std::fs::write(&path, [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        let matches = scan_for_tag("anything", &[path]);
        assert!(matches.is_empty());
    }

    #[test]
    fn scan_all_tags_collects_distinct_tags() {
        let dir = TempDir::new().unwrap();
        let path = write_test_file(
            &dir,
            "test.rs",
            "// supersigil: prop:a\n// supersigil: prop:b\n// supersigil: prop:a\n",
        );
        let tags = scan_all_tags(&[path]);
        assert_eq!(tags.len(), 3); // 3 matches total
        let unique: std::collections::HashSet<_> = tags.iter().map(|m| m.tag.as_str()).collect();
        assert!(unique.contains("prop:a"));
        assert!(unique.contains("prop:b"));
    }

    #[test]
    fn tag_with_special_regex_chars_is_escaped() {
        let dir = TempDir::new().unwrap();
        let path = write_test_file(&dir, "test.rs", "// supersigil: prop:foo.bar\n");
        let matches = scan_for_tag("prop:foo.bar", std::slice::from_ref(&path));
        assert_eq!(matches.len(), 1);
        // Ensure the dot is literal, not regex wildcard
        let no_match = scan_for_tag("prop:fooXbar", std::slice::from_ref(&path));
        assert!(no_match.is_empty());
    }
}
