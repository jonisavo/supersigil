//! Custom LSP request type for per-document component trees with verification status.
//!
//! - `supersigil/documentComponents`: request returning the component tree for a single document

use std::collections::HashMap;
use std::path::Path;

use lsp_types::request::Request;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request: supersigil/documentComponents
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DocumentComponentsRequest;

impl Request for DocumentComponentsRequest {
    type Params = DocumentComponentsParams;
    type Result = DocumentComponentsResult;
    const METHOD: &'static str = "supersigil/documentComponents";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentComponentsParams {
    pub uri: String,
}

// ---------------------------------------------------------------------------
// Result payload
// ---------------------------------------------------------------------------

/// The top-level response for `supersigil/documentComponents`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentComponentsResult {
    /// The document ID from front matter.
    pub document_id: String,
    /// Whether the returned data is stale (does not reflect current content).
    pub stale: bool,
    /// Project name in multi-project workspaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Fences in source order, each containing its subset of the component tree.
    pub fences: Vec<FenceData>,
    /// Outgoing graph edges from this document.
    pub edges: Vec<EdgeData>,
}

// ---------------------------------------------------------------------------
// FenceData
// ---------------------------------------------------------------------------

/// A single `supersigil-xml` fenced code block and its parsed components.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FenceData {
    /// Byte range of the fence in the source file `[start, end)`.
    pub byte_range: [usize; 2],
    /// Components extracted from this fence.
    pub components: Vec<RenderedComponent>,
}

// ---------------------------------------------------------------------------
// SourceRange
// ---------------------------------------------------------------------------

/// Line/column range in the source file, for click-to-source navigation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

// ---------------------------------------------------------------------------
// RenderedComponent
// ---------------------------------------------------------------------------

/// A single component in the response, enriched with verification status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenderedComponent {
    /// Component kind (e.g. "Criterion", "Task", "Decision").
    pub kind: String,
    /// Optional component ID (e.g. "req-1-2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Component attributes (key-value pairs from the XML element).
    pub attributes: HashMap<String, String>,
    /// Body text content, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_text: Option<String>,
    /// Child components.
    pub children: Vec<RenderedComponent>,
    /// Source range of this component in the file.
    pub source_range: SourceRange,
    /// Verification status, present for verifiable components (e.g. Criterion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationStatus>,
}

// ---------------------------------------------------------------------------
// VerificationStatus
// ---------------------------------------------------------------------------

/// Verification state and evidence for a verifiable component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationStatus {
    /// One of: "verified", "unverified", "partial", "failing".
    pub state: VerificationState,
    /// Evidence entries supporting the verification state.
    pub evidence: Vec<EvidenceEntry>,
}

// ---------------------------------------------------------------------------
// VerificationState
// ---------------------------------------------------------------------------

/// The verification state of a verifiable component.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerificationState {
    Verified,
    Unverified,
    Partial,
    Failing,
}

// ---------------------------------------------------------------------------
// EvidenceEntry
// ---------------------------------------------------------------------------

/// A single evidence entry linking a criterion to a test.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceEntry {
    /// Name of the test function or test case.
    pub test_name: String,
    /// Path to the test file (relative to project root).
    pub test_file: String,
    /// Classification of the test.
    pub test_kind: TestKind,
    /// How the evidence was discovered.
    pub evidence_kind: EvidenceKindLabel,
    /// Source line number (1-based) of the evidence.
    pub source_line: usize,
    /// Provenance chain describing how the evidence was discovered.
    pub provenance: Vec<ProvenanceEntry>,
}

// ---------------------------------------------------------------------------
// TestKind
// ---------------------------------------------------------------------------

/// Classification of the test that produced evidence.
///
/// Wire-format equivalent of [`supersigil_evidence::types::TestKind`].
/// Defined separately because the evidence crate type lacks `Deserialize`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Unit,
    Async,
    Property,
    Snapshot,
    Unknown,
}

// ---------------------------------------------------------------------------
// EvidenceKindLabel
// ---------------------------------------------------------------------------

/// How the evidence was originally authored or discovered.
///
/// Wire-format equivalent of [`supersigil_evidence::types::EvidenceKind`].
/// Defined separately because the evidence crate type lacks `Deserialize`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceKindLabel {
    #[serde(rename = "tag")]
    Tag,
    #[serde(rename = "file-glob")]
    FileGlob,
    #[serde(rename = "rust-attribute")]
    RustAttribute,
    #[serde(rename = "example")]
    Example,
}

// ---------------------------------------------------------------------------
// ProvenanceEntry
// ---------------------------------------------------------------------------

/// A tagged union describing how a piece of evidence was discovered.
///
/// Discriminated by the `kind` field in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum ProvenanceEntry {
    /// Evidence from a `<VerifiedBy>` tag in the spec.
    #[serde(rename = "verified-by-tag")]
    VerifiedByTag { tag: String },
    /// Evidence from a file glob pattern match.
    #[serde(rename = "verified-by-file-glob")]
    VerifiedByFileGlob { paths: Vec<String> },
    /// Evidence from a Rust `#[verifies(...)]` attribute.
    #[serde(rename = "rust-attribute")]
    RustAttribute { file: String, line: usize },
    /// Evidence from an `<Example>` component.
    #[serde(rename = "example")]
    Example { example_id: String },
}

// ---------------------------------------------------------------------------
// EdgeData
// ---------------------------------------------------------------------------

/// An outgoing graph edge from the document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeData {
    /// Source document ID.
    pub from: String,
    /// Target document ID.
    pub to: String,
    /// Edge kind (e.g. "implements", "depends", "references").
    pub kind: String,
}

// ---------------------------------------------------------------------------
// Builder: extract document components from server state
// ---------------------------------------------------------------------------

