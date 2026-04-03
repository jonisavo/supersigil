import { describe, expect, it } from 'vitest';
import {
  buildBadgeClass,
  buildClusterEdgeGroups,
  buildEdgeGroups,
  clearDetail,
  renderClusterDetail,
  renderDetail,
  renderEmpty,
} from './detail-panel.js';

describe('buildEdgeGroups', () => {
  it('separates incoming and outgoing edges for a given docId', () => {
    const edges = [
      { from: 'a', to: 'b', kind: 'Implements' },
      { from: 'b', to: 'c', kind: 'DependsOn' },
      { from: 'c', to: 'b', kind: 'References' },
    ];
    const groups = buildEdgeGroups(edges, 'b');
    expect(groups.incoming).toEqual([
      { from: 'a', to: 'b', kind: 'Implements' },
      { from: 'c', to: 'b', kind: 'References' },
    ]);
    expect(groups.outgoing).toEqual([{ from: 'b', to: 'c', kind: 'DependsOn' }]);
  });

  it('returns empty arrays when no edges match', () => {
    const edges = [{ from: 'x', to: 'y', kind: 'DependsOn' }];
    const groups = buildEdgeGroups(edges, 'z');
    expect(groups.incoming).toEqual([]);
    expect(groups.outgoing).toEqual([]);
  });

  it('handles empty edges array', () => {
    const groups = buildEdgeGroups([], 'a');
    expect(groups.incoming).toEqual([]);
    expect(groups.outgoing).toEqual([]);
  });

  it('does not double-count self-referencing edges', () => {
    const edges = [{ from: 'a', to: 'a', kind: 'References' }];
    const groups = buildEdgeGroups(edges, 'a');
    // Self-edge is both incoming and outgoing
    expect(groups.incoming).toEqual([{ from: 'a', to: 'a', kind: 'References' }]);
    expect(groups.outgoing).toEqual([{ from: 'a', to: 'a', kind: 'References' }]);
  });
});

describe('buildBadgeClass', () => {
  it('returns correct class for type badge', () => {
    expect(buildBadgeClass('type', 'requirements')).toBe('badge badge-type-requirements');
  });

  it('returns correct class for status badge', () => {
    expect(buildBadgeClass('status', 'approved')).toBe('badge badge-status-approved');
  });

  it('returns unknown type class for null value', () => {
    expect(buildBadgeClass('type', null)).toBe('badge badge-type-unknown');
  });

  it('returns draft status class for null status', () => {
    expect(buildBadgeClass('status', null)).toBe('badge badge-status-draft');
  });

  it('handles various type values', () => {
    expect(buildBadgeClass('type', 'design')).toBe('badge badge-type-design');
    expect(buildBadgeClass('type', 'adr')).toBe('badge badge-type-adr');
  });

  it('handles various status values', () => {
    expect(buildBadgeClass('status', 'draft')).toBe('badge badge-status-draft');
    expect(buildBadgeClass('status', 'proposed')).toBe('badge badge-status-proposed');
    expect(buildBadgeClass('status', 'deprecated')).toBe('badge badge-status-deprecated');
  });
});

