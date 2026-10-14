//! `supersigil judge` end to end: judgments recorded on saved reviews of
//! temporary git repositories, and every refusal recording nothing.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use common::fixture::{Fixture, basis_id, gate_row, review_store, slice, slice_text, write};
use serde_json::{Value, json};
use supersigil_record::reviews::{self, ItemId, Judgment, JudgmentKind, Who};
use supersigil_review::summary::shell_word;

/// The name of the person judging in every fixture.
const NAME: &str = "Ada Lovelace";
/// Their email.
const EMAIL: &str = "ada@example.com";

/// The refusal inside an agent session.
const AGENT: &str = "No judgment recorded. Judgments must be made by a person; this command is \
                     running inside a Claude Code session. The agent can list the open items \
                     (supersigil judge show) and prepare the exact command for you to inspect \
                     and run outside the agent.\n";

/// The refusal without a selected review.
const NO_SELECTION: &str = "No judgment recorded. No review selected in this worktree. Run \
                            supersigil review first, or pass --review <id> (supersigil judge \
                            list shows reviews).\n";

/// The refusal without a git identity, in `git commit`'s words.
const NO_IDENTITY: &str = "No judgment recorded. Author identity unknown

*** Please tell me who you are.

Run

  git config --global user.email \"you@example.com\"
  git config --global user.name \"Your Name\"

to set your account's default identity.
Omit --global to set the identity only in this repository.
";

/// Gives `f`'s repository its own git identity, which judging needs.
fn identify(f: &Fixture) {
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", EMAIL]);
}

/// The slice fixture with an identity, reviewed. Its gate: `old.txt`,
/// `lines:src/lib.rs`, `src/new.rs` (untracked), and `gap:src/lib.rs`, all
/// open.
fn reviewed() -> (Fixture, Value) {
    let f = Fixture::new();
    slice(&f);
    identify(&f);
    let review = f.json(&f.repo, &["review", "--format", "json"]);
    (f, review)
}

/// `supersigil judge <args>` in `dir`.
fn judge(f: &Fixture, dir: &Path, args: &[&str]) -> Command {
    f.plain(dir, &[&["judge"], args].concat())
}

/// What `supersigil judge --format terminal <args>` prints in `dir`; panics
/// unless it succeeds.
fn judged(f: &Fixture, dir: &Path, args: &[&str]) -> String {
    let output = judge(f, dir, &[&["--format", "terminal"], args].concat())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Runs `command`, a judge call expected to be refused: it exits 1, prints
/// nothing on standard output, and records no judgment and no ref. Returns
/// what it printed on standard error.
fn refused(f: &Fixture, command: &mut Command) -> String {
    let before = (recorded(f), refs(f));
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty());
    assert_eq!((recorded(f), refs(f)), before);
    String::from_utf8(output.stderr).unwrap()
}

/// Every judgment in `f`'s review store, in the order recorded; none
/// without a store.
fn recorded(f: &Fixture) -> Vec<Judgment> {
    reviews::find(&f.reviews(), &f.repo.join(".git"))
        .unwrap()
        .map_or_else(Vec::new, |store| {
            store.snapshot().unwrap().judgments().unwrap()
        })
}

/// The refs under `refs/supersigil/`.
fn refs(f: &Fixture) -> String {
    f.git(&["for-each-ref", "refs/supersigil/"])
}

/// The id of the item named `name` in `review`'s gate.
fn item(review: &Value, name: &str) -> ItemId {
    serde_json::from_value(gate_row(review, name)["id"].clone()).unwrap()
}

/// The short commit `HEAD` names.
fn head(f: &Fixture) -> String {
    f.git(&["rev-parse", "--short=7", "HEAD"]).trim().to_owned()
}

/// The next lines of a review whose open items are `names`, as typed.
fn next(names: &str) -> String {
    format!(
        "Next: supersigil judge acknowledge {names}\n  \
         or: supersigil judge dismiss <name>... --reason <text>\n  \
         then: supersigil judge complete\n"
    )
}