use supersigil_core::{DocumentGraph, EdgeKind, ExtractedComponent, SourcePosition, SpecDocument};
use supersigil_evidence::{EvidenceId, PluginProvenance, VerificationEvidenceRecord};
use supersigil_parser::{extract_front_matter, extract_markdown_fences};

/// Evidence index: `doc_id` -> `target_id` -> evidence IDs.
pub type EvidenceIndex = HashMap<String, HashMap<String, Vec<EvidenceId>>>;

/// Input for building a `DocumentComponentsResult`.
#[derive(Debug)]
pub struct BuildComponentsInput<'a> {
    /// The `SpecDocument` to render (may be from current or stale parse).
    pub doc: &'a SpecDocument,
    /// Whether the document data is stale.
    pub stale: bool,
    /// The full document content (used to extract fence byte ranges).
    pub content: &'a str,
    /// The document graph for edge lookups.
    pub graph: &'a DocumentGraph,
    /// Optional evidence index from a previous verify run.
    pub evidence_by_target: Option<&'a EvidenceIndex>,
    /// Optional evidence records (keyed by `EvidenceId`).
    /// When present, evidence details (test name, file, kind, provenance)
    /// are populated in the response.
    pub evidence_records: Option<&'a [VerificationEvidenceRecord]>,
    /// Project root for making evidence paths relative.
    pub project_root: &'a Path,
}

/// Build a `DocumentComponentsResult` from server state.
///
/// Groups the document's extracted components into fences (by correlating
/// byte offsets), enriches verifiable components with verification status,
/// and collects outgoing graph edges.
#[must_use]
pub fn build_document_components(input: &BuildComponentsInput<'_>) -> DocumentComponentsResult {
    let doc_id = &input.doc.frontmatter.id;

    // Extract fence byte ranges from the content.
    let fence_ranges = extract_fence_byte_ranges(input.content);

    let ctx = RenderCtx {
        doc_id,
        evidence_by_target: input.evidence_by_target,
        evidence_records: input.evidence_records,
        project_root: input.project_root,
    };

    // Group components into fences by byte offset.
    let fences = group_components_into_fences(&fence_ranges, &input.doc.components, &ctx);

    // Collect outgoing edges.
    let edges = collect_edges(input.graph, doc_id);

    let project = input.graph.doc_project(doc_id).map(str::to_owned);

    DocumentComponentsResult {
        document_id: doc_id.clone(),
        stale: input.stale,
        project,
        fences,
        edges,
    }
}

/// A fence byte range extracted from the document content.
struct FenceByteRange {
    /// Byte offset of the opening fence delimiter.
    fence_start: usize,
    /// Byte offset of the end of the closing fence delimiter.
    fence_end: usize,
    /// Byte offset of the content start (after the opening delimiter line).
    content_start: usize,
    /// Byte offset of the content end.
    content_end: usize,
}

/// Extract fence byte ranges from document content by re-parsing the markdown
/// to find supersigil-xml fence boundaries.
fn extract_fence_byte_ranges(content: &str) -> Vec<FenceByteRange> {
    // Extract front matter to compute body offset.
    let body_offset = match extract_front_matter(content, std::path::Path::new("")) {
        Ok(Some((_yaml, body))) => content.len() - body.len(),
        _ => 0,
    };
    let body = &content[body_offset..];

    let fences = extract_markdown_fences(body, body_offset);

    fences
        .xml_fences
        .iter()
        .map(|f| FenceByteRange {
            fence_start: f.fence_start,
            fence_end: f.fence_end,
            content_start: f.content_offset,
            content_end: f.content_offset + f.content.len(),
        })
        .collect()
}

/// Group extracted components into their containing fences.
/// Context for rendering components — bundles the parameters that are
/// threaded through `group_components_into_fences`, `to_rendered_component`,
/// and `compute_verification`.
struct RenderCtx<'a> {
    doc_id: &'a str,
    evidence_by_target: Option<&'a EvidenceIndex>,
    evidence_records: Option<&'a [VerificationEvidenceRecord]>,
    project_root: &'a Path,
}

fn group_components_into_fences(
    fence_ranges: &[FenceByteRange],
    components: &[ExtractedComponent],
    ctx: &RenderCtx<'_>,
) -> Vec<FenceData> {
    let mut fence_components: Vec<Vec<RenderedComponent>> = vec![Vec::new(); fence_ranges.len()];

    for comp in components {
        let byte_offset = comp.position.byte_offset;
        if let Some(fence_idx) = fence_ranges
            .iter()
            .position(|f| byte_offset >= f.content_start && byte_offset < f.content_end)
        {
            fence_components[fence_idx].push(to_rendered_component(comp, ctx));
        }
    }

    fence_ranges
        .iter()
        .zip(fence_components)
        .map(|(range, comps)| FenceData {
            byte_range: [range.fence_start, range.fence_end],
            components: comps,
        })
        .collect()
}

/// Convert an `ExtractedComponent` to a `RenderedComponent`, enriching
/// verifiable components with verification status.
fn to_rendered_component(comp: &ExtractedComponent, ctx: &RenderCtx<'_>) -> RenderedComponent {
    let id = comp.attributes.get("id").cloned();
    let verification = compute_verification(comp, id.as_ref(), ctx);

    let children = comp
        .children
        .iter()
        .map(|c| to_rendered_component(c, ctx))
        .collect();

    RenderedComponent {
        kind: comp.name.clone(),
        id,
        attributes: comp.attributes.clone(),
        body_text: comp.body_text.clone(),
        children,
        source_range: to_source_range(&comp.position, &comp.end_position),
        verification,
    }
}

/// Convert parser source positions to a `SourceRange`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source lines/columns will not exceed u32::MAX"
)]
fn to_source_range(start: &SourcePosition, end: &SourcePosition) -> SourceRange {
    SourceRange {
        start_line: start.line as u32,
        start_col: start.column as u32,
        end_line: end.line as u32,
        end_col: end.column as u32,
    }
}

