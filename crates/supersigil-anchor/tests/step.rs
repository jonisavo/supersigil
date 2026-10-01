//! Reversing one edit: candidates per operation, forward validation, and
//! the retained patch as a consistency check.

mod common;

use std::time::Duration;

use common::{
    create, finishes_within, overwrite, replace, replace_all, state, unknown_op, with_hashes,
    with_patch,
};
use supersigil_anchor::result::StopReason;
use supersigil_anchor::step::{Budget, Replacement, Reversed, execute_forward, reverse};
use supersigil_anchor::{DEFAULT_BUDGET_BYTES, State};
use supersigil_record::EventId;
use supersigil_record::observations::{Edit, FileState, Hunk, Material};

fn befores(reversed: &Reversed) -> Vec<String> {
    match reversed {
        Reversed::Candidates(all) => all
            .iter()
            .map(|r| String::from_utf8(r.before.bytes().unwrap_or_default().to_vec()).unwrap())
            .collect(),
        Reversed::Stop(reason) => panic!("stopped: {reason:?}"),
    }
}

fn run(edit: &Edit, current: &str, base: Option<&str>) -> Reversed {
    reverse(
        edit,
        &state(Some(current)),
        &state(base),
        &mut Budget::new(1 << 20),
    )
}

fn id(name: &str) -> EventId {
    EventId::new(name)
}

fn location_choices(reversed: &Reversed) -> Vec<bool> {
    match reversed {
        Reversed::Candidates(all) => all.iter().map(|r| r.location_choice).collect(),
        Reversed::Stop(reason) => panic!("stopped: {reason:?}"),
    }
}

#[test]
fn forward_replaces_the_first_occurrence() {
    let e = replace("e1", "t", 1, "x", "y");
    let forward = execute_forward(&e, &state(Some("x\nx\n"))).unwrap();
    assert_eq!(forward.after, state(Some("y\nx\n")));
    assert_eq!(
        forward.replacements,
        vec![Replacement {
            before: 0..1,
            after: 0..1
        }]
    );
}

#[test]
fn forward_replace_all_replaces_non_overlapping_occurrences_left_to_right() {
    let e = replace_all("e1", "t", 1, "a", "aa");
    let forward = execute_forward(&e, &state(Some("aba"))).unwrap();
    assert_eq!(forward.after, state(Some("aabaa")));
    assert_eq!(
        forward.replacements,
        vec![
            Replacement {
                before: 0..1,
                after: 0..2
            },
            Replacement {
                before: 2..3,
                after: 3..5
            },
        ]
    );
}

#[test]
fn forward_write_yields_its_content_and_spans_both_files() {
    let e = overwrite("w", "t", 1, "new\n");
    let forward = execute_forward(&e, &state(Some("old!\n"))).unwrap();
    assert_eq!(forward.after, state(Some("new\n")));
    assert_eq!(
        forward.replacements,
        vec![Replacement {
            before: 0..5,
            after: 0..4
        }]
    );
    assert!(execute_forward(&create("c", "t", 1, "a\n"), &State::Absent).is_some());
}

#[test]
fn forward_does_not_execute_unknown_operations_or_missing_text() {
    assert!(execute_forward(&unknown_op("u", "t", 1, "x", "y"), &state(Some("x"))).is_none());
    let mut e = replace("e", "t", 1, "x", "y");
    e.old_text = Material::unavailable("withheld");
    assert!(execute_forward(&e, &state(Some("x"))).is_none());
    assert!(execute_forward(&replace("e", "t", 1, "z", "y"), &state(Some("x"))).is_none());
}

#[test]
fn a_unique_occurrence_reverses_to_one_candidate() {
    let e = replace("e1", "t", 1, "hello {name}", "Hello, {name}!");
    let out = run(&e, "fn a() { Hello, {name}! }\n", None);
    assert_eq!(befores(&out), vec!["fn a() { hello {name} }\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(!all[0].location_choice);
}

#[test]
fn a_later_occurrence_the_edit_could_not_have_produced_is_rejected() {
    // Base "x\nx\n", an ordinary edit x -> y, target "x\ny\n": reversing the
    // only "y" gives the base, but the edit replaces the first "x".
    let e = replace("e1", "t", 1, "x", "y");
    assert_eq!(
        run(&e, "x\ny\n", Some("x\nx\n")),
        Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id("e1") })
    );
}

