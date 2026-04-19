//! Markdown fence extraction for supersigil documents.
//!
//! Uses a cheap top-level fence scan for the common case and falls back to
//! Markdown AST parsing when the body contains syntax that depends on fuller
//! Markdown block semantics, such as nested/container-style fences or raw HTML
//! flow blocks. Extracts `supersigil-xml` fenced code blocks containing XML
//! component markup.

use markdown::mdast;
use supersigil_core::SUPERSIGIL_XML_LANG;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Collected fences from Markdown parsing.
#[derive(Debug, Default, PartialEq)]
pub struct MarkdownFences {
    /// Content of `supersigil-xml` fenced code blocks with byte offsets.
    pub xml_fences: Vec<XmlFence>,
}

/// A fenced code block with language `supersigil-xml`.
#[derive(Debug, PartialEq)]
pub struct XmlFence {
    /// The raw content between the fences.
    pub content: String,
    /// Byte offset of the content start in the normalized source (after the
    /// opening delimiter line).
    pub content_offset: usize,
    /// Byte offset of the opening fence delimiter (`` ``` `` line) in the
    /// normalized source.
    pub fence_start: usize,
    /// Byte offset of the end of the closing fence delimiter in the normalized
    /// source.
    pub fence_end: usize,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a Markdown body and extract `supersigil-xml` fenced code blocks.
///
/// `body` is the document content after the front-matter block.
/// `body_offset` is the byte offset of `body` within the full normalized
/// source, used to produce file-absolute offsets.
///
/// Uses a fast top-level fence scan and falls back to Markdown parsing for
/// constructs that depend on fuller block parsing, such as blockquotes,
/// indented fences, or raw HTML flow blocks.
#[must_use]
pub fn extract_markdown_fences(body: &str, body_offset: usize) -> MarkdownFences {
    if !body.contains(SUPERSIGIL_XML_LANG) {
        return MarkdownFences::default();
    }

    scan_top_level_supersigil_xml_fences(body, body_offset)
        .unwrap_or_else(|| extract_markdown_fences_via_markdown(body, body_offset))
}

fn extract_markdown_fences_via_markdown(body: &str, body_offset: usize) -> MarkdownFences {
    let options = markdown::ParseOptions::default();

    let Ok(ast) = markdown::to_mdast(body, &options) else {
        return MarkdownFences::default();
    };

    let mut fences = MarkdownFences::default();
    collect_fences(&ast, body, body_offset, &mut fences);
    fences
}

#[derive(Clone, Copy)]
struct OpenFence {
    marker: u8,
    marker_len: usize,
    fence_start: usize,
    content_start: usize,
    is_supersigil: bool,
}

#[derive(Clone, Copy)]
struct FenceLine<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
struct ParsedFenceLine<'a> {
    marker: u8,
    marker_len: usize,
    info: &'a str,
}

fn scan_top_level_supersigil_xml_fences(body: &str, body_offset: usize) -> Option<MarkdownFences> {
    let mut fences = MarkdownFences::default();
    let mut open_fence: Option<OpenFence> = None;

    for line in iter_lines(body) {
        if let Some(open) = open_fence {
            if is_closing_fence(line.text, open.marker, open.marker_len) {
                if open.is_supersigil {
                    fences.xml_fences.push(XmlFence {
                        content: trim_fence_content(&body[open.content_start..line.start])
                            .to_owned(),
                        content_offset: body_offset + open.content_start,
                        fence_start: body_offset + open.fence_start,
                        fence_end: body_offset + line.end,
                    });
                }
                open_fence = None;
            }

            continue;
        }

        if line_requires_markdown_fallback(line.text) {
            return None;
        }

        let Some(parsed) = parse_top_level_opening_fence(line.text) else {
            continue;
        };

        let lang = parsed.info.split_whitespace().next().unwrap_or_default();
        open_fence = Some(OpenFence {
            marker: parsed.marker,
            marker_len: parsed.marker_len,
            fence_start: line.start,
            content_start: line.end,
            is_supersigil: lang == SUPERSIGIL_XML_LANG,
        });
    }

    if let Some(open) = open_fence
        && open.is_supersigil
    {
        fences.xml_fences.push(XmlFence {
            content: trim_fence_content(&body[open.content_start..]).to_owned(),
            content_offset: body_offset + open.content_start,
            fence_start: body_offset + open.fence_start,
            fence_end: body_offset + body.len(),
        });
    }

    Some(fences)
}