/// Compute verification status for a component.
///
/// Only `Criterion` components are verifiable. Returns `None` for
/// non-verifiable components or when no evidence index is available.
fn compute_verification(
    comp: &ExtractedComponent,
    comp_id: Option<&String>,
    ctx: &RenderCtx<'_>,
) -> Option<VerificationStatus> {
    if comp.name != "Criterion" {
        return None;
    }

    let criterion_id = comp_id?.as_str();
    let evidence_index = ctx.evidence_by_target?;

    let evidence_ids = evidence_index
        .get(ctx.doc_id)
        .and_then(|targets| targets.get(criterion_id));

    match evidence_ids {
        Some(ids) if !ids.is_empty() => {
            let entries = build_evidence_entries(ids, ctx.evidence_records, ctx.project_root);
            Some(VerificationStatus {
                state: VerificationState::Verified,
                evidence: entries,
            })
        }
        _ => Some(VerificationStatus {
            state: VerificationState::Unverified,
            evidence: Vec::new(),
        }),
    }
}

/// Build `EvidenceEntry` values from evidence IDs by looking up the records.
fn build_evidence_entries(
    ids: &[EvidenceId],
    records: Option<&[VerificationEvidenceRecord]>,
    project_root: &Path,
) -> Vec<EvidenceEntry> {
    let Some(records) = records else {
        return Vec::new();
    };

    ids.iter()
        .filter_map(|id| {
            let record = records.iter().find(|r| r.id == *id)?;
            Some(EvidenceEntry {
                test_name: record.test.name.clone(),
                test_file: relativize(&record.test.file, project_root),
                test_kind: map_test_kind(record.test.kind),
                evidence_kind: record
                    .kind()
                    .map_or(EvidenceKindLabel::Tag, map_evidence_kind),
                source_line: record.source_location.line,
                provenance: record
                    .provenance
                    .iter()
                    .map(|p| map_provenance(p, project_root))
                    .collect(),
            })
        })
        .collect()
}

