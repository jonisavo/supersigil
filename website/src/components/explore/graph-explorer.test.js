import { describe, expect, it } from 'vitest';
import {
  buildComponentLinks,
  buildComponentNodes,
  componentStrokeColor,
  computeClusterBounds,
  computeClusters,
  mount,
  nodeRadius,
  nodeStrokeColor,
} from './graph-explorer.js';

describe('graph-explorer stubs', () => {
  it('mount is exported and callable', () => {
    // mount requires a real DOM element with D3, so just check it's a function
    expect(typeof mount).toBe('function');
  });
});

describe('nodeRadius', () => {
  it('returns base radius for a document with no components', () => {
    const doc = { id: 'a', doc_type: null, status: null, title: 'A', components: [] };
    expect(nodeRadius(doc)).toBe(12);
  });

  it('scales with component count', () => {
    const doc = {
      id: 'a',
      doc_type: null,
      status: null,
      title: 'A',
      components: [
        { id: null, kind: 'section', body: null },
        { id: null, kind: 'section', body: null },
        { id: null, kind: 'section', body: null },
        { id: null, kind: 'section', body: null },
      ],
    };
    // 12 + Math.sqrt(4) * 3 = 12 + 6 = 18
    expect(nodeRadius(doc)).toBe(18);
  });

  it('handles single component', () => {
    const doc = {
      id: 'a',
      doc_type: null,
      status: null,
      title: 'A',
      components: [{ id: null, kind: 'section', body: null }],
    };
    // 12 + Math.sqrt(1) * 3 = 12 + 3 = 15
    expect(nodeRadius(doc)).toBe(15);
  });
});

describe('nodeStrokeColor', () => {
  it('returns teal CSS var for requirements', () => {
    const doc = { id: 'a', doc_type: 'requirements', status: null, title: 'A', components: [] };
    expect(nodeStrokeColor(doc)).toBe('var(--teal)');
  });

  it('returns green CSS var for design', () => {
    const doc = { id: 'a', doc_type: 'design', status: null, title: 'A', components: [] };
    expect(nodeStrokeColor(doc)).toBe('var(--green)');
  });

  it('returns gold CSS var for adr', () => {
    const doc = { id: 'a', doc_type: 'adr', status: null, title: 'A', components: [] };
    expect(nodeStrokeColor(doc)).toBe('var(--gold)');
  });

  it('returns dim CSS var for null doc_type', () => {
    const doc = { id: 'a', doc_type: null, status: null, title: 'A', components: [] };
    expect(nodeStrokeColor(doc)).toBe('var(--text-dim)');
  });

  it('returns dim CSS var for unknown doc_type', () => {
    const doc = { id: 'a', doc_type: 'other', status: null, title: 'A', components: [] };
    expect(nodeStrokeColor(doc)).toBe('var(--text-dim)');
  });
});

describe('componentStrokeColor', () => {
  it('returns teal for Criterion', () => {
    expect(componentStrokeColor('Criterion')).toBe('var(--teal)');
  });

  it('returns green for Task', () => {
    expect(componentStrokeColor('Task')).toBe('var(--green)');
  });

  it('returns gold for Decision', () => {
    expect(componentStrokeColor('Decision')).toBe('var(--gold)');
  });

  it('returns muted for Rationale', () => {
    expect(componentStrokeColor('Rationale')).toBe('var(--text-muted)');
  });

  it('returns muted for Alternative', () => {
    expect(componentStrokeColor('Alternative')).toBe('var(--text-muted)');
  });

  it('returns dim for unknown kind', () => {
    expect(componentStrokeColor('Other')).toBe('var(--text-dim)');
  });
});

describe('computeClusters', () => {
  it('groups documents by ID prefix', () => {
    const docs = [
      {
        id: 'parser/parser.req',
        doc_type: 'requirements',
        status: null,
        title: 'A',
        components: [],
      },
      { id: 'parser/parser.design', doc_type: 'design', status: null, title: 'B', components: [] },
      { id: 'cli/cli.req', doc_type: 'requirements', status: null, title: 'C', components: [] },
    ];
    const clusters = computeClusters(docs);
    expect(clusters).toHaveLength(2);

    const parserCluster = clusters.find((c) => c.name === 'parser');
    expect(parserCluster).toBeDefined();
    expect(parserCluster.docIds).toEqual(['parser/parser.req', 'parser/parser.design']);

    const cliCluster = clusters.find((c) => c.name === 'cli');
    expect(cliCluster).toBeDefined();
    expect(cliCluster.docIds).toEqual(['cli/cli.req']);
  });

  it('puts documents without slash into their own cluster', () => {
    const docs = [{ id: 'readme', doc_type: null, status: null, title: 'Readme', components: [] }];
    const clusters = computeClusters(docs);
    expect(clusters).toHaveLength(1);
    expect(clusters[0].name).toBe('readme');
    expect(clusters[0].docIds).toEqual(['readme']);
  });

  it('handles nested paths using everything before last slash', () => {
    const docs = [
      { id: 'a/b/c.req', doc_type: 'requirements', status: null, title: 'C', components: [] },
      { id: 'a/b/d.req', doc_type: 'requirements', status: null, title: 'D', components: [] },
    ];
    const clusters = computeClusters(docs);
    expect(clusters).toHaveLength(1);
    expect(clusters[0].name).toBe('a/b');
    expect(clusters[0].docIds).toEqual(['a/b/c.req', 'a/b/d.req']);
  });

  it('returns empty array for empty input', () => {
    expect(computeClusters([])).toEqual([]);
  });
});

