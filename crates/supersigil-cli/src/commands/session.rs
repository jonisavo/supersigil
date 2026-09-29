//! `supersigil session`: sync transcripts into the record and inspect it.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_record::observations::Observation;
use supersigil_record::store::Store;
use supersigil_record::{DerivationSet, Revision, SessionId};
use supersigil_session::discover::discover_transcripts;
use supersigil_session::sync::{SyncReport, sync};

use crate::commands::{
    SessionArgs, SessionCommand, SessionListArgs, SessionShowArgs, SessionSyncArgs,
};
use crate::error::CliError;
use crate::format::{ColorConfig, OutputFormat, escape_control, hint, write_json};
use crate::record_dir;

/// One row of `session list`.
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
                checkout.display()
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

/// Prints a sync report. Transcript paths, session ids, and record type
/// names come from outside the tool and are escaped.
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
            escape_control(&t.path.display().to_string()),
            t.new_observations,
            escape_control(session)
        )?;
        if t.trailing_partial {
            writeln!(out, "    incomplete final line left for the next sync")?;
        }
        if !t.unknown_records.is_empty() {
            let list: Vec<String> = t
                .unknown_records
                .iter()
                .map(|(k, v)| format!("{} ({v})", escape_control(k)))
                .collect();
            writeln!(out, "    unknown record types: {}", list.join(", "))?;
        }
        if t.malformed_lines > 0 {
            writeln!(out, "    malformed lines skipped: {}", t.malformed_lines)?;
        }
        if t.abandoned_tool_uses > 0 {
            writeln!(
                out,
                "    tool uses without a result: {}",
                t.abandoned_tool_uses
            )?;
        }
        if t.failed_tool_uses > 0 {
            writeln!(
                out,
                "    failed editing tool uses, not recorded as edits: {}",
                t.failed_tool_uses
            )?;
        }
        if t.outside_checkout > 0 {
            writeln!(
                out,
                "    edits outside the checkout dropped: {}",
                t.outside_checkout
            )?;
        }
    }
    Ok(())
}

fn summarize(store: &Store) -> Result<(u64, Vec<SessionSummary>), CliError> {
    let snapshot = store.snapshot()?;
    let mut rows = Vec::new();
    for session in snapshot.sessions() {
        let observations = snapshot.observations(&session)?;
        let derivations = snapshot.derivations(&session)?;
        let start = observations.iter().find_map(|o| match o {
            Observation::SessionStart(s) => Some(s),
            _ => None,
        });
        rows.push(SessionSummary {
            started: start.map(|s| s.time.as_str().to_owned()),
            branch: start.and_then(|s| s.branch.clone()),
            checkout: start.map(|s| s.checkout.clone()),
            turns: observations
                .iter()
                .filter(|o| matches!(o, Observation::Turn(_)))
                .count(),
            edits: observations
                .iter()
                .filter(|o| matches!(o, Observation::Edit(_)))
                .count(),
            commands: observations
                .iter()
                .filter(|o| matches!(o, Observation::Command(_)))
                .count(),
            restores: derivations.as_ref().map_or(0, |d| d.restores.len()),
            discontinuities: derivations.as_ref().map_or(0, |d| d.discontinuities.len()),
            session,
        });
    }
    Ok((snapshot.revision().get(), rows))
}

fn run_list(
    args: &SessionListArgs,
    records_dir: &Path,
    checkout: &Path,
    color: ColorConfig,
) -> Result<(), CliError> {
    let Some(store) = record_dir::find_record(records_dir, checkout)? else {
        let message = format!(
            "no record for {}; run `supersigil session sync` first",
            checkout.display()
        );
        match args.format {
            OutputFormat::Json => {
                hint(color, &message);
                write_json(&Vec::<SessionSummary>::new())?;
            }
            OutputFormat::Terminal => println!("{message}"),
        }
        return Ok(());
    };
    let (_, rows) = summarize(&store)?;
    match args.format {
        OutputFormat::Json => write_json(&rows)?,
        OutputFormat::Terminal => {
            // Session ids, times, and branches come from transcripts.
            let mut out = io::stdout().lock();
            for row in &rows {
                writeln!(
                    out,
                    "{}  {}  {}  {} turns, {} edits, {} commands, {} restores, {} discontinuities",
                    escape_control(row.session.as_str()),
                    escape_control(row.started.as_deref().unwrap_or("-")),
                    escape_control(row.branch.as_deref().unwrap_or("-")),
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
        return Err(CliError::CommandFailed(format!(
            "no record for {}; run `supersigil session sync` first",
            checkout.display()
        )));
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
            let ids: Vec<String> = many.iter().map(|id| escape_control(id.as_str())).collect();
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