describe('renderDetail', () => {
  /** @returns {HTMLElement} */
  function makeContainer() {
    return /** @type {HTMLElement} */ ({
      innerHTML: '',
      classList: {
        _classes: new Set(),
        add(cls) {
          this._classes.add(cls);
        },
        remove(cls) {
          this._classes.delete(cls);
        },
        contains(cls) {
          return this._classes.has(cls);
        },
      },
    });
  }

  it('sets innerHTML with document ID as title', () => {
    const container = makeContainer();
    const node = {
      id: 'parser/parser.req',
      doc_type: 'requirements',
      status: 'approved',
      title: 'Parser Requirements',
      components: [],
    };
    renderDetail(container, node, [], [], '');
    expect(container.innerHTML).toContain('parser/parser.req');
  });

  it('includes a Trace impact button with data-doc-id', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/a',
      doc_type: null,
      status: null,
      title: 'A',
      components: [],
    };
    renderDetail(container, node, [], [], '');
    expect(container.innerHTML).toContain('detail-panel-trace-btn');
    expect(container.innerHTML).toContain('data-doc-id="doc/a"');
    expect(container.innerHTML).toContain('Trace impact');
  });

  it('includes type and status badges', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/a',
      doc_type: 'design',
      status: 'draft',
      title: 'A',
      components: [],
    };
    renderDetail(container, node, [], [], '');
    expect(container.innerHTML).toContain('badge-type-design');
    expect(container.innerHTML).toContain('badge-status-draft');
  });

  it('renders incoming edges with arrow and doc id', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/b',
      doc_type: null,
      status: null,
      title: 'B',
      components: [],
    };
    const edges = [{ from: 'doc/a', to: 'doc/b', kind: 'Implements' }];
    renderDetail(container, node, edges, [], '');
    expect(container.innerHTML).toContain('\u2190');
    expect(container.innerHTML).toContain('doc/a');
    expect(container.innerHTML).toContain('Implements');
  });

  it('renders outgoing edges with arrow and doc id', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/b',
      doc_type: null,
      status: null,
      title: 'B',
      components: [],
    };
    const edges = [{ from: 'doc/b', to: 'doc/c', kind: 'DependsOn' }];
    renderDetail(container, node, edges, [], '');
    expect(container.innerHTML).toContain('\u2192');
    expect(container.innerHTML).toContain('doc/c');
    expect(container.innerHTML).toContain('DependsOn');
  });

  it('renders coverage when render data is provided', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/a',
      doc_type: 'requirements',
      status: 'approved',
      title: 'A',
      components: [
        { id: 'req-1', kind: 'Criterion', body: 'Must parse' },
        { id: 'task-1', kind: 'Task', body: 'Implement' },
      ],
    };
    const renderData = [
      {
        document_id: 'doc/a',
        fences: [
          {
            components: [
              { kind: 'Criterion', verification: { state: 'verified' } },
              { kind: 'Criterion', verification: { state: 'unverified' } },
            ],
          },
        ],
        edges: [],
      },
    ];
    renderDetail(container, node, [], renderData, '');
    expect(container.innerHTML).toContain('1/2 criteria verified');
    expect(container.innerHTML).toContain('50%');
  });

  it('renders without coverage when no render data matches', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/a',
      doc_type: 'requirements',
      status: 'approved',
      title: 'A',
      components: [],
    };
    renderDetail(container, node, [], [], '');
    expect(container.innerHTML).not.toContain('criteria verified');
  });

  it('renders empty state with instructions', () => {
    const container = makeContainer();
    renderEmpty(container);
    expect(container.innerHTML).toContain('Select a document');
  });

  it('adds open class to container', () => {
    const container = makeContainer();
    const node = {
      id: 'doc/a',
      doc_type: null,
      status: null,
      title: 'A',
      components: [],
    };
    renderDetail(container, node, [], [], '');
    expect(container.classList.contains('open')).toBe(true);
  });
});

describe('buildClusterEdgeGroups', () => {
  it('separates incoming and outgoing cross-cluster edges', () => {
    const clusterDocIds = new Set(['proj/a', 'proj/b']);
    const edges = [
      { from: 'proj/a', to: 'proj/b', kind: 'Implements' }, // internal, excluded
      { from: 'other/x', to: 'proj/a', kind: 'DependsOn' }, // incoming
      { from: 'proj/b', to: 'other/y', kind: 'References' }, // outgoing
    ];
    const groups = buildClusterEdgeGroups(edges, clusterDocIds);
    expect(groups.incoming).toEqual([{ from: 'other/x', to: 'proj/a', kind: 'DependsOn' }]);
    expect(groups.outgoing).toEqual([{ from: 'proj/b', to: 'other/y', kind: 'References' }]);
  });

  it('returns empty arrays when no cross-cluster edges exist', () => {
    const clusterDocIds = new Set(['proj/a', 'proj/b']);
    const edges = [{ from: 'proj/a', to: 'proj/b', kind: 'Implements' }];
    const groups = buildClusterEdgeGroups(edges, clusterDocIds);
    expect(groups.incoming).toEqual([]);
    expect(groups.outgoing).toEqual([]);
  });

  it('handles empty edges array', () => {
    const groups = buildClusterEdgeGroups([], new Set(['a']));
    expect(groups.incoming).toEqual([]);
    expect(groups.outgoing).toEqual([]);
  });
});

