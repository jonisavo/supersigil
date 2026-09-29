//! `supersigil session`: sync transcripts into the record and inspect it.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_record::observations::{Observation, session_start};
use supersigil_record::store::Store;
use supersigil_record::{DerivationSet, Revision, SessionId};
use supersigil_session::discover::discover_transcripts;
use supersigil_session::sync::{SyncReport, sync};

use crate::commands::{
    SessionArgs, SessionCommand, SessionListArgs, SessionShowArgs, SessionSyncArgs,
};
use crate::error::CliError;
use crate::format::{ColorConfig, OutputFormat, Untrusted, hint, write_json};
use crate::record_dir;

/// One row of `session list`; `started`, `branch`, and `checkout` come from
/// the start [`session_start`] selects.
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    /// Session id.
    pub session: SessionId,
    /// Time of the session start, if recorded.
    pub started: Option<String>,
    /// Branch at session start.
    pub branch: Option<String>,
    /// Checkout the session worked in.
    pub checkout: Option<PathBuf>,
    /// Number of turns.
    pub turns: usize,
    /// Number of edits.
    pub edits: usize,
    /// Number of commands.
    pub commands: usize,
    /// Number of derived restores.
    pub restores: usize,
    /// Number of derived discontinuities.
    pub discontinuities: usize,
}

#[derive(Debug, Serialize)]
struct ShowOutput {
    session: SessionId,
    revision: u64,
    observations: Vec<Observation>,
    derivations: Option<DerivationSet>,
}

/// Runs the `session` command group.
///
/// # Errors
///
/// Returns `CliError` if the record cannot be located, read, or written.
pub fn run(args: &SessionArgs, color: ColorConfig) -> Result<(), CliError> {
    let records_dir = record_dir::resolve_record_dir(args.record_dir.as_deref())?;
    let checkout = record_dir::canonical_checkout(args.checkout.as_deref())?;
    match &args.command {
        SessionCommand::Sync(sync_args) => run_sync(sync_args, &records_dir, &checkout, color),
        SessionCommand::List(list_args) => run_list(list_args, &records_dir, &checkout, color),
        SessionCommand::Show(show_args) => run_show(show_args, &records_dir, &checkout),
    }
}

fn claude_home(flag: Option<&Path>) -> Option<PathBuf> {
    flag.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}

fn run_sync(
    args: &SessionSyncArgs,
    records_dir: &Path,
    checkout: &Path,
    color: ColorConfig,
) -> Result<(), CliError> {
    let transcripts: Vec<PathBuf> = if args.transcripts.is_empty() {
        match claude_home(args.claude_home.as_deref()) {
            Some(home) => discover_transcripts(checkout, &home)?,
            None => Vec::new(),
        }
    } else {
        args.transcripts.clone()
    };
    if transcripts.is_empty() {
        hint(
            color,
            &format!(
                "no transcripts found for {}; pass --transcript <path> or --claude-home <dir>",
                Untrusted(&checkout.to_string_lossy())
            ),
        );
        if matches!(args.format, OutputFormat::Json) {
            let revision = match record_dir::find_record(records_dir, checkout)? {
                Some(store) => store.manifest()?.revision,
                None => Revision::ZERO,
            };
            write_json(&SyncReport {
                revision,
                sessions: Vec::new(),
                new_observations: 0,
                transcripts: Vec::new(),
            })?;
        }
        return Ok(());
    }
    let store = record_dir::open_or_create_record(records_dir, checkout)?;
    let report = sync(&store, checkout, &transcripts)?;
    match args.format {
        OutputFormat::Json => write_json(&report)?,
        OutputFormat::Terminal => print_sync_report(&report)?,
    }
    Ok(())
}

/// Prints a sync report.
fn print_sync_report(report: &SyncReport) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "synced {} observations from {} transcript(s) into revision {}",
        report.new_observations,
        report.transcripts.len(),
        report.revision.get()
    )?;
    for t in &report.transcripts {
        let session = t
            .session
            .as_ref()
            .map_or("unknown session", SessionId::as_str);
        writeln!(
            out,
            "  {}: {} new, {}",
            Untrusted(&t.path.to_string_lossy()),
            t.new_observations,
            Untrusted(session)
        )?;
        if let Some(reason) = &t.skipped {
            writeln!(out, "    skipped: {}", Untrusted(reason))?;
        }
        if let Some(nested) = &t.nested_checkout {
            writeln!(
                out,
                "    nested checkout: {}",
                Untrusted(&nested.to_string_lossy())
            )?;
        }
        if t.trailing_partial {
            writeln!(out, "    incomplete final line left for the next sync")?;
        }
        let counts = &t.counts;
        if !counts.unknown_records.is_empty() {
            let list: Vec<String> = counts
                .unknown_records
                .iter()
                .map(|(k, v)| format!("{} ({v})", Untrusted(k)))
                .collect();
            writeln!(out, "    unknown record types: {}", list.join(", "))?;
        }
        let lines = [
            ("malformed lines skipped", counts.malformed_lines),
            ("tool uses without a result", counts.abandoned_tool_uses),
            (
                "failed editing tool uses, not recorded as edits",
                counts.failed_tool_uses,
            ),
            (
                "edits and commands outside the checkout dropped",
                counts.outside_checkout,
            ),
            (
                "edits whose input and result name different files dropped",
                counts.conflicting_tool_results,
            ),
            (
                "tool results without a matching tool use dropped",
                counts.unmatched_tool_results,
            ),
            (
                "records of another session dropped",
                counts.session_mismatch,
            ),
            ("tool uses without an id dropped", counts.unnamed_tool_uses),
            (
                "unsupported editing tool uses, not recorded as edits",
                counts.unsupported_tool_uses,
            ),
        ];
        for (label, count) in lines {
            if count > 0 {
                writeln!(out, "    {label}: {count}")?;
            }
        }
    }
    Ok(())
}