#[test]
fn several_accepted_positions_are_a_location_choice() {
    let e = replace("e1", "t", 1, "x\n", "a\n");
    let out = run(&e, "a\na\n", None);
    assert_eq!(befores(&out), vec!["x\na\n", "a\nx\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(all.iter().all(|r| r.location_choice));
}

#[test]
fn a_known_before_hash_picks_among_positions() {
    let e = with_hashes(replace("e1", "t", 1, "x\n", "a\n"), "a\nx\n", "a\na\n");
    let out = run(&e, "a\na\n", None);
    assert_eq!(befores(&out), vec!["a\nx\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(!all[0].location_choice);
}

#[test]
fn a_contradicted_after_hash_stops_the_step() {
    let e = with_hashes(replace("e1", "t", 1, "x", "y"), "x\n", "y\n");
    assert_eq!(
        run(&e, "z\n", None),
        Reversed::Stop(StopReason::AfterHashMismatch { edit: id("e1") })
    );
}

#[test]
fn tab_indented_hunks_are_never_bytes() {
    // Claude Code's structuredPatch shows every tab as two spaces; the hunk
    // below is in that display form. Reversal takes bytes from the texts
    // only; the hunk is only checked against them.
    let before = "fn f() {\n\tif x {\n\t\treturn false;\n\t}\n}\n";
    let after = "fn f() {\n\tif x {\n\t\treturn true;\n\t}\n}\n";
    let mut e = replace("e1", "t", 1, "\t\treturn false;\n", "\t\treturn true;\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 2,
        old_lines: 3,
        new_start: 2,
        new_lines: 3,
        lines: vec![
            "   if x {".to_owned(),
            "-    return false;".to_owned(),
            "+    return true;".to_owned(),
            "   }".to_owned(),
        ],
    }]);
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn deletion_displayed_late_is_recovered() {
    // The deleted block begins with a line identical to the one before it,
    // so jsdiff may mark the removal a line late, and the texts alone allow
    // the block before or after that line (and at every other line start).
    // Whichever lines the patch marks, its old side is the base's lines.
    let base = "}\n\n// ---\n// ---\n// Exit\n// ---\n\nenum E;\n\n// Symbols\n";
    let block = "// ---\n// Exit\n// ---\n\nenum E;\n\n";
    let target = "}\n\n// ---\n// Symbols\n";
    let unpatched = replace("d", "t", 1, block, "");
    assert_eq!(befores(&run(&unpatched, target, None)).len(), 5);
    let unknown = with_patch(unpatched, base, target);
    assert_eq!(befores(&run(&unknown, target, None)), vec![base]);
    // With both hashes recorded, the hashes decide and the patch is not
    // consulted.
    let known = with_patch(
        with_hashes(replace("d", "t", 1, block, ""), base, target),
        base,
        target,
    );
    assert_eq!(befores(&run(&known, target, None)), vec![base]);
}

#[test]
fn a_shifted_deletion_display_keeps_only_the_candidate_the_patch_describes() {
    // Ten "x" lines and UNIQUE are deleted, and ten more "x" lines follow.
    // jsdiff keeps the first ten "x" lines as common and shows UNIQUE and
    // the ten after it as deleted: the display is shifted, but its old side
    // is still the before-file's lines at old_start.
    let x10 = "x\n".repeat(10);
    let block = format!("{x10}UNIQUE\n");
    let before = format!("head\n{block}{x10}tail\n");
    let after = format!("head\n{x10}tail\n");
    let unpatched = replace("d", "t", 1, &block, "");
    assert_eq!(befores(&run(&unpatched, &after, None)).len(), 13);
    let e = with_patch(unpatched, &before, &after);
    let hunk = &e.patch.retained().unwrap()[0];
    assert_eq!((hunk.old_start, hunk.lines[3].as_str()), (9, "-UNIQUE"));
    let out = run(&e, &after, None);
    assert_eq!(befores(&out), vec![before]);
    assert_eq!(location_choices(&out), vec![false]);
}

#[test]
fn a_hashless_deletion_in_a_large_file_has_one_candidate_within_the_budget() {
    // Without the patch every line start of 3,000 lines is a candidate, each
    // costing the whole file, and the default budget runs out.
    let lines: Vec<String> = (0..3_000).map(|i| format!("line {i:04}\n")).collect();
    let before = lines.concat();
    let after = [&lines[..1_500], &lines[1_501..]].concat().concat();
    let e = with_patch(replace("d", "t", 1, &lines[1_500], ""), &before, &after);
    let mut budget = Budget::new(DEFAULT_BUDGET_BYTES);
    let out = reverse(&e, &state(Some(&after)), &State::Absent, &mut budget);
    assert!(!budget.exhausted());
    assert_eq!(befores(&out), vec![before]);
}

#[test]
fn a_candidate_contradicting_the_patch_is_rejected() {
    let e = replace("e1", "t", 1, "x\n", "a\n");
    // Without a retained patch, every candidate is kept.
    assert_eq!(
        befores(&run(&e, "a\nb\na\n", None)),
        vec!["x\nb\na\n", "a\nb\nx\n"]
    );
    let patched = with_patch(e, "a\nb\nx\n", "a\nb\na\n");
    let out = run(&patched, "a\nb\na\n", None);
    assert_eq!(befores(&out), vec!["a\nb\nx\n"]);
    assert_eq!(location_choices(&out), vec![false]);
}

#[test]
fn current_bytes_contradicting_the_patch_stop_the_step() {
    let mut e = with_patch(
        replace("e1", "t", 1, "x\n", "a\n"),
        "a\nb\nx\n",
        "a\nb\na\n",
    );
    assert_eq!(
        run(&e, "a\nc\na\n", None),
        Reversed::Stop(StopReason::AfterPatchMismatch { edit: id("e1") })
    );
    e.patch = Material::unavailable("test");
    assert_eq!(befores(&run(&e, "a\nc\na\n", None)).len(), 2);
}

#[test]
fn a_tab_indented_patch_in_display_form_picks_the_true_candidate() {
    // Both "2;\n" positions reverse; the patch, which shows each tab as two
    // spaces, describes only the second.
    let before = "fn f() {\n\tlet a = 1;\n\tlet b = 2;\n}\n";
    let after = "fn f() {\n\tlet a = 1;\n\tlet b = 1;\n}\n";
    let unpatched = replace("e1", "t", 1, "2;\n", "1;\n");
    assert_eq!(befores(&run(&unpatched, after, None)).len(), 2);
    let e = with_patch(unpatched, before, after);
    assert_eq!(e.patch.retained().unwrap()[0].lines[1], "   let a = 1;");
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn patch_markers_and_carriage_returns_are_not_compared() {
    let before = "a\r\nb";
    let after = "a\r\nc";
    let e = with_patch(replace("e1", "t", 1, "b", "c"), before, after);
    let lines = &e.patch.retained().unwrap()[0].lines;
    assert!(lines.contains(&"\\ No newline at end of file".to_owned()));
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn deletion_candidates_are_every_line_start_and_the_end() {
    let e = replace("d", "t", 1, "c\n", "");
    assert_eq!(
        befores(&run(&e, "a\nb\n", None)),
        vec!["c\na\nb\n", "a\nc\nb\n", "a\nb\nc\n"]
    );
}

#[test]
fn a_deletion_without_a_line_terminator_cannot_be_located() {
    let e = replace("d", "t", 1, "abc", "");
    assert_eq!(
        run(&e, "x\n", None),
        Reversed::Stop(StopReason::NotLocatable { edit: id("d") })
    );
}

#[test]
fn replace_all_reverses_jointly_with_a_known_before_hash() {
    let e = with_hashes(replace_all("e1", "t", 1, "a", "b"), "a a\n", "b b\n");
    assert_eq!(befores(&run(&e, "b b\n", None)), vec!["a a\n"]);
}

#[test]
fn replace_all_without_a_before_hash_is_unverified() {
    let e = replace_all("e1", "t", 1, "a", "b");
    assert_eq!(
        run(&e, "b b\n", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e1") })
    );
}

#[test]
fn a_joint_inverse_that_fails_its_checks_is_unverified() {
    // "ax" with replace_all(x -> aa) gives "aaa"; the joint inverse of the
    // first "aa" gives "xa", which fails the before-hash.
    let spanning = with_hashes(replace_all("e1", "t", 1, "x", "aa"), "ax", "aaa");
    assert_eq!(
        run(&spanning, "aaa", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e1") })
    );
    // replace_all(a -> aa) never yields "aaa": the inverse "aa" fails
    // forward execution even with a matching before-hash.
    let growing = with_hashes(replace_all("e2", "t", 1, "a", "aa"), "aa", "aaa");
    assert_eq!(
        run(&growing, "aaa", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e2") })
    );
}

#[test]
fn a_creation_reverses_to_absent_only_when_its_content_is_the_target() {
    let e = create("c", "t", 1, "a\n");
    let Reversed::Candidates(all) = run(&e, "a\n", None) else {
        panic!("creation must reverse")
    };
    assert_eq!(all[0].before, State::Absent);
    assert_eq!(
        run(&e, "b\n", None),
        Reversed::Stop(StopReason::AfterHashMismatch { edit: id("c") })
    );
}

#[test]
fn an_overwrite_reverses_to_the_base_only_when_its_before_hash_is_the_base() {
    let mut e = overwrite("w", "t", 1, "new\n");
    assert_eq!(
        run(&e, "new\n", Some("old\n")),
        Reversed::Stop(StopReason::WholeFileWrite { edit: id("w") })
    );
    e = with_hashes(e, "old\n", "new\n");
    assert_eq!(befores(&run(&e, "new\n", Some("old\n"))), vec!["old\n"]);
    assert_eq!(
        run(&e, "new\n", Some("other\n")),
        Reversed::Stop(StopReason::WholeFileWrite { edit: id("w") })
    );
}

#[test]
fn an_overwrite_whose_output_is_not_the_current_bytes_is_rejected() {
    let mut e = overwrite("w", "t", 1, "c\n");
    e.after = FileState::unknown();
    assert_eq!(
        run(&e, "d\n", Some("old\n")),
        Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id("w") })
    );
}

#[test]
fn unknown_operations_and_missing_text_stop() {
    assert_eq!(
        run(&unknown_op("u", "t", 1, "x", "y"), "y", None),
        Reversed::Stop(StopReason::OperationUnknown { edit: id("u") })
    );
    let mut e = replace("e", "t", 1, "x", "y");
    e.new_text = Material::unavailable("withheld");
    assert_eq!(
        run(&e, "y", None),
        Reversed::Stop(StopReason::TextUnavailable { edit: id("e") })
    );
}

#[test]
fn an_exhausted_budget_returns_what_was_accepted() {
    let e = replace("d", "t", 1, "c\n", "");
    let mut budget = Budget::new(1);
    let out = reverse(&e, &state(Some("a\nb\n")), &State::Absent, &mut budget);
    assert_eq!(out, Reversed::Candidates(Vec::new()));
    assert!(budget.exhausted());
}

#[test]
fn a_known_hash_decides_without_the_patch() {
    // The patch shows the edit made from "x\na\n", the before-hash records
    // "a\nx\n". Without the hash the patch picks; with it, the hash decides
    // alone.
    let e = with_patch(replace("e1", "t", 1, "x\n", "a\n"), "x\na\n", "a\na\n");
    assert_eq!(befores(&run(&e, "a\na\n", None)), vec!["x\na\n"]);
    let mut hashed = with_hashes(e, "a\nx\n", "a\na\n");
    hashed.after = FileState::unknown();
    assert_eq!(befores(&run(&hashed, "a\na\n", None)), vec!["a\nx\n"]);

    // A hunk whose new side contradicts the current bytes stops the step
    // only while the after-hash is unknown. Its old side still picks the
    // candidate, since the before-hash is unknown.
    let mut e = replace("e2", "t", 1, "x\n", "a\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 3,
        new_start: 1,
        new_lines: 3,
        lines: vec![
            " a".to_owned(),
            " b".to_owned(),
            "-x".to_owned(),
            "+q".to_owned(),
        ],
    }]);
    assert_eq!(
        run(&e, "a\nb\na\n", None),
        Reversed::Stop(StopReason::AfterPatchMismatch { edit: id("e2") })
    );
    let mut hashed = with_hashes(e, "a\nb\nx\n", "a\nb\na\n");
    hashed.before = FileState::unknown();
    assert_eq!(befores(&run(&hashed, "a\nb\na\n", None)), vec!["a\nb\nx\n"]);

    // With both hashes known, a hunk in another display (tabs as four
    // spaces) neither stops the step nor rejects the true candidate.
    let mut e = with_hashes(replace("e3", "t", 1, "x\n", "y\n"), "\tx\n", "\ty\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        lines: vec!["-    x".to_owned(), "+    y".to_owned()],
    }]);
    assert_eq!(befores(&run(&e, "\ty\n", None)), vec!["\tx\n"]);
}

#[test]
fn a_hunk_holding_a_literal_tab_is_ignored() {
    // Claude Code shows every tab as two spaces, so a hunk holding a tab is
    // not in that display. The edit reverses as if no patch were retained:
    // no stop, no rejected candidate, and no reordering by its line numbers.
    let unpatched = replace("e1", "t", 1, "x\n", "y\n");
    let after = "\ty\n\ty\n";
    let mut e = unpatched.clone();
    e.patch = Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 2,
        new_start: 1,
        new_lines: 2,
        lines: vec![" \ty".to_owned(), "-\tx".to_owned(), "+\ty".to_owned()],
    }]);
    assert_eq!(run(&e, after, None), run(&unpatched, after, None));
    assert_eq!(
        befores(&run(&e, after, None)),
        vec!["\tx\n\ty\n", "\ty\n\tx\n"]
    );
}

