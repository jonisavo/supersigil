//! Incremental sync of the slice fixture into a record.

mod common;

use std::path::PathBuf;

use common::{SESSION, fixture, line_starts};

use supersigil_record::observations::{CaptureLimitation, Observation, Role, Source};
use supersigil_record::store::{Association, SourceCursor, Store};
use supersigil_record::{ContentId, DerivationSet, EventId, SessionId};
use supersigil_session::sync::sync;

struct Setup {
    _dir: tempfile::TempDir,
    store: Store,
    transcript: PathBuf,
    checkout: PathBuf,
}

fn setup(bytes: &[u8]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let checkout = PathBuf::from("/work/repo");
    let store = Store::create(
        &dir.path().join("record"),
        Association {
            checkout: checkout.clone(),
        },
    )
    .unwrap();
    let transcript = dir.path().join("slice.jsonl");
    std::fs::write(&transcript, bytes).unwrap();
    Setup {
        _dir: dir,
        store,
        transcript,
        checkout,
    }
}

fn turn_ids(store: &Store) -> Vec<String> {
    store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .iter()
        .filter_map(|o| match o {
            Observation::Turn(t) => Some(t.id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn derivations(store: &Store) -> DerivationSet {
    store
        .snapshot()
        .unwrap()
        .derivations(&SessionId::new(SESSION))
        .unwrap()
        .unwrap()
}

fn limitations(store: &Store) -> Vec<CaptureLimitation> {
    store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .into_iter()
        .filter_map(|o| match o {
            Observation::CaptureLimitation(l) => Some(l),
            _ => None,
        })
        .collect()
}

fn command_count(store: &Store) -> usize {
    store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .iter()
        .filter(|o| matches!(o, Observation::Command(_)))
        .count()
}

#[test]
fn sync_writes_session_start_observations_and_derivations() {
    let s = setup(&fixture());
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.revision.get(), 1);
    assert_eq!(report.sessions, vec![SessionId::new(SESSION)]);
    assert_eq!(report.new_observations, 26);
    assert_eq!(report.transcripts.len(), 1);
    assert!(!report.transcripts[0].trailing_partial);
    assert_eq!(
        report.transcripts[0].counts.unknown_records.get("ai-title"),
        Some(&1)
    );
    assert_eq!(report.transcripts[0].counts.failed_tool_uses, 0);

    let snapshot = s.store.snapshot().unwrap();
    let observations = snapshot.observations(&SessionId::new(SESSION)).unwrap();
    assert_eq!(observations.len(), 26);
    let Observation::SessionStart(start) = &observations[0] else {
        panic!("first observation must be the session start");
    };
    assert_eq!(start.source, Source::ClaudeCode);
    assert_eq!(start.checkout, PathBuf::from("/work/repo"));
    assert_eq!(start.branch.as_deref(), Some("main"));
    assert_eq!(start.time.as_str(), "2026-09-28T10:00:00.000Z");
    assert_eq!(
        start.source_ids.get("transcript").map(String::as_str),
        Some("slice.jsonl")
    );
    assert!(matches!(&observations[1], Observation::Turn(t) if t.role == Role::Human));

    let derivations = snapshot
        .derivations(&SessionId::new(SESSION))
        .unwrap()
        .unwrap();
    let session = SessionId::new(SESSION);
    assert_eq!(derivations.observation_revision.get(), 1);
    assert_eq!(derivations.restores.len(), 1);
    assert_eq!(
        derivations.restores[0].edit,
        EventId::derive("edit", &session, "toolu_04")
    );
    assert_eq!(
        derivations.restores[0].restores,
        vec![EventId::derive("edit", &session, "toolu_03")]
    );
    assert_eq!(derivations.discontinuities.len(), 1);
    assert_eq!(
        derivations.discontinuities[0].path,
        PathBuf::from("src/lib.rs")
    );
    assert_eq!(
        derivations.discontinuities[0].prev,
        EventId::derive("edit", &session, "toolu_01")
    );
    assert_eq!(
        derivations.discontinuities[0].next,
        EventId::derive("edit", &session, "toolu_05")
    );

    let cursor = &snapshot.manifest().cursors[&s.transcript.display().to_string()];
    assert_eq!(cursor.offset, fixture().len() as u64);
    assert_eq!(cursor.next_ordinal, 19);
    assert_eq!(cursor.session, Some(session));
}

#[test]
fn sync_is_idempotent() {
    let s = setup(&fixture());
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let again = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(again.new_observations, 0);
    assert_eq!(again.revision.get(), 1);
    assert_eq!(s.store.manifest().unwrap().revision.get(), 1);
    assert_eq!(turn_ids(&s.store).len(), 17);
}

#[test]
fn sync_resumes_after_partial_line() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes[..starts[18] + 40]);
    let first = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert!(first.transcripts[0].trailing_partial);
    assert_eq!(first.transcripts[0].consumed, starts[18] as u64);
    assert_eq!(turn_ids(&s.store).len(), 16);

    std::fs::write(&s.transcript, &bytes).unwrap();
    let second = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(second.new_observations, 1);
    assert_eq!(second.revision.get(), 2);
    let ids = turn_ids(&s.store);
    assert_eq!(ids.len(), 17);
    assert_eq!(ids.last().map(String::as_str), Some("a8"));
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), 17);
    assert_eq!(
        s.store
            .snapshot()
            .unwrap()
            .derivations(&SessionId::new(SESSION))
            .unwrap()
            .unwrap()
            .observation_revision
            .get(),
        2
    );
}