/// One summary row per session in the record's current revision.
fn summarize(store: &Store) -> Result<Vec<SessionSummary>, CliError> {
    let snapshot = store.snapshot()?;
    let mut rows = Vec::new();
    for session in snapshot.sessions() {
        let observations = snapshot.observations(&session)?;
        let derivations = snapshot.derivations(&session)?;
        let mut row = SessionSummary {
            session,
            started: None,
            branch: None,
            checkout: None,
            turns: 0,
            edits: 0,
            commands: 0,
            restores: derivations.as_ref().map_or(0, |d| d.restores.len()),
            discontinuities: derivations.as_ref().map_or(0, |d| d.discontinuities.len()),
        };
        if let Some(start) = session_start(&observations) {
            row.started = Some(start.time.as_str().to_owned());
            row.branch.clone_from(&start.branch);
            row.checkout = Some(start.checkout.clone());
        }
        for observation in &observations {
            match observation {
                Observation::Turn(_) => row.turns += 1,
                Observation::Edit(_) => row.edits += 1,
                Observation::Command(_) => row.commands += 1,
                _ => {}
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

/// What to say when `checkout` has no record yet.
fn no_record_message(checkout: &Path) -> String {
    format!(
        "no record for {}; run `supersigil session sync` first",
        Untrusted(&checkout.to_string_lossy())
    )
}

fn run_list(
    args: &SessionListArgs,
    records_dir: &Path,
    checkout: &Path,
    color: ColorConfig,
) -> Result<(), CliError> {
    let Some(store) = record_dir::find_record(records_dir, checkout)? else {
        let message = no_record_message(checkout);
        match args.format {
            OutputFormat::Json => {
                hint(color, &message);
                write_json(&Vec::<SessionSummary>::new())?;
            }
            OutputFormat::Terminal => println!("{message}"),
        }
        return Ok(());
    };
    let rows = summarize(&store)?;
    match args.format {
        OutputFormat::Json => write_json(&rows)?,
        OutputFormat::Terminal => {
            let mut out = io::stdout().lock();
            for row in &rows {
                writeln!(
                    out,
                    "{}  {}  {}  {} turns, {} edits, {} commands, {} restores, {} discontinuities",
                    Untrusted(row.session.as_str()),
                    Untrusted(row.started.as_deref().unwrap_or("-")),
                    Untrusted(row.branch.as_deref().unwrap_or("-")),
                    row.turns,
                    row.edits,
                    row.commands,
                    row.restores,
                    row.discontinuities
                )?;
            }
        }
    }
    Ok(())
}

fn run_show(args: &SessionShowArgs, records_dir: &Path, checkout: &Path) -> Result<(), CliError> {
    let Some(store) = record_dir::find_record(records_dir, checkout)? else {
        return Err(CliError::CommandFailed(no_record_message(checkout)));
    };
    let snapshot = store.snapshot()?;
    let matches: Vec<SessionId> = snapshot
        .sessions()
        .into_iter()
        .filter(|s| s.as_str().starts_with(&args.session))
        .collect();
    let session = match matches.as_slice() {
        [one] => one.clone(),
        [] => {
            return Err(CliError::CommandFailed(format!(
                "no session matching '{}'",
                args.session
            )));
        }
        many => {
            let ids: Vec<&str> = many.iter().map(SessionId::as_str).collect();
            return Err(CliError::CommandFailed(format!(
                "'{}' matches several sessions: {}",
                args.session,
                ids.join(", ")
            )));
        }
    };
    let output = ShowOutput {
        revision: snapshot.revision().get(),
        observations: snapshot.observations(&session)?,
        derivations: snapshot.derivations(&session)?,
        session,
    };
    write_json(&output)?;
    Ok(())
}
