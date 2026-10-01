//! HEAD comparison through the pure line-explanation interface.

use supersigil_anchor::lines::split_lines;
use supersigil_review::diff::COARSE_LINES;
use supersigil_review::outcome::AttributionState;
use supersigil_review::why::{Tristate, why_line};

#[test]
fn why_compares_aligned_blob_lines_and_their_terminators() {
    let target = split_lines(b"same\nchanged\nlast\r\n");
    let state = AttributionState::Unavailable {
        reason: "no recorded edits".to_owned(),
    };
    for (index, expected) in [(0, Tristate::No), (1, Tristate::Yes), (2, Tristate::Yes)] {
        let line = why_line(&state, index, &target, Ok(b"same\nold\nlast\n"));
        assert_eq!(line.differs_from_head, expected);
        assert_eq!(line.differs_from_head_reason, None);
    }
}

#[test]
fn why_keeps_coarse_comparison_uncertain_only_inside_the_changed_hunk() {
    let head = format!("prefix\n{}suffix\n", "old\n".repeat(COARSE_LINES + 1));
    let target = format!("prefix\n{}suffix\n", "new\n".repeat(COARSE_LINES + 1));
    let target = split_lines(target.as_bytes());
    let state = AttributionState::Unavailable {
        reason: "no recorded edits".to_owned(),
    };
    let middle = why_line(&state, 1, &target, Ok(head.as_bytes()));
    assert_eq!(middle.differs_from_head, Tristate::Unknown);
    assert_eq!(
        middle.differs_from_head_reason.as_deref(),
        Some("coarse diff")
    );
    for index in [0, target.len() - 1] {
        let line = why_line(&state, index, &target, Ok(head.as_bytes()));
        assert_eq!(line.differs_from_head, Tristate::No);
        assert_eq!(line.differs_from_head_reason, None);
    }
}

#[test]
fn why_preserves_the_reason_head_cannot_be_compared() {
    let target = split_lines(b"new\n");
    let state = AttributionState::Unavailable {
        reason: "no recorded edits".to_owned(),
    };
    let line = why_line(&state, 0, &target, Err("HEAD is unborn"));
    assert_eq!(line.differs_from_head, Tristate::Unknown);
    assert_eq!(
        line.differs_from_head_reason.as_deref(),
        Some("HEAD is unborn")
    );
}
