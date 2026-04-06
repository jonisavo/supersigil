/** Line/column range in the source file, for click-to-source navigation. */
export interface SourceRange {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

/** Classification of the test that produced evidence. */
export type TestKind =
  | "unit"
  | "async"
  | "property"
  | "snapshot"
  | "unknown";

/** How the evidence was originally authored or discovered. */
export type EvidenceKindLabel =
  | "tag"
  | "file-glob"
  | "rust-attribute"
  | "js-verifies";

/** A tagged union describing how a piece of evidence was discovered. */
export type ProvenanceEntry =
  | { kind: "verified-by-tag"; tag: string }
  | { kind: "verified-by-file-glob"; paths: string[] }
  | { kind: "rust-attribute"; file: string; line: number }
  | { kind: "js-verifies"; file: string; line: number };

/** A single evidence entry linking a criterion to a test. */
export interface EvidenceEntry {
  test_name: string;
  test_file: string;
  test_kind: TestKind;
  evidence_kind: EvidenceKindLabel;
  source_line: number;
  provenance: ProvenanceEntry[];
}

/** The verification state of a verifiable component. */
export type VerificationState =
  | "verified"
  | "unverified"
  | "partial"
  | "failing";

/** Verification state and evidence for a verifiable component. */
export interface VerificationStatus {
  state: VerificationState;
  evidence: EvidenceEntry[];
}

/** A single component in the response, enriched with verification status. */
export interface RenderedComponent {
  kind: string;
  id?: string;
  attributes: Record<string, string>;
  body_text?: string;
  children: RenderedComponent[];
  source_range: SourceRange;
  verification?: VerificationStatus;
}

/** A single supersigil-xml fenced code block and its parsed components. */
export interface FenceData {
  byte_range: [number, number];
  components: RenderedComponent[];
}

/** An outgoing graph edge from the document. */
export interface EdgeData {
  from: string;
  to: string;
  kind: string;
}

/** Host-provided link resolver for navigation targets. */
export interface LinkResolver {
  evidenceLink(file: string, line: number): string | null;
  documentLink(docId: string): string;
  criterionLink(docId: string, criterionId: string): string;
}