describe('computeClusterBounds', () => {
  it('computes bounding box with padding around nodes', () => {
    const cluster = { name: 'test', docIds: ['a', 'b'] };
    const nodePositions = new Map([
      ['a', { x: 10, y: 20, radius: 12 }],
      ['b', { x: 50, y: 60, radius: 15 }],
    ]);
    const bounds = computeClusterBounds(cluster, nodePositions);
    // padding = 30
    // x: min(10,50) - 12 - 30 = -32, max(10,50) + 15 + 30 = 95
    // y: min(20,60) - 12 - 30 = -22, max(20,60) + 15 + 30 = 105
    expect(bounds.x).toBe(-32);
    expect(bounds.y).toBe(-22);
    expect(bounds.width).toBe(127); // 95 - (-32)
    expect(bounds.height).toBe(127); // 105 - (-22)
  });

  it('handles single-node cluster', () => {
    const cluster = { name: 'solo', docIds: ['a'] };
    const nodePositions = new Map([['a', { x: 100, y: 200, radius: 10 }]]);
    const bounds = computeClusterBounds(cluster, nodePositions);
    // x: 100 - 10 - 30 = 60, max_x: 100 + 10 + 30 = 140
    // y: 200 - 10 - 30 = 160, max_y: 200 + 10 + 30 = 240
    expect(bounds.x).toBe(60);
    expect(bounds.y).toBe(160);
    expect(bounds.width).toBe(80); // 140 - 60
    expect(bounds.height).toBe(80); // 240 - 160
  });

  it('returns null when no node positions are found', () => {
    const cluster = { name: 'missing', docIds: ['z'] };
    const nodePositions = new Map();
    const bounds = computeClusterBounds(cluster, nodePositions);
    expect(bounds).toBeNull();
  });
});

describe('buildComponentNodes', () => {
  it('creates nodes for Criterion, Task, and Decision components', () => {
    const doc = {
      id: 'reqs/parser.req',
      doc_type: 'requirements',
      status: null,
      title: 'Parser Requirements',
      components: [
        { id: 'req-1', kind: 'Criterion', body: 'Must parse YAML' },
        { id: 'task-1', kind: 'Task', body: 'Implement parser', implements: ['req-1'] },
        { id: 'dec-1', kind: 'Decision', body: 'Use serde', children: [] },
      ],
    };
    const result = buildComponentNodes(doc);
    expect(result).toHaveLength(3);

    const criterion = result.find((n) => n.componentId === 'req-1');
    expect(criterion).toBeDefined();
    expect(criterion.kind).toBe('Criterion');
    expect(criterion.parentDocId).toBe('reqs/parser.req');
    expect(criterion.radius).toBe(8);
    expect(criterion.label).toBe('Criterion: req-1');

    const task = result.find((n) => n.componentId === 'task-1');
    expect(task).toBeDefined();
    expect(task.kind).toBe('Task');

    const decision = result.find((n) => n.componentId === 'dec-1');
    expect(decision).toBeDefined();
    expect(decision.kind).toBe('Decision');
  });

  it('creates child nodes for Decision children (Rationale, Alternative)', () => {
    const doc = {
      id: 'adr/logging',
      doc_type: 'adr',
      status: null,
      title: 'Logging ADR',
      components: [
        {
          id: 'dec-1',
          kind: 'Decision',
          body: 'Use tracing',
          children: [
            { id: 'rat-1', kind: 'Rationale', body: 'Performance' },
            { id: 'alt-1', kind: 'Alternative', body: 'Use log crate' },
          ],
        },
      ],
    };
    const result = buildComponentNodes(doc);
    // Decision + Rationale + Alternative = 3
    expect(result).toHaveLength(3);

    const rationale = result.find((n) => n.componentId === 'rat-1');
    expect(rationale).toBeDefined();
    expect(rationale.kind).toBe('Rationale');
    expect(rationale.parentDocId).toBe('adr/logging');
    expect(rationale.parentComponentId).toBe('dec-1');

    const alternative = result.find((n) => n.componentId === 'alt-1');
    expect(alternative).toBeDefined();
    expect(alternative.kind).toBe('Alternative');
    expect(alternative.parentComponentId).toBe('dec-1');
  });

  it('skips components without an id', () => {
    const doc = {
      id: 'doc/a',
      doc_type: null,
      status: null,
      title: 'A',
      components: [
        { id: null, kind: 'section', body: 'some text' },
        { id: 'req-1', kind: 'Criterion', body: 'A criterion' },
      ],
    };
    const result = buildComponentNodes(doc);
    expect(result).toHaveLength(1);
    expect(result[0].componentId).toBe('req-1');
  });

  it('returns empty array for document with no eligible components', () => {
    const doc = {
      id: 'doc/empty',
      doc_type: null,
      status: null,
      title: 'Empty',
      components: [{ id: null, kind: 'section', body: 'text' }],
    };
    const result = buildComponentNodes(doc);
    expect(result).toEqual([]);
  });

  it('generates unique node IDs scoped to the document', () => {
    const doc = {
      id: 'reqs/a',
      doc_type: 'requirements',
      status: null,
      title: 'A',
      components: [
        { id: 'req-1', kind: 'Criterion', body: 'C1' },
        { id: 'req-2', kind: 'Criterion', body: 'C2' },
      ],
    };
    const result = buildComponentNodes(doc);
    const ids = result.map((n) => n.id);
    expect(ids).toHaveLength(2);
    expect(new Set(ids).size).toBe(2);
    // IDs should reference the doc
    for (const id of ids) {
      expect(id).toContain('reqs/a');
    }
  });
});