#[test]
fn sync_waits_for_a_pending_tool_result() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes[..starts[15]]);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let cursor_offset =
        s.store.manifest().unwrap().cursors[&s.transcript.display().to_string()].offset;
    assert_eq!(cursor_offset, starts[14] as u64);
    assert_eq!(command_count(&s.store), 1);

    std::fs::write(&s.transcript, &bytes).unwrap();
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(command_count(&s.store), 2);
    let ids = turn_ids(&s.store);
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(ids.len(), 17);
    assert_eq!(unique.len(), 17);
}

#[test]
fn rewritten_shorter_transcript_is_read_from_the_start() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let before = derivations(&s.store);
    std::fs::write(&s.transcript, &bytes[..starts[3]]).unwrap();
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    // The three records are appended again under the same ids; the log is
    // append-only, so duplicates are visible and later layers dedupe by id.
    assert_eq!(report.transcripts[0].consumed, starts[3] as u64);
    assert_eq!(
        s.store.manifest().unwrap().cursors[&s.transcript.display().to_string()].offset,
        starts[3] as u64
    );
    let after = derivations(&s.store);
    assert_eq!(after.observation_revision.get(), 2);
    assert_eq!(after.restores, before.restores);
    assert_eq!(after.discontinuities, before.discontinuities);
    assert_eq!(after.restores.len(), 1);
    assert_eq!(after.discontinuities.len(), 1);
    assert!(after.discontinuities.iter().all(|d| d.prev != d.next));
}

#[test]
fn missing_transcript_is_an_io_error() {
    let s = setup(&fixture());
    let missing = s.transcript.with_file_name("missing.jsonl");
    assert!(matches!(
        sync(&s.store, &s.checkout, &[missing]),
        Err(supersigil_session::sync::SyncError::Io { .. })
    ));
}

#[test]
fn cursor_only_progress_is_committed() {
    let bytes = b"{\"type\":\"ai-title\",\"sessionId\":\"x\",\"title\":\"t\"}\n{not json\n";
    let s = setup(bytes);
    let key = s.transcript.display().to_string();
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let manifest = s.store.manifest().unwrap();
    assert_eq!(manifest.revision.get(), 1);
    assert_eq!(manifest.cursors[&key].offset, bytes.len() as u64);
    assert_eq!(manifest.cursors[&key].next_ordinal, 2);

    let again = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(again.new_observations, 0);
    assert_eq!(again.revision.get(), 1);
    assert_eq!(s.store.manifest().unwrap().revision.get(), 1);
    assert!(again.transcripts[0].counts.unknown_records.is_empty());
}