describe('renderClusterDetail', () => {
  /** @returns {HTMLElement} */
  function makeContainer() {
    return /** @type {HTMLElement} */ ({
      innerHTML: '',
      classList: {
        _classes: new Set(),
        add(cls) {
          this._classes.add(cls);
        },
        remove(cls) {
          this._classes.delete(cls);
        },
        contains(cls) {
          return this._classes.has(cls);
        },
      },
    });
  }

  const sampleDocs = [
    {
      id: 'proj/a',
      doc_type: 'requirements',
      status: 'approved',
      title: 'A',
      components: [
        { id: 'crit-1', kind: 'Criterion', body: 'Must do X' },
        { id: 'task-1', kind: 'Task', body: 'Implement X' },
      ],
    },
    {
      id: 'proj/b',
      doc_type: 'design',
      status: 'draft',
      title: 'B',
      components: [{ id: 'dec-1', kind: 'Decision', body: 'Use Y' }],
    },
    {
      id: 'proj/c',
      doc_type: 'requirements',
      status: 'approved',
      title: 'C',
      components: [],
    },
  ];

  const sampleEdges = [
    { from: 'proj/a', to: 'proj/b', kind: 'Implements' },
    { from: 'other/x', to: 'proj/a', kind: 'DependsOn' },
    { from: 'proj/b', to: 'other/y', kind: 'References' },
  ];

  it('shows cluster name as title', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.innerHTML).toContain('proj');
    expect(container.innerHTML).toContain('detail-panel-title');
  });

  it('shows document count', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.innerHTML).toContain('3');
    expect(container.innerHTML).toMatch(/documents/i);
  });

  it('shows type breakdown with badges', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    // 2 requirements, 1 design
    expect(container.innerHTML).toContain('badge-type-requirements');
    expect(container.innerHTML).toContain('badge-type-design');
    expect(container.innerHTML).toContain('2');
    expect(container.innerHTML).toContain('1');
  });

  it('shows status breakdown', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.innerHTML).toContain('badge-status-approved');
    expect(container.innerHTML).toContain('badge-status-draft');
  });

  it('shows component summary counts by kind', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.innerHTML).toContain('Criterion');
    expect(container.innerHTML).toContain('Task');
    expect(container.innerHTML).toContain('Decision');
  });

  it('shows cross-cluster edges grouped by direction', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    // Incoming: other/x -> proj/a
    expect(container.innerHTML).toContain('other/x');
    // Outgoing: proj/b -> other/y
    expect(container.innerHTML).toContain('other/y');
    expect(container.innerHTML).toMatch(/incoming/i);
    expect(container.innerHTML).toMatch(/outgoing/i);
  });

  it('adds open class to container', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.classList.contains('open')).toBe(true);
  });

  it('includes close button', () => {
    const container = makeContainer();
    renderClusterDetail(container, 'proj', sampleDocs, sampleEdges);
    expect(container.innerHTML).toContain('detail-panel-close');
  });

  it('handles cluster with no cross-cluster edges', () => {
    const container = makeContainer();
    const internalEdges = [{ from: 'proj/a', to: 'proj/b', kind: 'Implements' }];
    renderClusterDetail(container, 'proj', sampleDocs, internalEdges);
    // Should still render without errors
    expect(container.innerHTML).toContain('proj');
    expect(container.classList.contains('open')).toBe(true);
  });

  it('handles cluster with no components', () => {
    const container = makeContainer();
    const emptyDocs = [
      { id: 'proj/a', doc_type: 'requirements', status: 'draft', title: 'A', components: [] },
    ];
    renderClusterDetail(container, 'proj', emptyDocs, []);
    expect(container.innerHTML).toContain('proj');
    expect(container.innerHTML).toContain('1');
  });
});

describe('clearDetail', () => {
  it('removes open class from container', () => {
    const container = /** @type {HTMLElement} */ ({
      innerHTML: '<div>content</div>',
      classList: {
        _classes: new Set(['open']),
        add(cls) {
          this._classes.add(cls);
        },
        remove(cls) {
          this._classes.delete(cls);
        },
        contains(cls) {
          return this._classes.has(cls);
        },
      },
      addEventListener(event, handler) {
        // Simulate transitionend firing immediately for test
        if (event === 'transitionend') {
          handler();
        }
      },
      removeEventListener() {},
    });
    clearDetail(container);
    expect(container.classList.contains('open')).toBe(false);
  });

  it('clears innerHTML after transition', () => {
    const container = /** @type {HTMLElement} */ ({
      innerHTML: '<div>content</div>',
      classList: {
        _classes: new Set(['open']),
        add(cls) {
          this._classes.add(cls);
        },
        remove(cls) {
          this._classes.delete(cls);
        },
        contains(cls) {
          return this._classes.has(cls);
        },
      },
      addEventListener(event, handler) {
        if (event === 'transitionend') {
          handler();
        }
      },
      removeEventListener() {},
    });
    clearDetail(container);
    expect(container.innerHTML).toBe('');
  });
});
