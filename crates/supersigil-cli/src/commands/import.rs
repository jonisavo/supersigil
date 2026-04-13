use std::io::{self, Write};
use std::path::PathBuf;

use supersigil_import::{
    AmbiguityBreakdown, Diagnostic, ImportConfig, ImportSummary, import_kiro, plan_kiro_import,
};

use crate::commands::{ImportArgs, ImportSource};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus, Token};

/// Run the import command, reading Kiro specs and writing spec documents.
///
/// # Errors
///
/// Returns `CliError` on I/O failure or import errors.
pub fn run(args: &ImportArgs, color: ColorConfig) -> Result<ExitStatus, CliError> {
    match args.from {
        ImportSource::Kiro => {
            if args.check {
                run_kiro_check(args, color)
            } else {
                run_kiro_import(args, color)
            }
        }
    }
}

fn run_kiro_import(args: &ImportArgs, color: ColorConfig) -> Result<ExitStatus, CliError> {
    let kiro_specs_dir = args
        .source_dir
        .clone()
        .or_else(|| std::env::var_os("SUPERSIGIL_IMPORT_SOURCE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(".kiro/specs"));
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("specs"));

    let config = ImportConfig {
        kiro_specs_dir,
        output_dir,
        id_prefix: args.prefix.clone(),
        force: args.force,
    };

    if args.dry_run {
        let plan = plan_kiro_import(&config)?;
        print_diagnostics(&plan.diagnostics)?;

        let stdout = io::stdout();
        let mut out = stdout.lock();
        writeln!(
            out,
            "Dry run: {} documents planned",
            color.paint(Token::Count, &plan.documents.len().to_string()),
        )?;
        for doc in &plan.documents {
            let path_str = doc.output_path.display().to_string();
            writeln!(
                out,
                "  {} -> {}",
                color.paint(Token::DocId, &doc.document_id),
                color.paint(Token::Path, &path_str),
            )?;
        }
        print_summary(&mut out, &plan.summary, &plan.ambiguity_breakdown, color)?;
    } else {
        let result = import_kiro(&config)?;
        print_diagnostics(&result.diagnostics)?;

        let stdout = io::stdout();
        let mut out = stdout.lock();
        writeln!(
            out,
            "Imported {} files",
            color.paint(Token::Count, &result.files_written.len().to_string()),
        )?;
        for file in &result.files_written {
            let path_str = file.path.display().to_string();
            writeln!(
                out,
                "  {} -> {}",
                color.paint(Token::DocId, &file.document_id),
                color.paint(Token::Path, &path_str),
            )?;
        }
        print_summary(
            &mut out,
            &result.summary,
            &result.ambiguity_breakdown,
            color,
        )?;

        if std::path::Path::new(supersigil_core::CONFIG_FILENAME).exists() {
            format::hint(
                color,
                "Run `supersigil verify` to validate imported documents.",
            );
        } else {
            format::hint(
                color,
                "Run `supersigil init` to create a config, then `supersigil verify`.",
            );
        }
    }

    Ok(ExitStatus::Success)
}

fn run_kiro_check(args: &ImportArgs, color: ColorConfig) -> Result<ExitStatus, CliError> {
    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("specs"));

    let result = supersigil_import::check::check_markers(&output_dir)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let total = result.breakdown.total();

    if total == 0 {
        writeln!(
            out,
            "{} 0 import TODOs remaining",
            color.paint(Token::Success, "ok:"),
        )?;
        return Ok(ExitStatus::Success);
    }

    writeln!(
        out,
        "{} import TODOs remaining\n",
        color.paint(Token::Warning, &total.to_string()),
    )?;

    for marker in &result.markers {
        let path_str = marker.file.display().to_string();
        writeln!(
            out,
            "  {}:{} — {}",
            color.paint(Token::Path, &path_str),
            marker.line,
            marker.message,
        )?;
    }

    writeln!(out)?;
    writeln!(out, "Breakdown:")?;
    for (name, count) in result.breakdown.iter_named() {
        if count > 0 {
            writeln!(out, "  {name}: {count}")?;
        }
    }

    Ok(ExitStatus::VerifyFailed)
}

fn print_diagnostics(diagnostics: &[Diagnostic]) -> io::Result<()> {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    for diag in diagnostics {
        writeln!(err, "{diag}")?;
    }
    Ok(())
}

fn print_summary(
    out: &mut impl Write,
    summary: &ImportSummary,
    breakdown: &AmbiguityBreakdown,
    color: ColorConfig,
) -> io::Result<()> {
    writeln!(out, "\nSummary:")?;
    writeln!(out, "  features_processed: {}", summary.features_processed)?;
    writeln!(out, "  criteria_converted: {}", summary.criteria_converted)?;
    writeln!(out, "  validates_resolved: {}", summary.validates_resolved)?;
    writeln!(out, "  tasks_converted: {}", summary.tasks_converted)?;
    let total = breakdown.total();
    if total > 0 {
        writeln!(
            out,
            "  ambiguities: {}",
            color.paint(Token::Warning, &format!("{total} total")),
        )?;
        for (name, count) in breakdown.iter_named() {
            if count > 0 {
                writeln!(out, "    {name}: {count}")?;
            }
        }
    } else {
        writeln!(out, "  ambiguities: 0")?;
    }
    Ok(())
}