#[test]
fn acknowledging_records_the_items_and_says_what_stays_open() {
    let (f, review) = reviewed();
    let id = basis_id(&review);
    let id8 = &id[..8];

    let text = judged(&f, &f.repo, &["acknowledge", "old.txt"]);

    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(judgment.kind, JudgmentKind::Acknowledge);
    assert_eq!(judgment.basis.as_str(), id);
    assert_eq!(judgment.items, [item(&review, "old.txt")]);
    assert_eq!(judgment.reason, None);
    assert_eq!(
        judgment.who,
        Who {
            name: NAME.to_owned(),
            email: EMAIL.to_owned()
        }
    );
    assert_eq!(
        text,
        format!(
            "Acknowledged old.txt on review {id8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{id8}…\n\
             Judgment {}. Gate: 3 open: lines:src/lib.rs, src/new.rs, gap:src/lib.rs.\n{}",
            head(&f),
            judgment.id.short(),
            next("lines:src/lib.rs src/new.rs gap:src/lib.rs")
        )
    );
}

#[test]
fn ack_records_several_items_with_one_reason() {
    let (f, review) = reviewed();

    let text = judged(
        &f,
        &f.repo,
        &[
            "ack",
            "old.txt",
            "lines:src/lib.rs",
            "gap:src/lib.rs",
            "--reason",
            "read with the transcript",
        ],
    );

    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(judgment.kind, JudgmentKind::Acknowledge);
    assert_eq!(
        judgment.items,
        [
            item(&review, "old.txt"),
            item(&review, "lines:src/lib.rs"),
            item(&review, "gap:src/lib.rs"),
        ]
    );
    assert_eq!(judgment.reason.as_deref(), Some("read with the transcript"));
    assert!(
        text.starts_with(&format!(
            "Acknowledged old.txt, lines:src/lib.rs, and gap:src/lib.rs on review {} ",
            &basis_id(&review)[..8]
        )),
        "{text}"
    );
    assert!(text.contains(". Gate: 1 open: src/new.rs.\n"), "{text}");
}

#[test]
fn names_are_typed_and_shown_from_a_subdirectory() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];
    let src = f.repo.join("src");

    let text = judged(&f, &src, &["acknowledge", "new.rs", "lines:lib.rs"]);

    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(
        judgment.items,
        [
            item(&review, "src/new.rs"),
            item(&review, "lines:src/lib.rs")
        ]
    );
    // A path outside the directory is shown in full.
    let old = f.repo.join("old.txt").display().to_string();
    assert_eq!(
        text,
        format!(
            "Acknowledged new.rs and lines:lib.rs on review {id8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{id8}…\n\
             Judgment {}. Gate: 2 open: {old}, gap:lib.rs.\n{}",
            head(&f),
            judgment.id.short(),
            next(&format!("{} gap:lib.rs", shell_word(&old)))
        )
    );
}

#[test]
fn dismissing_an_untracked_file_says_how_to_review_its_content() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];

    let text = judged(
        &f,
        &f.repo,
        &[
            "dismiss",
            "src/new.rs",
            "--reason",
            "generated; regenerated by the build",
        ],
    );

    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(judgment.kind, JudgmentKind::Dismiss);
    assert_eq!(judgment.items, [item(&review, "src/new.rs")]);
    assert_eq!(
        judgment.reason.as_deref(),
        Some("generated; regenerated by the build")
    );
    assert_eq!(
        text,
        format!(
            "Dismissed src/new.rs on review {id8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{id8}…\n\
             The file stays outside the review; to review its content, run supersigil review \
             --include-untracked=src/new.rs.\n\
             Judgment {}. Gate: 3 open: old.txt, lines:src/lib.rs, gap:src/lib.rs.\n{}",
            head(&f),
            judgment.id.short(),
            next("old.txt lines:src/lib.rs gap:src/lib.rs")
        )
    );
}

