/**
 * Type declarations for @supersigil/preview.
 *
 * At build time, esbuild resolves this import to the preview kit's
 * dist/render.js via an alias. This file provides types for tsc.
 */
declare module "@supersigil/preview" {
  export interface SourceRange {
    start_line: number;
    start_col: number;
    end_line: number;
    end_col: number;
  }

  export type TestKind = "unit" | "async" | "property" | "snapshot" | "unknown";
  export type EvidenceKindLabel = "tag" | "file-glob" | "rust-attribute" | "example";
  export type VerificationState = "verified" | "unverified" | "partial" | "failing";

  export type ProvenanceEntry =
    | { kind: "verified-by-tag"; tag: string }
    | { kind: "verified-by-file-glob"; paths: string[] }
    | { kind: "rust-attribute"; file: string; line: number }
    | { kind: "example"; example_id: string };

  export interface EvidenceEntry {
    test_name: string;
    test_file: string;
    test_kind: TestKind;
    evidence_kind: EvidenceKindLabel;
    source_line: number;
    provenance: ProvenanceEntry[];
  }

  export interface VerificationStatus {
    state: VerificationState;
    evidence: EvidenceEntry[];
  }

  export interface RenderedComponent {
    kind: string;
    id?: string;
    attributes: Record<string, string>;
    body_text?: string;
    children: RenderedComponent[];
    source_range: SourceRange;
    verification?: VerificationStatus;
  }

  export interface FenceData {
    byte_range: [number, number];
    components: RenderedComponent[];
  }

  export interface EdgeData {
    from: string;
    to: string;
    kind: string;
  }

  export interface LinkResolver {
    evidenceLink(file: string, line: number): string | null;
    documentLink(docId: string): string;
    criterionLink(docId: string, criterionId: string): string;
  }

  export function renderComponentTree(
    fences: FenceData[],
    edges: EdgeData[],
    linkResolver: LinkResolver,
  ): string;

  export function filterNovelEdges(
    fences: FenceData[],
    edges: EdgeData[],
  ): EdgeData[];
}