fn iter_lines(body: &str) -> impl Iterator<Item = FenceLine<'_>> {
    let mut start = 0;

    std::iter::from_fn(move || {
        if start >= body.len() {
            return None;
        }

        let line_start = start;
        let line_end = body[line_start..]
            .find('\n')
            .map_or(body.len(), |index| line_start + index + 1);
        start = line_end;

        Some(FenceLine {
            text: body[line_start..line_end]
                .strip_suffix('\n')
                .unwrap_or(&body[line_start..line_end]),
            start: line_start,
            end: line_end,
        })
    })
}

fn line_requires_markdown_fallback(line: &str) -> bool {
    has_non_top_level_fence_prefix(line) || has_html_flow_prefix(line)
}

fn has_non_top_level_fence_prefix(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if trimmed.len() != line.len() && starts_with_fence_marker(trimmed) {
        return true;
    }

    let mut rest = trimmed;
    while let Some(stripped) = rest.strip_prefix('>') {
        rest = stripped.trim_start_matches([' ', '\t']);
        if starts_with_fence_marker(rest) {
            return true;
        }
    }

    false
}

fn has_html_flow_prefix(line: &str) -> bool {
    let Some(trimmed) = strip_leading_spaces(line, 3) else {
        return false;
    };

    let Some(rest) = trimmed.strip_prefix('<') else {
        return false;
    };

    matches!(rest.as_bytes().first().copied(), Some(b'!' | b'/' | b'?'))
        || rest
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn parse_top_level_opening_fence(line: &str) -> Option<ParsedFenceLine<'_>> {
    let parsed = parse_fence_markers(line)?;
    if parsed.marker == b'`' && parsed.info.contains('`') {
        return None;
    }

    Some(parsed)
}

fn is_closing_fence(line: &str, marker: u8, marker_len: usize) -> bool {
    let Some(line) = strip_leading_spaces(line, 3) else {
        return false;
    };
    let Some(parsed) = parse_fence_markers(line) else {
        return false;
    };

    parsed.marker == marker
        && parsed.marker_len >= marker_len
        && parsed.info.trim_matches([' ', '\t']).is_empty()
}

fn parse_fence_markers(line: &str) -> Option<ParsedFenceLine<'_>> {
    let bytes = line.as_bytes();
    let marker = *bytes.first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }

    let marker_len = bytes.iter().take_while(|byte| **byte == marker).count();
    if marker_len < 3 {
        return None;
    }

    Some(ParsedFenceLine {
        marker,
        marker_len,
        info: &line[marker_len..],
    })
}

fn starts_with_fence_marker(line: &str) -> bool {
    parse_fence_markers(line).is_some()
}

fn strip_leading_spaces(line: &str, max_spaces: usize) -> Option<&str> {
    let spaces = line
        .as_bytes()
        .iter()
        .take_while(|byte| **byte == b' ')
        .count();
    (spaces <= max_spaces).then_some(&line[spaces..])
}

fn trim_fence_content(content: &str) -> &str {
    content.strip_suffix('\n').unwrap_or(content)
}