#[test]
fn searching_repetitive_text_takes_linear_time() {
    // At every position of 4 MiB of "a", a 1 MiB run of "a" matches until
    // its final "b": comparing the text afresh at each position would take
    // minutes, for reversal and forward execution alike.
    let run_of_a = "a".repeat(1 << 20);
    let text = format!("{run_of_a}b");
    let current = "a".repeat(1 << 22);
    let e = replace("e1", "t", 1, "x", &text);
    let out = finishes_within(Duration::from_secs(30), move || {
        let mut budget = Budget::new(DEFAULT_BUDGET_BYTES);
        reverse(&e, &state(Some(&current)), &State::Absent, &mut budget)
    });
    assert_eq!(
        out,
        Reversed::Stop(StopReason::NotLocatable { edit: id("e1") })
    );
    let before = format!("{}{text}", "a".repeat(1 << 22));
    let e = replace("e2", "t", 1, &text, "x");
    let forward = finishes_within(Duration::from_secs(30), move || {
        execute_forward(&e, &state(Some(&before)))
    });
    let expected = format!("{}x", "a".repeat(1 << 22));
    assert_eq!(forward.map(|f| f.after), Some(state(Some(&expected))));
}

#[test]
fn reading_the_current_bytes_is_charged_before_any_candidate() {
    // A reversal that proposes no candidate still read the current bytes,
    // and the walk tries one per branch, so the reading itself is charged.
    let e = replace("e1", "t", 1, "x", "q");
    let current = state(Some("abc\n"));
    let mut budget = Budget::new(3);
    assert_eq!(
        reverse(&e, &current, &State::Absent, &mut budget),
        Reversed::Candidates(Vec::new())
    );
    assert!(budget.exhausted());
    // Reading 4 bytes, and the search for "q" prepares a 1-byte table.
    let mut budget = Budget::new(4 + 1);
    assert_eq!(
        reverse(&e, &current, &State::Absent, &mut budget),
        Reversed::Stop(StopReason::NotLocatable { edit: id("e1") })
    );
    assert!(!budget.exhausted());
    // Each candidate is charged after the reading: here 4 bytes, then 10
    // per candidate (the 6-byte candidate and the 4 bytes it executes
    // forward into). Those accepted before the budget ran out are kept.
    let d = replace("d", "t", 1, "c\n", "");
    let mut budget = Budget::new(4 + 10);
    let out = reverse(&d, &state(Some("a\nb\n")), &State::Absent, &mut budget);
    assert_eq!(befores(&out), vec!["c\na\nb\n"]);
    assert!(budget.exhausted());
}

