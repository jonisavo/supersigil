use regex::Regex;
use std::sync::LazyLock;

use crate::refs::parse_requirement_refs;

use super::RawRef;

/// Parsed design.md
#[derive(Debug, Clone)]
pub struct ParsedDesign {
    /// Document title extracted from the `# Design` heading.
    pub title: Option<String>,
    /// Sections parsed from the document.
    pub sections: Vec<DesignSection>,
}

/// A single section within a parsed design document.
#[derive(Debug, Clone)]
pub struct DesignSection {
    /// Section heading text.
    pub heading: String,
    /// Heading level (2 for `##`, 3 for `###`, etc.; 0 for synthetic preamble).
    pub level: u8,
    /// Content blocks within this section.
    pub content: Vec<DesignBlock>,
}

/// A content block within a design section.
#[derive(Debug, Clone)]
pub enum DesignBlock {
    /// A block of prose text.
    Prose(String),
    /// A fenced code block.
    CodeBlock {
        /// Language identifier (e.g., `rust`, `mermaid`), if specified.
        language: Option<String>,
        /// Code content inside the fence.
        content: String,
    },
    /// A `**Validates: ...**` line with parsed requirement references.
    ValidatesLine {
        /// The original raw line text.
        raw: String,
        /// Parsed requirement references.
        refs: Vec<RawRef>,
        /// Ambiguity markers for unparseable portions.
        markers: Vec<String>,
    },
}

// Regex patterns per the design document's parsing strategy table.
static DOC_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^# Design(?:\s+Document)?(?:: (.+))?$").expect("valid regex"));

static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{2,6})\s+(.+)$").expect("valid regex"));

static VALIDATES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*Validates:\s*(.+)\*\*$").expect("valid regex"));

static CODE_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^```(\w*)$").expect("valid regex"));

/// Parse a Kiro `design.md` file into a structured IR.
///
/// Uses line-by-line processing with regex patterns. Handles:
/// - Document title from `# Design Document: Title` or `# Design: Title`
/// - Sections with headings at levels 2–6
/// - Prose paragraphs, fenced code blocks, mermaid diagram blocks
/// - `**Validates: Requirements X.Y**` lines with ref extraction
/// - Non-requirement Validates targets (preserved as prose with ambiguity marker)
#[must_use]
pub fn parse_design(content: &str) -> ParsedDesign {
    let mut title: Option<String> = None;
    let mut sections: Vec<DesignSection> = Vec::new();
    // Content collected before the first heading goes into a synthetic top-level section.
    let mut preamble_blocks: Vec<DesignBlock> = Vec::new();
    let mut current_section: Option<SectionBuilder> = None;

    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut code_lines: Vec<String> = Vec::new();
    let mut prose_buf: Vec<String> = Vec::new();

    for line in content.lines() {
        // --- Code block handling (must be checked first) ---
        if in_code_block {
            if line.trim_start() == "```" {
                // End of code block — flush it
                let block = build_code_block(code_lang.as_deref(), &code_lines);
                push_block(&mut current_section, &mut preamble_blocks, block);
                in_code_block = false;
                code_lang = None;
                code_lines.clear();
            } else {
                code_lines.push(line.to_string());
            }
            continue;
        }

        // Check for code fence start
        if let Some(caps) = CODE_FENCE_RE.captures(line) {
            // Flush any accumulated prose before the code block
            flush_prose(&mut prose_buf, &mut current_section, &mut preamble_blocks);
            let lang = caps.get(1).map_or("", |m| m.as_str());
            code_lang = if lang.is_empty() {
                None
            } else {
                Some(lang.to_string())
            };
            in_code_block = true;
            code_lines.clear();
            continue;
        }

        // --- Document title ---
        if let Some(caps) = DOC_TITLE_RE.captures(line) {
            flush_prose(&mut prose_buf, &mut current_section, &mut preamble_blocks);
            title = caps.get(1).map(|m| m.as_str().trim().to_string());
            continue;
        }

        // --- Section heading ---
        if let Some(caps) = HEADING_RE.captures(line) {
            flush_prose(&mut prose_buf, &mut current_section, &mut preamble_blocks);
            flush_section(&mut current_section, &mut sections);

            #[allow(
                clippy::cast_possible_truncation,
                reason = "heading levels 2–6 always fit in u8"
            )]
            let level = caps[1].len() as u8;
            let heading = caps[2].trim().to_string();
            current_section = Some(SectionBuilder {
                heading,
                level,
                content: Vec::new(),
            });
            continue;
        }

        // --- Validates line ---
        if let Some(caps) = VALIDATES_RE.captures(line) {
            flush_prose(&mut prose_buf, &mut current_section, &mut preamble_blocks);
            let raw_value = caps[1].trim().to_string();
            let block = parse_validates_block(line, &raw_value);
            push_block(&mut current_section, &mut preamble_blocks, block);
            continue;
        }

        // --- Regular prose line ---
        prose_buf.push(line.to_string());
    }

    // Flush remaining state
    flush_prose(&mut prose_buf, &mut current_section, &mut preamble_blocks);
    flush_section(&mut current_section, &mut sections);

    // If there's preamble content, insert it as a synthetic section
    if !preamble_blocks.is_empty() {
        sections.insert(
            0,
            DesignSection {
                heading: String::new(),
                level: 0,
                content: preamble_blocks,
            },
        );
    }

    ParsedDesign { title, sections }
}

