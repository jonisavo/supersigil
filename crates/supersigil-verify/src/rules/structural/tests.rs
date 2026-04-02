use super::*;
use crate::test_helpers::*;
use supersigil_core::SpanKind;

mod cardinality;
mod misc;
mod placement;
mod sequential_ids;

// Shared helpers for placement and cardinality tests.

fn make_code_block() -> supersigil_core::CodeBlock {
    supersigil_core::CodeBlock {
        lang: Some("bash".into()),
        content: "echo hello".into(),
        content_offset: 0,
        content_end_offset: "echo hello".len(),
        span_kind: SpanKind::RefFence,
    }
}

fn make_example(
    children: Vec<supersigil_core::ExtractedComponent>,
    line: usize,
) -> supersigil_core::ExtractedComponent {
    supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children,
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(line),
        end_position: pos(line),
    }
}

fn make_expected(line: usize) -> supersigil_core::ExtractedComponent {
    supersigil_core::ExtractedComponent {
        name: EXPECTED.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line),
    }
}