/// Make a path relative to the project root, or return as-is if not a child.
fn relativize(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn map_test_kind(kind: supersigil_evidence::TestKind) -> TestKind {
    match kind {
        supersigil_evidence::TestKind::Unit => TestKind::Unit,
        supersigil_evidence::TestKind::Async => TestKind::Async,
        supersigil_evidence::TestKind::Property => TestKind::Property,
        supersigil_evidence::TestKind::Snapshot => TestKind::Snapshot,
        supersigil_evidence::TestKind::Unknown => TestKind::Unknown,
    }
}

fn map_evidence_kind(kind: supersigil_evidence::EvidenceKind) -> EvidenceKindLabel {
    match kind {
        supersigil_evidence::EvidenceKind::Tag => EvidenceKindLabel::Tag,
        supersigil_evidence::EvidenceKind::FileGlob => EvidenceKindLabel::FileGlob,
        supersigil_evidence::EvidenceKind::RustAttribute => EvidenceKindLabel::RustAttribute,
        supersigil_evidence::EvidenceKind::Example => EvidenceKindLabel::Example,
    }
}

fn map_provenance(prov: &PluginProvenance, project_root: &Path) -> ProvenanceEntry {
    match prov {
        PluginProvenance::VerifiedByTag { tag, .. } => {
            ProvenanceEntry::VerifiedByTag { tag: tag.clone() }
        }
        PluginProvenance::VerifiedByFileGlob { paths, .. } => ProvenanceEntry::VerifiedByFileGlob {
            paths: paths.clone(),
        },
        PluginProvenance::RustAttribute { attribute_span } => ProvenanceEntry::RustAttribute {
            file: relativize(&attribute_span.file, project_root),
            line: attribute_span.line,
        },
        PluginProvenance::Example { example_id, .. } => ProvenanceEntry::Example {
            example_id: example_id.clone(),
        },
    }
}

/// Collect outgoing edges from the document graph for a given document.
fn collect_edges(graph: &DocumentGraph, doc_id: &str) -> Vec<EdgeData> {
    graph
        .edges()
        .filter(|(src, _target, _kind)| *src == doc_id)
        .map(|(src, target, kind)| EdgeData {
            from: src.to_owned(),
            to: target.to_owned(),
            kind: edge_kind_label(kind),
        })
        .collect()
}

/// Convert an `EdgeKind` to its string label.
fn edge_kind_label(kind: EdgeKind) -> String {
    kind.as_str().to_lowercase()
}

// ---------------------------------------------------------------------------
// Test helper
// ---------------------------------------------------------------------------

/// Build a representative `DocumentComponentsResult` for use in downstream tests.
///
/// This constructs a realistic payload with multiple fences, nested components,
/// verification status variants, and graph edges.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "test fixture builder benefits from being a single literal"
)]
pub fn sample_document_components_result() -> DocumentComponentsResult {
    DocumentComponentsResult {
        document_id: "auth/req".to_owned(),
        stale: false,
        project: Some("workspace".to_owned()),
        fences: vec![
            FenceData {
                byte_range: [100, 500],
                components: vec![RenderedComponent {
                    kind: "AcceptanceCriteria".to_owned(),
                    id: None,
                    attributes: HashMap::new(),
                    body_text: None,
                    children: vec![
                        RenderedComponent {
                            kind: "Criterion".to_owned(),
                            id: Some("crit-1".to_owned()),
                            attributes: HashMap::from([("id".to_owned(), "crit-1".to_owned())]),
                            body_text: Some("Users SHALL authenticate with OAuth2.".to_owned()),
                            children: vec![],
                            source_range: SourceRange {
                                start_line: 5,
                                start_col: 3,
                                end_line: 10,
                                end_col: 1,
                            },
                            verification: Some(VerificationStatus {
                                state: VerificationState::Verified,
                                evidence: vec![EvidenceEntry {
                                    test_name: "test_oauth2_flow".to_owned(),
                                    test_file: "tests/auth.rs".to_owned(),
                                    test_kind: TestKind::Unit,
                                    evidence_kind: EvidenceKindLabel::RustAttribute,
                                    source_line: 42,
                                    provenance: vec![ProvenanceEntry::RustAttribute {
                                        file: "tests/auth.rs".to_owned(),
                                        line: 42,
                                    }],
                                }],
                            }),
                        },
                        RenderedComponent {
                            kind: "Criterion".to_owned(),
                            id: Some("crit-2".to_owned()),
                            attributes: HashMap::from([("id".to_owned(), "crit-2".to_owned())]),
                            body_text: Some("Sessions SHALL expire after 30 minutes.".to_owned()),
                            children: vec![],
                            source_range: SourceRange {
                                start_line: 11,
                                start_col: 3,
                                end_line: 15,
                                end_col: 1,
                            },
                            verification: Some(VerificationStatus {
                                state: VerificationState::Unverified,
                                evidence: vec![],
                            }),
                        },
                        RenderedComponent {
                            kind: "Criterion".to_owned(),
                            id: Some("crit-3".to_owned()),
                            attributes: HashMap::from([("id".to_owned(), "crit-3".to_owned())]),
                            body_text: Some("Tokens SHALL be rotated on refresh.".to_owned()),
                            children: vec![],
                            source_range: SourceRange {
                                start_line: 16,
                                start_col: 3,
                                end_line: 20,
                                end_col: 1,
                            },
                            verification: Some(VerificationStatus {
                                state: VerificationState::Partial,
                                evidence: vec![EvidenceEntry {
                                    test_name: "test_token_rotation".to_owned(),
                                    test_file: "tests/token.rs".to_owned(),
                                    test_kind: TestKind::Async,
                                    evidence_kind: EvidenceKindLabel::Tag,
                                    source_line: 88,
                                    provenance: vec![ProvenanceEntry::VerifiedByTag {
                                        tag: "auth:token-rotate".to_owned(),
                                    }],
                                }],
                            }),
                        },
                        RenderedComponent {
                            kind: "Criterion".to_owned(),
                            id: Some("crit-4".to_owned()),
                            attributes: HashMap::from([("id".to_owned(), "crit-4".to_owned())]),
                            body_text: Some("Invalid tokens SHALL return 401.".to_owned()),
                            children: vec![],
                            source_range: SourceRange {
                                start_line: 21,
                                start_col: 3,
                                end_line: 25,
                                end_col: 1,
                            },
                            verification: Some(VerificationStatus {
                                state: VerificationState::Failing,
                                evidence: vec![EvidenceEntry {
                                    test_name: "test_invalid_token_401".to_owned(),
                                    test_file: "tests/auth.rs".to_owned(),
                                    test_kind: TestKind::Unit,
                                    evidence_kind: EvidenceKindLabel::RustAttribute,
                                    source_line: 120,
                                    provenance: vec![ProvenanceEntry::RustAttribute {
                                        file: "tests/auth.rs".to_owned(),
                                        line: 120,
                                    }],
                                }],
                            }),
                        },
                    ],
                    source_range: SourceRange {
                        start_line: 4,
                        start_col: 1,
                        end_line: 26,
                        end_col: 1,
                    },
                    verification: None,
                }],
            },
            FenceData {
                byte_range: [600, 900],
                components: vec![RenderedComponent {
                    kind: "Task".to_owned(),
                    id: Some("task-1".to_owned()),
                    attributes: HashMap::from([
                        ("id".to_owned(), "task-1".to_owned()),
                        ("status".to_owned(), "done".to_owned()),
                    ]),
                    body_text: Some("Implement OAuth2 login endpoint.".to_owned()),
                    children: vec![],
                    source_range: SourceRange {
                        start_line: 30,
                        start_col: 1,
                        end_line: 35,
                        end_col: 1,
                    },
                    verification: None,
                }],
            },
        ],
        edges: vec![
            EdgeData {
                from: "auth/req".to_owned(),
                to: "auth/design".to_owned(),
                kind: "implements".to_owned(),
            },
            EdgeData {
                from: "auth/req".to_owned(),
                to: "core/req".to_owned(),
                kind: "depends".to_owned(),
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use supersigil_rust::verifies;

    /// Helper: serialize to JSON and deserialize back, asserting round-trip equality.
    fn assert_round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let deserialized: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(value, &deserialized);
    }

    // -- SourceRange --

    #[test]
    fn source_range_round_trip() {
        let range = SourceRange {
            start_line: 1,
            start_col: 10,
            end_line: 3,
            end_col: 42,
        };
        assert_round_trip(&range);
    }

    // -- VerificationState --

    #[test]
    fn verification_state_round_trip_all_variants() {
        for state in [
            VerificationState::Verified,
            VerificationState::Unverified,
            VerificationState::Partial,
            VerificationState::Failing,
        ] {
            assert_round_trip(&state);
        }
    }

    #[test]
    fn verification_state_serializes_as_lowercase() {
        assert_eq!(
            serde_json::to_string(&VerificationState::Verified).unwrap(),
            r#""verified""#,
        );
        assert_eq!(
            serde_json::to_string(&VerificationState::Unverified).unwrap(),
            r#""unverified""#,
        );
        assert_eq!(
            serde_json::to_string(&VerificationState::Partial).unwrap(),
            r#""partial""#,
        );
        assert_eq!(
            serde_json::to_string(&VerificationState::Failing).unwrap(),
            r#""failing""#,
        );
    }

    // -- TestKind --

    #[test]
    fn test_kind_round_trip_all_variants() {
        for kind in [
            TestKind::Unit,
            TestKind::Async,
            TestKind::Property,
            TestKind::Snapshot,
            TestKind::Unknown,
        ] {
            assert_round_trip(&kind);
        }
    }

    #[test]
    fn test_kind_serializes_as_lowercase() {
        assert_eq!(serde_json::to_string(&TestKind::Unit).unwrap(), r#""unit""#,);
        assert_eq!(
            serde_json::to_string(&TestKind::Unknown).unwrap(),
            r#""unknown""#,
        );
    }

    // -- EvidenceKindLabel --

    #[test]
    fn evidence_kind_label_round_trip_all_variants() {
        for kind in [
            EvidenceKindLabel::Tag,
            EvidenceKindLabel::FileGlob,
            EvidenceKindLabel::RustAttribute,
            EvidenceKindLabel::Example,
        ] {
            assert_round_trip(&kind);
        }
    }

    #[test]
    fn evidence_kind_label_serializes_with_hyphens() {
        assert_eq!(
            serde_json::to_string(&EvidenceKindLabel::Tag).unwrap(),
            r#""tag""#,
        );
        assert_eq!(
            serde_json::to_string(&EvidenceKindLabel::FileGlob).unwrap(),
            r#""file-glob""#,
        );
        assert_eq!(
            serde_json::to_string(&EvidenceKindLabel::RustAttribute).unwrap(),
            r#""rust-attribute""#,
        );
        assert_eq!(
            serde_json::to_string(&EvidenceKindLabel::Example).unwrap(),
            r#""example""#,
        );
    }

    // -- ProvenanceEntry --

    #[test]
    fn provenance_verified_by_tag_round_trip() {
        let entry = ProvenanceEntry::VerifiedByTag {
            tag: "auth:crit1".to_owned(),
        };
        assert_round_trip(&entry);
    }

    #[test]
    fn provenance_verified_by_file_glob_round_trip() {
        let entry = ProvenanceEntry::VerifiedByFileGlob {
            paths: vec!["tests/auth_*.rs".to_owned()],
        };
        assert_round_trip(&entry);
    }

    #[test]
    fn provenance_rust_attribute_round_trip() {
        let entry = ProvenanceEntry::RustAttribute {
            file: "tests/auth.rs".to_owned(),
            line: 42,
        };
        assert_round_trip(&entry);
    }

    #[test]
    fn provenance_example_round_trip() {
        let entry = ProvenanceEntry::Example {
            example_id: "ex-1".to_owned(),
        };
        assert_round_trip(&entry);
    }

    #[test]
    fn provenance_tag_discriminator_in_json() {
        let entry = ProvenanceEntry::VerifiedByTag {
            tag: "auth:crit1".to_owned(),
        };
        let json: serde_json::Value = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "verified-by-tag");
        assert_eq!(json["tag"], "auth:crit1");
    }

    #[test]
    fn provenance_rust_attr_discriminator_in_json() {
        let entry = ProvenanceEntry::RustAttribute {
            file: "src/lib.rs".to_owned(),
            line: 10,
        };
        let json: serde_json::Value = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "rust-attribute");
        assert_eq!(json["file"], "src/lib.rs");
        assert_eq!(json["line"], 10);
    }

    #[test]
    fn provenance_example_discriminator_in_json() {
        let entry = ProvenanceEntry::Example {
            example_id: "ex-1".to_owned(),
        };
        let json: serde_json::Value = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "example");
    }

    #[test]
    fn provenance_file_glob_discriminator_in_json() {
        let entry = ProvenanceEntry::VerifiedByFileGlob {
            paths: vec!["tests/*.rs".to_owned()],
        };
        let json: serde_json::Value = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "verified-by-file-glob");
    }

    // -- EvidenceEntry --

    #[test]
    #[verifies("spec-rendering/req#req-1-3")]
    fn evidence_entry_round_trip() {
        let entry = EvidenceEntry {
            test_name: "test_oauth2_flow".to_owned(),
            test_file: "tests/auth.rs".to_owned(),
            test_kind: TestKind::Unit,
            evidence_kind: EvidenceKindLabel::RustAttribute,
            source_line: 42,
            provenance: vec![ProvenanceEntry::RustAttribute {
                file: "tests/auth.rs".to_owned(),
                line: 42,
            }],
        };
        assert_round_trip(&entry);
    }

    #[test]
    #[verifies("spec-rendering/req#req-1-3")]
    fn evidence_entry_with_tag_provenance_round_trip() {
        let entry = EvidenceEntry {
            test_name: "test_session_expiry".to_owned(),
            test_file: "tests/session.rs".to_owned(),
            test_kind: TestKind::Async,
            evidence_kind: EvidenceKindLabel::Tag,
            source_line: 100,
            provenance: vec![ProvenanceEntry::VerifiedByTag {
                tag: "session:timeout".to_owned(),
            }],
        };
        assert_round_trip(&entry);
    }

    // -- VerificationStatus --

    #[test]
    #[verifies("spec-rendering/req#req-1-3")]
    fn verification_status_verified_round_trip() {
        let status = VerificationStatus {
            state: VerificationState::Verified,
            evidence: vec![EvidenceEntry {
                test_name: "test_it".to_owned(),
                test_file: "tests/it.rs".to_owned(),
                test_kind: TestKind::Unit,
                evidence_kind: EvidenceKindLabel::RustAttribute,
                source_line: 10,
                provenance: vec![ProvenanceEntry::RustAttribute {
                    file: "tests/it.rs".to_owned(),
                    line: 10,
                }],
            }],
        };
        assert_round_trip(&status);
    }

    #[test]
    #[verifies("spec-rendering/req#req-1-3")]
    fn verification_status_unverified_round_trip() {
        let status = VerificationStatus {
            state: VerificationState::Unverified,
            evidence: vec![],
        };
        assert_round_trip(&status);
    }

    // -- EdgeData --

    #[test]
    fn edge_data_round_trip() {
        let edge = EdgeData {
            from: "auth/req".to_owned(),
            to: "auth/design".to_owned(),
            kind: "implements".to_owned(),
        };
        assert_round_trip(&edge);
    }

    // -- FenceData --

    #[test]
    fn fence_data_round_trip() {
        let fence = FenceData {
            byte_range: [100, 500],
            components: vec![RenderedComponent {
                kind: "Task".to_owned(),
                id: Some("t-1".to_owned()),
                attributes: HashMap::from([("id".to_owned(), "t-1".to_owned())]),
                body_text: Some("Do the thing.".to_owned()),
                children: vec![],
                source_range: SourceRange {
                    start_line: 5,
                    start_col: 1,
                    end_line: 20,
                    end_col: 1,
                },
                verification: None,
            }],
        };
        assert_round_trip(&fence);
    }

    // -- RenderedComponent --

    #[test]
    #[verifies("spec-rendering/req#req-1-2")]
    fn rendered_component_with_all_fields_round_trip() {
        let comp = RenderedComponent {
            kind: "Criterion".to_owned(),
            id: Some("crit-1".to_owned()),
            attributes: HashMap::from([("id".to_owned(), "crit-1".to_owned())]),
            body_text: Some("Users SHALL authenticate.".to_owned()),
            children: vec![RenderedComponent {
                kind: "VerifiedBy".to_owned(),
                id: None,
                attributes: HashMap::from([("tag".to_owned(), "auth:crit1".to_owned())]),
                body_text: None,
                children: vec![],
                source_range: SourceRange {
                    start_line: 8,
                    start_col: 5,
                    end_line: 10,
                    end_col: 5,
                },
                verification: None,
            }],
            source_range: SourceRange {
                start_line: 5,
                start_col: 1,
                end_line: 12,
                end_col: 1,
            },
            verification: Some(VerificationStatus {
                state: VerificationState::Verified,
                evidence: vec![EvidenceEntry {
                    test_name: "test_auth".to_owned(),
                    test_file: "tests/auth.rs".to_owned(),
                    test_kind: TestKind::Unit,
                    evidence_kind: EvidenceKindLabel::Tag,
                    source_line: 15,
                    provenance: vec![ProvenanceEntry::VerifiedByTag {
                        tag: "auth:crit1".to_owned(),
                    }],
                }],
            }),
        };
        assert_round_trip(&comp);
    }

    #[test]
    #[verifies("spec-rendering/req#req-1-2")]
    fn rendered_component_minimal_round_trip() {
        let comp = RenderedComponent {
            kind: "Decision".to_owned(),
            id: None,
            attributes: HashMap::new(),
            body_text: None,
            children: vec![],
            source_range: SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            verification: None,
        };
        assert_round_trip(&comp);
    }

    #[test]
    #[verifies("spec-rendering/req#req-1-2")]
    fn rendered_component_omits_none_fields() {
        let comp = RenderedComponent {
            kind: "Task".to_owned(),
            id: None,
            attributes: HashMap::new(),
            body_text: None,
            children: vec![],
            source_range: SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
            verification: None,
        };
        let json: serde_json::Value = serde_json::to_value(&comp).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert!(!json.as_object().unwrap().contains_key("body_text"));
        assert!(!json.as_object().unwrap().contains_key("verification"));
    }

    // -- DocumentComponentsParams --

    #[test]
    fn params_round_trip() {
        let params = DocumentComponentsParams {
            uri: "file:///project/specs/auth.md".to_owned(),
        };
        assert_round_trip(&params);
    }

    // -- DocumentComponentsResult --

    #[test]
    #[verifies("spec-rendering/req#req-1-2")]
    #[verifies("spec-rendering/req#req-1-3")]
    fn full_result_round_trip() {
        let result = sample_document_components_result();
        assert_round_trip(&result);
    }

    #[test]
    fn empty_result_round_trip() {
        let result = DocumentComponentsResult {
            document_id: "empty/doc".to_owned(),
            stale: true,
            project: None,
            fences: vec![],
            edges: vec![],
        };
        assert_round_trip(&result);
    }

    #[test]
    #[verifies("spec-rendering/req#req-1-3")]
    fn sample_helper_has_all_verification_states() {
        let result = sample_document_components_result();

        // Collect all verification states present in the sample.
        let mut states = Vec::new();
        for fence in &result.fences {
            collect_states(&fence.components, &mut states);
        }

        assert!(
            states.contains(&VerificationState::Verified),
            "sample should contain a verified component",
        );
        assert!(
            states.contains(&VerificationState::Unverified),
            "sample should contain an unverified component",
        );
        assert!(
            states.contains(&VerificationState::Partial),
            "sample should contain a partial component",
        );
        assert!(
            states.contains(&VerificationState::Failing),
            "sample should contain a failing component",
        );
    }

    fn collect_states(components: &[RenderedComponent], states: &mut Vec<VerificationState>) {
        for comp in components {
            if let Some(v) = &comp.verification {
                states.push(v.state);
            }
            collect_states(&comp.children, states);
        }
    }

    #[test]
    fn sample_helper_has_edges() {
        let result = sample_document_components_result();
        assert!(
            result.edges.len() >= 2,
            "sample should have at least two edges",
        );
    }

    #[test]
    fn sample_helper_has_multiple_fences() {
        let result = sample_document_components_result();
        assert!(
            result.fences.len() >= 2,
            "sample should have at least two fences",
        );
    }

    #[test]
    fn sample_helper_has_nested_components() {
        let result = sample_document_components_result();
        let first_fence = &result.fences[0];
        assert!(
            !first_fence.components[0].children.is_empty(),
            "first fence should have nested children",
        );
    }

    #[test]
    fn sample_helper_has_evidence_with_provenance() {
        let result = sample_document_components_result();
        let mut found = false;
        for fence in &result.fences {
            for comp in &fence.components {
                check_evidence(&comp.children, &mut found);
            }
        }
        assert!(
            found,
            "sample should have at least one evidence entry with provenance"
        );
    }

    fn check_evidence(components: &[RenderedComponent], found: &mut bool) {
        for comp in components {
            if let Some(v) = &comp.verification {
                for e in &v.evidence {
                    if !e.provenance.is_empty() {
                        *found = true;
                    }
                }
            }
            check_evidence(&comp.children, found);
        }
    }

    // ======================================================================
    // Builder tests (Task 2: handler logic)
    // ======================================================================

    mod builder_tests {
        use std::path::PathBuf;

        use supersigil_core::{build_graph, test_helpers::single_project_config};
        use supersigil_rust::verifies;

        use super::super::*;

        // -- Helper: parse a markdown string into a SpecDocument --------

        fn parse_doc(content: &str) -> SpecDocument {
            let defs = supersigil_core::ComponentDefs::defaults();
            let result = supersigil_parser::parse_content_recovering(
                std::path::Path::new("specs/test/req.md"),
                content,
                &defs,
            )
            .expect("parse should succeed");
            match result.result {
                supersigil_core::ParseResult::Document(doc) => doc,
                supersigil_core::ParseResult::NotSupersigil(_) => panic!("expected Document"),
            }
        }

        fn parse_doc_with_errors(content: &str) -> (Option<SpecDocument>, bool) {
            let defs = supersigil_core::ComponentDefs::defaults();
            match supersigil_parser::parse_content_recovering(
                std::path::Path::new("specs/test/req.md"),
                content,
                &defs,
            ) {
                Ok(recovered) => match recovered.result {
                    supersigil_core::ParseResult::Document(doc) => {
                        let has_errors = !recovered.fatal_errors.is_empty();
                        (Some(doc), has_errors)
                    }
                    supersigil_core::ParseResult::NotSupersigil(_) => (None, false),
                },
                Err(_) => (None, true),
            }
        }

        fn build_graph_with_docs(docs: Vec<SpecDocument>) -> DocumentGraph {
            let config = single_project_config();
            build_graph(docs, &config).expect("graph")
        }

        // -----------------------------------------------------------------
        // (a) Single-fence document with verified criterion
        // -----------------------------------------------------------------

        #[test]
        #[verifies("spec-rendering/req#req-1-1")]
        fn single_fence_verified_criterion() {
            let content = "\
---
supersigil:
  id: test/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"c1\">
    Users SHALL authenticate.
  </Criterion>
</AcceptanceCriteria>
```
";
            let doc = parse_doc(content);
            let graph = build_graph_with_docs(vec![doc.clone()]);

            // Build evidence index marking c1 as verified.
            let mut evidence: HashMap<String, HashMap<String, Vec<EvidenceId>>> = HashMap::new();
            evidence
                .entry("test/req".to_owned())
                .or_default()
                .insert("c1".to_owned(), vec![EvidenceId::new(0)]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: false,
                content,
                graph: &graph,
                evidence_by_target: Some(&evidence),
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert_eq!(result.document_id, "test/req");
            assert!(!result.stale);
            assert_eq!(result.fences.len(), 1);

            let fence = &result.fences[0];
            assert!(fence.byte_range[0] < fence.byte_range[1]);
            assert_eq!(fence.components.len(), 1); // AcceptanceCriteria

            let ac = &fence.components[0];
            assert_eq!(ac.kind, "AcceptanceCriteria");
            assert_eq!(ac.children.len(), 1);

            let crit = &ac.children[0];
            assert_eq!(crit.kind, "Criterion");
            assert_eq!(crit.id.as_deref(), Some("c1"));
            assert!(crit.body_text.is_some());

            let v = crit
                .verification
                .as_ref()
                .expect("should have verification");
            assert_eq!(v.state, VerificationState::Verified);
        }

        // -----------------------------------------------------------------
        // (b) Multi-fence document with mixed verification states
        // -----------------------------------------------------------------

        #[test]
        #[verifies("spec-rendering/req#req-1-1")]
        fn multi_fence_mixed_verification() {
            let content = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"c1\">
    Users SHALL authenticate with OAuth2.
  </Criterion>
  <Criterion id=\"c2\">
    Sessions SHALL expire after 30 minutes.
  </Criterion>
</AcceptanceCriteria>
```

Some prose between fences.

```supersigil-xml
<Task id=\"t1\" status=\"open\">
  Implement OAuth2 login.
</Task>
```
";
            let doc = parse_doc(content);
            let graph = build_graph_with_docs(vec![doc.clone()]);

            // Only c1 has evidence, c2 does not.
            let mut evidence: HashMap<String, HashMap<String, Vec<EvidenceId>>> = HashMap::new();
            evidence
                .entry("auth/req".to_owned())
                .or_default()
                .insert("c1".to_owned(), vec![EvidenceId::new(0)]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: false,
                content,
                graph: &graph,
                evidence_by_target: Some(&evidence),
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert_eq!(result.document_id, "auth/req");
            assert!(!result.stale);
            assert_eq!(result.fences.len(), 2, "should have two fences");

            // First fence: AcceptanceCriteria with two criteria.
            let fence1 = &result.fences[0];
            assert_eq!(fence1.components.len(), 1);
            let ac = &fence1.components[0];
            assert_eq!(ac.kind, "AcceptanceCriteria");
            assert_eq!(ac.children.len(), 2);

            let c1 = &ac.children[0];
            assert_eq!(c1.id.as_deref(), Some("c1"));
            assert_eq!(
                c1.verification.as_ref().unwrap().state,
                VerificationState::Verified,
            );

            let c2 = &ac.children[1];
            assert_eq!(c2.id.as_deref(), Some("c2"));
            assert_eq!(
                c2.verification.as_ref().unwrap().state,
                VerificationState::Unverified,
            );

            // Second fence: Task (non-verifiable, no verification status).
            let fence2 = &result.fences[1];
            assert_eq!(fence2.components.len(), 1);
            let task = &fence2.components[0];
            assert_eq!(task.kind, "Task");
            assert_eq!(task.id.as_deref(), Some("t1"));
            assert!(task.verification.is_none());

            // Fence byte ranges should be ordered and non-overlapping.
            assert!(
                fence1.byte_range[1] <= fence2.byte_range[0],
                "fences should be ordered by position",
            );
        }

        // -----------------------------------------------------------------
        // (c) Document with parse errors returns stale data
        // -----------------------------------------------------------------

        #[test]
        #[verifies("spec-rendering/req#req-1-4")]
        fn parse_errors_return_stale_data() {
            // A document with a missing required attribute (Criterion without id)
            // produces fatal errors but a partial document via recovering parse.
            let content = "\
---
supersigil:
  id: test/partial
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>broken criterion without id</Criterion>
  <Criterion id=\"ok-1\">This one is valid.</Criterion>
</AcceptanceCriteria>
```
";
            let (doc, has_errors) = parse_doc_with_errors(content);
            assert!(has_errors, "should have fatal errors");
            let doc = doc.expect("should have partial document");
            let graph = build_graph_with_docs(vec![]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: true, // Caller sets stale when doc came from partial_file_parses.
                content,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert_eq!(result.document_id, "test/partial");
            assert!(result.stale, "result should be marked stale");
            // The partial document still has components.
            assert_eq!(result.fences.len(), 1);
            assert!(!result.fences[0].components.is_empty());
        }

        // -----------------------------------------------------------------
        // (d) Unknown URI returns empty fence list
        // -----------------------------------------------------------------

        #[test]
        #[verifies("spec-rendering/req#req-1-4")]
        fn unknown_doc_returns_empty_fences() {
            // When no document is found, the handler returns an empty result.
            // We test the builder with an empty-content scenario.
            let content = "";
            let doc = SpecDocument {
                path: PathBuf::from("specs/nonexistent.md"),
                frontmatter: supersigil_core::Frontmatter {
                    id: String::new(),
                    doc_type: None,
                    status: None,
                },
                extra: HashMap::new(),
                components: Vec::new(),
                warnings: Vec::new(),
            };
            let graph = build_graph_with_docs(vec![]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: false,
                content,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert!(result.fences.is_empty(), "should have no fences");
            assert!(result.edges.is_empty(), "should have no edges");
        }

        // -----------------------------------------------------------------
        // Edge collection
        // -----------------------------------------------------------------

        #[test]
        #[verifies("spec-rendering/req#req-1-1")]
        fn edges_are_collected_from_graph() {
            let content_req = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<Criterion id=\"c1\">Users SHALL authenticate.</Criterion>
```
";
            let content_design = "\
---
supersigil:
  id: auth/design
  type: design
  status: approved
---

```supersigil-xml
<Implements refs=\"auth/req\" />
```
";
            let doc_req = parse_doc(content_req);
            let doc_design = parse_doc(content_design);
            let graph = build_graph_with_docs(vec![doc_req.clone(), doc_design]);

            let result_req = build_document_components(&BuildComponentsInput {
                doc: &doc_req,
                stale: false,
                content: content_req,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            // The design doc implements auth/req, so auth/req should not
            // have outgoing implements edges (it's the target, not source).
            assert!(
                result_req.edges.is_empty(),
                "auth/req should not have outgoing implements edges",
            );

            // auth/design has an outgoing "implements" edge to auth/req.
            let result_design = build_document_components(&BuildComponentsInput {
                doc: graph.document("auth/design").unwrap(),
                stale: false,
                content: content_design,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert!(
                result_design.edges.iter().any(|e| e.from == "auth/design"
                    && e.to == "auth/req"
                    && e.kind == "implements"),
                "should have implements edge from design to req: {:?}",
                result_design.edges,
            );
        }

        // -----------------------------------------------------------------
        // No evidence index means no verification status
        // -----------------------------------------------------------------

        #[test]
        fn no_evidence_index_means_no_verification() {
            let content = "\
---
supersigil:
  id: test/req
  type: requirements
  status: approved
---

```supersigil-xml
<Criterion id=\"c1\">Users SHALL authenticate.</Criterion>
```
";
            let doc = parse_doc(content);
            let graph = build_graph_with_docs(vec![doc.clone()]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: false,
                content,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert_eq!(result.fences.len(), 1);
            let crit = &result.fences[0].components[0];
            assert_eq!(crit.kind, "Criterion");
            assert!(
                crit.verification.is_none(),
                "without evidence index, verification should be None",
            );
        }

        // -----------------------------------------------------------------
        // Fence byte ranges are correct
        // -----------------------------------------------------------------

        #[test]
        fn fence_byte_ranges_cover_delimiters() {
            let content = "\
---
supersigil:
  id: test/req
  type: requirements
  status: approved
---

```supersigil-xml
<Criterion id=\"c1\">hello</Criterion>
```
";
            let doc = parse_doc(content);
            let graph = build_graph_with_docs(vec![doc.clone()]);

            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale: false,
                content,
                graph: &graph,
                evidence_by_target: None,
                evidence_records: None,
                project_root: Path::new(""),
            });

            assert_eq!(result.fences.len(), 1);
            let fence = &result.fences[0];

            // The byte range should start at the opening ``` and extend past the closing ```.
            let range_start = fence.byte_range[0];
            let range_end = fence.byte_range[1];
            assert!(range_start < range_end);

            // The content at range_start should be the opening fence delimiter.
            let slice = &content[range_start..range_end];
            assert!(
                slice.starts_with("```supersigil-xml"),
                "fence range should start with opening delimiter, got: {:?}",
                &slice[..40.min(slice.len())],
            );
            assert!(
                slice.trim_end().ends_with("```"),
                "fence range should end with closing delimiter",
            );
        }
    }
}