/// Build a `DesignBlock` for a completed code block.
fn build_code_block(lang: Option<&str>, lines: &[String]) -> DesignBlock {
    DesignBlock::CodeBlock {
        language: lang.map(String::from),
        content: lines.join("\n"),
    }
}

/// Parse a `**Validates: ...**` line into either a `ValidatesLine` block
/// (when it references requirements) or a `Prose` block with ambiguity marker
/// (when it references non-requirement targets).
fn parse_validates_block(raw_line: &str, value: &str) -> DesignBlock {
    let (refs, markers) = parse_requirement_refs(value);
    if refs.is_empty() {
        DesignBlock::Prose(format!(
            "{raw_line}\n\
             <!-- TODO(supersigil-import): Validates line references non-requirement \
             target: '{value}' -->"
        ))
    } else {
        DesignBlock::ValidatesLine {
            raw: raw_line.to_string(),
            refs,
            markers,
        }
    }
}

/// Temporary builder for accumulating section content during parsing.
struct SectionBuilder {
    heading: String,
    level: u8,
    content: Vec<DesignBlock>,
}

/// Push a block into the current section, or into the preamble if no section is active.
fn push_block(
    current_section: &mut Option<SectionBuilder>,
    preamble: &mut Vec<DesignBlock>,
    block: DesignBlock,
) {
    if let Some(section) = current_section {
        section.content.push(block);
    } else {
        preamble.push(block);
    }
}

/// Flush accumulated prose lines into a `Prose` block.
fn flush_prose(
    buf: &mut Vec<String>,
    current_section: &mut Option<SectionBuilder>,
    preamble: &mut Vec<DesignBlock>,
) {
    if buf.is_empty() {
        return;
    }
    let text = super::join_trimmed(buf);
    if !text.is_empty() {
        push_block(current_section, preamble, DesignBlock::Prose(text));
    }
    buf.clear();
}

