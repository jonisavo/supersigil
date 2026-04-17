//! Supersigil Language Server Protocol implementation.

use std::path::{Path, PathBuf};

use lsp_types::Url;
use supersigil_core::SUPERSIGIL_XML_LANG;

pub mod code_actions;
pub mod code_lens;
pub mod commands;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod document_components;
pub mod document_list;
pub mod document_symbols;
pub mod graph_data;
pub mod hover;
pub mod position;
pub mod references;
pub mod rename;
pub mod state;

pub(crate) const REF_ATTRS: &[&str] = &["refs", "implements", "depends"];

pub(crate) const DIAGNOSTIC_SOURCE: &str = "supersigil";

pub(crate) fn path_to_url(path: &Path) -> Option<Url> {
    if path.is_absolute() {
        Url::from_file_path(path).ok()
    } else {
        let abs = if cfg!(windows) {
            PathBuf::from(r"C:\").join(path)
        } else {
            PathBuf::from("/").join(path)
        };
        Url::from_file_path(&abs).ok()
    }
}

/// A supersigil-xml fence region found in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FenceRegion {
    /// 0-based line number of the opening delimiter (e.g. `` ```supersigil-xml ``).
    pub open_line: usize,
    /// 0-based line number of the closing delimiter (e.g. `` ``` ``).
    pub close_line: usize,
}

/// Returns all `supersigil-xml` fence regions in `content`.
///
/// Uses a lightweight line-by-line scan — no full Markdown parse.
/// Handles backtick and tilde fences of varying lengths (>= 3), nested fences
/// (where a shorter delimiter inside a longer one is ignored), and HTML
/// comments outside of fences.
pub(crate) fn supersigil_fence_regions(content: &str) -> Vec<FenceRegion> {
    let mut regions = Vec::new();
    // State: None = not in any fence,
    //        Some((fence_char, open_count, is_supersigil, open_line))
    let mut fence_state: Option<(u8, usize, bool, usize)> = None;
    let mut in_html_comment = false;

    for (i, l) in content.lines().enumerate() {
        let trimmed = l.trim_start();

        if fence_state.is_none() {
            // Outside a code fence — handle HTML comment boundaries.
            if in_html_comment {
                if trimmed.contains("-->") {
                    in_html_comment = false;
                }
                continue;
            }

            if let Some(after_open) = trimmed.strip_prefix("<!--") {
                if !after_open.contains("-->") {
                    in_html_comment = true;
                }
                continue;
            }
        }

        // Detect fence delimiter: backtick run or tilde run of length >= 3.
        let (fence_char, fence_count) = {
            let bt = trimmed.bytes().take_while(|&b| b == b'`').count();
            let tl = trimmed.bytes().take_while(|&b| b == b'~').count();
            if bt >= 3 {
                (b'`', bt)
            } else if tl >= 3 {
                (b'~', tl)
            } else {
                (0u8, 0usize)
            }
        };

        if fence_count >= 3 {
            let after_fence = &trimmed[fence_count..];

            if let Some((open_char, open_count, is_supersigil, open_line)) = fence_state {
                // Inside a fence — check for closing delimiter.
                if fence_char == open_char
                    && fence_count >= open_count
                    && after_fence.trim().is_empty()
                {
                    if is_supersigil {
                        regions.push(FenceRegion {
                            open_line,
                            close_line: i,
                        });
                    }
                    fence_state = None;
                }
                // Not a valid close — content line inside the fence.
            } else {
                // Not inside any fence — this is an opening fence line.
                let info_string = after_fence.trim();
                let is_supersigil = info_string == SUPERSIGIL_XML_LANG
                    || info_string
                        .strip_prefix(SUPERSIGIL_XML_LANG)
                        .is_some_and(|rest| rest.starts_with(' '));
                fence_state = Some((fence_char, fence_count, is_supersigil, i));
            }
        }
    }

    regions
}