#[test]
fn a_growing_joint_inverse_is_charged_before_it_is_built() {
    // Replacing 5,000 "a" with one "b" in a file that already holds 5,000
    // "b" leaves 5,001 bytes, whose joint inverse holds 25,005,000. It is
    // charged in full before it is built, so a 1 MiB budget runs out; with
    // room, it is built and the before-hash rejects it.
    let run_of_a = "a".repeat(5_000);
    let before = format!("{run_of_a}{}", "b".repeat(5_000));
    let after = "b".repeat(5_001);
    let e = with_hashes(replace_all("e1", "t", 1, &run_of_a, "b"), &before, &after);
    let mut budget = Budget::new(1 << 20);
    assert_eq!(
        reverse(&e, &state(Some(&after)), &State::Absent, &mut budget),
        Reversed::Candidates(Vec::new())
    );
    assert!(budget.exhausted());
    let mut budget = Budget::new(DEFAULT_BUDGET_BYTES);
    assert_eq!(
        reverse(&e, &state(Some(&after)), &State::Absent, &mut budget),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e1") })
    );
    assert!(!budget.exhausted());
}

#[test]
fn preparing_a_search_is_charged_unless_no_match_fits() {
    // Searching for a text first prepares a table as long as the text,
    // charged after the reading: 1,000 bytes cover the reading, not the
    // table for a 601-byte text.
    let current = "a".repeat(1_000);
    let text = format!("{}b", "a".repeat(600));
    let e = replace("e1", "t", 1, "x", &text);
    let mut budget = Budget::new(1_000 + 600);
    assert_eq!(
        reverse(&e, &state(Some(&current)), &State::Absent, &mut budget),
        Reversed::Candidates(Vec::new())
    );
    assert!(budget.exhausted());
    // A text longer than the current bytes cannot occur in them: nothing is
    // prepared or charged, however far it exceeds the budget.
    let huge = "b".repeat(10 << 20);
    let e = replace("e2", "t", 1, "x", &huge);
    let mut budget = Budget::new(1_000);
    assert_eq!(
        reverse(&e, &state(Some(&current)), &State::Absent, &mut budget),
        Reversed::Stop(StopReason::NotLocatable { edit: id("e2") })
    );
    assert!(!budget.exhausted());
    let e = with_hashes(replace_all("e3", "t", 1, "x", &huge), "x", &current);
    let mut budget = Budget::new(1_000);
    assert_eq!(
        reverse(&e, &state(Some(&current)), &State::Absent, &mut budget),
        Reversed::Stop(StopReason::NotLocatable { edit: id("e3") })
    );
    assert!(!budget.exhausted());
}