#[test]
fn appended_records_without_a_session_id_join_the_known_session() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes[..starts[18]]);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(turn_ids(&s.store).len(), 16);

    let last = String::from_utf8(bytes[starts[18]..].to_vec())
        .unwrap()
        .replace(&format!(r#""sessionId":"{SESSION}","#), "");
    let mut appended = bytes[..starts[18]].to_vec();
    appended.extend_from_slice(last.as_bytes());
    std::fs::write(&s.transcript, &appended).unwrap();
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.new_observations, 1);
    assert!(report.transcripts[0].counts.unknown_records.is_empty());
    assert_eq!(turn_ids(&s.store).last().map(String::as_str), Some("a8"));
}

#[test]
fn capture_limitations_are_recorded_with_the_cursor_advance() {
    let s = setup(&fixture());
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let recorded = limitations(&s.store);
    assert_eq!(recorded.len(), 1);
    let limitation = &recorded[0];
    assert_eq!(limitation.session, SessionId::new(SESSION));
    assert_eq!(limitation.transcript, "slice.jsonl");
    assert_eq!(limitation.from_ordinal, 0);
    assert_eq!(limitation.to_ordinal, 19);
    assert_eq!(limitation.counts.unknown_records.get("ai-title"), Some(&1));
    assert_eq!(limitation.counts.malformed_lines, 0);
    // It is the last observation of the call, after the parsed ones.
    let observations = s
        .store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap();
    assert!(matches!(
        observations.last(),
        Some(Observation::CaptureLimitation(_))
    ));

    let again = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(again.new_observations, 0);
    assert_eq!(limitations(&s.store).len(), 1);
}

#[test]
fn same_length_rewrite_is_read_from_the_start() {
    let bytes = fixture();
    let s = setup(&bytes);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let key = s.transcript.display().to_string();
    assert_eq!(
        s.store.manifest().unwrap().cursors[&key].prefix_hash,
        Some(ContentId::of(&bytes))
    );

    let rewritten = String::from_utf8(bytes.clone()).unwrap().replacen(
        "Add a greeting function",
        "Add a greeting FUNCTION",
        1,
    );
    assert_eq!(rewritten.len(), bytes.len());
    assert_ne!(rewritten.as_bytes(), bytes.as_slice());
    std::fs::write(&s.transcript, &rewritten).unwrap();
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.transcripts[0].consumed, bytes.len() as u64);
    assert!(report.new_observations > 0);
    let cursor = &s.store.manifest().unwrap().cursors[&key];
    assert_eq!(cursor.offset, bytes.len() as u64);
    assert_eq!(
        cursor.prefix_hash,
        Some(ContentId::of(rewritten.as_bytes()))
    );
    let human_texts: Vec<String> = s
        .store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .into_iter()
        .filter_map(|o| match o {
            Observation::Turn(t) if t.role == Role::Human => t.excerpt.retained().cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(human_texts.len(), 2);
    assert!(human_texts[1].contains("FUNCTION"));
}

#[test]
fn cursor_offset_without_a_prefix_hash_is_read_from_the_start() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes);
    let key = s.transcript.display().to_string();
    let mut tx = s.store.begin().unwrap();
    tx.set_cursor(
        &key,
        SourceCursor {
            offset: starts[3] as u64,
            next_ordinal: 3,
            session: Some(SessionId::new(SESSION)),
            prefix_hash: None,
        },
    );
    tx.commit().unwrap();

    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.transcripts[0].consumed, bytes.len() as u64);
    assert_eq!(turn_ids(&s.store).len(), 17);
    let cursor = &s.store.manifest().unwrap().cursors[&key];
    assert_eq!(cursor.offset, bytes.len() as u64);
    assert_eq!(cursor.next_ordinal, 19);
    assert_eq!(cursor.prefix_hash, Some(ContentId::of(&bytes)));
}
