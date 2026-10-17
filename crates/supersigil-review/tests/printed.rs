//! Printed output: every line escaped once, one note after the commands.

use std::path::PathBuf;

use supersigil_review::printed::{Addressing, Printed, Shell, aligned, escape_control};

/// The note for a `--checkout` value shown as `<as given>` on POSIX shells.
const CHECKOUT_NOTE: &str = "The --checkout value is not valid UTF-8 or holds a control \
                             character, so commands show <as given> for it: pass it as given.";

/// Addressing whose `--checkout` holds an escape sequence.
fn unprintable_checkout() -> Addressing {
    Addressing {
        checkout: Some(PathBuf::from("repo\u{1b}[2K")),
        ..Addressing::default()
    }
}

#[test]
fn escape_control_makes_control_characters_visible() {
    assert_eq!(escape_control("a\u{1b}[2Kb"), r"a\x1b[2Kb");
    assert_eq!(escape_control("bell\u{7}"), r"bell\x07");
    assert_eq!(escape_control("line\nbreak\r"), r"line\x0abreak\x0d");
    assert_eq!(escape_control("nul\u{0}del\u{7f}"), r"nul\x00del\x7f");
    assert_eq!(escape_control("c1\u{9b}31m"), r"c1\x9b31m");
    assert_eq!(escape_control("tab\tstays"), "tab\tstays");
    assert_eq!(escape_control("ünïcode ✔ \u{a0}"), "ünïcode ✔ \u{a0}");
}

#[test]
fn escaping_twice_changes_nothing() {
    let once = escape_control("a\u{1b}[2K\u{9b}\nb");
    assert_eq!(escape_control(&once), once);
}

#[test]
fn every_line_is_escaped_and_the_note_follows_the_commands() {
    let addressing = unprintable_checkout();
    let mut out = Printed::new();
    out.line("  worktree: /work\u{1b}]0;title\u{7}");
    let review = out.command(addressing.review(""));
    out.line(format!("Next: {review}"));
    assert_eq!(
        out.finish(),
        [
            r"  worktree: /work\x1b]0;title\x07",
            "Next: supersigil review --checkout=<as given>",
            CHECKOUT_NOTE,
        ]
    );
}

#[test]
fn a_note_taken_early_is_not_repeated_at_the_end() {
    let addressing = unprintable_checkout();
    let mut out = Printed::new();
    let review = out.command(addressing.review(""));
    out.line(format!("Next: {review}"));
    let note = out.take_note();
    assert_eq!(note.as_deref(), Some(CHECKOUT_NOTE));
    out.lines(note);
    out.line("Long output");
    assert_eq!(
        out.text(),
        format!("Next: supersigil review --checkout=<as given>\n{CHECKOUT_NOTE}\nLong output\n")
    );
}

#[test]
fn output_without_values_shown_as_given_has_no_note() {
    let mut out = Printed::new();
    let review = out.command(Addressing::default().review(""));
    out.line(format!("Next: {review}"));
    assert_eq!(out.take_note(), None);
    assert_eq!(out.finish(), ["Next: supersigil review"]);
}

#[test]
fn columns_align_by_their_escaped_width() {
    let rows = vec![
        ["a\u{7}".to_owned(), "x".to_owned()],
        ["abcde".to_owned(), "y".to_owned()],
    ];
    assert_eq!(aligned(rows), [r"a\x07  x", "abcde  y"]);
}

#[test]
fn the_note_lists_retried_flags_in_the_order_first_given() {
    let addressing = Addressing {
        shell: Shell::Windows,
        retry: vec![
            ("--include-untracked", Some("safe.rs".to_owned())),
            ("--target", Some("HEAD@{1}".to_owned())),
            ("--include-untracked", Some("a b.rs".to_owned())),
        ],
        ..Addressing::default()
    };
    let mut out = Printed::new();
    out.command(addressing.retry());
    let note = out.take_note().unwrap();
    assert!(
        note.starts_with("The --include-untracked and --target values "),
        "{note}"
    );
}
