use std::io::{self, Write};
use std::path::PathBuf;

use supersigil_import::{Diagnostic, ImportConfig, ImportSummary, import_kiro, plan_kiro_import};

use crate::commands::{ImportArgs, ImportSource};
use crate::error::CliError;

/// Run the import command, reading Kiro specs and writing MDX output.
///
/// # Errors
///
/// Returns `CliError` on I/O failure or import errors.
pub fn run(args: &ImportArgs) -> Result<(), CliError> {
    match args.from {
        ImportSource::Kiro => run_kiro_import(args),
    }
}

fn run_kiro_import(args: &ImportArgs) -> Result<(), CliError> {
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
        writeln!(out, "Dry run: {} documents planned", plan.documents.len())?;
        for doc in &plan.documents {
            writeln!(
                out,
                "  {} -> {}",
                doc.document_id,
                doc.output_path.display()
            )?;
        }
        print_summary(&mut out, &plan.summary, plan.ambiguity_count)?;
    } else {
        let result = import_kiro(&config)?;
        print_diagnostics(&result.diagnostics)?;

        let stdout = io::stdout();
        let mut out = stdout.lock();
        writeln!(out, "Imported {} files", result.files_written.len())?;
        for file in &result.files_written {
            writeln!(out, "  {} -> {}", file.document_id, file.path.display())?;
        }
        print_summary(&mut out, &result.summary, result.ambiguity_count)?;
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
