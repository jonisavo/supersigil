/**
 * @module detail-panel
 * Sidebar rendering for document and component detail views.
 * Now includes full spec content via the preview kit's renderComponentTree.
 */

import { componentColor } from './graph-data.js';

/** @typedef {import('./graph-explorer.js').DocumentNode} DocumentNode */
/** @typedef {import('./graph-explorer.js').Component} Component */
/** @typedef {import('./graph-explorer.js').Edge} Edge */

/**
 * Escape a string for safe insertion into HTML.
 *
 * @param {string|null} str
 * @returns {string}
 */
function escHtml(str) {
  if (!str) return '';
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * Render one edge list item.
 *
 * @param {string} arrow - Direction arrow (e.g. "\u2190" or "\u2192").
 * @param {string} peerId - The linked document ID.
 * @param {string} kind - The edge kind label.
 * @returns {string}
 */
function edgeListItemHtml(arrow, peerId, kind) {
  return `<li class="edge-list-item"><span class="edge-list-kind">${arrow}</span><span class="edge-list-target" data-doc-id="${escHtml(peerId)}">${escHtml(peerId)}</span><span class="edge-list-kind">${escHtml(kind)}</span></li>`;
}

/**
 * Build incoming and outgoing edge groups for a given document ID.
 *
 * @param {Edge[]} edges - All edges in the graph.
 * @param {string} docId - The document ID to filter by.
 * @returns {{ incoming: Edge[], outgoing: Edge[] }}
 */
export function buildEdgeGroups(edges, docId) {
  /** @type {Edge[]} */
  const incoming = [];
  /** @type {Edge[]} */
  const outgoing = [];

  for (const edge of edges) {
    if (edge.to === docId) {
      incoming.push(edge);
    }
    if (edge.from === docId) {
      outgoing.push(edge);
    }
  }

  return { incoming, outgoing };
}

/**
 * Build the CSS class string for a badge.
 *
 * @param {'type' | 'status'} category - The badge category.
 * @param {string|null} value - The badge value (e.g., "requirements", "approved").
 * @returns {string}
 */
export function buildBadgeClass(category, value) {
  if (category === 'type') {
    const typeValue = value ?? 'unknown';
    return `badge badge-type-${typeValue}`;
  }
  const statusValue = value ?? 'draft';
  return `badge badge-status-${statusValue}`;
}

/**
 * Count criteria and their verification states from render data fences.
 *
 * @param {any[]} fences
 * @returns {{ total: number, verified: number }}
 */
function countCriteria(fences) {
  let total = 0;
  let verified = 0;

  function visit(components) {
    for (const comp of components) {
      if (comp.kind === 'Criterion') {
        total++;
        const state = comp.verification?.state ?? 'unverified';
        if (state === 'verified') verified++;
      }
      if (comp.children) visit(comp.children);
    }
  }

  for (const fence of fences) {
    if (fence.components) visit(fence.components);
  }
  return { total, verified };
}

/**
 * Create the explorer link resolver for in-panel document navigation.
 *
 * @param {string} repositoryUrl
 * @returns {Object}
 */
function createExplorerLinkResolver(repositoryUrl) {
  return {
    evidenceLink: (file, line) => `${repositoryUrl}/blob/main/${file}#L${line}`,
    documentLink: (docId) => `#/doc/${encodeURIComponent(docId)}`,
    criterionLink: (docId, _criterionId) => `#/doc/${encodeURIComponent(docId)}`,
  };
}

/**
 * Render the detail panel for the given document node with full spec content.
 *
 * @param {HTMLElement} container - The sidebar element to render into.
 * @param {DocumentNode} node - The document node to display.
 * @param {Edge[]} edges - All edges in the graph.
 * @param {any[]} renderData - The render data array from render-data.json.
 * @param {string} repositoryUrl - The base repository URL for evidence links.
 * @returns {void}
 */
export function renderDetail(container, node, edges, renderData, repositoryUrl) {
  cancelPendingClear(container);
  const { incoming, outgoing } = buildEdgeGroups(edges, node.id);

  // Build edges HTML
  let edgesHtml = '';
  if (incoming.length > 0 || outgoing.length > 0) {
    const edgeItems = [];
    for (const edge of incoming) {
      edgeItems.push(edgeListItemHtml('\u2190', edge.from, edge.kind));
    }
    for (const edge of outgoing) {
      edgeItems.push(edgeListItemHtml('\u2192', edge.to, edge.kind));
    }
    edgesHtml = `<div class="detail-section"><div class="detail-section-label">Edges</div><ul class="edge-list">${edgeItems.join('')}</ul></div>`;
  }

  // Build badge row
  const typeClass = buildBadgeClass('type', node.doc_type);
  const statusClass = buildBadgeClass('status', node.status);
  const typeLabel = node.doc_type ?? 'unknown';
  const statusLabel = node.status ?? 'draft';

  // Coverage from render data
  const renderDoc = renderData?.find((d) => d.document_id === node.id);
  let coverageHtml = '';
  if (renderDoc) {
    const { total, verified } = countCriteria(renderDoc.fences);
    if (total > 0) {
      const pct = Math.round((verified / total) * 100);
      coverageHtml = `<div class="detail-section"><div class="detail-coverage"><span class="detail-coverage-bar"><span class="detail-coverage-bar-fill" style="width: ${pct}%"></span></span><span class="detail-coverage-text">${verified}/${total} criteria verified (${pct}%)</span></div></div>`;
    }
  }

  // Trace impact button
  const traceBtn = `<div class="detail-section"><button class="detail-panel-trace-btn" data-doc-id="${escHtml(node.id)}">Trace impact</button></div>`;

  // Render spec content using the preview kit
  let specContentHtml = '';
  if (renderDoc && typeof window !== 'undefined' && window.__supersigilRender) {
    const linkResolver = createExplorerLinkResolver(repositoryUrl);
    try {
      specContentHtml = window.__supersigilRender.renderComponentTree(
        renderDoc.fences,
        renderDoc.edges,
        linkResolver,
      );
    } catch (err) {
      console.error('Failed to render spec content:', err);
      specContentHtml = '<p class="detail-spec-empty">Failed to render spec content.</p>';
    }
  }

  const specSection = specContentHtml
    ? `<div class="detail-spec-content">${specContentHtml}</div>`
    : '';

  container.innerHTML = `<div class="detail-panel-header"><div class="detail-panel-title">${escHtml(node.id)}</div><button class="detail-panel-close" aria-label="Close">\u2715</button></div><div class="detail-panel-body"><div class="detail-section"><span class="${typeClass}">${escHtml(typeLabel)}</span> <span class="${statusClass}">${escHtml(statusLabel)}</span></div>${coverageHtml}${traceBtn}${edgesHtml}${specSection}</div>`;

  container.classList.add('open');
}

/**
 * Build cross-cluster edge groups: edges where one endpoint is inside the cluster
 * and the other is outside.
 *
 * @param {Edge[]} edges - All edges in the graph.
 * @param {Set<string>} clusterDocIds - Set of document IDs in the cluster.
 * @returns {{ incoming: Edge[], outgoing: Edge[] }}
 */
export function buildClusterEdgeGroups(edges, clusterDocIds) {
  /** @type {Edge[]} */
  const incoming = [];
  /** @type {Edge[]} */
  const outgoing = [];

  for (const edge of edges) {
    const fromInside = clusterDocIds.has(edge.from);
    const toInside = clusterDocIds.has(edge.to);

    if (!fromInside && toInside) {
      incoming.push(edge);
    } else if (fromInside && !toInside) {
      outgoing.push(edge);
    }
    // Both inside = internal edge, both outside = irrelevant — skip both
  }

  return { incoming, outgoing };
}

/**
 * Render the cluster detail panel showing aggregated info for a cluster.
 *
 * @param {HTMLElement} container - The sidebar element to render into.
 * @param {string} clusterName - The cluster name.
 * @param {DocumentNode[]} documents - The documents in this cluster.
 * @param {Edge[]} edges - All edges in the graph.
 * @returns {void}
 */
export function renderClusterDetail(container, clusterName, documents, edges) {
  cancelPendingClear(container);
  const docIdSet = new Set(documents.map((d) => d.id));

  // Summary: document count
  const docCount = documents.length;

  // Type breakdown
  /** @type {Map<string, number>} */
  const typeCounts = new Map();
  for (const doc of documents) {
    const t = doc.doc_type ?? 'unknown';
    typeCounts.set(t, (typeCounts.get(t) ?? 0) + 1);
  }
  const typeBadges = [...typeCounts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(
      ([t, count]) => `<span class="${buildBadgeClass('type', t)}">${count} ${escHtml(t)}</span>`,
    )
    .join(' ');

  // Status breakdown
  /** @type {Map<string, number>} */
  const statusCounts = new Map();
  for (const doc of documents) {
    const s = doc.status ?? 'draft';
    statusCounts.set(s, (statusCounts.get(s) ?? 0) + 1);
  }
  const statusBadges = [...statusCounts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(
      ([s, count]) => `<span class="${buildBadgeClass('status', s)}">${count} ${escHtml(s)}</span>`,
    )
    .join(' ');

  // Components summary (total counts by kind across all docs)
  /** @type {Map<string, number>} */
  const compKindCounts = new Map();
  for (const doc of documents) {
    for (const comp of doc.components) {
      if (comp.id) {
        compKindCounts.set(comp.kind, (compKindCounts.get(comp.kind) ?? 0) + 1);
      }
    }
  }
  let componentsHtml = '';
  if (compKindCounts.size > 0) {
    const compItems = [...compKindCounts.entries()]
      .sort((a, b) => b[1] - a[1])
      .map(
        ([kind, count]) =>
          `<li class="component-item"><span class="component-item-kind" style="border-color: ${componentColor(kind)}; color: ${componentColor(kind)}">${escHtml(kind)}</span><span class="component-item-id">${count}</span></li>`,
      )
      .join('');
    componentsHtml = `<div class="detail-section"><div class="detail-section-label">Components</div><ul class="component-list">${compItems}</ul></div>`;
  }

  // Cross-cluster edges
  const { incoming, outgoing } = buildClusterEdgeGroups(edges, docIdSet);

  let edgesHtml = '';
  if (incoming.length > 0 || outgoing.length > 0) {
    const edgeItems = [];

    if (incoming.length > 0) {
      edgeItems.push(
        `<li class="edge-list-item" style="margin-bottom: 0.25rem"><strong style="font-size: 0.75rem; color: var(--text-muted)">Incoming (${incoming.length})</strong></li>`,
      );
      for (const edge of incoming) {
        edgeItems.push(edgeListItemHtml('\u2190', edge.from, edge.kind));
      }
    }

    if (outgoing.length > 0) {
      edgeItems.push(
        `<li class="edge-list-item" style="margin-top: 0.5rem; margin-bottom: 0.25rem"><strong style="font-size: 0.75rem; color: var(--text-muted)">Outgoing (${outgoing.length})</strong></li>`,
      );
      for (const edge of outgoing) {
        edgeItems.push(edgeListItemHtml('\u2192', edge.to, edge.kind));
      }
    }

    edgesHtml = `<div class="detail-section"><div class="detail-section-label">Cross-cluster Edges</div><ul class="edge-list">${edgeItems.join('')}</ul></div>`;
  }

  container.innerHTML = `<div class="detail-panel-header"><div class="detail-panel-title">${escHtml(clusterName)}</div><button class="detail-panel-close" aria-label="Close">\u2715</button></div><div class="detail-panel-body"><div class="detail-section"><div class="detail-section-label">Summary</div><div class="detail-section-content">${docCount} documents</div></div><div class="detail-section"><div class="detail-section-label">Types</div><div class="detail-section-content">${typeBadges}</div></div><div class="detail-section"><div class="detail-section-label">Status</div><div class="detail-section-content">${statusBadges}</div></div>${componentsHtml}${edgesHtml}</div>`;

  container.classList.add('open');
}

/**
 * Cancel any pending async clear on the panel. Called before rendering
 * new content so the old cleanup doesn't wipe it.
 *
 * @param {HTMLElement} container
 */
function cancelPendingClear(container) {
  const timer = /** @type {any} */ (container)._clearTimer;
  if (timer) {
    clearTimeout(timer);
    /** @type {any} */ (container)._clearTimer = null;
  }
  const handler = /** @type {any} */ (container)._clearHandler;
  if (handler) {
    container.removeEventListener('transitionend', handler);
    /** @type {any} */ (container)._clearHandler = null;
  }
}

/**
 * Clear the detail panel.
 *
 * @param {HTMLElement} container
 * @returns {void}
 */
export function clearDetail(container) {
  cancelPendingClear(container);
  container.classList.remove('open');
  container.innerHTML = '';
}

/**
 * Render the empty state for the spec panel.
 *
 * @param {HTMLElement} container
 * @returns {void}
 */
/**
 * Render the default panel showing the document index grouped by project.
 * Falls back to a hint message if no data is provided.
 */
export function renderEmpty(container, graphData, renderData) {
  if (!graphData || !graphData.documents || graphData.documents.length === 0) {
    container.innerHTML = `<div class="detail-spec-empty"><div>Select a document in the graph<br/>to view its specification</div><div class="detail-spec-empty-hint">Click a node or use / to search</div></div>`;
    return;
  }

  // Build coverage map from render data (reuses countCriteria)
  const coverageMap = new Map();
  if (renderData) {
    for (const doc of renderData) {
      const cov = countCriteria(doc.fences || []);
      if (cov.total > 0) {
        coverageMap.set(doc.document_id, cov);
      }
    }
  }

  // Detect multi-project: any document has a project field
  const isMultiProject = graphData.documents.some((d) => d.project);

  // Group documents: Project → Prefix → Documents
  // In single-project mode, skip the project layer.
  const typeOrder = { requirements: 0, design: 1, tasks: 2, adr: 3, documentation: 4 };

  function docPrefix(id) {
    const slashIdx = id.indexOf('/');
    return slashIdx > 0 ? id.substring(0, slashIdx) : id;
  }

  function docSuffix(id) {
    const slashIdx = id.indexOf('/');
    return slashIdx > 0 ? id.substring(slashIdx + 1) : id;
  }

  // Build nested structure: Map<project, Map<prefix, doc[]>>
  const tree = new Map();
  for (const doc of graphData.documents) {
    const project = isMultiProject ? (doc.project || '(ungrouped)') : '(all)';
    const prefix = docPrefix(doc.id);
    if (!tree.has(project)) tree.set(project, new Map());
    const prefixMap = tree.get(project);
    if (!prefixMap.has(prefix)) prefixMap.set(prefix, []);
    prefixMap.get(prefix).push(doc);
  }

  // Sort projects
  const sortedProjects = [...tree.entries()].sort((a, b) => a[0].localeCompare(b[0]));

  // Global stats
  let globalTotal = 0;
  let globalVerified = 0;
  for (const { total, verified } of coverageMap.values()) {
    globalTotal += total;
    globalVerified += verified;
  }
  const globalPct = globalTotal > 0 ? Math.round((globalVerified / globalTotal) * 100) : 0;

  let html = `<div class="detail-panel-header"><div class="detail-panel-title">Spec Index</div></div>`;
  html += `<div class="detail-panel-body">`;

  // Global coverage
  if (globalTotal > 0) {
    html += `<div class="detail-index-coverage">${globalVerified}/${globalTotal} criteria verified (${globalPct}%)<div class="detail-coverage-bar"><div class="detail-coverage-fill" style="width:${globalPct}%"></div></div></div>`;
  }

  html += `<div class="detail-index-hint">Click a document to view its specification</div>`;

  function renderDocList(docs) {
    let out = '';
    docs.sort((a, b) => {
      const ta = typeOrder[a.doc_type] ?? 5;
      const tb = typeOrder[b.doc_type] ?? 5;
      return ta !== tb ? ta - tb : a.id.localeCompare(b.id);
    });
    for (const doc of docs) {
      const cov = coverageMap.get(doc.id);
      const covLabel = cov ? ` ${cov.verified}/${cov.total}` : '';
      const typeLabel = doc.doc_type || '';
      const statusLabel = doc.status || '';

      out += `<a href="#/doc/${encodeURIComponent(doc.id)}" class="detail-index-doc">`;
      out += `<span class="detail-index-doc-id">${escHtml(docSuffix(doc.id))}</span>`;
      out += `<span class="detail-index-doc-meta">`;
      if (typeLabel) out += `<span class="detail-badge detail-badge-type-${escHtml(typeLabel)}">${escHtml(typeLabel)}</span>`;
      if (statusLabel) out += `<span class="detail-badge detail-badge-status">${escHtml(statusLabel)}</span>`;
      if (covLabel) out += `<span class="detail-index-doc-cov">${covLabel}</span>`;
      out += `</span>`;
      out += `</a>`;
    }
    return out;
  }

  for (const [project, prefixMap] of sortedProjects) {
    const sortedPrefixes = [...prefixMap.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    const totalDocs = [...prefixMap.values()].reduce((n, d) => n + d.length, 0);

    if (isMultiProject) {
      // Project level
      html += `<details class="detail-index-project" open>`;
      html += `<summary class="detail-index-project-title">${escHtml(project)} <span class="detail-index-group-count">${totalDocs}</span></summary>`;
    }

    // Prefix groups within the project
    for (const [prefix, docs] of sortedPrefixes) {
      html += `<details class="detail-index-group" open>`;
      html += `<summary class="detail-index-group-title">${escHtml(prefix)} <span class="detail-index-group-count">${docs.length}</span></summary>`;
      html += renderDocList(docs);
      html += `</details>`;
    }

    if (isMultiProject) {
      html += `</details>`;
    }
  }

  html += `</div>`;
  container.innerHTML = html;
}