#[test]
fn a_retained_patch_is_charged_before_it_is_read() {
    // The hunk holds 10 MiB of text; the budget covers reading the current
    // bytes but not the hunk.
    let mut e = replace("e1", "t", 1, "x\n", "a\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 2,
        new_start: 1,
        new_lines: 2,
        lines: vec![" ".repeat(10 << 20), "-x".to_owned(), "+a".to_owned()],
    }]);
    let mut budget = Budget::new(1 << 10);
    assert_eq!(
        reverse(&e, &state(Some("a\n")), &State::Absent, &mut budget),
        Reversed::Candidates(Vec::new())
    );
    assert!(budget.exhausted());
}

#[test]
fn candidate_positions_are_generated_under_the_budget() {
    // 8 Mi lines of "a": every line is a position for "a\n", and the patch
    // puts the edit at line 4 Mi. The budget covers reading the bytes and a
    // few cheap checks, nearest the patch's line first, then runs out:
    // finding, collecting, and ordering every position first would take
    // far longer.
    let lines = 1 << 23;
    let current = "a\n".repeat(lines);
    let mut e = replace("e1", "t", 1, "x\n", "a\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 1 << 22,
        old_lines: 1,
        new_start: 1 << 22,
        new_lines: 1,
        lines: vec!["-x".to_owned(), "+a".to_owned()],
    }]);
    let (out, exhausted) = finishes_within(Duration::from_secs(2), move || {
        let mut budget = Budget::new(2 * lines as u64 + 1024);
        let out = reverse(&e, &state(Some(&current)), &State::Absent, &mut budget);
        (out, budget.exhausted())
    });
    assert_eq!(out, Reversed::Candidates(Vec::new()));
    assert!(exhausted);
}

