use std::path::PathBuf;
use std::time::Duration;

use supersigil_core::{SourcePosition, SpanKind};
use supersigil_evidence::VerifiableRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchFormat {
    Text,
    Json,
    Regex,
    Snapshot,
}

/// Byte-offset span into the source file for an `Expected` body.
#[derive(Debug, Clone, Copy)]
pub struct BodySpan {
    pub start: usize,
    pub end: usize,
    pub kind: SpanKind,
}

#[derive(Debug, Clone)]
pub struct ExpectedSpec {
    pub status: Option<u32>,
    pub format: MatchFormat,
    pub contains: Option<String>,
    pub body: Option<String>,
    pub body_span: Option<BodySpan>,
}

#[derive(Debug, Clone)]
pub struct ExampleSpec {
    pub doc_id: String,
    pub example_id: String,
    pub lang: String,
    pub runner: String,
    pub verifies: Vec<VerifiableRef>,
    pub code: String,
    pub expected: Option<ExpectedSpec>,
    pub timeout: u64,
    pub env: Vec<(String, String)>,
    pub setup: Option<PathBuf>,
    pub position: SourcePosition,
    pub source_path: PathBuf,
}

#[derive(Debug)]
pub struct ExampleResult {
    pub spec: ExampleSpec,
    pub outcome: ExampleOutcome,
    pub duration: Duration,
}

#[derive(Debug)]
pub enum ExampleOutcome {
    Pass,
    Fail(Vec<MatchFailure>),
    Timeout,
    Error(String),
}

#[derive(Debug)]
pub struct MatchFailure {
    pub check: MatchCheck,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, PartialEq)]
pub enum MatchCheck {
    Status,
    Contains,
    Body,
}
