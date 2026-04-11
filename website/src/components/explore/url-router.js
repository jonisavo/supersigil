/**
 * @module url-router
 * Hash-based deep-linking for the graph explorer.
 * Encodes selected node, trace state, and filter state in the URL hash
 * so that links are shareable and bookmarkable.
 */

/**
 * @typedef {Object} RouterState
 * @property {string|null} doc - Selected document ID.
 * @property {boolean} trace - Whether impact trace is active.
 * @property {Object|null} filter
 * @property {string[]} filter.types - Active type filters.
 * @property {string|null} filter.status - Active status filter.
 */

/** @returns {RouterState} */
function defaultState() {
  return { doc: null, trace: false, filter: null };
}

/**
 * Parse a filter segment string like "type:requirements,design;status:approved"
 * into a filter object.
 *
 * @param {string} filterStr
 * @returns {{ types: string[], status: string|null } | null}
 */
function parseFilterSegment(filterStr) {
  if (!filterStr) return null;

  /** @type {string[]} */
  let types = [];
  /** @type {string|null} */
  let status = null;

  const pairs = filterStr.split(';');
  for (const pair of pairs) {
    const colonIdx = pair.indexOf(':');
    if (colonIdx === -1) continue;
    const key = pair.slice(0, colonIdx);
    const value = pair.slice(colonIdx + 1);
    if (key === 'type') {
      types = value.split(',').filter((v) => v.length > 0);
    } else if (key === 'status') {
      status = value || null;
    }
  }

  if (types.length === 0 && status === null) return null;
  return { types, status };
}

/**
 * Build a filter segment string from a filter object.
 *
 * @param {{ types: string[], status: string|null }} filter
 * @returns {string} The filter segment (without leading "/filter/"), or empty string.
 */
function buildFilterSegment(filter) {
  const parts = [];
  if (filter.types.length > 0) {
    parts.push(`type:${filter.types.join(',')}`);
  }
  if (filter.status) {
    parts.push(`status:${filter.status}`);
  }
  return parts.join(';');
}

/**
 * Parse a URL hash string into router state.
 *
 * Supported patterns:
 * - `#/doc/{id}` — select a document
 * - `#/doc/{id}/trace` — select a document with impact trace active
 * - `#/filter/type:{csv};status:{value}` — filter state
 * - `#/doc/{id}/filter/...` — document + filter
 * - `#/doc/{id}/trace/filter/...` — document + trace + filter
 *
 * @param {string} hash - The hash string (e.g., location.hash).
 * @returns {RouterState}
 */
export function parseHash(hash) {
  const state = defaultState();

  // Strip leading '#'
  let path = hash.startsWith('#') ? hash.slice(1) : hash;
  // Strip leading '/'
  if (path.startsWith('/')) path = path.slice(1);

  if (!path) return state;

  // Must start with a known segment
  if (path.startsWith('doc/')) {
    // Extract doc ID: consume greedily until we hit /trace or /filter or end
    const rest = path.slice(4); // after "doc/"

    // Find /trace or /filter boundaries
    const traceIdx = rest.indexOf('/trace');
    const filterIdx = rest.indexOf('/filter/');

    // Determine where the doc ID ends
    let docEnd = rest.length;

    if (traceIdx !== -1 && filterIdx !== -1) {
      docEnd = Math.min(traceIdx, filterIdx);
    } else if (traceIdx !== -1) {
      docEnd = traceIdx;
    } else if (filterIdx !== -1) {
      docEnd = filterIdx;
    }

    // But we need to be careful: /trace must be a complete segment boundary.
    // Find the actual /trace that's at a segment boundary (followed by end, /, or /filter).
    // We look for "/trace" where the next char is end, "/" followed by "filter/", or nothing.
    let actualTraceIdx = -1;
    let searchFrom = 0;
    while (searchFrom < rest.length) {
      const idx = rest.indexOf('/trace', searchFrom);
      if (idx === -1) break;
      const afterTrace = idx + 6; // length of "/trace"
      // Valid if at end of string, or followed by "/filter/"
      if (afterTrace === rest.length || rest.slice(afterTrace).startsWith('/filter/')) {
        actualTraceIdx = idx;
        break;
      }
      searchFrom = idx + 1;
    }

    let actualFilterIdx = -1;
    if (actualTraceIdx !== -1) {
      // Filter comes after /trace
      const afterTrace = rest.slice(actualTraceIdx + 6);
      if (afterTrace.startsWith('/filter/')) {
        actualFilterIdx = actualTraceIdx + 6 + 8; // skip "/filter/"
      }
      docEnd = actualTraceIdx;
    } else {
      // Look for /filter/ as end of doc ID
      searchFrom = 0;
      while (searchFrom < rest.length) {
        const idx = rest.indexOf('/filter/', searchFrom);
        if (idx === -1) break;
        actualFilterIdx = idx + 8; // skip "/filter/"
        docEnd = idx;
        break;
      }
    }

    const rawDoc = rest.slice(0, docEnd);
    state.doc = rawDoc ? decodeURIComponent(rawDoc) : null;
    state.trace = actualTraceIdx !== -1;

    if (actualFilterIdx !== -1) {
      state.filter = parseFilterSegment(rest.slice(actualFilterIdx));
    }
  } else if (path.startsWith('filter/')) {
    const filterStr = path.slice(7); // after "filter/"
    state.filter = parseFilterSegment(filterStr);
  }
  // Any other path prefix is ignored (returns default state)

  return state;
}

/**
 * Generate a hash string from a router state object.
 * Produces a minimal hash (omits empty segments).
 *
 * @param {RouterState} state
 * @returns {string} The hash string, or empty string if state is default.
 */
export function buildHash(state) {
  let hash = '';

  if (state.doc) {
    hash += `/doc/${state.doc}`;
    if (state.trace) {
      hash += '/trace';
    }
  }

  if (state.filter) {
    const filterStr = buildFilterSegment(state.filter);
    if (filterStr) {
      hash += `/filter/${filterStr}`;
    }
  }

  return hash ? `#${hash}` : '';
}

/**
 * Register a callback to be invoked whenever the URL hash changes.
 *
 * @param {(state: RouterState) => void} callback
 * @returns {() => void} An unsubscribe function.
 */
export function onHashChange(callback) {
  if (typeof window === 'undefined') return () => {};

  /** @param {HashChangeEvent} _event */
  function handler(_event) {
    const state = parseHash(location.hash);
    callback(state);
  }
  window.addEventListener('hashchange', handler);
  return () => window.removeEventListener('hashchange', handler);
}