#[test]
fn the_review_command_for_an_untracked_file_takes_a_leading_dash_as_its_path() {
    let f = Fixture::new();
    slice(&f);
    identify(&f);
    // The session wrote `-notes` where the slice fixture writes
    // `src/new.rs`.
    std::fs::rename(f.repo.join("src/new.rs"), f.repo.join("-notes")).unwrap();
    let text = slice_text().replace("/work/repo/src/new.rs", "/work/repo/-notes");
    f.transcript(&f.repo, "slice.jsonl", &text);
    let review = f.json(&f.repo, &["review", "--format", "json"]);
    assert_eq!(gate_row(&review, "-notes")["origin"], "untracked_recorded");

    let text = judged(
        &f,
        &f.repo,
        &["dismiss", "--reason", "scratch notes", "--", "-notes"],
    );

    let printed = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("The file stays outside the review; to review its content, run ")?
                .strip_suffix('.')
        })
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(printed, "supersigil review --include-untracked=-notes");
    let words: Vec<&str> = printed.split(' ').skip(1).collect();
    let output = f
        .plain(&f.repo, &[words.as_slice(), &["--format", "json"]].concat())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let included: Value = serde_json::from_slice(&output.stdout).unwrap();
    let files: Vec<&Value> = included["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| &file["path"])
        .collect();
    assert!(files.contains(&&json!("-notes")), "{files:?}");
}

#[test]
fn a_comment_on_the_review_or_on_an_item_closes_nothing() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];

    let on_review = judged(&f, &f.repo, &["comment", "--text", "Careful work."]);
    let on_item = judged(
        &f,
        &f.repo,
        &["comment", "src/new.rs", "--text", "Ask why."],
    );

    let [whole, of_item] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();
    assert_eq!(whole.kind, JudgmentKind::Comment);
    assert!(whole.items.is_empty());
    assert_eq!(whole.text.as_deref(), Some("Careful work."));
    assert_eq!(of_item.kind, JudgmentKind::Comment);
    assert_eq!(of_item.items, [item(&review, "src/new.rs")]);
    assert_eq!(of_item.text.as_deref(), Some("Ask why."));
    let open = "Gate: 4 open: old.txt, lines:src/lib.rs, src/new.rs, gap:src/lib.rs.";
    assert_eq!(
        on_review,
        format!(
            "Commented on review {id8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{id8}…\n\
             Judgment {}. {open}\n{}",
            head(&f),
            whole.id.short(),
            next("old.txt lines:src/lib.rs src/new.rs gap:src/lib.rs")
        )
    );
    assert!(
        on_item.starts_with(&format!(
            "Commented on src/new.rs on review {id8} (working tree over {}).\nJudgment {}. \
             {open}\n",
            head(&f),
            of_item.id.short()
        )),
        "{on_item}"
    );
}

#[test]
fn completing_counts_the_judgment_that_closes_each_item() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];
    judged(&f, &f.repo, &["acknowledge", "old.txt", "lines:src/lib.rs"]);

    let closed = judged(
        &f,
        &f.repo,
        &[
            "dismiss",
            "src/new.rs",
            "gap:src/lib.rs",
            "--reason",
            "generated",
        ],
    );
    let complete = judged(&f, &f.repo, &["complete", "--text", "Read it all."]);

    let [acknowledged, dismissed, completion] = <[Judgment; 3]>::try_from(recorded(&f)).unwrap();
    assert!(
        closed.starts_with(&format!(
            "Dismissed src/new.rs and gap:src/lib.rs on review {id8} "
        )),
        "{closed}"
    );
    assert!(
        closed.ends_with(&format!(
            "\nJudgment {}. Gate: 0 open. Review not yet complete.\nNext: supersigil judge \
             complete\n",
            dismissed.id.short()
        )),
        "{closed}"
    );
    assert_eq!(completion.kind, JudgmentKind::Complete);
    assert_eq!(completion.text.as_deref(), Some("Read it all."));
    assert_eq!(
        completion.closes,
        BTreeMap::from([
            (item(&review, "old.txt"), acknowledged.id.clone()),
            (item(&review, "lines:src/lib.rs"), acknowledged.id.clone()),
            (item(&review, "src/new.rs"), dismissed.id.clone()),
            (item(&review, "gap:src/lib.rs"), dismissed.id.clone()),
        ])
    );
    assert_eq!(
        complete,
        format!(
            "Completed review {id8} (working tree over {}).\n\
             Judgment {}. Gate: 0 open. Review complete.\n",
            head(&f),
            completion.id.short()
        )
    );
}