#[test]
fn many_accepted_positions_are_told_apart_without_comparing_states() {
    // Putting the "x" back at any of 12,000 "a" bytes gives a distinct
    // before-state that executes forward to the current bytes, so every
    // position is accepted. Comparing each with every accepted state would
    // take far longer than validating them.
    let current = "a".repeat(12_000);
    let e = replace("e1", "t", 1, "x", "a");
    let out = finishes_within(Duration::from_secs(5), move || {
        let mut budget = Budget::new(u64::MAX);
        reverse(&e, &state(Some(&current)), &State::Absent, &mut budget)
    });
    let Reversed::Candidates(all) = out else {
        panic!("every position is accepted")
    };
    assert_eq!(all.len(), 12_000);
    assert!(all.iter().all(|r| r.location_choice));
}

#[test]
fn positions_giving_one_before_state_are_one_candidate() {
    // "x" -> "xx" leaves "xxx" from "xx": putting "x" back at either
    // occurrence of "xx" gives "xx", whose first "x" forward execution
    // replaces both times.
    let out = run(&replace("e1", "t", 1, "x", "xx"), "xxx", None);
    assert_eq!(befores(&out), vec!["xx"]);
    assert_eq!(location_choices(&out), vec![false]);
    // Every line start of "a\na\n" gives "a\na\na\n" back.
    let out = run(&replace("d", "t", 1, "a\n", ""), "a\na\n", None);
    assert_eq!(befores(&out), vec!["a\na\na\n"]);
    assert_eq!(location_choices(&out), vec![false]);
}
