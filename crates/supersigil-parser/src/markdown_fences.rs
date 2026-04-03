//! Markdown fence extraction for supersigil documents.
//!
//! Parses a Markdown body (standard Markdown constructs) and extracts two
//! categories of fenced code blocks:
//!
//! - **`supersigil-xml` fences** — fenced code blocks with language identifier
//!   `supersigil-xml`, containing XML component markup.
//! - **`supersigil-ref` fences** — fenced code blocks with a `supersigil-ref=<target>`
//!   token in their info-string meta, referencing test content.

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
    /// Code fences with `supersigil-ref` meta attributes.
    pub ref_fences: Vec<RefFence>,
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

/// A fenced code block carrying a `supersigil-ref=<target>` meta token.
#[derive(Debug, PartialEq)]
pub struct RefFence {
    /// The supersigil-ref target (e.g. `"echo-test"` or `"create-task"`).
    pub target: String,
    /// The fragment portion after `#`, if present.
    pub fragment: Option<String>,
    /// Fence language identifier (e.g. `"sh"`, `"json"`).
    pub lang: Option<String>,
    /// Raw code content.
    pub content: String,
    /// Byte offset of the fence start in the normalized source.
    pub content_offset: usize,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `supersigil-ref=<value>` token from a meta string.
///
/// The meta string may contain multiple whitespace-delimited tokens. We look
/// for the first token starting with `supersigil-ref=` and extract the value.
/// The value extends to the next whitespace or end of meta. An optional `#`
/// fragment separator splits the value into target and fragment.
///
/// Returns `None` if no `supersigil-ref=` token is found.
fn parse_ref_meta(meta: &str) -> Option<(String, Option<String>)> {
    const PREFIX: &str = "supersigil-ref=";

    let token = meta.split_whitespace().find(|t| t.starts_with(PREFIX))?;
    let value = &token[PREFIX.len()..];

    if value.is_empty() {
        return None;
    }

    if let Some(hash_pos) = value.find('#') {
        let target = &value[..hash_pos];
        let fragment = &value[hash_pos + 1..];
        if target.is_empty() {
            return None;
        }
        Some((
            target.to_owned(),
            if fragment.is_empty() {
                None
            } else {
                Some(fragment.to_owned())
            },
        ))
    } else {
        Some((value.to_owned(), None))
    }
}

/// Parse a Markdown body and extract `supersigil-xml` and `supersigil-ref`
/// fenced code blocks.
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
            } else if let Some(ref meta) = code.meta
                && let Some((target, fragment)) = parse_ref_meta(meta)
            {
                fences.ref_fences.push(RefFence {
                    target,
                    fragment,
                    lang: code.lang.clone(),
                    content: code.value.clone(),
                    content_offset: offset,
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
        assert!(result.ref_fences.is_empty());
    }

    // supersigil: md-fence-detection
    // supersigil: md-xml-fence-collection
    #[test]
    fn detects_supersigil_xml_fence() {
        let body = "# Title\n\n```supersigil-xml\n<Spec id=\"s1\">hello</Spec>\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.xml_fences.len(), 1);
        assert_eq!(result.xml_fences[0].content, "<Spec id=\"s1\">hello</Spec>");
        assert!(result.ref_fences.is_empty());
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
        assert!(result.ref_fences.is_empty());
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

    // -- supersigil-ref meta parsing: value extraction ---------------------

    // supersigil: md-ref-fence-meta-parsing
    // supersigil: md-ref-fence-recording
    #[test]
    fn detects_ref_fence_simple_target() {
        let body = "```sh supersigil-ref=echo-test\necho hello\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
        assert_eq!(result.ref_fences.len(), 1);
        let rf = &result.ref_fences[0];
        assert_eq!(rf.target, "echo-test");
        assert_eq!(rf.fragment, None);
        assert_eq!(rf.lang.as_deref(), Some("sh"));
        assert_eq!(rf.content, "echo hello");
    }

    // supersigil: md-ref-fence-recording
    #[test]
    fn ref_fence_content_offset_includes_body_offset() {
        let body_offset = 100;
        let body = "```sh supersigil-ref=test\ndata\n```\n";
        let result = extract_markdown_fences(body, body_offset);
        assert_eq!(result.ref_fences.len(), 1);
        // Opening fence line "```sh supersigil-ref=test\n" is 26 bytes.
        assert_eq!(
            result.ref_fences[0].content_offset,
            body_offset + "```sh supersigil-ref=test\n".len()
        );
    }

    // -- supersigil-ref meta parsing: # fragment split ---------------------

    // supersigil: md-ref-fence-meta-parsing
    #[test]
    fn ref_fence_with_fragment() {
        let body = "```json supersigil-ref=create-task#expected\n{}\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.ref_fences.len(), 1);
        let rf = &result.ref_fences[0];
        assert_eq!(rf.target, "create-task");
        assert_eq!(rf.fragment, Some("expected".to_owned()));
    }

    #[test]
    fn ref_fence_fragment_with_trailing_hash_no_fragment_value() {
        // `supersigil-ref=foo#` — hash present but no fragment after it
        let body = "```sh supersigil-ref=foo#\ncontent\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.ref_fences.len(), 1);
        let rf = &result.ref_fences[0];
        assert_eq!(rf.target, "foo");
        assert_eq!(rf.fragment, None);
    }

    // -- supersigil-ref meta parsing: whitespace delimiting ----------------

    // supersigil: md-ref-fence-meta-parsing
    #[test]
    fn ref_fence_with_other_meta_tokens() {
        let body = "```sh supersigil-ref=my-test {1,3}\nsome code\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.ref_fences.len(), 1);
        let rf = &result.ref_fences[0];
        assert_eq!(rf.target, "my-test");
        assert_eq!(rf.fragment, None);
        assert_eq!(rf.lang.as_deref(), Some("sh"));
    }

    #[test]
    fn ref_meta_not_first_token() {
        // The supersigil-ref token doesn't have to be the first meta token.
        let body = "```sh {1,3} supersigil-ref=other-test\ncontent\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert_eq!(result.ref_fences.len(), 1);
        let rf = &result.ref_fences[0];
        assert_eq!(rf.target, "other-test");
    }

    // -- Fences with no meta -----------------------------------------------

    #[test]
    fn code_fence_with_lang_but_no_meta() {
        let body = "```python\nprint('hello')\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
        assert!(result.ref_fences.is_empty());
    }

    #[test]
    fn code_fence_with_no_lang_no_meta() {
        let body = "```\nplain text\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.xml_fences.is_empty());
        assert!(result.ref_fences.is_empty());
    }

    #[test]
    fn ref_fence_with_no_lang() {
        // A fence with no lang but with meta containing supersigil-ref.
        // markdown-rs puts the first word as `lang` and the rest as `meta`,
        // so `supersigil-ref=foo` becomes lang="supersigil-ref=foo", meta=None.
        // This should NOT be detected as a ref fence (it's a lang, not meta).
        let body = "```supersigil-ref=foo\ncontent\n```\n";
        let result = extract_markdown_fences(body, 0);
        assert!(result.ref_fences.is_empty());
        assert!(result.xml_fences.is_empty());
    }

    // -- Mixed fences -------------------------------------------------------

    #[test]
    fn mixed_xml_and_ref_fences() {
        let body = "\
Some text.

```supersigil-xml
<Spec id=\"s1\">content</Spec>
```

```sh supersigil-ref=echo-test#cmd
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
        assert_eq!(result.ref_fences.len(), 1);
        assert_eq!(result.ref_fences[0].target, "echo-test");
        assert_eq!(result.ref_fences[0].fragment, Some("cmd".to_owned()));
        assert_eq!(result.ref_fences[0].lang.as_deref(), Some("sh"));
    }

    // -- parse_ref_meta unit tests -----------------------------------------

    #[test]
    fn parse_ref_meta_simple() {
        let (target, fragment) = parse_ref_meta("supersigil-ref=hello").unwrap();
        assert_eq!(target, "hello");
        assert_eq!(fragment, None);
    }

    #[test]
    fn parse_ref_meta_with_fragment() {
        let (target, fragment) = parse_ref_meta("supersigil-ref=a#b").unwrap();
        assert_eq!(target, "a");
        assert_eq!(fragment, Some("b".to_owned()));
    }

    #[test]
    fn parse_ref_meta_among_other_tokens() {
        let (target, fragment) = parse_ref_meta("{1,3} supersigil-ref=x#y other").unwrap();
        assert_eq!(target, "x");
        assert_eq!(fragment, Some("y".to_owned()));
    }

    #[test]
    fn parse_ref_meta_missing_returns_none() {
        assert!(parse_ref_meta("no-match here").is_none());
    }

    #[test]
    fn parse_ref_meta_empty_value_returns_none() {
        assert!(parse_ref_meta("supersigil-ref=").is_none());
    }

    #[test]
    fn parse_ref_meta_only_hash_returns_none() {
        // `supersigil-ref=#fragment` — no target before the hash
        assert!(parse_ref_meta("supersigil-ref=#fragment").is_none());
    }
}