describe('buildComponentLinks', () => {
  it('creates links from Decision to its children', () => {
    const doc = {
      id: 'adr/logging',
      doc_type: 'adr',
      status: null,
      title: 'Logging ADR',
      components: [
        {
          id: 'dec-1',
          kind: 'Decision',
          body: 'Use tracing',
          children: [
            { id: 'rat-1', kind: 'Rationale', body: 'Performance' },
            { id: 'alt-1', kind: 'Alternative', body: 'Use log crate' },
          ],
        },
      ],
    };
    const componentNodes = buildComponentNodes(doc);
    const links = buildComponentLinks(componentNodes);

    const decisionNode = componentNodes.find((n) => n.componentId === 'dec-1');
    const rationaleNode = componentNodes.find((n) => n.componentId === 'rat-1');
    const altNode = componentNodes.find((n) => n.componentId === 'alt-1');

    const decToRat = links.find(
      (l) => l.source === decisionNode.id && l.target === rationaleNode.id,
    );
    expect(decToRat).toBeDefined();
    expect(decToRat.kind).toBe('has_child');

    const decToAlt = links.find((l) => l.source === decisionNode.id && l.target === altNode.id);
    expect(decToAlt).toBeDefined();
    expect(decToAlt.kind).toBe('has_child');
  });

  it('creates links from Task to Criterion via implements', () => {
    const doc = {
      id: 'reqs/parser.req',
      doc_type: 'requirements',
      status: null,
      title: 'Parser Requirements',
      components: [
        { id: 'req-1', kind: 'Criterion', body: 'Must parse YAML' },
        { id: 'req-2', kind: 'Criterion', body: 'Must validate' },
        { id: 'task-1', kind: 'Task', body: 'Implement parser', implements: ['req-1', 'req-2'] },
      ],
    };
    const componentNodes = buildComponentNodes(doc);
    const links = buildComponentLinks(componentNodes);

    const taskNode = componentNodes.find((n) => n.componentId === 'task-1');
    const req1Node = componentNodes.find((n) => n.componentId === 'req-1');
    const req2Node = componentNodes.find((n) => n.componentId === 'req-2');

    const taskToReq1 = links.find((l) => l.source === taskNode.id && l.target === req1Node.id);
    expect(taskToReq1).toBeDefined();
    expect(taskToReq1.kind).toBe('implements');

    const taskToReq2 = links.find((l) => l.source === taskNode.id && l.target === req2Node.id);
    expect(taskToReq2).toBeDefined();
    expect(taskToReq2.kind).toBe('implements');
  });

  it('skips implements refs that do not match any component node', () => {
    const doc = {
      id: 'reqs/a',
      doc_type: 'requirements',
      status: null,
      title: 'A',
      components: [{ id: 'task-1', kind: 'Task', body: 'Do stuff', implements: ['nonexistent'] }],
    };
    const componentNodes = buildComponentNodes(doc);
    const links = buildComponentLinks(componentNodes);
    expect(links).toEqual([]);
  });

  it('returns empty array when there are no relationships', () => {
    const doc = {
      id: 'reqs/a',
      doc_type: 'requirements',
      status: null,
      title: 'A',
      components: [
        { id: 'req-1', kind: 'Criterion', body: 'C1' },
        { id: 'req-2', kind: 'Criterion', body: 'C2' },
      ],
    };
    const componentNodes = buildComponentNodes(doc);
    const links = buildComponentLinks(componentNodes);
    expect(links).toEqual([]);
  });
});