/// Recursively walk the AST collecting `Code` nodes.
fn collect_fences(node: &mdast::Node, body: &str, body_offset: usize, fences: &mut MarkdownFences) {
    match node {
        mdast::Node::Code(code) => {
            // `pos.start.offset` points to the opening fence delimiter (``` line).
            // The actual content begins on the next line, so we advance past the
            // first newline to get the byte offset of the code block content.
            let (offset, fence_start_abs, fence_end_abs) =
                code.position.as_ref().map_or((0, 0, 0), |pos| {
                    let fence_start = pos.start.offset;
                    let content_offset = body[fence_start..]
                        .find('\n')
                        .map_or(body_offset + fence_start, |nl| {
                            body_offset + fence_start + nl + 1
                        });
                    (
                        content_offset,
                        body_offset + fence_start,
                        body_offset + pos.end.offset,
                    )
                });

            if code.lang.as_deref() == Some(SUPERSIGIL_XML_LANG) {
                fences.xml_fences.push(XmlFence {
                    content: code.value.clone(),
                    content_offset: offset,
                    fence_start: fence_start_abs,
                    fence_end: fence_end_abs,
                });
            }
        }
        other => {
            if let Some(children) = other.children() {
                for child in children {
                    collect_fences(child, body, body_offset, fences);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_scanner_detects_tilde_fence_with_longer_closer() {
        let body = "~~~supersigil-xml\n<X/>\n~~~~\n";
        let result = scan_top_level_supersigil_xml_fences(body, 0)
            .expect("top-level tilde fence should stay on fast path");
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<X/>");
    }

    #[test]
    fn top_level_scanner_tracks_closing_fence_range() {
        let body = "```supersigil-xml\n<X/>\n```\n";
        let result = scan_top_level_supersigil_xml_fences(body, 7)
            .expect("top-level fence should stay on fast path");
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].fence_start, 7);
        assert_eq!(result.xml_fences[0].content_offset, 25);
        assert_eq!(result.xml_fences[0].fence_end, 34);
    }

    #[test]
    fn top_level_scanner_accepts_closing_fence_with_three_spaces() {
        let body = "```supersigil-xml\n<X/>\n   ```\n";
        let result = scan_top_level_supersigil_xml_fences(body, 0)
            .expect("closing fence with up to three leading spaces stays on fast path");
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<X/>");
        assert_eq!(result.xml_fences[0].fence_end, body.len());
    }

    #[test]
    fn top_level_scanner_returns_none_for_blockquote_fence() {
        let body = "> ```supersigil-xml\n> <X/>\n> ```\n";
        assert!(
            scan_top_level_supersigil_xml_fences(body, 0).is_none(),
            "blockquote fences should fall back to the full Markdown parser"
        );
    }

    #[test]
    fn top_level_scanner_returns_none_for_html_wrapped_example() {
        let body = "<div>\n```supersigil-xml\n<X/>\n```\n</div>\n";
        assert!(
            scan_top_level_supersigil_xml_fences(body, 0).is_none(),
            "raw HTML flow blocks should fall back to the full Markdown parser"
        );
    }

    #[test]
    fn extract_markdown_fences_falls_back_for_blockquote_fence() {
        let body = "> ```supersigil-xml\n> <X/>\n> ```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<X/>");
    }

    #[test]
    fn extract_markdown_fences_ignores_html_wrapped_examples() {
        let body = "\
<div>
```supersigil-xml
<Example/>
```
</div>

```supersigil-xml
<Live/>
```
";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<Live/>");
    }

    #[test]
    fn extract_markdown_fences_falls_back_for_indented_fence() {
        let body = "  ```supersigil-xml\n  <X/>\n  ```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<X/>");
    }

    // -- Fence detection ---------------------------------------------------

    // supersigil: md-fence-detection
    #[test]
    fn no_fences_returns_empty() {
        let body = "# Hello\n\nSome paragraph text.\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
    }

    // supersigil: md-fence-detection
    // supersigil: md-xml-fence-collection
    #[test]
    fn detects_supersigil_xml_fence() {
        let body = "# Title\n\n```supersigil-xml\n<Spec id=\"s1\">hello</Spec>\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<Spec id=\"s1\">hello</Spec>");
    }

    // supersigil: md-xml-fence-collection
    #[test]
    fn detects_multiple_xml_fences() {
        let body = "\
```supersigil-xml
<A/>
```

```supersigil-xml
<B/>
```
";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 2);
        assert_eq!(result.xml_fences[0].content, "<A/>");
        assert_eq!(result.xml_fences[1].content, "<B/>");
    }

    // supersigil: md-xml-fence-collection
    #[test]
    fn xml_fence_offset_includes_body_offset() {
        let body_offset = 42;
        let body = "```supersigil-xml\n<X/>\n```\n";
        let result = extract_markdown_fences(body, body_offset);
        assert_eq!(result.xml_fences.len(), 1);
        // The opening fence line "```supersigil-xml\n" is 18 bytes,
        // so the content starts at body_offset + 18.
        assert_eq!(
            result.xml_fences[0].content_offset,
            body_offset + "```supersigil-xml\n".len()
        );
    }

    // -- Language matching -------------------------------------------------

    #[test]
    fn non_supersigil_xml_lang_ignored() {
        let body = "```rust\nfn main() {}\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
    }

    #[test]
    fn supersigil_xml_is_case_sensitive() {
        let body = "```Supersigil-xml\n<X/>\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
    }

    #[test]
    fn supersigil_xml_with_meta_still_detected() {
        // Even if there's meta after the language, it should still be detected
        // because markdown parses the first word as `lang`.
        let body = "```supersigil-xml some-meta\n<X/>\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<X/>");
    }

    // -- Fences with no meta -----------------------------------------------

    #[test]
    fn code_fence_with_lang_but_no_meta() {
        let body = "```python\nprint('hello')\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
    }

    #[test]
    fn code_fence_with_no_lang_no_meta() {
        let body = "```\nplain text\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
    }

    // -- Mixed fences -------------------------------------------------------

    #[test]
    fn non_supersigil_fences_ignored_alongside_xml() {
        let body = "\
Some text.

```supersigil-xml
<Spec id=\"s1\">content</Spec>
```

```sh
echo hello
```

```rust
fn main() {}
```
";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(
            result.xml_fences[0].content,
            "<Spec id=\"s1\">content</Spec>"
        );
    }
}
