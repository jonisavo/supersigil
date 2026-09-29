//! Incremental sync of the slice fixture into a record.

mod common;

use std::path::{Path, PathBuf};

use common::{SESSION, fixture, line_starts};

use supersigil_record::observations::{
    CaptureLimitation, Observation, Role, SessionStart, Source, session_start,
};
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
    assert_eq!(report.new_observations, 25);
    assert_eq!(report.transcripts.len(), 1);
    assert!(!report.transcripts[0].trailing_partial);
    assert!(report.transcripts[0].counts.is_empty());
    assert_eq!(report.transcripts[0].counts.failed_tool_uses, 0);

    let snapshot = s.store.snapshot().unwrap();
    let observations = snapshot.observations(&SessionId::new(SESSION)).unwrap();
    assert_eq!(observations.len(), 25);
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
    // The plain fixture loses nothing, so it records no limitation.
    let plain = setup(&fixture());
    sync(
        &plain.store,
        &plain.checkout,
        std::slice::from_ref(&plain.transcript),
    )
    .unwrap();
    assert!(limitations(&plain.store).is_empty());

    let mut bytes = fixture();
    bytes.extend_from_slice(
        format!("{{\"type\":\"totally-new\",\"sessionId\":\"{SESSION}\"}}\n").as_bytes(),
    );
    let s = setup(&bytes);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let recorded = limitations(&s.store);
    assert_eq!(recorded.len(), 1);
    let limitation = &recorded[0];
    assert_eq!(limitation.session, SessionId::new(SESSION));
    assert_eq!(limitation.transcript, "slice.jsonl");
    assert_eq!(limitation.from_ordinal, 0);
    assert_eq!(limitation.to_ordinal, 20);
    assert_eq!(
        limitation.counts.unknown_records.get("totally-new"),
        Some(&1)
    );
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

#[test]
fn a_transcript_of_only_unknown_records_keeps_its_capture_limitation() {
    let bytes = b"{\"type\":\"totally-new\",\"sessionId\":\"s\"}\n";
    let s = setup(bytes);
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.transcripts[0].session, Some(SessionId::new("s")));
    let observations = s
        .store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new("s"))
        .unwrap();
    let recorded: Vec<&CaptureLimitation> = observations
        .iter()
        .filter_map(|o| match o {
            Observation::CaptureLimitation(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].counts.unknown_records.get("totally-new"),
        Some(&1)
    );
    assert_eq!(recorded[0].from_ordinal, 0);
    assert_eq!(recorded[0].to_ordinal, 1);
}

#[test]
fn an_empty_session_id_does_not_block_sync() {
    let bytes = concat!(
        r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"hi"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
        "\n",
    );
    let s = setup(bytes.as_bytes());
    let key = s.transcript.display().to_string();
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.transcripts[0].session, None);
    assert_eq!(
        report.transcripts[0]
            .counts
            .unknown_records
            .get("no-session"),
        Some(&2)
    );
    let manifest = s.store.manifest().unwrap();
    assert_eq!(manifest.cursors[&key].offset, bytes.len() as u64);
    assert!(manifest.logs.is_empty());
}

/// A subagent transcript of the fixture's session: one `Write` and its result.
fn subagent_transcript() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"assistant","uuid":"sa1","parentUuid":null,"sessionId":"{SESSION}","agentId":"agent1","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:45.000Z","isSidechain":true,"message":{{"id":"msg_side","role":"assistant","content":[{{"type":"tool_use","id":"toolu_side","name":"Write","input":{{"file_path":"/work/repo/src/side.rs","content":"pub fn side() {{}}\n"}}}}]}}}}"#
        ),
        format_args!(
            r#"{{"type":"user","uuid":"su1","parentUuid":"sa1","sessionId":"{SESSION}","agentId":"agent1","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:46.000Z","isSidechain":true,"isMeta":false,"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_side","content":"ok"}}]}},"toolUseResult":{{"type":"create","filePath":"/work/repo/src/side.rs","content":"pub fn side() {{}}\n","structuredPatch":[]}}}}"#
        ),
    )
}