/// Returns `true` if the given 0-based line is inside a `supersigil-xml`
/// fenced code block in `content`.
///
/// Uses a lightweight line-by-line scan — no full Markdown parse.
/// A line is "inside" a fence if it is strictly between an opening delimiter
/// (`` ```supersigil-xml `` or `~~~supersigil-xml`) and its matching closing
/// delimiter. The delimiter lines themselves are NOT considered "inside" the
/// fence.
///
/// HTML comments (`<!-- ... -->`) outside of fences are skipped so that
/// commented-out fence examples in scaffold files do not confuse the scanner.
pub(crate) fn is_in_supersigil_fence(content: &str, line: u32) -> bool {
    let target = line as usize;
    supersigil_fence_regions(content)
        .iter()
        .any(|r| target > r.open_line && target < r.close_line)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_in_supersigil_fence --

    #[test]
    fn inside_supersigil_fence() {
        let content =
            "# Title\n```supersigil-xml\n<Criterion id=\"c1\">\nbody\n</Criterion>\n```\nafter";
        // Line 0: # Title
        // Line 1: ```supersigil-xml  (opening delimiter)
        // Line 2: <Criterion ...>
        // Line 3: body
        // Line 4: </Criterion>
        // Line 5: ```  (closing delimiter)
        // Line 6: after
        assert!(is_in_supersigil_fence(content, 2));
        assert!(is_in_supersigil_fence(content, 3));
        assert!(is_in_supersigil_fence(content, 4));
    }

    #[test]
    fn outside_supersigil_fence() {
        let content = "before\n```supersigil-xml\ninside\n```\nafter";
        // Line 0: before
        // Line 4: after
        assert!(!is_in_supersigil_fence(content, 0));
        assert!(!is_in_supersigil_fence(content, 4));
    }

    #[test]
    fn on_opening_delimiter() {
        let content = "```supersigil-xml\ninside\n```";
        assert!(!is_in_supersigil_fence(content, 0));
    }

    #[test]
    fn on_closing_delimiter() {
        let content = "```supersigil-xml\ninside\n```";
        assert!(!is_in_supersigil_fence(content, 2));
    }

    #[test]
    fn multiple_fences() {
        let content = "```supersigil-xml\nfirst\n```\nbetween\n```supersigil-xml\nsecond\n```";
        // Line 0: ```supersigil-xml
        // Line 1: first
        // Line 2: ```
        // Line 3: between
        // Line 4: ```supersigil-xml
        // Line 5: second
        // Line 6: ```
        assert!(is_in_supersigil_fence(content, 1));
        assert!(!is_in_supersigil_fence(content, 3));
        assert!(is_in_supersigil_fence(content, 5));
    }

    #[test]
    fn no_fences_at_all() {
        let content = "just plain\nmarkdown\ntext";
        assert!(!is_in_supersigil_fence(content, 0));
        assert!(!is_in_supersigil_fence(content, 1));
        assert!(!is_in_supersigil_fence(content, 2));
    }

    #[test]
    fn fence_with_four_backticks() {
        let content = "````supersigil-xml\ninside\n````";
        assert!(!is_in_supersigil_fence(content, 0));
        assert!(is_in_supersigil_fence(content, 1));
        assert!(!is_in_supersigil_fence(content, 2));
    }

    #[test]
    fn non_supersigil_fence() {
        let content = "```json\n{\"key\": \"value\"}\n```";
        // Line 1 is inside a json fence, not a supersigil-xml fence.
        assert!(!is_in_supersigil_fence(content, 0));
        assert!(!is_in_supersigil_fence(content, 1));
        assert!(!is_in_supersigil_fence(content, 2));
    }

    #[test]
    fn non_supersigil_fence_does_not_confuse_subsequent_fence() {
        let content = "```json\n{}\n```\n```supersigil-xml\n<Task id=\"t1\" />\n```";
        // Line 0: ```json
        // Line 1: {}
        // Line 2: ```
        // Line 3: ```supersigil-xml
        // Line 4: <Task ...>
        // Line 5: ```
        assert!(!is_in_supersigil_fence(content, 1));
        assert!(is_in_supersigil_fence(content, 4));
    }

    #[test]
    fn between_fences_is_outside() {
        let content = "```supersigil-xml\na\n```\nmiddle\n```supersigil-xml\nb\n```";
        assert!(!is_in_supersigil_fence(content, 3));
    }

    #[test]
    fn nested_fence_four_backtick_outer() {
        // 4-backtick outer fence containing a 3-backtick inner block
        let content = "````supersigil-xml\nsome content\n```\nstill inside\n````";
        // Line 0: ````supersigil-xml (opening, 4 backticks)
        // Line 1: some content
        // Line 2: ``` (only 3 backticks — not enough to close)
        // Line 3: still inside
        // Line 4: ```` (closing, 4 backticks)
        assert!(is_in_supersigil_fence(content, 1));
        assert!(is_in_supersigil_fence(content, 2));
        assert!(is_in_supersigil_fence(content, 3));
        assert!(!is_in_supersigil_fence(content, 4));
    }

    #[test]
    fn fence_with_trailing_content_in_info_string() {
        let content = "```supersigil-xml some-extra-info\ninside\n```";
        assert!(is_in_supersigil_fence(content, 1));
    }

    // -- HTML comment handling (Task 5) --

    #[test]
    fn commented_fence_does_not_confuse_scanner() {
        // <!-- ```supersigil-xml   (line 0)
        // <Implements refs="" />   (line 1)
        // ``` -->                  (line 2)
        //                          (line 3)
        // ```supersigil-xml        (line 4)
        // <Task id="t1" />         (line 5)
        // ```                      (line 6)
        let content = "<!-- ```supersigil-xml\n<Implements refs=\"\" />\n``` -->\n\n```supersigil-xml\n<Task id=\"t1\" />\n```";
        assert!(is_in_supersigil_fence(content, 5)); // inside real fence
        assert!(!is_in_supersigil_fence(content, 1)); // inside comment
        assert!(!is_in_supersigil_fence(content, 2)); // inside comment
    }

    #[test]
    fn multiline_comment_with_fence_inside() {
        // <!-- Subtasks:           (line 0)
        //                          (line 1)
        // ```supersigil-xml        (line 2)
        // <Task id="t1" ... />     (line 3)
        // ```                      (line 4)
        // -->                      (line 5)
        //                          (line 6)
        // ```supersigil-xml        (line 7)
        // <Task id="real" />       (line 8)
        // ```                      (line 9)
        let content = "<!-- Subtasks:\n\n```supersigil-xml\n<Task id=\"t1\" status=\"draft\" />\n```\n-->\n\n```supersigil-xml\n<Task id=\"real\" />\n```";
        assert!(!is_in_supersigil_fence(content, 3)); // inside comment
        assert!(is_in_supersigil_fence(content, 8)); // inside real fence
    }

    // -- Tilde fence support (Task 6) --

    #[test]
    fn tilde_fence_recognized() {
        // ~~~supersigil-xml  (line 0)
        // <Task id="t1" />  (line 1)
        // ~~~               (line 2)
        let content = "~~~supersigil-xml\n<Task id=\"t1\" />\n~~~";
        assert!(is_in_supersigil_fence(content, 1));
        assert!(!is_in_supersigil_fence(content, 0));
        assert!(!is_in_supersigil_fence(content, 2));
    }

    #[test]
    fn tilde_fence_not_closed_by_backticks() {
        // ~~~supersigil-xml  (line 0)
        // <Task />           (line 1)
        // ```                (line 2) — backticks don't close a tilde fence
        // still inside       (line 3)
        // ~~~                (line 4)
        let content = "~~~supersigil-xml\n<Task />\n```\nstill inside\n~~~";
        assert!(is_in_supersigil_fence(content, 1));
        assert!(is_in_supersigil_fence(content, 3)); // ``` doesn't close ~~~
        assert!(!is_in_supersigil_fence(content, 4)); // not inside after ~~~
    }
}