/// Flush the current section builder into the sections list.
fn flush_section(current: &mut Option<SectionBuilder>, sections: &mut Vec<DesignSection>) {
    if let Some(builder) = current.take() {
        sections.push(DesignSection {
            heading: builder.heading,
            level: builder.level,
            content: builder.content,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_design() {
        let result = parse_design("");
        assert!(result.title.is_none());
        assert!(result.sections.is_empty());
    }

    #[test]
    fn parse_title_with_design_document_prefix() {
        let input = "# Design Document: My Feature\n\nSome overview text.";
        let result = parse_design(input);
        assert_eq!(result.title.as_deref(), Some("My Feature"));
    }

    #[test]
    fn parse_title_with_design_prefix() {
        let input = "# Design: Another Feature\n\nOverview.";
        let result = parse_design(input);
        assert_eq!(result.title.as_deref(), Some("Another Feature"));
    }

    #[test]
    fn parse_title_without_title_text() {
        let input = "# Design Document\n\nOverview.";
        let result = parse_design(input);
        // No title text after the colon — title should be None
        assert!(result.title.is_none());
    }

    #[test]
    fn parse_sections_and_prose() {
        let input = "\
## Overview

This is the overview.

## Architecture

Architecture details here.";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].heading, "Overview");
        assert_eq!(result.sections[0].level, 2);
        assert_eq!(result.sections[1].heading, "Architecture");
    }

    #[test]
    fn parse_code_block() {
        let input = "\
## Components

```rust
fn main() {}
```";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].content.len(), 1);
        match &result.sections[0].content[0] {
            DesignBlock::CodeBlock { language, content } => {
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(content, "fn main() {}");
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parse_mermaid_block() {
        let input = "\
## Diagram

```mermaid
graph TD
    A --> B
```";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].content.len(), 1);
        match &result.sections[0].content[0] {
            DesignBlock::CodeBlock {
                language: Some(lang),
                content,
            } if lang == "mermaid" => {
                assert!(content.contains("graph TD"));
                assert!(content.contains("A --> B"));
            }
            other => panic!("expected CodeBlock(mermaid), got {other:?}"),
        }
    }

    #[test]
    fn parse_validates_line_with_refs() {
        let input = "\
## Property 1

Some property description.

**Validates: Requirements 1.1, 1.2**";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 1);
        let blocks = &result.sections[0].content;
        // Should have prose + validates
        let validates = blocks
            .iter()
            .find(|b| matches!(b, DesignBlock::ValidatesLine { .. }));
        assert!(validates.is_some(), "expected a ValidatesLine block");
        if let Some(DesignBlock::ValidatesLine { refs, .. }) = validates {
            assert_eq!(refs.len(), 2);
            assert_eq!(refs[0].requirement_number, "1");
            assert_eq!(refs[0].criterion_index, "1");
            assert_eq!(refs[1].requirement_number, "1");
            assert_eq!(refs[1].criterion_index, "2");
        }
    }

    #[test]
    fn parse_validates_non_requirement_target() {
        let input = "\
## Section

**Validates: Design Decision 5**";
        let result = parse_design(input);
        let blocks = &result.sections[0].content;
        // Should be preserved as prose with ambiguity marker
        let prose = blocks.iter().find(|b| matches!(b, DesignBlock::Prose(_)));
        assert!(prose.is_some(), "expected a Prose block for non-req target");
        if let Some(DesignBlock::Prose(text)) = prose {
            assert!(text.contains("TODO(supersigil-import)"));
            assert!(text.contains("non-requirement target"));
        }
    }

    #[test]
    fn parse_preamble_before_first_heading() {
        let input = "\
# Design Document: Feature

Some preamble text before any section.

## First Section

Content.";
        let result = parse_design(input);
        assert_eq!(result.title.as_deref(), Some("Feature"));
        // Preamble should be in a synthetic section at index 0
        assert!(result.sections.len() >= 2);
        assert_eq!(result.sections[0].heading, "");
        assert_eq!(result.sections[0].level, 0);
    }

    #[test]
    fn parse_mixed_content() {
        let input = "\
## Overview

Intro paragraph.

```rust
struct Foo;
```

More prose after code.

```mermaid
graph LR
    X --> Y
```

**Validates: Requirements 3.1**";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 1);
        let blocks = &result.sections[0].content;
        // Should have: Prose, CodeBlock, Prose, CodeBlock(mermaid), ValidatesLine
        assert_eq!(blocks.len(), 5);
        assert!(matches!(blocks[0], DesignBlock::Prose(_)));
        assert!(matches!(blocks[1], DesignBlock::CodeBlock { .. }));
        assert!(matches!(blocks[2], DesignBlock::Prose(_)));
        assert!(matches!(
            &blocks[3],
            DesignBlock::CodeBlock { language: Some(l), .. } if l == "mermaid"
        ));
        assert!(matches!(blocks[4], DesignBlock::ValidatesLine { .. }));
    }

    #[test]
    fn parse_code_block_no_language() {
        let input = "\
## Section

```
plain text block
```";
        let result = parse_design(input);
        match &result.sections[0].content[0] {
            DesignBlock::CodeBlock { language, content } => {
                assert!(language.is_none());
                assert_eq!(content, "plain text block");
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_heading_levels() {
        let input = "\
## Level 2

### Level 3

#### Level 4";
        let result = parse_design(input);
        assert_eq!(result.sections.len(), 3);
        assert_eq!(result.sections[0].level, 2);
        assert_eq!(result.sections[1].level, 3);
        assert_eq!(result.sections[2].level, 4);
    }
}
