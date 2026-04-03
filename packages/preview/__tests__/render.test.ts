import { describe, it, expect } from "vitest";
import { renderComponentTree } from "../src/render.js";
import type {
  FenceData,
  EdgeData,
  LinkResolver,
  RenderedComponent,
} from "../src/types.js";

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

function mockLinkResolver(): LinkResolver {
  return {
    evidenceLink: (file, line) => `test://evidence/${file}#L${line}`,
    documentLink: (docId) => `test://doc/${docId}`,
    criterionLink: (docId, criterionId) =>
      `test://criterion/${docId}#${criterionId}`,
  };
}

function sourceRange() {
  return { start_line: 1, start_col: 0, end_line: 1, end_col: 10 };
}

function criterion(
  id: string,
  state: "verified" | "unverified" | "partial" | "failing",
  evidenceCount = 0,
): RenderedComponent {
  const evidence = Array.from({ length: evidenceCount }, (_, i) => ({
    test_name: `test_${id}_${i}`,
    test_file: `tests/${id}.rs`,
    test_kind: "unit" as const,
    evidence_kind: "rust-attribute" as const,
    source_line: 10 + i,
    provenance: [
      {
        kind: "rust-attribute" as const,
        file: `tests/${id}.rs`,
        line: 10 + i,
      },
    ],
  }));

  return {
    kind: "Criterion",
    id,
    attributes: {},
    body_text: `Description of ${id}`,
    children: [],
    source_range: sourceRange(),
    verification: { state, evidence },
  };
}

// ---------------------------------------------------------------------------
// Basic structure
// ---------------------------------------------------------------------------

