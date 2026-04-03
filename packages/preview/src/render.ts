import type {
  FenceData,
  EdgeData,
  LinkResolver,
  RenderedComponent,
  EvidenceEntry,
  ProvenanceEntry,
} from "./types.js";

export { type FenceData, type EdgeData, type LinkResolver } from "./types.js";
export type { RenderedComponent } from "./types.js";

// ---------------------------------------------------------------------------
// HTML utilities
// ---------------------------------------------------------------------------

function esc(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Escape HTML then convert backtick-quoted spans to `<code>` elements. */
function escWithInlineCode(text: string): string {
  const escaped = esc(text);
  return escaped.replace(/`([^`]+)`/g, '<code class="supersigil-inline-code">$1</code>');
}

// ---------------------------------------------------------------------------
// Link ref parsing
// ---------------------------------------------------------------------------

/** Parse a `refs` attribute value like `"doc-a#crit-1, doc-b"` into individual refs. */
function parseRefs(refs: string): string[] {
  return refs
    .split(",")
    .map((r) => r.trim())
    .filter((r) => r.length > 0);
}

/** Resolve a single ref string to a URL using the link resolver. */
function resolveRef(ref: string, linkResolver: LinkResolver): string {
  if (ref.includes("#")) {
    const idx = ref.indexOf("#");
    const docId = ref.slice(0, idx);
    const criterionId = ref.slice(idx + 1);
    return linkResolver.criterionLink(docId, criterionId);
  }
  return linkResolver.documentLink(ref);
}

// ---------------------------------------------------------------------------
// Component renderers
// ---------------------------------------------------------------------------

function renderCriterion(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const id = component.id ?? "";
  const state = component.verification?.state ?? "unverified";
  const evidence = component.verification?.evidence ?? [];
  const bodyText = component.body_text ?? "";

  const badgeLabel = state.charAt(0).toUpperCase() + state.slice(1);
  const evidenceCount = evidence.length;
  const tooltipText = `${badgeLabel}: ${evidenceCount} evidence item${evidenceCount !== 1 ? "s" : ""}`;

  let html = `<div class="supersigil-criterion" data-criterion-id="${esc(id)}">`;
  html += `<div class="supersigil-criterion-header">`;
  html += `<span class="supersigil-badge supersigil-badge--${esc(state)}" title="${esc(tooltipText)}">${esc(badgeLabel)}</span>`;
  if (id) {
    html += `<code class="supersigil-criterion-id">${esc(id)}</code>`;
  }
  html += `</div>`;

  if (bodyText) {
    html += `<p class="supersigil-criterion-body">${escWithInlineCode(bodyText)}</p>`;
  }

  if (evidence.length > 0) {
    html += renderEvidenceSection(evidence, linkResolver);
  }

  // Render children, but skip VerifiedBy when evidence is already shown
  // (the evidence section surfaces the same info with more detail)
  const hasEvidence = evidence.length > 0;
  for (const child of component.children) {
    if (hasEvidence && child.kind === "VerifiedBy") continue;
    html += renderComponent(child, linkResolver);
  }

  html += `</div>`;
  return html;
}

function renderProvenanceEntry(
  entry: ProvenanceEntry,
  linkResolver: LinkResolver,
): string {
  switch (entry.kind) {
    case "verified-by-tag":
      return `<span class="supersigil-provenance-entry">Tag: ${esc(entry.tag)}</span>`;
    case "verified-by-file-glob":
      return `<span class="supersigil-provenance-entry">File glob: ${esc(entry.paths.join(", "))}</span>`;
    case "rust-attribute": {
      const link = linkResolver.evidenceLink(entry.file, entry.line);
      return `<a href="${esc(link)}" class="supersigil-provenance-entry">${esc(entry.file)}:${entry.line}</a>`;
    }
    case "example":
      return `<span class="supersigil-provenance-entry">Example: ${esc(entry.example_id)}</span>`;
  }
}

function renderProvenance(
  provenance: ProvenanceEntry[],
  linkResolver: LinkResolver,
): string {
  if (provenance.length === 0) return "";

  let html = `<ul class="supersigil-provenance">`;
  for (const entry of provenance) {
    html += `<li>${renderProvenanceEntry(entry, linkResolver)}</li>`;
  }
  html += `</ul>`;
  return html;
}

function renderEvidenceSection(
  evidence: EvidenceEntry[],
  linkResolver: LinkResolver,
): string {
  let html = `<div class="supersigil-evidence">`;
  html += `<button class="supersigil-evidence-toggle" aria-expanded="false">`;
  html += `Evidence (${evidence.length})`;
  html += `</button>`;
  html += `<ul class="supersigil-evidence-list" hidden>`;

  for (const entry of evidence) {
    const link = linkResolver.evidenceLink(entry.test_file, entry.source_line);
    html += `<li class="supersigil-evidence-item">`;
    html += `<a href="${esc(link)}" class="supersigil-evidence-link">`;
    html += `<code>${esc(entry.test_name)}</code>`;
    html += `</a>`;
    html += `<span class="supersigil-evidence-meta">`;
    html += `<span class="supersigil-test-kind">${esc(entry.test_kind)}</span>`;
    html += `<span class="supersigil-evidence-kind">${esc(entry.evidence_kind)}</span>`;
    html += `</span>`;
    html += renderProvenance(entry.provenance, linkResolver);
    html += `</li>`;
  }

  html += `</ul>`;
  html += `</div>`;
  return html;
}

function renderDecision(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const id = component.id ?? "";
  const bodyText = component.body_text ?? "";

  let html = `<div class="supersigil-decision" data-decision-id="${esc(id)}">`;

  html += `<div class="supersigil-decision-header">`;
  html += `<span class="supersigil-decision-label">Decision</span>`;
  if (id) {
    html += ` <code class="supersigil-decision-id">${esc(id)}</code>`;
  }
  html += `</div>`;

  if (bodyText) {
    html += `<p class="supersigil-decision-body">${escWithInlineCode(bodyText)}</p>`;
  }

  for (const child of component.children) {
    if (child.kind === "Rationale") {
      html += renderRationale(child);
    } else if (child.kind === "Alternative") {
      html += renderAlternative(child);
    } else {
      html += renderComponent(child, linkResolver);
    }
  }

  html += `</div>`;
  return html;
}

function renderRationale(component: RenderedComponent): string {
  const bodyText = component.body_text ?? "";
  let html = `<div class="supersigil-rationale">`;
  html += `<div class="supersigil-rationale-label">Rationale</div>`;
  if (bodyText) {
    html += `<p>${escWithInlineCode(bodyText)}</p>`;
  }
  html += `</div>`;
  return html;
}

function renderAlternative(component: RenderedComponent): string {
  const id = component.id ?? "";
  const status = component.attributes.status ?? "";
  const bodyText = component.body_text ?? "";

  let html = `<div class="supersigil-alternative" data-status="${esc(status)}">`;
  html += `<div class="supersigil-alternative-header">`;
  html += `<span class="supersigil-alternative-label">Alternative</span>`;
  if (id) {
    html += ` <code class="supersigil-alternative-id">${esc(id)}</code>`;
  }
  if (status) {
    html += ` <span class="supersigil-alternative-status">${esc(status)}</span>`;
  }
  html += `</div>`;
  if (bodyText) {
    html += `<p>${escWithInlineCode(bodyText)}</p>`;
  }
  html += `</div>`;
  return html;
}

function renderExample(component: RenderedComponent): string {
  const id = component.id ?? "";
  const runner = component.attributes.runner ?? "";
  const language = component.attributes.lang ?? "";
  const targets = component.attributes.verifies ?? "";

  let html = `<div class="supersigil-example"`;
  if (id) {
    html += ` data-example-id="${esc(id)}"`;
  }
  html += `>`;

  html += `<div class="supersigil-example-header">`;
  if (id) {
    html += `<code class="supersigil-example-id">${esc(id)}</code>`;
  }
  html += `</div>`;

  html += `<div class="supersigil-example-meta">`;
  if (runner) {
    html += `<span class="supersigil-example-runner"><span class="supersigil-label">Runner:</span> ${esc(runner)}</span>`;
  }
  if (language) {
    html += `<span class="supersigil-example-language"><span class="supersigil-label">Language:</span> ${esc(language)}</span>`;
  }
  if (targets) {
    html += `<span class="supersigil-example-targets"><span class="supersigil-label">Targets:</span> ${esc(targets)}</span>`;
  }
  html += `</div>`;

  html += `</div>`;
  return html;
}

function renderAcceptanceCriteria(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  let html = `<div class="supersigil-acceptance-criteria">`;
  for (const child of component.children) {
    html += renderComponent(child, linkResolver);
  }
  html += `</div>`;
  return html;
}

function renderLinkPill(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const kind = component.kind;
  const refs = component.attributes.refs ?? "";
  const parsedRefs = parseRefs(refs);

  let html = `<span class="supersigil-link-pill supersigil-link-pill--${esc(kind.toLowerCase())}">`;
  html += `<span class="supersigil-link-pill-label">${esc(kind)}</span>`;

  for (const ref of parsedRefs) {
    const url = resolveRef(ref, linkResolver);
    html += `<a href="${esc(url)}" class="supersigil-link-pill-ref">${esc(ref)}</a>`;
  }

  html += `</span>`;
  return html;
}

function renderVerifiedBy(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const strategy = component.attributes.strategy ?? "";
  const tag = component.attributes.tag ?? "";
  const paths = component.attributes.paths ?? "";

  let html = `<div class="supersigil-verified-by">`;
  html += `<span class="supersigil-verified-by-label">VerifiedBy</span> `;

  if (strategy === "tag" && tag) {
    html += `<span class="supersigil-verified-by-strategy">tag</span> `;
    html += `<code class="supersigil-inline-code">${esc(tag)}</code>`;
  } else if (strategy === "file-glob" && paths) {
    const pathList = parseRefs(paths);
    html += `<span class="supersigil-verified-by-strategy">file-glob</span> `;
    for (let i = 0; i < pathList.length; i++) {
      if (i > 0) html += ", ";
      const url = linkResolver.evidenceLink(pathList[i], 1);
      html += `<a href="${esc(url)}" class="supersigil-verified-by-path"><code class="supersigil-inline-code">${esc(pathList[i])}</code></a>`;
    }
  } else if (strategy) {
    html += `<span class="supersigil-verified-by-strategy">${esc(strategy)}</span>`;
  }

  html += `</div>`;
  return html;
}

function renderTrackedFiles(component: RenderedComponent): string {
  const paths = parseRefs(component.attributes.paths ?? "");

  if (paths.length === 0) {
    return `<div class="supersigil-tracked-files"><span class="supersigil-tracked-files-label">TrackedFiles</span> <span class="supersigil-text-muted">(none)</span></div>`;
  }

  let html = `<details class="supersigil-tracked-files">`;
  html += `<summary class="supersigil-tracked-files-summary">`;
  html += `<span class="supersigil-tracked-files-label">Tracks</span> `;
  html += `<span class="supersigil-tracked-files-count">${paths.length} file${paths.length === 1 ? "" : " pattern"}${paths.length === 1 ? "" : "s"}</span>`;
  html += `</summary>`;
  html += `<ul class="supersigil-tracked-files-list">`;
  for (const p of paths) {
    html += `<li><code class="supersigil-inline-code">${esc(p)}</code></li>`;
  }
  html += `</ul>`;
  html += `</details>`;
  return html;
}

function renderTask(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const id = component.id ?? "";
  const status = component.attributes.status ?? "";
  const implementsAttr = component.attributes.implements ?? "";
  const dependsAttr = component.attributes.depends ?? "";
  const bodyText = component.body_text ?? "";

  const statusClass = status
    ? `supersigil-task-status--${esc(status.toLowerCase().replace(/\s+/g, "-"))}`
    : "";

  let html = `<div class="supersigil-task" data-id="${esc(id)}" data-status="${esc(status)}">`;

  // Header: id + status badge
  html += `<div class="supersigil-task-header">`;
  html += `<code class="supersigil-task-id">${esc(id)}</code>`;
  if (status) {
    html += ` <span class="supersigil-task-status ${statusClass}">${esc(status)}</span>`;
  }
  html += `</div>`;

  // Body text
  if (bodyText) {
    html += `<p class="supersigil-task-body">${escWithInlineCode(bodyText)}</p>`;
  }

  // Implements refs
  if (implementsAttr) {
    const refs = parseRefs(implementsAttr);
    html += `<div class="supersigil-task-implements">`;
    html += `<span class="supersigil-task-implements-label">Implements</span> `;
    for (let i = 0; i < refs.length; i++) {
      if (i > 0) html += ", ";
      const url = resolveRef(refs[i], linkResolver);
      html += `<a href="${esc(url)}" class="supersigil-task-ref">${esc(refs[i])}</a>`;
    }
    html += `</div>`;
  }

  // Depends refs
  if (dependsAttr) {
    const deps = parseRefs(dependsAttr);
    html += `<div class="supersigil-task-depends">`;
    html += `<span class="supersigil-task-depends-label">Depends on</span> `;
    html += deps.map((d: string) => `<code class="supersigil-inline-code">${esc(d)}</code>`).join(", ");
    html += `</div>`;
  }

  // Child tasks
  for (const child of component.children) {
    html += renderComponent(child, linkResolver);
  }

  html += `</div>`;
  return html;
}

function renderGenericComponent(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  const id = component.id ?? "";
  const bodyText = component.body_text ?? "";

  let html = `<div class="supersigil-component" data-kind="${esc(component.kind)}"`;
  if (id) {
    html += ` data-id="${esc(id)}"`;
  }
  html += `>`;

  html += `<div class="supersigil-component-header">`;
  html += `<span class="supersigil-component-kind">${esc(component.kind)}</span>`;
  if (id) {
    html += `<code class="supersigil-component-id">${esc(id)}</code>`;
  }
  html += `</div>`;

  if (bodyText) {
    html += `<p>${escWithInlineCode(bodyText)}</p>`;
  }

  for (const child of component.children) {
    html += renderComponent(child, linkResolver);
  }

  html += `</div>`;
  return html;
}

// ---------------------------------------------------------------------------
// Component dispatcher
// ---------------------------------------------------------------------------

const LINK_PILL_KINDS = new Set([
  "References",
  "DependsOn",
  "Implements",
]);

function renderComponent(
  component: RenderedComponent,
  linkResolver: LinkResolver,
): string {
  switch (component.kind) {
    case "Criterion":
      return renderCriterion(component, linkResolver);
    case "Decision":
      return renderDecision(component, linkResolver);
    case "Example":
      return renderExample(component);
    case "AcceptanceCriteria":
      return renderAcceptanceCriteria(component, linkResolver);
    case "Rationale":
      return renderRationale(component);
    case "Alternative":
      return renderAlternative(component);
    case "Task":
      return renderTask(component, linkResolver);
    case "TrackedFiles":
      return renderTrackedFiles(component);
    case "VerifiedBy":
      return renderVerifiedBy(component, linkResolver);
    default:
      if (LINK_PILL_KINDS.has(component.kind)) {
        return renderLinkPill(component, linkResolver);
      }
      return renderGenericComponent(component, linkResolver);
  }
}

// ---------------------------------------------------------------------------
// Edge rendering
// ---------------------------------------------------------------------------

function renderEdges(
  edges: EdgeData[],
  linkResolver: LinkResolver,
): string {
  if (edges.length === 0) return "";

  let html = `<div class="supersigil-edges">`;
  for (const edge of edges) {
    const url = linkResolver.documentLink(edge.to);
    html += `<a href="${esc(url)}" class="supersigil-edge supersigil-edge--${esc(edge.kind.toLowerCase())}">`;
    html += `<span class="supersigil-edge-kind">${esc(edge.kind)}</span>`;
    html += `<span class="supersigil-edge-target">${esc(edge.to)}</span>`;
    html += `</a>`;
  }
  html += `</div>`;
  return html;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Pure function that renders a component tree to an HTML string.
 *
 * Accepts fence-grouped component data, graph edges, and a host-provided
 * link resolver. Returns a self-contained HTML fragment suitable for
 * embedding in a preview pane.
 */
/** Filter edges to only those not already visible as fence components. */
export function filterNovelEdges(
  fences: FenceData[],
  edges: EdgeData[],
): EdgeData[] {
  const seen = collectFenceRelationships(fences);
  return edges.filter((e) => !seen.has(`${e.kind.toLowerCase()}:${e.to}`));
}

/** Collect relationship refs already visible as components in fences. */
function collectFenceRelationships(fences: FenceData[]): Set<string> {
  const seen = new Set<string>();

  function walk(components: RenderedComponent[]): void {
    for (const c of components) {
      if (LINK_PILL_KINDS.has(c.kind)) {
        const refs = parseRefs(c.attributes.refs ?? "");
        for (const ref of refs) {
          // Normalize: strip fragment to get doc-level ref
          const docRef = ref.includes("#") ? ref.split("#")[0] : ref;
          seen.add(`${c.kind.toLowerCase()}:${docRef}`);
        }
      }
      walk(c.children);
    }
  }

  for (const fence of fences) {
    walk(fence.components);
  }
  return seen;
}

export function renderComponentTree(
  fences: FenceData[],
  edges: EdgeData[],
  linkResolver: LinkResolver,
): string {
  if (fences.length === 0 && edges.length === 0) return "";

  let html = "";

  for (const fence of fences) {
    html += `<div class="supersigil-block">`;
    for (const component of fence.components) {
      html += renderComponent(component, linkResolver);
    }
    html += `</div>`;
  }

  // Filter out edges that are already visible as fence components
  const fenceRels = collectFenceRelationships(fences);
  const novelEdges = edges.filter(
    (e) => !fenceRels.has(`${e.kind.toLowerCase()}:${e.to}`),
  );
  html += renderEdges(novelEdges, linkResolver);

  return html;
}
