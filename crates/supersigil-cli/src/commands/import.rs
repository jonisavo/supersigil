use std::io::{self, Write};
use std::path::PathBuf;

use supersigil_import::{Diagnostic, ImportConfig, ImportSummary, import_kiro, plan_kiro_import};

use crate::commands::{ImportArgs, ImportSource};
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};

/// Run the import command, reading Kiro specs and writing spec documents.
///
/// # Errors
///
/// Returns `CliError` on I/O failure or import errors.
pub fn run(args: &ImportArgs, color: ColorConfig) -> Result<(), CliError> {
    match args.from {
        ImportSource::Kiro => run_kiro_import(args, color),
    }
}

fn run_kiro_import(args: &ImportArgs, color: ColorConfig) -> Result<(), CliError> {
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
        let c = color;
        writeln!(
            out,
            "Dry run: {} documents planned",
            c.paint(Token::Count, &plan.documents.len().to_string()),
        )?;
        for doc in &plan.documents {
            let path_str = doc.output_path.display().to_string();
            writeln!(
                out,
                "  {} -> {}",
                c.paint(Token::DocId, &doc.document_id),
                c.paint(Token::Path, &path_str),
            )?;
        }
        print_summary(&mut out, &plan.summary, plan.ambiguity_count)?;
    } else {
        let result = import_kiro(&config)?;
        print_diagnostics(&result.diagnostics)?;

        let stdout = io::stdout();
        let mut out = stdout.lock();
        let c = color;
        writeln!(
            out,
            "Imported {} files",
            c.paint(Token::Count, &result.files_written.len().to_string()),
        )?;
        for file in &result.files_written {
            let path_str = file.path.display().to_string();
            writeln!(
                out,
                "  {} -> {}",
                c.paint(Token::DocId, &file.document_id),
                c.paint(Token::Path, &path_str),
            )?;
        }
        print_summary(&mut out, &result.summary, result.ambiguity_count)?;

        // Next-step hint
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

    Ok(())
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
    ambiguity_count: usize,
) -> io::Result<()> {
    writeln!(out, "\nSummary:")?;
    writeln!(out, "  features_processed: {}", summary.features_processed)?;
    writeln!(out, "  criteria_converted: {}", summary.criteria_converted)?;
    writeln!(out, "  validates_resolved: {}", summary.validates_resolved)?;
    writeln!(out, "  tasks_converted: {}", summary.tasks_converted)?;
    writeln!(out, "  ambiguity_count: {ambiguity_count}")?;
    Ok(())
}
