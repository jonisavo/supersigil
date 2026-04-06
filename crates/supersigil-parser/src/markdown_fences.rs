//! Markdown fence extraction for supersigil documents.
//!
//! Parses a Markdown body (standard Markdown constructs) and extracts
//! `supersigil-xml` fenced code blocks containing XML component markup.

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
/// Uses default Markdown constructs.
#[must_use]
pub fn extract_markdown_fences(body: &str, body_offset: usize) -> MarkdownFences {
    let options = markdown::ParseOptions::default();

    let Ok(ast) = markdown::to_mdast(body, &options) else {
        return MarkdownFences::default();
    };

    let mut fences = MarkdownFences::default();
    collect_fences(&ast, body, body_offset, &mut fences);
    fences
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