describe("renderComponentTree", () => {
  it("returns empty string for empty fences", () => {
    const html = renderComponentTree([], [], mockLinkResolver());
    expect(html).toBe("");
  });

  it("wraps each fence in a supersigil-block div", () => {
    const fences: FenceData[] = [
      { byte_range: [0, 100], components: [] },
      { byte_range: [200, 300], components: [] },
    ];
    const html = renderComponentTree(fences, [], mockLinkResolver());
    const matches = html.match(/class="supersigil-block"/g);
    expect(matches).toHaveLength(2);
  });

  // ---------------------------------------------------------------------------
  // Criterion rendering
  // ---------------------------------------------------------------------------

  describe("Criterion component", () => {
    it("renders with the correct CSS class", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified")],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('class="supersigil-criterion"');
    });

    it("renders criterion id as data attribute", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified")],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('data-criterion-id="req-1"');
    });

    it("renders body text", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "unverified")],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("Description of req-1");
    });

    it.each([
      ["verified", "supersigil-badge--verified"],
      ["unverified", "supersigil-badge--unverified"],
      ["partial", "supersigil-badge--partial"],
      ["failing", "supersigil-badge--failing"],
    ] as const)("renders %s badge with correct class", (state, expected) => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [criterion("req-1", state)] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain(expected);
      expect(html).toContain("supersigil-badge");
    });

    it("renders badge tooltip with verification state", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 2)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("title=");
      expect(html).toMatch(/verified/i);
    });

    it("renders evidence list when evidence exists", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 2)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-evidence");
      expect(html).toContain("test_req-1_0");
      expect(html).toContain("test_req-1_1");
    });

    it("renders evidence toggle button", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 1)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-evidence-toggle");
    });

    it("evidence list starts hidden", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 1)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toMatch(/supersigil-evidence-list[^>]*hidden/);
    });

    it("uses link resolver for evidence links", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 1)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("test://evidence/tests/req-1.rs#L10");
    });

    it("renders evidence test kind and evidence kind", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "partial", 1)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("unit");
      expect(html).toContain("rust-attribute");
    });

    it("renders no evidence section for unverified criterion with no evidence", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "unverified", 0)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).not.toContain("supersigil-evidence-toggle");
      expect(html).not.toContain("supersigil-evidence-list");
    });

    it("renders provenance for rust-attribute evidence", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [criterion("req-1", "verified", 1)],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-provenance");
      expect(html).toContain("tests/req-1.rs:10");
    });

    it("renders provenance for verified-by-tag evidence", () => {
      const comp: RenderedComponent = {
        kind: "Criterion",
        id: "req-tag",
        attributes: {},
        body_text: "Tagged criterion",
        children: [],
        source_range: sourceRange(),
        verification: {
          state: "verified",
          evidence: [
            {
              test_name: "test_tagged",
              test_file: "tests/tagged.rs",
              test_kind: "unit",
              evidence_kind: "tag",
              source_line: 5,
              provenance: [
                { kind: "verified-by-tag", tag: "my-tag" },
              ],
            },
          ],
        },
      };
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [comp] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("Tag: my-tag");
    });

    it("renders provenance for verified-by-file-glob evidence", () => {
      const comp: RenderedComponent = {
        kind: "Criterion",
        id: "req-glob",
        attributes: {},
        body_text: "Glob criterion",
        children: [],
        source_range: sourceRange(),
        verification: {
          state: "verified",
          evidence: [
            {
              test_name: "test_globbed",
              test_file: "tests/glob.rs",
              test_kind: "unit",
              evidence_kind: "file-glob",
              source_line: 1,
              provenance: [
                { kind: "verified-by-file-glob", paths: ["tests/**/*.rs", "src/**/*.rs"] },
              ],
            },
          ],
        },
      };
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [comp] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("File glob: tests/**/*.rs, src/**/*.rs");
    });

    it("renders provenance for example evidence", () => {
      const comp: RenderedComponent = {
        kind: "Criterion",
        id: "req-ex",
        attributes: {},
        body_text: "Example criterion",
        children: [],
        source_range: sourceRange(),
        verification: {
          state: "verified",
          evidence: [
            {
              test_name: "test_example",
              test_file: "tests/ex.rs",
              test_kind: "unit",
              evidence_kind: "example",
              source_line: 1,
              provenance: [
                { kind: "example", example_id: "ex-123" },
              ],
            },
          ],
        },
      };
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [comp] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("Example: ex-123");
    });

    it("does not render provenance list when provenance is empty", () => {
      const comp: RenderedComponent = {
        kind: "Criterion",
        id: "req-empty-prov",
        attributes: {},
        body_text: "No provenance",
        children: [],
        source_range: sourceRange(),
        verification: {
          state: "verified",
          evidence: [
            {
              test_name: "test_no_prov",
              test_file: "tests/no_prov.rs",
              test_kind: "unit",
              evidence_kind: "rust-attribute",
              source_line: 1,
              provenance: [],
            },
          ],
        },
      };
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [comp] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).not.toContain("supersigil-provenance");
    });
  });

  // ---------------------------------------------------------------------------
  // Decision rendering
  // ---------------------------------------------------------------------------

  describe("Decision component", () => {
    function decision(): RenderedComponent {
      return {
        kind: "Decision",
        id: "dec-1",
        attributes: {},
        body_text: "Decision body text",
        children: [
          {
            kind: "Rationale",
            attributes: {},
            body_text: "The rationale text",
            children: [],
            source_range: sourceRange(),
          },
          {
            kind: "Alternative",
            id: "alt-1",
            attributes: { status: "rejected" },
            body_text: "Rejected alternative text",
            children: [],
            source_range: sourceRange(),
          },
          {
            kind: "Alternative",
            id: "alt-2",
            attributes: { status: "considered" },
            body_text: "Considered alternative text",
            children: [],
            source_range: sourceRange(),
          },
        ],
        source_range: sourceRange(),
      };
    }

    it("renders with the correct CSS class", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [decision()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('class="supersigil-decision"');
    });

    it("renders Decision label and id", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [decision()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('data-decision-id="dec-1"');
      expect(html).toContain("supersigil-decision-label");
      expect(html).toContain(">Decision<");
    });

    it("renders rationale", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [decision()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-rationale");
      expect(html).toContain("The rationale text");
    });

    it("renders alternatives with label and status", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [decision()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-alternative");
      expect(html).toContain("supersigil-alternative-label");
      expect(html).toContain(">Alternative<");
      expect(html).toContain("Rejected alternative text");
      expect(html).toContain("rejected");
      expect(html).toContain("Considered alternative text");
      expect(html).toContain("considered");
    });

    it("renders decision body text", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [decision()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("Decision body text");
    });
  });

  // ---------------------------------------------------------------------------
  // Example rendering
  // ---------------------------------------------------------------------------

  describe("Example component", () => {
    function example(): RenderedComponent {
      return {
        kind: "Example",
        id: "ex-1",
        attributes: {
          runner: "cargo-test",
          lang: "rust",
          verifies: "req-1,req-2",
        },
        body_text: undefined,
        children: [],
        source_range: sourceRange(),
      };
    }

    it("renders with the correct CSS class", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [example()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('class="supersigil-example"');
    });

    it("shows runner", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [example()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("cargo-test");
    });

    it("shows language", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [example()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("rust");
    });

    it("shows verification targets", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [example()] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("req-1");
      expect(html).toContain("req-2");
    });
  });

  // ---------------------------------------------------------------------------
  // AcceptanceCriteria rendering
  // ---------------------------------------------------------------------------

  describe("AcceptanceCriteria component", () => {
    it("renders as a wrapper with correct CSS class", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "AcceptanceCriteria",
              attributes: {},
              children: [
                criterion("req-1", "verified"),
                criterion("req-2", "unverified"),
              ],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-acceptance-criteria");
      expect(html).toContain("supersigil-criterion");
    });

    it("renders all child criteria", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "AcceptanceCriteria",
              attributes: {},
              children: [
                criterion("req-1", "verified"),
                criterion("req-2", "failing"),
              ],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain('data-criterion-id="req-1"');
      expect(html).toContain('data-criterion-id="req-2"');
    });
  });

  // ---------------------------------------------------------------------------
  // Link pill components
  // ---------------------------------------------------------------------------

  describe("Link pill components", () => {
    it("renders VerifiedBy with strategy and details", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "VerifiedBy",
              attributes: { strategy: "file-glob", paths: "src/tests.rs, tests/auth.rs" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-verified-by");
      expect(html).toContain("VerifiedBy");
      expect(html).toContain("file-glob");
      expect(html).toContain("src/tests.rs");
      expect(html).toContain("tests/auth.rs");
    });

    it("renders VerifiedBy with tag strategy", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "VerifiedBy",
              attributes: { strategy: "tag", tag: "auth:login" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-verified-by");
      expect(html).toContain("tag");
      expect(html).toContain("auth:login");
    });

    it("renders References as link pill", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "References",
              attributes: { refs: "doc-b#crit-2" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-link-pill");
      expect(html).toContain("References");
      expect(html).toContain("test://criterion/doc-b#crit-2");
    });

    it("renders DependsOn as link pill", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "DependsOn",
              attributes: { refs: "other-doc" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-link-pill");
      expect(html).toContain("DependsOn");
      expect(html).toContain("test://doc/other-doc");
    });

    it("renders Implements as link pill", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Implements",
              attributes: { refs: "parent-doc" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-link-pill");
      expect(html).toContain("Implements");
      expect(html).toContain("test://doc/parent-doc");
    });

    it("handles multiple refs separated by commas", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "References",
              attributes: { refs: "doc-a#crit-1, doc-b#crit-2" },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("test://criterion/doc-a#crit-1");
      expect(html).toContain("test://criterion/doc-b#crit-2");
    });
  });

  // ---------------------------------------------------------------------------
  // Edge rendering
  // ---------------------------------------------------------------------------

  describe("Edge rendering", () => {
    it("renders edges as link pills after fences", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [] },
      ];
      const edges: EdgeData[] = [
        { from: "doc-a", to: "doc-b", kind: "implements" },
        { from: "doc-a", to: "doc-c", kind: "references" },
      ];
      const html = renderComponentTree(fences, edges, mockLinkResolver());
      expect(html).toContain("supersigil-edges");
      expect(html).toContain("test://doc/doc-b");
      expect(html).toContain("test://doc/doc-c");
    });

    it("does not render edges section when there are no edges", () => {
      const fences: FenceData[] = [
        { byte_range: [0, 100], components: [] },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).not.toContain("supersigil-edges");
    });
  });

  // ---------------------------------------------------------------------------
  // HTML escaping
  // ---------------------------------------------------------------------------

  describe("HTML escaping", () => {
    it("escapes HTML special characters in body text", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Criterion",
              id: "xss",
              attributes: {},
              body_text: '<script>alert("xss")</script>',
              children: [],
              source_range: sourceRange(),
              verification: { state: "unverified", evidence: [] },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).not.toContain("<script>");
      expect(html).toContain("&lt;script&gt;");
    });

    it("escapes HTML special characters in attribute values", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Criterion",
              id: 'test"><img src=x onerror=alert(1)>',
              attributes: {},
              children: [],
              source_range: sourceRange(),
              verification: { state: "unverified", evidence: [] },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).not.toContain('"><img');
      expect(html).toContain("&quot;");
    });
  });

  // ---------------------------------------------------------------------------
  // Inline code rendering
  // ---------------------------------------------------------------------------

  describe("inline code", () => {
    it("renders backtick-quoted text as code elements", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Criterion",
              id: "c1",
              attributes: { id: "c1" },
              body_text:
                "THE `Config_Loader` SHALL deserialize `supersigil.toml` into a `Config` value.",
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 3, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain(
        '<code class="supersigil-inline-code">Config_Loader</code>',
      );
      expect(html).toContain(
        '<code class="supersigil-inline-code">supersigil.toml</code>',
      );
      expect(html).toContain(
        '<code class="supersigil-inline-code">Config</code>',
      );
    });

    it("still escapes HTML inside backticks", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Criterion",
              id: "c1",
              attributes: { id: "c1" },
              body_text: "Use `<script>` tag carefully.",
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 2, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain(
        '<code class="supersigil-inline-code">&lt;script&gt;</code>',
      );
      expect(html).not.toContain("<script>");
    });
  });

  // ---------------------------------------------------------------------------
  // Task rendering
  // ---------------------------------------------------------------------------

  describe("Task component", () => {
    it("renders task with status badge and implements refs", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 200],
          components: [
            {
              kind: "Task",
              id: "task-1",
              attributes: {
                id: "task-1",
                status: "done",
                implements: "auth/req#req-1-1, auth/req#req-1-2",
              },
              body_text: "Implement login endpoint",
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 5, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-task");
      expect(html).toContain("task-1");
      expect(html).toContain("supersigil-task-status--done");
      expect(html).toContain("done");
      expect(html).toContain("Implements");
      expect(html).toContain("auth/req#req-1-1");
      expect(html).toContain("auth/req#req-1-2");
      expect(html).toContain("Implement login endpoint");
    });

    it("renders task with depends", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Task",
              id: "task-2",
              attributes: {
                id: "task-2",
                status: "in-progress",
                depends: "task-1",
              },
              body_text: "Add rate limiting",
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 3, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-task-status--in-progress");
      expect(html).toContain("Depends on");
      expect(html).toContain("task-1");
    });
  });

  // ---------------------------------------------------------------------------
  // TrackedFiles rendering
  // ---------------------------------------------------------------------------

  describe("TrackedFiles component", () => {
    it("renders file paths as a collapsible list", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "TrackedFiles",
              attributes: { paths: "src/lib.rs, src/types.rs, tests/*.rs" },
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 1, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-tracked-files");
      expect(html).toContain("Tracks");
      expect(html).toContain("3 file patterns");
      expect(html).toContain("src/lib.rs");
      expect(html).toContain("src/types.rs");
      expect(html).toContain("tests/*.rs");
    });

    it("renders single file without plural", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 50],
          components: [
            {
              kind: "TrackedFiles",
              attributes: { paths: "src/main.rs" },
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 1, end_col: 1 },
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("1 file");
      expect(html).not.toContain("patterns");
    });
  });

  // ---------------------------------------------------------------------------
  // Edge deduplication
  // ---------------------------------------------------------------------------

  describe("edge deduplication", () => {
    it("does not render edges already visible as fence components", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Implements",
              attributes: { refs: "auth/req" },
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 1, end_col: 1 },
            },
          ],
        },
      ];
      const edges: EdgeData[] = [
        { from: "auth/design", to: "auth/req", kind: "implements" },
      ];
      const html = renderComponentTree(fences, edges, mockLinkResolver());
      // The edge should be filtered out since Implements refs="auth/req" is in the fence
      expect(html).not.toContain("supersigil-edge");
    });

    it("renders edges not present in fence components", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "Implements",
              attributes: { refs: "auth/req" },
              children: [],
              source_range: { start_line: 1, start_col: 1, end_line: 1, end_col: 1 },
            },
          ],
        },
      ];
      const edges: EdgeData[] = [
        { from: "auth/design", to: "auth/req", kind: "implements" },
        { from: "auth/design", to: "infra/req", kind: "references" },
      ];
      const html = renderComponentTree(fences, edges, mockLinkResolver());
      // Only the references edge should appear (implements is deduplicated)
      expect(html).toContain("supersigil-edge");
      expect(html).toContain("infra/req");
    });
  });

  // ---------------------------------------------------------------------------
  // Snapshot test for a multi-fence component tree
  // ---------------------------------------------------------------------------

  describe("snapshot: multi-fence component tree", () => {
    it("matches snapshot for a representative component tree", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 200],
          components: [
            {
              kind: "Implements",
              attributes: { refs: "parent-req" },
              children: [],
              source_range: sourceRange(),
            },
            {
              kind: "AcceptanceCriteria",
              attributes: {},
              children: [
                criterion("req-1-1", "verified", 2),
                criterion("req-1-2", "partial", 1),
                criterion("req-1-3", "unverified", 0),
              ],
              source_range: sourceRange(),
            },
          ],
        },
        {
          byte_range: [300, 500],
          components: [
            {
              kind: "Decision",
              id: "dec-shared-js",
              attributes: {},
              body_text: "Use shared TypeScript rendering.",
              children: [
                {
                  kind: "References",
                  attributes: { refs: "parent-req#req-2-1" },
                  children: [],
                  source_range: sourceRange(),
                },
                {
                  kind: "Rationale",
                  attributes: {},
                  body_text: "All consumers are JS environments.",
                  children: [],
                  source_range: sourceRange(),
                },
                {
                  kind: "Alternative",
                  id: "rust-html",
                  attributes: { status: "rejected" },
                  body_text: "Rust-emitted HTML.",
                  children: [],
                  source_range: sourceRange(),
                },
              ],
              source_range: sourceRange(),
            },
          ],
        },
        {
          byte_range: [600, 700],
          components: [
            {
              kind: "Example",
              id: "ex-render",
              attributes: {
                runner: "vitest",
                lang: "typescript",
                verifies: "req-1-1,req-1-2",
              },
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];

      const edges: EdgeData[] = [
        { from: "this-doc", to: "parent-req", kind: "implements" },
        { from: "this-doc", to: "other-doc", kind: "references" },
      ];

      const html = renderComponentTree(fences, edges, mockLinkResolver());
      expect(html).toMatchSnapshot();
    });
  });

  // ---------------------------------------------------------------------------
  // Unknown component kinds
  // ---------------------------------------------------------------------------

  describe("unknown component kinds", () => {
    it("renders unknown components with a generic wrapper", () => {
      const fences: FenceData[] = [
        {
          byte_range: [0, 100],
          components: [
            {
              kind: "UnknownThing",
              id: "unk-1",
              attributes: {},
              body_text: "Unknown body",
              children: [],
              source_range: sourceRange(),
            },
          ],
        },
      ];
      const html = renderComponentTree(fences, [], mockLinkResolver());
      expect(html).toContain("supersigil-component");
      expect(html).toContain("UnknownThing");
    });
  });
});
