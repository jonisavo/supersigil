import { describe, expect, it } from 'vitest';
import { extractFilterOptions, filterDocuments, searchDocuments } from './graph-data.js';

// ===== Test fixtures =====

/** @returns {import('./graph-explorer.js').DocumentNode[]} */
function makeDocs() {
  return [
    {
      id: 'req/auth',
      doc_type: 'requirements',
      status: 'approved',
      title: 'Authentication Requirements',
      components: [],
    },
    {
      id: 'req/rbac',
      doc_type: 'requirements',
      status: 'draft',
      title: 'RBAC Requirements',
      components: [],
    },
    {
      id: 'design/auth-flow',
      doc_type: 'design',
      status: 'approved',
      title: 'Auth Flow Design',
      components: [],
    },
    {
      id: 'adr/001',
      doc_type: 'adr',
      status: 'accepted',
      title: 'Use JWT tokens',
      components: [],
    },
    {
      id: 'tasks/sprint-1',
      doc_type: 'tasks',
      status: 'implemented',
      title: 'Sprint 1 Tasks',
      components: [],
    },
    {
      id: 'misc/notes',
      doc_type: null,
      status: null,
      title: 'Misc Notes',
      components: [],
    },
  ];
}

// ===== extractFilterOptions =====

describe('extractFilterOptions', () => {
  it('returns sorted unique doc_type values, excluding null', () => {
    const docs = makeDocs();
    const options = extractFilterOptions(docs);
    expect(options.types).toEqual(['adr', 'design', 'requirements', 'tasks']);
  });

  it('returns sorted unique status values, excluding null', () => {
    const docs = makeDocs();
    const options = extractFilterOptions(docs);
    expect(options.statuses).toEqual(['accepted', 'approved', 'draft', 'implemented']);
  });

  it('returns empty arrays for empty document list', () => {
    const options = extractFilterOptions([]);
    expect(options.types).toEqual([]);
    expect(options.statuses).toEqual([]);
  });

  it('handles documents where all doc_type and status are null', () => {
    const docs = [
      { id: 'a', doc_type: null, status: null, title: 'A', components: [] },
      { id: 'b', doc_type: null, status: null, title: 'B', components: [] },
    ];
    const options = extractFilterOptions(docs);
    expect(options.types).toEqual([]);
    expect(options.statuses).toEqual([]);
  });

  it('deduplicates repeated type and status values', () => {
    const docs = [
      {
        id: 'a',
        doc_type: 'design',
        status: 'draft',
        title: 'A',
        components: [],
      },
      {
        id: 'b',
        doc_type: 'design',
        status: 'draft',
        title: 'B',
        components: [],
      },
      {
        id: 'c',
        doc_type: 'design',
        status: 'approved',
        title: 'C',
        components: [],
      },
    ];
    const options = extractFilterOptions(docs);
    expect(options.types).toEqual(['design']);
    expect(options.statuses).toEqual(['approved', 'draft']);
  });
});

// ===== filterDocuments =====

describe('filterDocuments', () => {
  it('returns all document IDs when no filters are active', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, { types: new Set(), status: null });
    expect(result).toEqual(new Set(docs.map((d) => d.id)));
  });

  it('returns all document IDs when status is "all"', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, { types: new Set(), status: 'all' });
    expect(result).toEqual(new Set(docs.map((d) => d.id)));
  });

  it('filters by a single doc_type', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(['requirements']),
      status: null,
    });
    expect(result).toEqual(new Set(['req/auth', 'req/rbac']));
  });

  it('filters by multiple doc_types', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(['requirements', 'adr']),
      status: null,
    });
    expect(result).toEqual(new Set(['req/auth', 'req/rbac', 'adr/001']));
  });

  it('filters by status only', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(),
      status: 'approved',
    });
    expect(result).toEqual(new Set(['req/auth', 'design/auth-flow']));
  });

  it('filters by both type and status', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(['requirements']),
      status: 'approved',
    });
    expect(result).toEqual(new Set(['req/auth']));
  });

  it('returns empty set when no documents match', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(['requirements']),
      status: 'implemented',
    });
    expect(result).toEqual(new Set());
  });

  it('documents with null doc_type are excluded when type filter is active', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(['requirements']),
      status: null,
    });
    expect(result.has('misc/notes')).toBe(false);
  });

  it('documents with null status are excluded when status filter is active', () => {
    const docs = makeDocs();
    const result = filterDocuments(docs, {
      types: new Set(),
      status: 'draft',
    });
    expect(result.has('misc/notes')).toBe(false);
  });

  it('returns empty set for empty document list', () => {
    const result = filterDocuments([], { types: new Set(), status: null });
    expect(result).toEqual(new Set());
  });
});

// ===== searchDocuments =====

describe('searchDocuments', () => {
  it('returns empty array for empty query', () => {
    const docs = makeDocs();
    const result = searchDocuments(docs, '');
    expect(result).toEqual([]);
  });

  it('returns empty array for whitespace-only query', () => {
    const docs = makeDocs();
    const result = searchDocuments(docs, '   ');
    expect(result).toEqual([]);
  });

  it('matches on ID substring', () => {
    const docs = makeDocs();
    const result = searchDocuments(docs, 'req/');
    expect(result.length).toBe(2);
    expect(result.map((d) => d.id)).toContain('req/auth');
    expect(result.map((d) => d.id)).toContain('req/rbac');
  });

  it('matches on title substring', () => {
    const docs = makeDocs();
    const result = searchDocuments(docs, 'JWT');
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('adr/001');
  });

  it('performs case-insensitive matching', () => {
    const docs = makeDocs();
    const resultLower = searchDocuments(docs, 'authentication');
    const resultUpper = searchDocuments(docs, 'AUTHENTICATION');
    const resultMixed = searchDocuments(docs, 'AuThEnTiCaTiOn');
    expect(resultLower).toHaveLength(1);
    expect(resultUpper).toHaveLength(1);
    expect(resultMixed).toHaveLength(1);
    expect(resultLower[0].id).toBe('req/auth');
    expect(resultUpper[0].id).toBe('req/auth');
    expect(resultMixed[0].id).toBe('req/auth');
  });

  it('returns empty array when no documents match', () => {
    const docs = makeDocs();
    const result = searchDocuments(docs, 'zzz-no-match');
    expect(result).toEqual([]);
  });

  it('returns empty array when document list is empty', () => {
    const result = searchDocuments([], 'auth');
    expect(result).toEqual([]);
  });

  it('orders exact ID matches before ID substring matches before title-only matches', () => {
    const docs = [
      { id: 'auth', doc_type: null, status: null, title: 'Exact ID doc', components: [] },
      { id: 'design/auth-flow', doc_type: null, status: null, title: 'Auth Flow', components: [] },
      {
        id: 'misc/notes',
        doc_type: null,
        status: null,
        title: 'Notes about auth',
        components: [],
      },
    ];
    const result = searchDocuments(docs, 'auth');
    // Exact ID match first
    expect(result[0].id).toBe('auth');
    // ID substring match second
    expect(result[1].id).toBe('design/auth-flow');
    // Title-only match last
    expect(result[2].id).toBe('misc/notes');
  });

  it('matches documents where both ID and title match (no duplicates)', () => {
    const docs = makeDocs();
    // 'auth' appears in both id 'req/auth' and title 'Authentication Requirements'
    const result = searchDocuments(docs, 'auth');
    const authIds = result.filter((d) => d.id === 'req/auth');
    expect(authIds).toHaveLength(1);
  });
});
