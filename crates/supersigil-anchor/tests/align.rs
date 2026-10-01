//! Aligning lines across one edit from the replacement pairs.

mod common;

use std::time::Duration;

use common::{create, finishes_within, replace, replace_all, state};
use supersigil_anchor::State;
use supersigil_anchor::align::{Alignment, Region, align};
use supersigil_anchor::step::execute_forward;
use supersigil_record::observations::Edit;

/// Executes `edit` on `before` and aligns the result.
fn aligned(edit: &Edit, before: Option<&str>) -> Vec<Alignment> {
    let before = state(before);
    let forward = execute_forward(edit, &before).expect("the edit executes");
    align(&before, &forward.after, &forward.replacements).collect()
}

fn region(before: std::ops::Range<usize>, after: std::ops::Range<usize>) -> Region {
    Region { before, after }
}

#[test]
fn duplicate_lines_around_a_replacement_follow_the_bytes() {
    // Replacing "A\nX\n" with "A\n" in A X X B: the surviving X is the
    // original third line, whatever a diff would pair it with.
    let e = replace("e", "t", 1, "A\nX\n", "A\n");
    let all = aligned(&e, Some("A\nX\nX\nB\n"));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kept, vec![(0, 0), (2, 1), (3, 2)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
}

#[test]
fn a_split_line_is_one_region_and_later_lines_stay_kept() {
    let e = replace("e", "t", 1, "x", "x\n");
    let all = aligned(&e, Some("AxZ\nEND\n"));
    assert_eq!(all[0].kept, vec![(1, 2)]);
    assert_eq!(all[0].regions, vec![region(0..1, 0..2)]);
}

#[test]
fn a_joined_line_is_one_region() {
    let e = replace("e", "t", 1, "x\n", "x");
    let all = aligned(&e, Some("Ax\nZ\nEND\n"));
    assert_eq!(all[0].kept, vec![(2, 1)]);
    assert_eq!(all[0].regions, vec![region(0..2, 0..1)]);
}

#[test]
fn a_deletion_that_joins_two_lines_uses_the_bytes_it_removed() {
    // Forward execution removes the first "foo\n", inside "foofoo\n".
    let e = replace("d", "t", 1, "foo\n", "");
    let all = aligned(&e, Some("foofoo\nfoo\nEND\n"));
    assert_eq!(all[0].kept, vec![(2, 1)]);
    assert_eq!(all[0].regions, vec![region(0..2, 0..1)]);
}

#[test]
fn a_deletion_at_a_line_start_changes_no_target_line() {
    let e = replace("d", "t", 1, "b\n", "");
    let all = aligned(&e, Some("a\nb\nc\n"));
    assert_eq!(all[0].kept, vec![(0, 0), (2, 1)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
}

#[test]
fn whole_line_context_inside_a_replacement_is_kept() {
    let e = replace("e", "t", 1, "a\nb\nc\n", "a\nB\nc\n");
    let all = aligned(&e, Some("a\nb\nc\n"));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kept, vec![(0, 0), (2, 2)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..2)]);
}

#[test]
fn partial_line_context_is_not_kept() {
    // The replacement starts inside line 0, so its first line is not whole.
    let e = replace("e", "t", 1, "b\nc\n", "B\nc\n");
    let all = aligned(&e, Some("ab\nc\n"));
    assert_eq!(all[0].kept, vec![(1, 1)]);
    assert_eq!(all[0].regions, vec![region(0..1, 0..1)]);
}

#[test]
fn overlapping_prefix_and_suffix_context_yield_two_alignments() {
    let e = replace("e", "t", 1, "x\nx\n", "x\n");
    let all = aligned(&e, Some("x\nx\n"));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kept, vec![(0, 0)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
    assert_eq!(all[1].kept, vec![(1, 0)]);
    assert_eq!(all[1].regions, vec![region(0..1, 0..0)]);
}

#[test]
fn a_creation_puts_every_line_in_one_region() {
    let e = create("c", "t", 1, "a\nb\n");
    let forward = execute_forward(&e, &State::Absent).unwrap();
    let all: Vec<Alignment> =
        align(&State::Absent, &forward.after, &forward.replacements).collect();
    assert_eq!(all[0].kept, Vec::<(usize, usize)>::new());
    assert_eq!(all[0].regions, vec![region(0..0, 0..2)]);
}

#[test]
fn context_choices_are_independent_per_replacement_pair() {
    // Two occurrences of "x\nx\n" contracted to "x\n": each pair chooses
    // prefix-first or suffix-first on its own, so the mixed choices are
    // alignments too.
    let e = replace_all("e", "t", 1, "x\nx\n", "x\n");
    let kept: Vec<Vec<(usize, usize)>> = aligned(&e, Some("x\nx\nx\nx\n"))
        .into_iter()
        .map(|a| a.kept)
        .collect();
    assert_eq!(
        kept,
        vec![
            vec![(0, 0), (2, 1)],
            vec![(1, 0), (2, 1)],
            vec![(0, 0), (3, 1)],
            vec![(1, 0), (3, 1)],
        ]
    );
}

#[test]
fn aligning_many_replacements_takes_linear_time() {
    // 100,000 replacements among 200,000 lines: checking every line against
    // every replacement would take minutes.
    let before = "a\nk\n".repeat(100_000);
    let e = replace_all("e", "t", 1, "a\n", "b\n");
    let all = finishes_within(Duration::from_secs(30), move || aligned(&e, Some(&before)));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kept.len(), 100_000);
    assert_eq!(all[0].kept[..2], [(1, 1), (3, 3)]);
    assert_eq!(all[0].regions.len(), 100_000);
    assert_eq!(all[0].regions[1], region(2..3, 2..3));
}