#[test]
fn subagent_transcript_joins_the_parent_session() {
    for main_first in [true, false] {
        let s = setup(&fixture());
        let side = s.transcript.with_file_name("agent-agent1.jsonl");
        std::fs::write(&side, subagent_transcript()).unwrap();
        let order = if main_first {
            vec![s.transcript.clone(), side.clone()]
        } else {
            vec![side.clone(), s.transcript.clone()]
        };
        let report = sync(&s.store, &s.checkout, &order).unwrap();
        assert_eq!(report.sessions, vec![SessionId::new(SESSION)], "{order:?}");

        let snapshot = s.store.snapshot().unwrap();
        assert_eq!(snapshot.sessions(), vec![SessionId::new(SESSION)]);
        let observations = snapshot.observations(&SessionId::new(SESSION)).unwrap();
        // Each transcript records where it saw the session start.
        let starts: Vec<&SessionStart> = observations
            .iter()
            .filter_map(|o| match o {
                Observation::SessionStart(start) => Some(start),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 2, "{order:?}");
        assert_eq!(
            starts.iter().filter(|start| start.sidechain).count(),
            1,
            "{order:?}"
        );
        // Whatever the order, the session starts where the main transcript
        // says it does.
        let start = session_start(&observations).unwrap();
        assert!(!start.sidechain, "{order:?}");
        assert_eq!(start.time.as_str(), "2026-09-28T10:00:00.000Z", "{order:?}");
        assert_eq!(start.branch.as_deref(), Some("main"), "{order:?}");
        assert_eq!(start.checkout, PathBuf::from("/work/repo"), "{order:?}");
        assert_eq!(
            start.source_ids.get("transcript").map(String::as_str),
            Some("slice.jsonl"),
            "{order:?}"
        );
        let edits: Vec<_> = observations
            .iter()
            .filter_map(|o| match o {
                Observation::Edit(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(edits.len(), 6, "{order:?}");
        let side_edit = edits
            .iter()
            .find(|e| e.path == Path::new("src/side.rs"))
            .unwrap();
        assert_eq!(side_edit.agent_id.as_deref(), Some("agent1"));
        assert_eq!(side_edit.transcript.as_deref(), Some("agent-agent1.jsonl"));
        assert!(
            edits
                .iter()
                .filter(|e| e.path != Path::new("src/side.rs"))
                .all(|e| e.agent_id.is_none() && e.transcript.as_deref() == Some("slice.jsonl"))
        );
        let side_turns: Vec<_> = observations
            .iter()
            .filter_map(|o| match o {
                Observation::Turn(t) if t.agent_id.as_deref() == Some("agent1") => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(side_turns.len(), 2, "{order:?}");
        assert!(
            side_turns
                .iter()
                .all(|t| t.sidechain && t.transcript.as_deref() == Some("agent-agent1.jsonl"))
        );
        assert!(observations.iter().all(|o| match o {
            Observation::Command(c) => c.transcript.as_deref() == Some("slice.jsonl"),
            _ => true,
        }));
        assert!(
            observations
                .iter()
                .filter_map(|o| match o {
                    Observation::Turn(t) if t.agent_id.is_none() => Some(t),
                    _ => None,
                })
                .all(|t| !t.sidechain)
        );
        let side_cursor = &snapshot.manifest().cursors[&side.display().to_string()];
        assert_eq!(side_cursor.session, Some(SessionId::new(SESSION)));
    }
}

#[test]
fn a_later_main_transcript_takes_over_the_session_start() {
    let s = setup(&fixture());
    let side = s.transcript.with_file_name("agent-agent1.jsonl");
    std::fs::write(&side, subagent_transcript()).unwrap();
    let session = SessionId::new(SESSION);
    let observations = |store: &Store| store.snapshot().unwrap().observations(&session).unwrap();
    let start_count = |observations: &[Observation]| {
        observations
            .iter()
            .filter(|o| matches!(o, Observation::SessionStart(_)))
            .count()
    };

    // Without the main transcript, the subagent's is the only evidence.
    sync(&s.store, &s.checkout, std::slice::from_ref(&side)).unwrap();
    let first = observations(&s.store);
    assert_eq!(start_count(&first), 1);
    let start = session_start(&first).unwrap();
    assert!(start.sidechain);
    assert_eq!(
        start.source_ids.get("transcript").map(String::as_str),
        Some("agent-agent1.jsonl")
    );
    assert_eq!(start.time.as_str(), "2026-09-28T10:00:45.000Z");

    // The main transcript arriving later records its own start, which the
    // read model then prefers.
    let report = sync(&s.store, &s.checkout, &[s.transcript.clone(), side]).unwrap();
    assert!(report.new_observations > 0);
    let second = observations(&s.store);
    assert_eq!(start_count(&second), 2);
    let start = session_start(&second).unwrap();
    assert!(!start.sidechain);
    assert_eq!(
        start.source_ids.get("transcript").map(String::as_str),
        Some("slice.jsonl")
    );
    assert_eq!(start.time.as_str(), "2026-09-28T10:00:00.000Z");
    assert_eq!(turn_ids(&s.store).len(), 19);
}