#[test]
fn json_output_holds_the_judgment_the_review_and_its_gate() {
    let (f, review) = reviewed();

    // Piped, so `auto` prints JSON.
    let output = judge(&f, &f.repo, &["acknowledge", "old.txt"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(
        printed["judgment"],
        serde_json::to_value(&judgment).unwrap()
    );
    assert_eq!(printed["review"], basis_id(&review));
    assert_eq!(printed["target"], review["basis"]["document"]["target"]);
    assert_eq!(printed["gate"]["open"], 3);
    assert_eq!(gate_row(&printed, "old.txt")["state"], "acknowledged");
    assert_eq!(
        gate_row(&printed, "old.txt")["judgment"]["id"],
        judgment.id.as_str()
    );
    assert_eq!(
        printed["status"],
        json!({"complete": false, "completion": null})
    );
}

#[test]
fn the_first_judgment_keeps_the_reviewed_snapshot_under_a_ref() {
    let (f, review) = reviewed();
    let id = basis_id(&review);
    let reference = format!("refs/supersigil/reviews/{id}");
    let base = f.git(&["rev-parse", "HEAD"]).trim().to_owned();

    judged(&f, &f.repo, &["acknowledge", "old.txt"]);

    let kept = f.git(&["rev-parse", &reference]).trim().to_owned();
    assert_eq!(
        f.git(&["rev-parse", &format!("{reference}^{{tree}}")])
            .trim(),
        review["target"]["tree"]
    );
    assert_eq!(
        f.git(&["rev-list", "--parents", "-n", "1", &reference])
            .trim(),
        format!("{kept} {base}")
    );
    assert_eq!(
        f.git(&["log", "-1", "--format=%an <%ae>|%cn <%ce>|%s", &reference])
            .trim(),
        format!("{NAME} <{EMAIL}>|{NAME} <{EMAIL}>|supersigil review {id}")
    );

    let second = judged(
        &f,
        &f.repo,
        &["dismiss", "src/new.rs", "--reason", "generated"],
    );

    assert!(!second.contains("Kept the reviewed snapshot"), "{second}");
    assert_eq!(f.git(&["rev-parse", &reference]).trim(), kept);
    assert_eq!(refs(&f).lines().count(), 1, "{}", refs(&f));
}

#[test]
fn a_review_named_with_the_flag_is_judged_instead_of_the_selection() {
    let (f, first) = reviewed();
    write(&f.repo, "notes.txt", "changed since\n");
    let second = f.json(&f.repo, &["review", "--format", "json"]);
    let (first, second) = (basis_id(&first), basis_id(&second));
    assert_ne!(first, second);

    let text = judged(
        &f,
        &f.repo,
        &["acknowledge", "old.txt", "--review", &first[..8]],
    );

    let [judgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    assert_eq!(judgment.basis.as_str(), first);
    assert!(
        text.starts_with(&format!("Acknowledged old.txt on review {} ", &first[..8])),
        "{text}"
    );
    // The selection stays, so every next command names the review judged.
    let flag = format!(" --review={}", &first[..8]);
    let next: Vec<&str> = text
        .lines()
        .skip_while(|line| !line.starts_with("Next: "))
        .collect();
    assert_eq!(next.len(), 3, "{text}");
    assert!(next.iter().all(|line| line.ends_with(&flag)), "{text}");
    let store = review_store(&f, &f.reviews());
    let selections = store.snapshot().unwrap().manifest().selections.clone();
    assert_eq!(selections[&f.repo].as_str(), second);
}

#[test]
fn a_judgment_carried_from_a_review_of_the_same_content_lets_another_complete() {
    let f = Fixture::new();
    identify(&f);
    write(&f.repo, "a.txt", "one\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "a.txt", "two\n");
    f.json(&f.repo, &["review", "--format", "json"]);
    judged(&f, &f.repo, &["acknowledge", "a.txt"]);
    let [acknowledgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    // Another worktree with the same change is the same content.
    let other = f.root.join("other");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &other.display().to_string(),
    ]);
    write(&other, "a.txt", "two\n");
    let review = f.json(&other, &["review", "--format", "json"]);
    assert_ne!(basis_id(&review), acknowledgment.basis.as_str());

    let again = refused(&f, &mut judge(&f, &other, &["acknowledge", "a.txt"]));
    judged(&f, &other, &["complete"]);

    assert_eq!(
        again,
        format!(
            "No judgment recorded. Already closed; undo it first:\n  a.txt: acknowledged by \
             {NAME} on {} on review {}, carried (judgment {})\nNext: supersigil judge undo \
             a.txt\n",
            acknowledgment.at.date(),
            acknowledgment.basis.short(),
            acknowledgment.id.short()
        )
    );
    let [_, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();
    assert_eq!(completion.basis.as_str(), basis_id(&review));
    assert_eq!(
        completion.closes,
        BTreeMap::from([(item(&review, "a.txt"), acknowledgment.id)])
    );
}

#[test]
fn names_that_read_as_flags_follow_a_double_dash_in_every_next_command() {
    let f = Fixture::new();
    identify(&f);
    write(&f.repo, "-x.rs", "one\n");
    write(&f.repo, "-y.rs", "one\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "-x.rs", "two\n");
    write(&f.repo, "-y.rs", "two\n");
    let summary = f
        .supersigil(&f.repo, &["review", "--format", "terminal"])
        .output()
        .unwrap();
    let summary = String::from_utf8(summary.stdout).unwrap();
    assert!(
        summary.contains("\nNext: supersigil judge acknowledge -- -x.rs -y.rs\n"),
        "{summary}"
    );

    let text = judged(&f, &f.repo, &["acknowledge", "--", "-x.rs"]);

    let printed = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: supersigil "))
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(printed, "judge acknowledge -- -y.rs");
    let words: Vec<&str> = printed.split(' ').collect();
    let output = f.plain(&f.repo, &words).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(recorded(&f).len(), 2);
}

#[test]
fn flag_values_that_start_with_a_dash_stay_values_in_every_next_command() {
    let f = Fixture::new();
    identify(&f);
    write(&f.repo, "notes.txt", "draft\n");
    f.commit(&f.repo, "base");
    // A worktree and a reviews directory whose paths start with a dash.
    f.git(&["worktree", "add", "-q", "-b", "dash", "../-wt"]);
    write(&f.root.join("-wt"), "notes.txt", "final\n");
    let flags = ["--checkout=-wt", "--review-dir=-reviews"];
    let notes = f.root.join("-wt/notes.txt");
    let acknowledge = [&["acknowledge", notes.to_str().unwrap()][..], &flags].concat();

    let unselected = refused(&f, &mut judge(&f, &f.root, &acknowledge));
    assert!(
        unselected.contains(" Run supersigil review --checkout=-wt --review-dir=-reviews first,"),
        "{unselected}"
    );
    f.json(
        &f.root,
        &[&["review", "--format", "json"][..], &flags].concat(),
    );
    let text = judged(&f, &f.root, &acknowledge);

    let printed = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: supersigil "))
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(
        printed,
        "judge complete --checkout=-wt --review-dir=-reviews"
    );
    let words: Vec<&str> = printed.split(' ').collect();
    let output = f.plain(&f.root, &words).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = review_store(&f, &f.root.join("-reviews"));
    assert_eq!(store.snapshot().unwrap().judgments().unwrap().len(), 2);
}

/// Needs a filesystem that accepts a file name that is not UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn commands_and_refusals_show_a_flag_value_that_cannot_be_printed_as_given() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let f = Fixture::new();
    slice(&f);
    identify(&f);
    // Printed lossily, this directory would read as `r-\u{fffd}`, another
    // directory, which can hold another review store.
    let reviews = f.root.join(OsStr::from_bytes(b"r-\xff"));
    let in_reviews = |args: &[&str]| {
        let mut command = judge(&f, &f.repo, args);
        command.arg("--review-dir").arg(&reviews);
        command
    };
    let note = "The --review-dir value is not valid UTF-8 or holds a control character, so \
                commands show <as given> for it: pass it as given.";

    let unselected = refused(&f, &mut in_reviews(&["acknowledge", "old.txt"]));
    let review = f
        .supersigil(&f.repo, &["review", "--format", "json"])
        .arg("--review-dir")
        .arg(&reviews)
        .output()
        .unwrap();
    assert!(review.status.success());
    let acknowledged = in_reviews(&["--format", "terminal", "acknowledge", "old.txt"])
        .output()
        .unwrap();
    let open = refused(&f, &mut in_reviews(&["complete"]));

    assert_eq!(
        unselected,
        format!(
            "No judgment recorded. No review selected in this worktree. Run supersigil review \
             --review-dir=<as given> first, or pass --review <id> (supersigil judge list \
             --review-dir=<as given> shows reviews).\n{note}\n"
        )
    );
    assert!(
        acknowledged.status.success(),
        "{}",
        String::from_utf8_lossy(&acknowledged.stderr)
    );
    let acknowledged = String::from_utf8(acknowledged.stdout).unwrap();
    for text in [&acknowledged, &open] {
        assert!(
            text.ends_with(&format!(
                "\n  then: supersigil judge complete --review-dir=<as given>\n{note}\n"
            )),
            "{text}"
        );
    }
    for text in [&unselected, &acknowledged, &open] {
        assert_eq!(text.matches(note).count(), 1, "{text}");
        assert!(!text.contains('\u{fffd}'), "{text}");
    }
    let store = review_store(&f, &reviews);
    assert_eq!(store.snapshot().unwrap().judgments().unwrap().len(), 1);
}

#[test]
fn help_defines_each_verb_and_what_judging_keeps() {
    let output = common::supersigil_cmd()
        .args(["judge", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for text in [
        "acknowledge  Acknowledge items: \"I have considered this item\"",
        "dismiss      Dismiss items: \"it needs no follow-up, for the recorded reason\"",
        "comment      Add a note to the review or an item; it leaves the item's state alone",
        "complete     Record completion of this saved review; it does not commit or merge",
        "A completion carries over to any review of the same content, not only to the same \
         review id.",
        "Judgments stay on this machine: refs/supersigil/ is not pushed by default.",
        "The first judgment on a review needs a git identity and keeps the reviewed snapshot \
         as a ref.",
        "judge records against the saved review and never looks at the working tree.",
    ] {
        assert!(help.contains(text), "{text:?} not in:\n{help}");
    }
}

#[test]
fn inside_an_agent_session_nothing_is_recorded() {
    let (f, _) = reviewed();

    for marker in ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID"] {
        let message = refused(
            &f,
            judge(&f, &f.repo, &["acknowledge", "old.txt"]).env(marker, "1"),
        );
        assert_eq!(message, AGENT);
    }
}

#[test]
fn without_a_selected_review_nothing_is_recorded() {
    let f = Fixture::new();
    slice(&f);
    identify(&f);

    let without_store = refused(&f, &mut judge(&f, &f.repo, &["acknowledge", "old.txt"]));
    // A review in an agent session is saved without being selected.
    let agent = f
        .supersigil(&f.repo, &["review", "--format", "json"])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert!(agent.status.success());
    let unselected = refused(&f, &mut judge(&f, &f.repo, &["acknowledge", "old.txt"]));

    assert_eq!(without_store, NO_SELECTION);
    assert_eq!(unselected, NO_SELECTION);
}

/// Needs a filesystem that accepts a file name that is not UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn a_worktree_whose_path_is_not_utf8_has_no_saved_review_to_judge() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let f = Fixture::new();
    let repo = f.root.join(OsStr::from_bytes(b"caf\xe9"));
    std::fs::create_dir(&repo).unwrap();
    f.git_in(&repo, &["init", "-q", "-b", "main"]);
    f.git_in(&repo, &["config", "user.name", NAME]);
    f.git_in(&repo, &["config", "user.email", EMAIL]);
    write(&repo, "notes.txt", "draft\n");
    f.commit(&repo, "base");
    write(&repo, "notes.txt", "final\n");
    let review = f.json(&repo, &["review", "--format", "json"]);
    assert_eq!(review["basis"], Value::Null);

    let message = refused(&f, &mut judge(&f, &repo, &["acknowledge", "notes.txt"]));

    // Running the review again cannot help.
    assert_eq!(
        message,
        "No judgment recorded. The review store cannot hold this worktree's path (it is not \
         valid UTF-8), so this worktree has no saved reviews.\n"
    );
}

#[test]
fn a_review_prefix_must_name_one_saved_review() {
    let (f, review) = reviewed();
    let id = basis_id(&review);
    let unknown = if id.starts_with("00000000") {
        "ffffffff"
    } else {
        "00000000"
    };
    // Another saved review whose id shares the first 8 digits.
    let twin = format!(
        "{}{}",
        &id[..8],
        if id.ends_with('0') { "1" } else { "0" }.repeat(56)
    );
    let manifest = review_store(&f, &f.reviews()).root().join("manifest.json");
    let mut json: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    json["bases"][&twin] = json["bases"][&id].clone();
    std::fs::write(&manifest, serde_json::to_vec(&json).unwrap()).unwrap();
    let mut both = [id.clone(), twin];
    both.sort();
    let with_review = |prefix: &str| {
        refused(
            &f,
            &mut judge(&f, &f.repo, &["ack", "old.txt", "--review", prefix]),
        )
    };

    assert_eq!(
        with_review(unknown),
        format!(
            "No judgment recorded. No saved review starts with {unknown} (supersigil judge list \
             shows reviews).\n"
        )
    );
    assert_eq!(
        with_review(&id[..8]),
        format!(
            "No judgment recorded. Several saved reviews start with {}; give more digits:\n  \
             {}\n  {}\n",
            &id[..8],
            both[0],
            both[1]
        )
    );
    assert_eq!(
        with_review("c82d"),
        "No judgment recorded. Name a review by at least 8 hex digits of its id, not c82d \
         (supersigil judge list shows reviews).\n"
    );
}

#[test]
fn each_name_must_name_one_gate_item() {
    let (f, _) = reviewed();

    let message = refused(
        &f,
        &mut judge(
            &f,
            &f.repo,
            &["acknowledge", "nope.rs", "src/lib.rs", "src"],
        ),
    );

    assert_eq!(
        message,
        "No judgment recorded. Each name must name one gate item:\n  \
         no gate item is named nope.rs; nearby: old.txt\n  \
         src/lib.rs names 2 gate items; name one of: lines:src/lib.rs, gap:src/lib.rs\n  \
         src is a directory; name each item; there are no wildcards: lines:src/lib.rs, \
         src/new.rs, gap:src/lib.rs\n"
    );
}

#[test]
fn a_dismissal_needs_a_reason() {
    let (f, _) = reviewed();

    let without = refused(&f, &mut judge(&f, &f.repo, &["dismiss", "src/new.rs"]));
    let blank = refused(
        &f,
        &mut judge(&f, &f.repo, &["dismiss", "src/new.rs", "--reason", " "]),
    );

    for message in [without, blank] {
        assert_eq!(
            message,
            "No judgment recorded. A dismissal needs --reason.\n"
        );
    }
}

#[test]
fn an_item_already_closed_is_not_judged_again() {
    let (f, _) = reviewed();
    judged(&f, &f.repo, &["acknowledge", "old.txt"]);
    let [acknowledgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();

    let message = refused(
        &f,
        &mut judge(
            &f,
            &f.repo,
            &["dismiss", "src/new.rs", "old.txt", "--reason", "noise"],
        ),
    );

    assert_eq!(
        message,
        format!(
            "No judgment recorded. Already closed; undo it first:\n  old.txt: acknowledged by \
             {NAME} on {} (judgment {})\nNext: supersigil judge undo old.txt\n",
            acknowledgment.at.date(),
            acknowledgment.id.short()
        )
    );
}

#[test]
fn completing_needs_every_item_closed_whatever_the_comments_say() {
    let (f, review) = reviewed();
    judged(&f, &f.repo, &["comment", "old.txt", "--text", "Fine."]);
    judged(&f, &f.repo, &["acknowledge", "lines:src/lib.rs"]);

    let message = refused(&f, &mut judge(&f, &f.repo, &["complete"]));

    assert_eq!(
        message,
        format!(
            "No judgment recorded. Review {} has 3 open items: old.txt, src/new.rs, \
             gap:src/lib.rs. Comments do not close items.\n{}",
            &basis_id(&review)[..8],
            next("old.txt src/new.rs gap:src/lib.rs")
        )
    );
}

#[test]
fn a_review_is_completed_once() {
    let (f, review) = reviewed();
    judged(
        &f,
        &f.repo,
        &[
            "acknowledge",
            "old.txt",
            "lines:src/lib.rs",
            "src/new.rs",
            "gap:src/lib.rs",
        ],
    );
    judged(&f, &f.repo, &["complete"]);
    let [_, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    let message = refused(&f, &mut judge(&f, &f.repo, &["complete"]));

    assert_eq!(
        message,
        format!(
            "No judgment recorded. Review {} is already complete: completed on {} by {NAME} \
             (judgment {}).\n",
            &basis_id(&review)[..8],
            completion.at.date(),
            completion.id.short()
        )
    );
}

#[test]
fn without_a_git_identity_nothing_is_recorded() {
    // Git's global and system configuration are isolated, and the
    // repository sets no identity of its own.
    let f = Fixture::new();
    slice(&f);
    f.json(&f.repo, &["review", "--format", "json"]);

    let message = refused(&f, &mut judge(&f, &f.repo, &["acknowledge", "old.txt"]));

    assert_eq!(message, NO_IDENTITY);
}

#[test]
fn a_busy_review_store_records_nothing() {
    let (f, _) = reviewed();
    let store = review_store(&f, &f.reviews());
    // Another command, say a review, holding the store's writer lock.
    let held = store.begin().unwrap();

    let started = Instant::now();
    let message = refused(&f, &mut judge(&f, &f.repo, &["acknowledge", "old.txt"]));

    assert!(started.elapsed() < Duration::from_secs(30));
    drop(held);
    assert_eq!(
        message,
        "No judgment recorded. The review store is busy; try again.\n"
    );
}

#[test]
fn a_snapshot_git_pruned_before_the_first_judgment_needs_a_new_review() {
    let (f, _) = reviewed();
    // The captured working tree is unreachable until a judgment keeps it.
    f.git(&["prune", "--expire=now"]);

    let message = refused(&f, &mut judge(&f, &f.repo, &["acknowledge", "old.txt"]));

    assert_eq!(
        message,
        "No judgment recorded. The reviewed snapshot is no longer in the repository; run \
         supersigil review again.\n"
    );
}
