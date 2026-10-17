//! Tests `supersigil judge` in temporary git repositories.
//! Judgments apply to saved reviews, and refused calls record nothing.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::fixture::{
    Fixture, LIB_FINAL, basis_id, gate_row, review_store, slice, slice_text, word, write,
};
use serde_json::{Value, json};
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{
    self, Basis, BasisId, GateItem, ItemId, Judgment, JudgmentKind, PathBytes, Subject, Who,
    Withdrawal,
};
use supersigil_record::time::UtcTime;

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

/// Reviews the slice fixture with a git identity. Leaves all gate items open:
/// `old.txt`, `lines:src/lib.rs`, untracked `src/new.rs`, and `gap:src/lib.rs`.
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

/// Runs `supersigil judge --format terminal <args>` in `dir` and returns its output.
/// Panics unless it succeeds.
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

/// Runs `command` and checks that it exits 1, prints no standard output,
/// and records no judgment or ref. Returns its standard error.
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
            next(&format!("{} gap:lib.rs", word(&old)))
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
fn a_completion_carries_the_judgments_it_counted_to_another_worktree() {
    // a.txt is acknowledged on a review of only it; b.txt on a later review
    // of both in the same worktree, which is completed with a.txt carried.
    // Another worktree with the same change relates to the completed review
    // only, so a.txt carries through the completion's count.
    let f = Fixture::new();
    identify(&f);
    write(&f.repo, "a.txt", "one\n");
    write(&f.repo, "b.txt", "one\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "a.txt", "two\n");
    f.json(&f.repo, &["review", "--format", "json"]);
    judged(&f, &f.repo, &["acknowledge", "a.txt"]);
    write(&f.repo, "b.txt", "two\n");
    f.json(&f.repo, &["review", "--format", "json"]);
    judged(&f, &f.repo, &["acknowledge", "b.txt"]);
    judged(&f, &f.repo, &["complete"]);
    let [first, _, completion] = <[Judgment; 3]>::try_from(recorded(&f)).unwrap();
    let other = f.root.join("other");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &other.display().to_string(),
    ]);
    write(&other, "a.txt", "two\n");
    write(&other, "b.txt", "two\n");

    let carried = f.json(&other, &["review", "--format", "json"]);
    judged(&f, &other, &["undo", "a.txt"]);
    let undone = f.json(&other, &["review", "--format", "json"]);

    assert_eq!(carried["status"]["complete"], true, "{carried:#}");
    assert_eq!(carried["gate"]["open"], 0);
    assert_eq!(gate_row(&carried, "a.txt")["state"], "carried");
    assert_eq!(
        gate_row(&carried, "a.txt")["judgment"]["id"],
        first.id.as_str()
    );
    let undo = recorded(&f).pop().unwrap();
    assert_eq!(
        undo.withdraws,
        [
            withdrawal(&first, Some(item(&carried, "a.txt"))),
            withdrawal(&completion, None),
        ]
    );
    assert_eq!(undone["status"]["complete"], false, "{undone:#}");
    assert_eq!(gate_row(&undone, "a.txt")["state"], "open");
    assert_eq!(gate_row(&undone, "b.txt")["state"], "carried");
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
    let short = refused(&f, &mut in_reviews(&["undo", "--judgment", "1234"]));
    let shown = in_reviews(&["--format", "terminal", "show"])
        .output()
        .unwrap();

    assert_eq!(
        unselected,
        format!(
            "No judgment recorded. No review selected in this worktree. Run supersigil review \
             --review-dir=<as given> first, or pass --review <id> (supersigil judge list \
             --review-dir=<as given> shows reviews).\n{note}\n"
        )
    );
    assert_eq!(
        short,
        format!(
            "No judgment recorded. Name a judgment by at least 8 characters of its id, not 1234 \
             (supersigil judge show --review-dir=<as given> lists each judgment's id).\n{note}\n"
        )
    );
    for output in [&acknowledged, &shown] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let acknowledged = String::from_utf8(acknowledged.stdout).unwrap();
    let shown = String::from_utf8(shown.stdout).unwrap();
    for text in [&acknowledged, &open, &shown] {
        assert!(
            text.ends_with(&format!(
                "\n  then: supersigil judge complete --review-dir=<as given>\n{note}\n"
            )),
            "{text}"
        );
    }
    for text in [&unselected, &acknowledged, &open, &short, &shown] {
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
        "undo         Withdraw the judgments closing items, or one judgment by its id",
        "list         List the saved reviews with judgments and each worktree's selection",
        "show         Show a saved review's gate, comments, and withdrawn judgments",
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
    judged(&f, &f.repo, &["acknowledge", "src/new.rs"]);
    let [acknowledgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();

    for marker in ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID"] {
        for args in [
            &["acknowledge", "old.txt"][..],
            &["undo", "src/new.rs"],
            &["undo", "--judgment", acknowledgment.id.as_str()],
        ] {
            let message = refused(&f, judge(&f, &f.repo, args).env(marker, "1"));
            assert_eq!(message, AGENT);
        }
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
fn a_reason_given_is_never_blank() {
    let (f, _) = reviewed();
    judged(&f, &f.repo, &["acknowledge", "old.txt"]);
    let [acknowledgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();

    let calls: [&[&str]; 3] = [
        &["acknowledge", "src/new.rs", "--reason", ""],
        &["undo", "old.txt", "--reason", " "],
        &[
            "undo",
            "--judgment",
            acknowledgment.id.short(),
            "--reason",
            "\t",
        ],
    ];
    for args in calls {
        assert_eq!(
            refused(&f, &mut judge(&f, &f.repo, args)),
            "No judgment recorded. A reason can't be blank; give one or leave out --reason.\n",
            "{args:?}"
        );
    }
}

#[test]
fn a_text_given_is_never_blank() {
    let (f, _) = reviewed();
    judged(
        &f,
        &f.repo,
        &[
            "acknowledge",
            "old.txt",
            "src/new.rs",
            "lines:src/lib.rs",
            "gap:src/lib.rs",
        ],
    );

    let calls: [(&[&str], &str); 3] = [
        (
            &["comment", "--text", ""],
            "A comment's text can't be blank.",
        ),
        (
            &["comment", "old.txt", "--text", " \n"],
            "A comment's text can't be blank.",
        ),
        (
            &["complete", "--text", "\t"],
            "A completion's text can't be blank; give some or leave out --text.",
        ),
    ];
    for (args, message) in calls {
        assert_eq!(
            refused(&f, &mut judge(&f, &f.repo, args)),
            format!("No judgment recorded. {message}\n"),
            "{args:?}"
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

/// Reviews the slice fixture, acknowledges every gate item, and completes it.
/// Returns the fixture, review, acknowledgment, and completion.
fn completed() -> (Fixture, Value, Judgment, Judgment) {
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
    let [acknowledgment, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();
    (f, review, acknowledgment, completion)
}

/// Reviews changed `a.txt` and `b.txt`, acknowledges both with a reason, and
/// completes the review. Creates worktree `other` with the same changes and
/// reviews it. Returns the fixture, second worktree, and both reviews.
fn carried() -> (Fixture, PathBuf, Value, Value) {
    let f = Fixture::new();
    identify(&f);
    write(&f.repo, "a.txt", "one\n");
    write(&f.repo, "b.txt", "one\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "a.txt", "two\n");
    write(&f.repo, "b.txt", "two\n");
    let first = f.json(&f.repo, &["review", "--format", "json"]);
    judged(
        &f,
        &f.repo,
        &["acknowledge", "a.txt", "b.txt", "--reason", "checked"],
    );
    judged(&f, &f.repo, &["complete"]);
    let other = f.root.join("other");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &other.display().to_string(),
    ]);
    write(&other, "a.txt", "two\n");
    write(&other, "b.txt", "two\n");
    let second = f.json(&other, &["review", "--format", "json"]);
    (f, other, first, second)
}

/// Builds a withdrawal of `judgment` for `item`, or the whole judgment if absent.
fn withdrawal(judgment: &Judgment, item: Option<ItemId>) -> Withdrawal {
    Withdrawal {
        judgment: judgment.id.clone(),
        item,
    }
}

#[test]
fn undoing_an_item_withdraws_its_acknowledgment_and_opens_it_again() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];
    judged(&f, &f.repo, &["acknowledge", "old.txt", "src/new.rs"]);
    let kept = refs(&f);

    let text = judged(&f, &f.repo, &["undo", "old.txt", "--reason", "wrong item"]);

    let [acknowledgment, undo] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();
    assert_eq!(undo.kind, JudgmentKind::Undo);
    assert_eq!(undo.basis, acknowledgment.basis);
    assert_eq!(undo.items, [item(&review, "old.txt")]);
    assert_eq!(undo.reason.as_deref(), Some("wrong item"));
    assert_eq!(
        undo.withdraws,
        [withdrawal(&acknowledgment, Some(item(&review, "old.txt")))]
    );
    assert_eq!(refs(&f), kept);
    assert_eq!(
        text,
        format!(
            "Undid old.txt on review {id8} (working tree over {}).\n\
             Withdrew your acknowledgment of old.txt ({}, review {id8}).\n\
             Judgment {}. Gate: 3 open: old.txt, lines:src/lib.rs, gap:src/lib.rs.\n{}",
            head(&f),
            acknowledgment.at.date(),
            undo.id.short(),
            next("old.txt lines:src/lib.rs gap:src/lib.rs")
        )
    );
}

#[test]
fn undoing_an_item_of_a_completed_review_withdraws_the_completion_too() {
    let (f, review, acknowledgment, completion) = completed();
    let id8 = &basis_id(&review)[..8];

    let text = judged(&f, &f.repo, &["undo", "gap:src/lib.rs"]);

    let [_, _, undo] = <[Judgment; 3]>::try_from(recorded(&f)).unwrap();
    assert_eq!(
        undo.withdraws,
        [
            withdrawal(&acknowledgment, Some(item(&review, "gap:src/lib.rs"))),
            withdrawal(&completion, None),
        ]
    );
    assert_eq!(
        text,
        format!(
            "Undid gap:src/lib.rs on review {id8} (working tree over {}).\n\
             Withdrew your acknowledgment of gap:src/lib.rs ({}, review {id8}).\n\
             Also withdrew the completion of review {id8}.\n\
             Judgment {}. Gate: 1 open: gap:src/lib.rs.\n{}",
            head(&f),
            acknowledgment.at.date(),
            undo.id.short(),
            next("gap:src/lib.rs")
        )
    );
}

#[test]
fn undoing_a_carried_acknowledgment_withdraws_it_and_the_completion_counting_it() {
    let (f, other, first, second) = carried();
    let (first8, second8) = (&basis_id(&first)[..8], &basis_id(&second)[..8]);
    let [acknowledgment, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    let text = judged(&f, &other, &["undo", "a.txt"]);

    let [_, _, undo] = <[Judgment; 3]>::try_from(recorded(&f)).unwrap();
    assert_eq!(undo.basis.as_str(), basis_id(&second));
    assert_eq!(
        undo.withdraws,
        [
            withdrawal(&acknowledgment, Some(item(&second, "a.txt"))),
            withdrawal(&completion, None),
        ]
    );
    assert_eq!(
        text,
        format!(
            "Undid a.txt on review {second8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{second8}…\n\
             Withdrew your acknowledgment of a.txt ({}, review {first8}).\n\
             Also withdrew the completion of review {first8}.\n\
             Judgment {}. Gate: 1 open: a.txt.\n{}",
            head(&f),
            acknowledgment.at.date(),
            undo.id.short(),
            next("a.txt")
        )
    );
    // Withdrawn on the first review as well: the judgment covered the
    // same evidence there.
    let again = f.json(&f.repo, &["review", "--format", "json"]);
    assert_eq!(gate_row(&again, "a.txt")["state"], "open");
    assert_eq!(gate_row(&again, "b.txt")["state"], "acknowledged");
    assert_eq!(again["status"]["complete"], false);
}

#[test]
fn only_a_closed_item_can_be_undone() {
    let (f, first) = reviewed();
    judged(&f, &f.repo, &["acknowledge", "old.txt", "lines:src/lib.rs"]);
    let [acknowledgment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();

    let open = refused(
        &f,
        &mut judge(&f, &f.repo, &["undo", "old.txt", "src/new.rs"]),
    );
    // The library changes again: the acknowledgment of its lines no
    // longer matches what the new review shows.
    write(&f.repo, "src/lib.rs", &format!("{LIB_FINAL}// more\n"));
    f.json(&f.repo, &["review", "--format", "json"]);
    let reopened = refused(&f, &mut judge(&f, &f.repo, &["undo", "lines:src/lib.rs"]));

    assert_eq!(
        open,
        "No judgment recorded. Nothing closes these items, so there is nothing to undo:\n  \
         src/new.rs: open\n"
    );
    assert_eq!(
        reopened,
        format!(
            "No judgment recorded. Nothing closes these items, so there is nothing to undo:\n  \
             lines:src/lib.rs: reopened: file content or mode changed; evidence behind the \
             lines changed; line explanations changed since your acknowledgment (judgment {}, \
             review {})\n",
            acknowledgment.id.short(),
            &basis_id(&first)[..8]
        )
    );
}

#[test]
fn undoing_a_judgment_by_id_withdraws_it_whole() {
    let (f, review) = reviewed();
    let id8 = &basis_id(&review)[..8];
    judged(&f, &f.repo, &["acknowledge", "old.txt", "src/new.rs"]);
    judged(
        &f,
        &f.repo,
        &["acknowledge", "lines:src/lib.rs", "gap:src/lib.rs"],
    );
    judged(&f, &f.repo, &["comment", "--text", "Careful work."]);
    judged(&f, &f.repo, &["complete"]);
    let [both, _, comment, completion] = <[Judgment; 4]>::try_from(recorded(&f)).unwrap();
    let date = both.at.date();

    let uncommented = judged(&f, &f.repo, &["undo", "--judgment", comment.id.short()]);
    let uncompleted = judged(&f, &f.repo, &["undo", "--judgment", completion.id.as_str()]);
    judged(&f, &f.repo, &["complete"]);
    let unacknowledged = judged(
        &f,
        &f.repo,
        &[
            "undo",
            "--judgment",
            &both.id.as_str()[..10],
            "--reason",
            "too fast",
        ],
    );

    let [.., first, second, again, third] = <[Judgment; 8]>::try_from(recorded(&f)).unwrap();
    for undo in [&first, &second, &third] {
        assert_eq!(undo.kind, JudgmentKind::Undo);
        assert!(undo.items.is_empty());
    }
    assert_eq!(first.withdraws, [withdrawal(&comment, None)]);
    assert_eq!(second.withdraws, [withdrawal(&completion, None)]);
    assert_eq!(
        third.withdraws,
        [
            withdrawal(&both, Some(item(&review, "old.txt"))),
            withdrawal(&both, Some(item(&review, "src/new.rs"))),
            withdrawal(&again, None),
        ]
    );
    assert_eq!(third.reason.as_deref(), Some("too fast"));
    let undid = |judgment: &Judgment| {
        format!(
            "Undid judgment {} on review {id8} (working tree over {}).\n",
            judgment.id.short(),
            head(&f)
        )
    };
    assert_eq!(
        uncommented,
        format!(
            "{}Withdrew your comment on review {id8} ({date}).\n\
             Judgment {}. Gate: 0 open. Review complete.\n",
            undid(&comment),
            first.id.short()
        )
    );
    assert_eq!(
        uncompleted,
        format!(
            "{}Withdrew your completion of review {id8} ({date}).\n\
             Judgment {}. Gate: 0 open. Review not yet complete.\n\
             Next: supersigil judge complete\n",
            undid(&completion),
            second.id.short()
        )
    );
    assert_eq!(
        unacknowledged,
        format!(
            "{}Withdrew your acknowledgment of old.txt and src/new.rs ({date}, review {id8}).\n\
             Also withdrew the completion of review {id8}.\n\
             Judgment {}. Gate: 2 open: old.txt, src/new.rs.\n{}",
            undid(&both),
            third.id.short(),
            next("old.txt src/new.rs")
        )
    );
}

#[test]
fn a_judgment_id_prefix_must_name_one_judgment() {
    let (f, review) = reviewed();
    // Two comments whose ids share their first 8 characters.
    let basis: BasisId = serde_json::from_value(review["basis"]["id"].clone()).unwrap();
    let store = review_store(&f, &f.reviews());
    let mut tx = store.begin().unwrap();
    for last in ["1", "2"] {
        tx.append_judgment(&Judgment {
            id: JudgmentId::new(format!("deadbeef-0000-4000-8000-00000000000{last}")),
            basis: basis.clone(),
            kind: JudgmentKind::Comment,
            items: Vec::new(),
            reason: None,
            text: Some("Twin.".to_owned()),
            closes: BTreeMap::new(),
            withdraws: Vec::new(),
            who: Who {
                name: NAME.to_owned(),
                email: EMAIL.to_owned(),
            },
            at: UtcTime::now(),
        })
        .unwrap();
    }
    tx.commit(UtcTime::now()).unwrap();
    let undo = |prefix: &str| refused(&f, &mut judge(&f, &f.repo, &["undo", "--judgment", prefix]));

    assert_eq!(
        undo("deadbee"),
        "No judgment recorded. Name a judgment by at least 8 characters of its id, not deadbee \
         (supersigil judge show lists each judgment's id).\n"
    );
    assert_eq!(
        undo("0123abcd"),
        "No judgment recorded. No judgment starts with 0123abcd (supersigil judge show lists \
         each judgment's id).\n"
    );
    assert_eq!(
        undo("deadbeef"),
        "No judgment recorded. Several judgments start with deadbeef; give more characters:\n  \
         deadbeef-0000-4000-8000-000000000001\n  deadbeef-0000-4000-8000-000000000002\n"
    );
}

/// The lines `judge list` prints in `dir`, each split into its columns.
fn listed(f: &Fixture, dir: &Path, env: &[(&str, &str)]) -> Vec<Vec<String>> {
    let output = judge(f, dir, &["--format", "terminal", "list"])
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            line.split("  ")
                .map(str::trim)
                .filter(|column| !column.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// `columns` as owned strings.
fn row(columns: &[&str]) -> Vec<String> {
    columns.iter().map(|&column| column.to_owned()).collect()
}

#[test]
fn list_shows_judged_reviews_most_recently_judged_first_and_each_selection() {
    let (f, first, ..) = completed();
    write(&f.repo, "notes.txt", "changed since\n");
    let second = f.json(&f.repo, &["review", "--format", "json"]);
    judged(&f, &f.repo, &["comment", "--text", "Later."]);
    // Another worktree's review, selected there and never judged.
    let other = f.root.join("other");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &other.display().to_string(),
    ]);
    let third = f.json(&other, &["review", "--format", "json"]);
    let (first8, second8, third8) = (
        &basis_id(&first)[..8],
        &basis_id(&second)[..8],
        &basis_id(&third)[..8],
    );
    let date = recorded(&f)[0].at.date();
    let compared = format!("working tree over {}", head(&f));
    let (repo, other_path) = (f.repo.display().to_string(), other.display().to_string());
    let header = row(&[
        "review",
        "target",
        "worktree",
        "last judged",
        "status",
        "by",
    ]);
    let second_row = row(&[
        &format!("* {second8}"),
        &compared,
        &repo,
        &date,
        "1 open",
        NAME,
    ]);
    let first_row = row(&[first8, &compared, &repo, &date, "complete", NAME]);
    let third_row = row(&[
        &format!("* {third8}"),
        &compared,
        &other_path,
        "not judged",
        "0 open",
    ]);

    let before = listed(&f, &f.repo, &[]);
    // Reading needs no person: an agent may list.
    let by_agent = listed(&f, &other, &[("CLAUDECODE", "1")]);
    judged(
        &f,
        &f.repo,
        &["comment", "--review", first8, "--text", "Again."],
    );
    let after = listed(&f, &f.repo, &[]);

    assert_eq!(
        before,
        [
            header.clone(),
            second_row.clone(),
            first_row.clone(),
            third_row.clone()
        ]
    );
    assert_eq!(by_agent, before);
    assert_eq!(after, [header, first_row, second_row, third_row]);
}

#[test]
fn list_prints_each_review_as_json() {
    let (f, first, ..) = completed();
    write(&f.repo, "notes.txt", "changed since\n");
    let second = f.json(&f.repo, &["review", "--format", "json"]);
    let [_, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    // Piped, so `auto` prints JSON.
    let output = judge(&f, &f.repo, &["list"]).output().unwrap();

    assert!(output.status.success());
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        printed,
        json!({"reviews": [
            {
                "review": basis_id(&first),
                "selected": false,
                "worktree": f.repo,
                "base": first["basis"]["document"]["base"],
                "target": first["basis"]["document"]["target"],
                "complete": true,
                "open": 0,
                "last_judgment": {
                    "id": completion.id.as_str(),
                    "kind": "complete",
                    "review": basis_id(&first),
                    "who": {"name": NAME, "email": EMAIL},
                    "at": completion.at.to_string(),
                },
            },
            {
                "review": basis_id(&second),
                "selected": true,
                "worktree": f.repo,
                "base": second["basis"]["document"]["base"],
                "target": second["basis"]["document"]["target"],
                "complete": false,
                "open": 1,
                "last_judgment": null,
            },
        ]})
    );
}

#[test]
fn list_without_a_review_store_lists_nothing() {
    let f = Fixture::new();
    write(&f.repo, "a.txt", "one\n");
    f.commit(&f.repo, "base");

    let text = judged(&f, &f.repo, &["list"]);
    let output = judge(&f, &f.repo, &["list"]).output().unwrap();

    assert_eq!(text, "No judged or selected reviews in this repository.\n");
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"reviews": []})
    );
    // Reading created no store.
    assert!(!f.reviews().exists());
}

#[test]
fn list_indexes_each_gate_once_however_many_reviews_are_related() {
    // 600 commented reviews of the worktree over one base, each of another
    // target tree with the same 50 gate items, as reviewing a branch again
    // and again collects; every one is related to every other.
    let (f, review) = reviewed();
    let document: Basis = serde_json::from_value(review["basis"]["document"].clone()).unwrap();
    let template = &document.gate[0];
    let gate: Vec<GateItem> = (0..50)
        .map(|k| {
            GateItem::new(
                Subject::Path(PathBytes::new(format!("src/f{k}.rs"))),
                template.detail.clone(),
                template.fingerprint.clone(),
            )
        })
        .collect();
    let store = review_store(&f, &f.reviews());
    let mut tx = store.begin().unwrap();
    for k in 0..600 {
        let mut basis = document.clone();
        basis.target.tree = format!("{k:040x}");
        basis.gate.clone_from(&gate);
        let basis = tx.save_basis(&basis, UtcTime::now()).unwrap();
        tx.append_judgment(&Judgment {
            id: JudgmentId::generate(),
            basis,
            kind: JudgmentKind::Comment,
            items: Vec::new(),
            reason: None,
            text: Some("Again.".to_owned()),
            closes: BTreeMap::new(),
            withdraws: Vec::new(),
            who: Who {
                name: NAME.to_owned(),
                email: EMAIL.to_owned(),
            },
            at: UtcTime::now(),
        })
        .unwrap();
    }
    tx.commit(UtcTime::now()).unwrap();

    // Indexing every related review's gate again for each listed review
    // took about 6 s here in a debug build; indexing each gate once takes
    // under a second.
    let started = Instant::now();
    let rows = listed(&f, &f.repo, &[]);
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    // The header, the 600, and the selected review.
    assert_eq!(rows.len(), 602);
    assert_eq!(
        rows.iter()
            .filter(|row| row.iter().any(|column| column == "50 open"))
            .count(),
        600
    );
}

#[test]
fn a_judgment_naming_every_item_of_a_large_gate_lists_each_by_name() {
    // Few enough names to fit in a Windows command line.
    let (f, review) = reviewed();
    let document: Basis = serde_json::from_value(review["basis"]["document"].clone()).unwrap();
    let template = &document.gate[0];
    let mut basis = document.clone();
    basis.gate = (0..500)
        .map(|k| {
            GateItem::new(
                Subject::Path(PathBytes::new(format!("src/f{k}.rs"))),
                template.detail.clone(),
                template.fingerprint.clone(),
            )
        })
        .collect();
    let store = review_store(&f, &f.reviews());
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(&basis, UtcTime::now()).unwrap();
    tx.commit(UtcTime::now()).unwrap();
    let names: Vec<String> = (0..500).map(|k| format!("src/f{k}.rs")).collect();
    let mut args = vec!["acknowledge", "--review", &id.as_str()[..8]];
    args.extend(names.iter().map(String::as_str));

    let text = judged(&f, &f.repo, &args);

    let first = text.lines().next().unwrap();
    let listed = format!("{}, and {}", names[..499].join(", "), names[499]);
    assert!(
        first.starts_with(&format!(
            "Acknowledged {listed} on review {} ",
            &id.as_str()[..8]
        )),
        "{}",
        &first[..200.min(first.len())]
    );
    assert!(
        text.contains("Gate: 0 open."),
        "{}",
        &text[text.len() - 200..]
    );
}

/// `judgment` as reading commands print it in JSON.
fn info(judgment: &Judgment) -> Value {
    let mut info = json!({
        "id": judgment.id.as_str(),
        "kind": judgment.kind,
        "review": judgment.basis.as_str(),
        "who": judgment.who,
        "at": judgment.at.to_string(),
    });
    if let Some(reason) = &judgment.reason {
        info["reason"] = json!(reason);
    }
    if let Some(text) = &judgment.text {
        info["text"] = json!(text);
    }
    info
}

/// The header `judge show` prints for `review`, reviewed in `worktree`
/// over the commit `head`, with the whole working tree in scope.
fn shown_header(review: &Value, worktree: &Path, head: &str) -> String {
    let document = &review["basis"]["document"];
    format!(
        "Review {}: working tree over {head}.\n  worktree: {}\n  base: {} (tree {})\n  \
         target: working tree (tree {})\n  scope: all tracked paths\n",
        &basis_id(review)[..8],
        worktree.display(),
        document["base"]["commit"].as_str().unwrap(),
        document["base"]["tree"].as_str().unwrap(),
        document["target"]["tree"].as_str().unwrap(),
    )
}

/// The slice fixture reviewed, then: `old.txt` acknowledged with a reason,
/// `src/new.rs` dismissed, a comment on the review and one on
/// `src/new.rs`, and `lines:src/lib.rs` acknowledged and undone. Returns
/// the six judgments in that order.
fn judged_with_history() -> (Fixture, Value, [Judgment; 6]) {
    let (f, review) = reviewed();
    for args in [
        &[
            "acknowledge",
            "old.txt",
            "--reason",
            "read with the transcript",
        ][..],
        &["dismiss", "src/new.rs", "--reason", "generated"],
        &["comment", "--text", "Careful work."],
        &["comment", "src/new.rs", "--text", "Ask why."],
        &["acknowledge", "lines:src/lib.rs"],
        &["undo", "lines:src/lib.rs", "--reason", "wrong item"],
    ] {
        judged(&f, &f.repo, args);
    }
    let judgments = <[Judgment; 6]>::try_from(recorded(&f)).unwrap();
    (f, review, judgments)
}

#[test]
fn show_prints_the_selected_review_with_reasons_comments_and_history() {
    let (
        f,
        review,
        [
            acknowledgment,
            dismissal,
            on_review,
            on_item,
            mistaken,
            undo,
        ],
    ) = judged_with_history();
    let by = |judgment: &Judgment| {
        format!(
            "by {NAME} on {} (judgment {})",
            judgment.at.date(),
            judgment.id.short()
        )
    };

    let text = judged(&f, &f.repo, &["show"]);
    // Reading needs no person: an agent may show.
    let by_agent = judge(&f, &f.repo, &["--format", "terminal", "show"])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();

    assert_eq!(
        text,
        format!(
            "{}Review not complete.\nGate: 2 open, 2 closed.\n\n  \
             old.txt           acknowledged  {}: read with the transcript\n  \
             lines:src/lib.rs  open          4 unexplained lines: 4 unattributed\n  \
             src/new.rs        dismissed     {}: generated\n  \
             gap:src/lib.rs    open          1 gap between recorded edits\n\n\
             Comments:\n  on the review, {}: Careful work.\n  on src/new.rs, {}: Ask why.\n\n\
             History:\n  acknowledgment of lines:src/lib.rs {}, withdrawn {}: wrong item\n\n{}",
            shown_header(&review, &f.repo, &head(&f)),
            by(&acknowledgment),
            by(&dismissal),
            by(&on_review),
            by(&on_item),
            by(&mistaken),
            by(&undo),
            next("lines:src/lib.rs gap:src/lib.rs")
        )
    );
    assert!(by_agent.status.success());
    assert_eq!(String::from_utf8(by_agent.stdout).unwrap(), text);
}

#[test]
fn show_prints_the_review_as_json() {
    let (f, review, [acknowledgment, _, on_review, on_item, mistaken, undo]) =
        judged_with_history();

    // Piped, so `auto` prints JSON.
    let output = judge(&f, &f.repo, &["show"]).output().unwrap();

    assert!(output.status.success());
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let document = &review["basis"]["document"];
    assert_eq!(printed["review"], basis_id(&review));
    for field in ["worktree", "base", "target", "scope"] {
        assert_eq!(printed[field], document[field], "{field}");
    }
    assert_eq!(
        printed["status"],
        json!({"complete": false, "completion": null})
    );
    assert_eq!(printed["same_content_completions"], json!([]));
    assert_eq!(printed["gate"]["open"], 2);
    assert_eq!(
        gate_row(&printed, "old.txt")["judgment"],
        info(&acknowledgment)
    );
    assert_eq!(
        printed["comments"],
        json!([
            {"judgment": info(&on_review), "items": []},
            {"judgment": info(&on_item), "items": ["src/new.rs"]},
        ])
    );
    assert_eq!(
        printed["history"],
        json!([{
            "judgment": info(&mistaken),
            "items": ["lines:src/lib.rs"],
            "undo": info(&undo),
        }])
    );
}

#[test]
fn show_names_a_review_and_its_closures_carried_from_a_completed_review_of_the_same_content() {
    let (f, other, first, second) = carried();
    let (first8, second8) = (&basis_id(&first)[..8], &basis_id(&second)[..8]);
    let [acknowledgment, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();
    let date = acknowledgment.at.date();
    let carried = format!(
        "acknowledged  carried from review {first8}, by {NAME} on {date} (judgment {}): checked",
        acknowledgment.id.short()
    );
    let body = format!(
        "Review not complete.\nCompleted {date} for the same content as review {first8}; \
         supersigil review shows whether it carries over.\nGate: 0 open, 2 closed.\n\n  \
         a.txt  {carried}\n  b.txt  {carried}\n\n"
    );

    let selected = judged(&f, &other, &["show"]);
    let named = judged(&f, &f.repo, &["show", second8]);
    let completed = judged(&f, &f.repo, &["show"]);
    let output = judge(&f, &other, &["show"]).output().unwrap();

    let header = shown_header(&second, &other, &head(&f));
    assert_eq!(
        selected,
        format!("{header}{body}Next: supersigil judge complete\n")
    );
    // From the first worktree, which selects the first review, every next
    // command names the second.
    assert_eq!(
        named,
        format!("{header}{body}Next: supersigil judge complete --review={second8}\n")
    );
    assert_eq!(
        completed,
        format!(
            "{}Review complete: completed on {date} by {NAME} (judgment {}).\nGate: 0 open, 2 \
             closed.\n\n  a.txt  acknowledged  by {NAME} on {date} (judgment {}): checked\n  \
             b.txt  acknowledged  by {NAME} on {date} (judgment {}): checked\n",
            shown_header(&first, &f.repo, &head(&f)),
            completion.id.short(),
            acknowledgment.id.short(),
            acknowledgment.id.short()
        )
    );
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["review"], basis_id(&second));
    assert_eq!(
        printed["same_content_completions"],
        json!([info(&completion)])
    );
    assert_eq!(gate_row(&printed, "a.txt")["state"], "carried");
    assert_eq!(
        gate_row(&printed, "a.txt")["judgment"],
        info(&acknowledgment)
    );
}

#[test]
fn show_needs_a_review_to_show() {
    let f = Fixture::new();
    write(&f.repo, "a.txt", "one\n");
    f.commit(&f.repo, "base");
    let show = |args: &[&str]| {
        let output = judge(&f, &f.repo, &[&["show"], args].concat())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        String::from_utf8(output.stderr).unwrap()
    };

    assert_eq!(
        show(&[]),
        "No review selected in this worktree. Run supersigil review first, or pass --review \
         <id> (supersigil judge list shows reviews).\n"
    );
    assert_eq!(
        show(&["0123abcd"]),
        "No saved review starts with 0123abcd (supersigil judge list shows reviews).\n"
    );
    // Reading created no store.
    assert!(!f.reviews().exists());
}

#[test]
fn show_of_a_review_with_nothing_to_review_suggests_no_judgment() {
    let f = Fixture::new();
    write(&f.repo, "a.txt", "one\n");
    f.commit(&f.repo, "base");
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let text = judged(&f, &f.repo, &["show"]);
    let output = judge(&f, &f.repo, &["show"]).output().unwrap();

    assert_eq!(
        text,
        format!(
            "{}Nothing to review: no changes in scope.\nGate: no items.\n",
            shown_header(&review, &f.repo, &head(&f))
        )
    );
    // The JSON says what it always says.
    assert!(output.status.success());
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        printed["status"],
        json!({"complete": false, "completion": null})
    );
    assert_eq!(printed["gate"], json!({"open": 0, "items": []}));
}

#[test]
fn a_write_names_completions_of_the_same_content_instead_of_calling_the_review_incomplete() {
    let (f, other, first, second) = carried();
    let (first8, second8) = (&basis_id(&first)[..8], &basis_id(&second)[..8]);
    let [_, completion] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    let commented = judged(&f, &other, &["comment", "--text", "Looks right."]);
    let output = judge(
        &f,
        &other,
        &["--format", "json", "comment", "--text", "Still right."],
    )
    .output()
    .unwrap();

    let [.., note, _] = <[Judgment; 4]>::try_from(recorded(&f)).unwrap();
    assert_eq!(
        commented,
        format!(
            "Commented on review {second8} (working tree over {}).\n\
             Kept the reviewed snapshot as refs/supersigil/reviews/{second8}…\n\
             Judgment {}. Gate: 0 open.\n\
             Completed {} for the same content as review {first8}; supersigil review shows \
             whether it carries over.\nNext: supersigil judge complete\n",
            head(&f),
            note.id.short(),
            completion.at.date()
        )
    );
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["status"]["complete"], false);
    assert_eq!(
        printed["same_content_completions"],
        json!([info(&completion)])
    );
}

#[test]
fn undoing_by_id_a_judgment_of_another_review_names_that_review() {
    let (f, other, first, second) = carried();
    let (first8, second8) = (&basis_id(&first)[..8], &basis_id(&second)[..8]);
    judged(&f, &f.repo, &["comment", "--text", "Careful work."]);
    let [.., note] = <[Judgment; 3]>::try_from(recorded(&f)).unwrap();
    let note8 = note.id.short();

    let message = refused(&f, &mut judge(&f, &other, &["undo", "--judgment", note8]));

    assert_eq!(
        message,
        format!(
            "No judgment recorded. Judgment {note8} was recorded on review {first8} and closes \
             none of review {second8}'s items.\nTo withdraw it there: supersigil judge undo \
             --judgment {} --review={first8}\n",
            note.id
        )
    );
}

#[test]
fn an_undo_and_a_judgment_already_withdrawn_cannot_be_undone() {
    let (f, _) = reviewed();
    judged(&f, &f.repo, &["acknowledge", "old.txt"]);
    judged(&f, &f.repo, &["undo", "old.txt"]);
    let [acknowledgment, undo] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    let of_undo = refused(
        &f,
        &mut judge(&f, &f.repo, &["undo", "--judgment", undo.id.as_str()]),
    );
    let withdrawn = refused(
        &f,
        &mut judge(
            &f,
            &f.repo,
            &["undo", "--judgment", acknowledgment.id.as_str()],
        ),
    );

    assert_eq!(
        of_undo,
        format!(
            "No judgment recorded. Judgment {} is an undo; an undo cannot be withdrawn.\n",
            undo.id.short()
        )
    );
    assert_eq!(
        withdrawn,
        format!(
            "No judgment recorded. Judgment {} is already withdrawn.\n",
            acknowledgment.id.short()
        )
    );
}

#[test]
fn history_names_the_item_of_a_withdrawn_comment() {
    let (f, _) = reviewed();
    judged(
        &f,
        &f.repo,
        &["comment", "old.txt", "--text", "Check deletion"],
    );
    let [comment] = <[Judgment; 1]>::try_from(recorded(&f)).unwrap();
    judged(&f, &f.repo, &["undo", "--judgment", comment.id.as_str()]);
    let [_, undo] = <[Judgment; 2]>::try_from(recorded(&f)).unwrap();

    let text = judged(&f, &f.repo, &["show"]);
    let output = judge(&f, &f.repo, &["show"]).output().unwrap();

    let line = format!(
        "  comment on old.txt by {NAME} on {} (judgment {}), withdrawn by {NAME} on {} \
         (judgment {})\n",
        comment.at.date(),
        comment.id.short(),
        undo.at.date(),
        undo.id.short()
    );
    assert!(text.contains(&line), "{text}");
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["history"][0]["items"], json!(["old.txt"]));
}
