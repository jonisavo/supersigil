//! Property tests of anchor against an independent forward interpreter.
//!
//! The oracle in this file applies generated operations to bytes and tags
//! every byte with its source (a base offset, the operation that wrote it,
//! or an unrecorded change). It re-implements the recorder's semantics and
//! shares no code with `supersigil-anchor`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    WT, append, append_creating, create, in_worktree, overwrite, replace, replace_all, request,
    state, unknown_op, with_hashes, with_patch,
};
use proptest::prelude::*;
use supersigil_anchor::provenance::replay;
use supersigil_anchor::step::Budget;
use supersigil_anchor::walk::{WalkedChain, walk};
use supersigil_anchor::{
    CandidateEdit, ChainClass, ChainEnd, DEFAULT_BUDGET_BYTES, LineOutcome, Origin, Provenance,
    State, TargetKind, attribute,
};
use supersigil_record::observations::{Content, Edit, EditOperation, FileState};
use supersigil_record::{ContentId, EventId};

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

/// Where a byte came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    /// This offset of the base.
    Base(usize),
    /// Written by the operation with this index.
    Op(usize),
    /// Written by a change nothing recorded.
    Unrecorded,
}

/// Bytes with the source of each.
#[derive(Clone, Debug)]
struct Doc {
    bytes: Vec<u8>,
    tags: Vec<Tag>,
}

impl Doc {
    fn new(text: &str, tag: impl Fn(usize) -> Tag) -> Self {
        Self {
            bytes: text.as_bytes().to_vec(),
            tags: (0..text.len()).map(tag).collect(),
        }
    }

    fn text(&self) -> String {
        String::from_utf8(self.bytes.clone()).unwrap()
    }

    /// Replaces `len` bytes at `at` with `with`, tagged `tag`.
    fn splice(&self, at: usize, len: usize, with: &str, tag: Tag) -> Self {
        let mut bytes = self.bytes[..at].to_vec();
        bytes.extend_from_slice(with.as_bytes());
        bytes.extend_from_slice(&self.bytes[at + len..]);
        let mut tags = self.tags[..at].to_vec();
        tags.extend(std::iter::repeat_n(tag, with.len()));
        tags.extend_from_slice(&self.tags[at + len..]);
        Self { bytes, tags }
    }

    /// Line spans, each ending after its newline.
    fn lines(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut start = 0;
        for (i, b) in self.bytes.iter().enumerate() {
            if *b == b'\n' {
                spans.push((start, i + 1));
                start = i + 1;
            }
        }
        if start < self.bytes.len() {
            spans.push((start, self.bytes.len()));
        }
        spans
    }
}

/// The recorder's semantics, written again: an Edit replaces the first
/// occurrence (every non-overlapping one for replace-all); a Write's
/// result is its content; an append's is the bytes before it, if any,
/// followed by its non-empty text.
fn oracle_forward(edit: &Edit, before: Option<&[u8]>) -> Option<Vec<u8>> {
    let text = |m: &supersigil_record::observations::Material<String>| {
        m.retained().map(|s| s.as_bytes().to_vec())
    };
    match edit.operation {
        EditOperation::Write => text(&edit.new_text),
        EditOperation::Append => {
            let added = text(&edit.new_text).filter(|added| !added.is_empty())?;
            let mut out = before.unwrap_or_default().to_vec();
            out.extend_from_slice(&added);
            Some(out)
        }
        EditOperation::Unknown => None,
        EditOperation::Replace => {
            let (old, new, before) = (text(&edit.old_text)?, text(&edit.new_text)?, before?);
            if old.is_empty() {
                return None;
            }
            let mut out = Vec::new();
            let mut i = 0;
            let mut replaced = false;
            while i < before.len() {
                if (!replaced || edit.replace_all) && before[i..].starts_with(&old) {
                    out.extend_from_slice(&new);
                    i += old.len();
                    replaced = true;
                } else {
                    out.push(before[i]);
                    i += 1;
                }
            }
            replaced.then_some(out)
        }
    }
}

