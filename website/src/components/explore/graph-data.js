/**
 * @module graph-data
 * Data loading, filtering, and search logic for the graph explorer.
 */

/** @typedef {import('./graph-explorer.js').GraphJSON} GraphJSON */
/** @typedef {import('./graph-explorer.js').DocumentNode} DocumentNode */
/** @typedef {import('./graph-explorer.js').Edge} Edge */

/**
 * Load graph data from a URL.
 *
 * @param {string} url - The URL to fetch graph JSON from.
 * @returns {Promise<GraphJSON>}
 */
export async function loadGraph(url) {
  const res = await fetch(url);
  return res.json();
}

/**
 * Extract unique filter options from a set of documents.
 *
 * @param {DocumentNode[]} documents
 * @returns {{ types: string[], statuses: string[] }}
 */
export function extractFilterOptions(documents) {
  /** @type {Set<string>} */
  const typeSet = new Set();
  /** @type {Set<string>} */
  const statusSet = new Set();

  for (const doc of documents) {
    if (doc.doc_type != null) typeSet.add(doc.doc_type);
    if (doc.status != null) statusSet.add(doc.status);
  }

  return {
    types: [...typeSet].sort(),
    statuses: [...statusSet].sort(),
  };
}

/**
 * Filter documents by doc_type and/or status.
 * Returns a Set of visible document IDs.
 *
 * @param {DocumentNode[]} documents
 * @param {{ types: Set<string>, status: string|null }} filters
 * @returns {Set<string>}
 */
export function filterDocuments(documents, filters) {
  const { types, status } = filters;
  const hasTypeFilter = types && types.size > 0;
  const hasStatusFilter = status != null && status !== 'all';

  /** @type {Set<string>} */
  const visible = new Set();

  for (const doc of documents) {
    if (hasTypeFilter && (doc.doc_type == null || !types.has(doc.doc_type))) continue;
    if (hasStatusFilter && doc.status !== status) continue;
    visible.add(doc.id);
  }

  return visible;
}

/**
 * Search documents by title (case-insensitive substring match).
 *
 * @param {DocumentNode[]} documents
 * @param {string} query
 * @returns {DocumentNode[]}
 */
/**
 * Return the CSS color variable for a component kind.
 * Shared by the graph renderer (node stroke) and the detail panel (indicator dot).
 *
 * @param {string} kind
 * @returns {string}
 */
export function componentColor(kind) {
  switch (kind) {
    case 'Criterion':
      return 'var(--teal)';
    case 'Task':
      return 'var(--green)';
    case 'Decision':
      return 'var(--gold)';
    case 'Rationale':
    case 'Alternative':
      return 'var(--text-muted)';
    default:
      return 'var(--text-dim)';
  }
}

export function searchDocuments(documents, query) {
  const q = query.trim().toLowerCase();
  if (q === '') return [];

  /** @type {{ doc: DocumentNode, rank: number }[]} */
  const scored = [];

  for (const doc of documents) {
    const idLower = doc.id.toLowerCase();
    const titleLower = (doc.title ?? '').toLowerCase();

    const idMatch = idLower.includes(q);
    const titleMatch = titleLower.includes(q);

    if (!idMatch && !titleMatch) continue;

    // Rank: exact ID match = 0, ID substring match = 1, title-only match = 2
    let rank;
    if (idLower === q) {
      rank = 0;
    } else if (idMatch) {
      rank = 1;
    } else {
      rank = 2;
    }

    scored.push({ doc, rank });
  }

  scored.sort((a, b) => a.rank - b.rank);
  return scored.map((s) => s.doc);
}