fn hash_matches(recorded: &FileState, bytes: Option<&[u8]>) -> bool {
    match (recorded, bytes) {
        (FileState::Absent, None)
        | (
            FileState::Present {
                content: Content::Unknown,
            },
            Some(_),
        ) => true,
        (
            FileState::Present {
                content: Content::Known(id),
            },
            Some(b),
        ) => ContentId::of(b) == *id,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Restricted histories: one transcript, whole-line operations, fresh lines
// ---------------------------------------------------------------------------

struct Built {
    base: Option<String>,
    target: String,
    edits: Vec<CandidateEdit>,
    /// Target line index of the unrecorded line, when there is one.
    unrecorded: Option<usize>,
    /// The oracle's outcome per target line.
    expected: Vec<Origin>,
}

/// One generated operation: (kind, pick, hash known?, patch retained?).
/// Kind 0 replaces the picked line with a fresh one, 1 deletes it, and 2
/// appends a fresh line at the end. A line that descends from an appended
/// one is replaced where it would be deleted: appending a line and then
/// deleting it returns to an earlier state, which leaves two readings.
type Op = (u8, u16, bool, bool);

/// Builds a history from generated choices. With `interruption =
/// Some((after, pick))`, a fresh line nothing records is inserted after
/// that many operations and never touched again.
fn build(base_present: bool, size: usize, ops: &[Op], interruption: Option<(u16, u16)>) -> Built {
    let mut counter = 0;
    // Tab-indented, so a retained patch shows every line in display form.
    let mut fresh = || {
        counter += 1;
        format!("\tL{counter:04}\n")
    };
    let mut edits = Vec::new();
    let (base, mut doc, mut k) = if base_present {
        let text: String = (0..size).map(|_| fresh()).collect();
        let doc = Doc::new(&text, Tag::Base);
        (Some(text), doc, 0)
    } else {
        let content: String = (0..size).map(|_| fresh()).collect();
        edits.push(candidate(create("op0", "t", 1, &content)));
        (None, Doc::new(&content, |_| Tag::Op(0)), 1)
    };
    // Operations whose line descends from an appended one.
    let mut appended = BTreeSet::new();
    let mut pending =
        interruption.map(|(after, pick)| (usize::from(after) % (ops.len() + 1), pick));
    for (step, &(kind, pick, known, patched)) in ops.iter().enumerate() {
        if let Some((_, at)) = pending.filter(|&(after, _)| after == step) {
            doc = interrupt(&doc, at, &mut fresh);
            pending = None;
        }
        let lines: Vec<(usize, usize)> = doc
            .lines()
            .into_iter()
            .filter(|&(s, _)| doc.tags[s] != Tag::Unrecorded)
            .collect();
        if lines.is_empty() {
            break;
        }
        let ordinal = u64::try_from(k).unwrap() + 1;
        let (next, mut edit) = if kind == 2 {
            let new = fresh();
            let next = doc.splice(doc.bytes.len(), 0, &new, Tag::Op(k));
            appended.insert(k);
            (next, append(&format!("op{k}"), "t", ordinal, &new))
        } else {
            let (s, e) = lines[usize::from(pick) % lines.len()];
            let old = String::from_utf8(doc.bytes[s..e].to_vec()).unwrap();
            let from_append = matches!(doc.tags[s], Tag::Op(j) if appended.contains(&j));
            let new = if kind == 1 && !from_append {
                String::new()
            } else {
                fresh()
            };
            if from_append {
                appended.insert(k);
            }
            let next = doc.splice(s, e - s, &new, Tag::Op(k));
            (next, replace(&format!("op{k}"), "t", ordinal, &old, &new))
        };
        if known {
            edit = with_hashes(edit, &doc.text(), &next.text());
        }
        if patched {
            edit = with_patch(edit, &doc.text(), &next.text());
        }
        edits.push(candidate(edit));
        doc = next;
        k += 1;
    }
    // The history may end early when every line is gone; the interruption
    // then comes last.
    if let Some((_, at)) = pending {
        doc = interrupt(&doc, at, &mut fresh);
    }
    let (expected, unrecorded) = expected_origins(base.as_deref(), &doc);
    Built {
        base,
        target: doc.text(),
        edits,
        unrecorded,
        expected,
    }
}

/// The oracle's outcome per line of `doc`, and the line index of the
/// unrecorded line when there is one.
fn expected_origins(base: Option<&str>, doc: &Doc) -> (Vec<Origin>, Option<usize>) {
    let base_starts = base
        .map(|b| Doc::new(b, Tag::Base).lines())
        .unwrap_or_default();
    let base_line = |offset: usize| {
        base_starts
            .iter()
            .position(|&(s, e)| s <= offset && offset < e)
            .unwrap()
    };
    let mut unrecorded = None;
    let expected = doc
        .lines()
        .iter()
        .enumerate()
        .map(|(line, &(s, e))| match doc.tags[s] {
            Tag::Base(o) => {
                assert!(
                    doc.tags[s..e]
                        .iter()
                        .all(|t| matches!(t, Tag::Base(x) if base_line(*x) == base_line(o)))
                );
                Origin::Base(base_line(o))
            }
            Tag::Op(k) => {
                assert!(doc.tags[s..e].iter().all(|t| *t == Tag::Op(k)));
                Origin::Introduced(EventId::new(format!("op{k}")))
            }
            Tag::Unrecorded => {
                unrecorded = Some(line);
                Origin::Unexplained
            }
        })
        .collect();
    (expected, unrecorded)
}

fn candidate(edit: Edit) -> CandidateEdit {
    in_worktree(edit, WT)
}

fn interrupt(doc: &Doc, pick: u16, fresh: &mut impl FnMut() -> String) -> Doc {
    let mut starts: Vec<usize> = doc.lines().iter().map(|&(s, _)| s).collect();
    starts.push(doc.bytes.len());
    let at = starts[usize::from(pick) % starts.len()];
    doc.splice(at, 0, &fresh(), Tag::Unrecorded)
}

fn restricted() -> impl Strategy<Value = (bool, usize, Vec<Op>)> {
    (
        any::<bool>(),
        1usize..=5,
        prop::collection::vec((0u8..3, any::<u16>(), any::<bool>(), any::<bool>()), 1..=4),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// (a) Over histories with an unambiguous inverse, a
    /// chain exact from the base exists and every line matches the oracle.
    #[test]
    fn completeness((base_present, size, ops) in restricted()) {
        let built = build(base_present, size, &ops, None);
        let out = attribute(request(
            built.base.as_deref(),
            Some(&built.target),
            TargetKind::WorkingTree,
            built.edits,
        ));
        prop_assert!(out.chains.iter().any(|c| c.class == ChainClass::ExactFromBase));
        for (line, expected) in built.expected.iter().enumerate() {
            match &out.target[line] {
                LineOutcome::Agreed { provenance, .. } => {
                    prop_assert_eq!(&provenance.origins, &BTreeSet::from([expected.clone()]));
                }
                other @ LineOutcome::Ambiguous { .. } => {
                    prop_assert!(false, "line {} is {:?}", line, other);
                }
            }
        }
    }

    /// (b) With one observable unrecorded line, nothing is exact from the
    /// base and that line is never attributed to a recorded edit.
    #[test]
    fn soundness(
        (base_present, size, ops) in restricted(),
        after in any::<u16>(),
        pick in any::<u16>(),
    ) {
        let built = build(base_present, size, &ops, Some((after, pick)));
        let line = built.unrecorded.expect("the interruption survives");
        let out = attribute(request(
            built.base.as_deref(),
            Some(&built.target),
            TargetKind::WorkingTree,
            built.edits,
        ));
        prop_assert!(out.chains.iter().all(|c| c.class != ChainClass::ExactFromBase));
        let claims: Vec<&BTreeSet<Origin>> = match &out.target[line] {
            LineOutcome::Agreed { provenance, .. } => vec![&provenance.origins],
            LineOutcome::Ambiguous { readings } => readings.iter().map(|r| &r.value.origins).collect(),
        };
        for origins in claims {
            prop_assert!(!origins.iter().any(|o| matches!(o, Origin::Introduced(_))));
        }
    }
}

// ---------------------------------------------------------------------------
// Unrestricted histories: duplicates, whitespace, replace-all, writes,
// appends, unknown operations, two transcripts and worktrees, equal ordinals
// ---------------------------------------------------------------------------

// "\ta\n" and "  a\n" look the same in a patch.
const ALPHABET: [&str; 7] = ["a\n", "b\n", "a b\n", "ab\n", "\n", "  a\n", "\ta\n"];

#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent generated choice"
)]
struct OpChoice {
    kind: u8,
    start: u16,
    len: u8,
    replacement: Vec<u8>,
    transcript: bool,
    same_ordinal: bool,
    known: bool,
    patched: bool,
}

fn op_choice() -> impl Strategy<Value = OpChoice> {
    (
        0u8..6,
        any::<u16>(),
        1u8..6,
        prop::collection::vec(0u8..7, 0..3),
        any::<bool>(),
        prop::bool::weighted(0.2),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(kind, start, len, replacement, transcript, same_ordinal, known, patched)| OpChoice {
                kind,
                start,
                len,
                replacement,
                transcript,
                same_ordinal,
                known,
                patched,
            },
        )
}

struct Unrestricted {
    base: Option<String>,
    target: Option<String>,
    edits: Vec<CandidateEdit>,
    kind: TargetKind,
}

fn build_unrestricted(
    base: Option<Vec<u8>>,
    ops: &[OpChoice],
    second_worktree: bool,
    commit: bool,
) -> Unrestricted {
    let lines =
        |picks: &[u8]| -> String { picks.iter().map(|&i| ALPHABET[usize::from(i)]).collect() };
    let base = base.map(|picks| lines(&picks));
    let mut current: Option<String> = base.clone();
    let mut edits = Vec::new();
    let mut ordinals: BTreeMap<bool, u64> = BTreeMap::new();
    for (k, op) in ops.iter().enumerate() {
        let id = format!("op{k}");
        let transcript = if op.transcript { "t2" } else { "t1" };
        let replacement = lines(&op.replacement);
        let edit = match (op.kind, current.as_deref()) {
            (5, None) => append_creating(&id, transcript, 0, &replacement),
            (_, None) => create(&id, transcript, 0, &replacement),
            (0 | 1 | 4, Some(text)) if !text.is_empty() => {
                let start = usize::from(op.start) % text.len();
                let end = (start + usize::from(op.len)).min(text.len());
                let Some(old) = text.get(start..end) else {
                    continue;
                };
                match op.kind {
                    0 => replace(&id, transcript, 0, old, &replacement),
                    1 => replace_all(&id, transcript, 0, old, &replacement),
                    _ => unknown_op(&id, transcript, 0, old, &replacement),
                }
            }
            (2, Some(_)) => overwrite(&id, transcript, 0, &replacement),
            (5, Some(_)) => append(&id, transcript, 0, &replacement),
            _ => continue,
        };
        let before = current.as_deref().map(str::as_bytes);
        let after = if edit.operation == EditOperation::Unknown {
            // An unknown operation still happened: apply it as a replacement.
            let mut as_replace = edit.clone();
            as_replace.operation = EditOperation::Replace;
            oracle_forward(&as_replace, before)
        } else {
            oracle_forward(&edit, before)
        };
        let Some(after) = after else { continue };
        let after = String::from_utf8(after).unwrap();
        let ordinal = ordinals.entry(op.transcript).or_insert(0);
        if !op.same_ordinal || *ordinal == 0 {
            *ordinal += 1;
        }
        let mut edit = edit;
        edit.source_ordinal = *ordinal;
        if op.known && edit.operation != EditOperation::Write {
            edit.before = current.as_deref().map_or(FileState::Absent, |b| {
                FileState::known(ContentId::of(b.as_bytes()))
            });
            edit.after = FileState::known(ContentId::of(after.as_bytes()));
        }
        // Claude Code records no patch for a file a Write creates.
        if let (true, Some(before)) = (op.patched, current.as_deref()) {
            edit = with_patch(edit, before, &after);
        }
        let worktree = if second_worktree && op.transcript {
            "/work/w1"
        } else {
            WT
        };
        edits.push(in_worktree(edit, worktree));
        current = Some(after);
    }
    Unrestricted {
        base,
        target: current,
        edits,
        kind: if commit {
            TargetKind::Commit
        } else {
            TargetKind::WorkingTree
        },
    }
}

fn unrestricted() -> impl Strategy<Value = (Option<Vec<u8>>, Vec<OpChoice>, bool, bool)> {
    (
        prop::option::of(prop::collection::vec(0u8..7, 0..5)),
        prop::collection::vec(op_choice(), 1..5),
        any::<bool>(),
        any::<bool>(),
    )
}

fn edit_ids(chain: &WalkedChain) -> BTreeSet<EventId> {
    chain.chain.edits.iter().cloned().collect()
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// (c) Everything anchor reports is justified by its own evidence.
    #[test]
    fn justification((base, ops, second, commit) in unrestricted()) {
        let built = build_unrestricted(base, &ops, second, commit);
        let req = request(built.base.as_deref(), built.target.as_deref(), built.kind, built.edits);
        let by_id: BTreeMap<EventId, Edit> = req
            .edits
            .iter()
            .map(|c| (c.edit.id.clone(), c.edit.clone()))
            .collect();
        let walked = walk(&req);
        for chain in walked.chains.iter().chain(&walked.set_aside) {
            for step in &chain.steps {
                let edit = &by_id[&step.edit];
                let forward = oracle_forward(edit, step.before.bytes());
                prop_assert_eq!(forward.as_deref(), step.after.bytes());
                prop_assert!(hash_matches(&edit.before, step.before.bytes()));
                prop_assert!(hash_matches(&edit.after, step.after.bytes()));
            }
            let from_base = chain.chain.end == ChainEnd::Base;
            prop_assert_eq!(from_base, chain.chain.class == ChainClass::ExactFromBase);
            if from_base {
                prop_assert_eq!(&chain.start, &req.base);
                let mut bytes = req.base.bytes().map(<[u8]>::to_vec);
                for step in &chain.steps {
                    bytes = oracle_forward(&by_id[&step.edit], bytes.as_deref());
                }
                prop_assert_eq!(bytes.as_deref(), req.target.bytes());
            }
        }
        for aside in &walked.set_aside {
            prop_assert!(aside.chain.end != ChainEnd::Base);
            let covered = walked.chains.iter().any(|c| {
                c.chain.end == ChainEnd::Base && edit_ids(aside).is_subset(&edit_ids(c))
            });
            prop_assert!(covered, "set aside without a covering chain: {:?}", aside.chain);
        }
        let base_lines = common::line_count(&req.base);
        let mut budget = Budget::new(DEFAULT_BUDGET_BYTES);
        let replayed: Vec<_> = walked
            .chains
            .iter()
            .map(|c| replay(c, base_lines, &mut budget).expect("replay fits the budget"))
            .collect();
        let out = attribute(req);
        prop_assert_eq!(out.chains.len(), walked.chains.len());
        for (line, outcome) in out.target.iter().enumerate() {
            // Agreement is on the full provenance: origins and contributors.
            let all: Vec<&Provenance> = replayed.iter().map(|p| &p.target[line]).collect();
            let agree = all.windows(2).all(|w| w[0] == w[1]);
            match outcome {
                LineOutcome::Agreed { provenance, .. } => {
                    prop_assert!(agree);
                    if let Some(first) = all.first() {
                        prop_assert_eq!(provenance, *first);
                    }
                }
                LineOutcome::Ambiguous { .. } => prop_assert!(!agree),
            }
        }
    }
}

#[test]
fn the_oracle_replaces_the_first_occurrence_like_the_recorder() {
    let e = replace("e", "t", 1, "x", "y");
    assert_eq!(
        oracle_forward(&e, Some(b"x\nx\n")),
        Some(b"y\nx\n".to_vec())
    );
    let all = replace_all("e", "t", 1, "a", "aa");
    assert_eq!(oracle_forward(&all, Some(b"aba")), Some(b"aabaa".to_vec()));
    let added = append("e", "t", 1, "c\n");
    assert_eq!(oracle_forward(&added, Some(b"a")), Some(b"ac\n".to_vec()));
    assert_eq!(oracle_forward(&added, None), Some(b"c\n".to_vec()));
    assert_eq!(oracle_forward(&append("e", "t", 1, ""), Some(b"a")), None);
    assert_eq!(state(Some("a")), State::Present(b"a".to_vec()));
}
