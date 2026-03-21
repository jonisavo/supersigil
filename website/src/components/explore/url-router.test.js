import { describe, expect, it } from 'vitest';
import { buildHash, onHashChange, parseHash } from './url-router.js';

describe('parseHash', () => {
  it('returns default state for empty string', () => {
    const state = parseHash('');
    expect(state).toEqual({ doc: null, trace: false, filter: null });
  });

  it('returns default state for bare hash', () => {
    const state = parseHash('#');
    expect(state).toEqual({ doc: null, trace: false, filter: null });
  });

  it('returns default state for just a slash', () => {
    const state = parseHash('#/');
    expect(state).toEqual({ doc: null, trace: false, filter: null });
  });

  it('parses a simple doc ID', () => {
    const state = parseHash('#/doc/parser/parser.req');
    expect(state).toEqual({ doc: 'parser/parser.req', trace: false, filter: null });
  });

  it('parses a doc ID with trace', () => {
    const state = parseHash('#/doc/parser/parser.req/trace');
    expect(state).toEqual({ doc: 'parser/parser.req', trace: true, filter: null });
  });

  it('parses filter with types only', () => {
    const state = parseHash('#/filter/type:requirements,design');
    expect(state).toEqual({
      doc: null,
      trace: false,
      filter: { types: ['requirements', 'design'], status: null },
    });
  });

  it('parses filter with status only', () => {
    const state = parseHash('#/filter/status:approved');
    expect(state).toEqual({
      doc: null,
      trace: false,
      filter: { types: [], status: 'approved' },
    });
  });

  it('parses filter with both type and status', () => {
    const state = parseHash('#/filter/type:requirements,design;status:approved');
    expect(state).toEqual({
      doc: null,
      trace: false,
      filter: { types: ['requirements', 'design'], status: 'approved' },
    });
  });

  it('parses doc with filter', () => {
    const state = parseHash('#/doc/graph/graph.design/filter/type:design');
    expect(state).toEqual({
      doc: 'graph/graph.design',
      trace: false,
      filter: { types: ['design'], status: null },
    });
  });

  it('parses doc with trace and filter', () => {
    const state = parseHash('#/doc/parser/parser.req/trace/filter/type:requirements;status:draft');
    expect(state).toEqual({
      doc: 'parser/parser.req',
      trace: true,
      filter: { types: ['requirements'], status: 'draft' },
    });
  });

  it('handles deeply nested doc ID', () => {
    const state = parseHash('#/doc/a/b/c/d.req');
    expect(state).toEqual({ doc: 'a/b/c/d.req', trace: false, filter: null });
  });

  it('handles doc ID with dots', () => {
    const state = parseHash('#/doc/my.project/spec.v2.req');
    expect(state).toEqual({ doc: 'my.project/spec.v2.req', trace: false, filter: null });
  });

  it('returns default state for malformed hash', () => {
    const state = parseHash('#/unknown/segment');
    expect(state).toEqual({ doc: null, trace: false, filter: null });
  });

  it('parses single type filter', () => {
    const state = parseHash('#/filter/type:requirements');
    expect(state).toEqual({
      doc: null,
      trace: false,
      filter: { types: ['requirements'], status: null },
    });
  });

  it('trace without doc is ignored', () => {
    // /trace is only valid after /doc/{id}, so standalone /trace is invalid
    const state = parseHash('#/trace');
    expect(state).toEqual({ doc: null, trace: false, filter: null });
  });
});

describe('buildHash', () => {
  it('returns empty string for default state', () => {
    expect(buildHash({ doc: null, trace: false, filter: null })).toBe('');
  });

  it('builds hash for doc only', () => {
    expect(buildHash({ doc: 'parser/parser.req', trace: false, filter: null })).toBe(
      '#/doc/parser/parser.req',
    );
  });

  it('builds hash for doc with trace', () => {
    expect(buildHash({ doc: 'parser/parser.req', trace: true, filter: null })).toBe(
      '#/doc/parser/parser.req/trace',
    );
  });

  it('builds hash for filter with types only', () => {
    expect(
      buildHash({ doc: null, trace: false, filter: { types: ['requirements'], status: null } }),
    ).toBe('#/filter/type:requirements');
  });

  it('builds hash for filter with types and status', () => {
    expect(
      buildHash({
        doc: null,
        trace: false,
        filter: { types: ['requirements', 'design'], status: 'approved' },
      }),
    ).toBe('#/filter/type:requirements,design;status:approved');
  });

  it('builds hash for filter with status only', () => {
    expect(buildHash({ doc: null, trace: false, filter: { types: [], status: 'approved' } })).toBe(
      '#/filter/status:approved',
    );
  });

  it('builds hash for doc with filter', () => {
    expect(
      buildHash({
        doc: 'graph/graph.design',
        trace: false,
        filter: { types: ['design'], status: null },
      }),
    ).toBe('#/doc/graph/graph.design/filter/type:design');
  });

  it('builds hash for doc with trace and filter', () => {
    expect(
      buildHash({
        doc: 'parser/parser.req',
        trace: true,
        filter: { types: ['requirements'], status: 'draft' },
      }),
    ).toBe('#/doc/parser/parser.req/trace/filter/type:requirements;status:draft');
  });

  it('omits trace when false even with doc', () => {
    const hash = buildHash({ doc: 'a/b', trace: false, filter: null });
    expect(hash).not.toContain('trace');
  });

  it('omits filter segment when filter has empty types and null status', () => {
    const hash = buildHash({ doc: null, trace: false, filter: { types: [], status: null } });
    expect(hash).toBe('');
  });

  it('trace without doc is ignored in output', () => {
    // trace only makes sense with a doc, so buildHash should omit it
    const hash = buildHash({ doc: null, trace: true, filter: null });
    expect(hash).toBe('');
  });
});

describe('round-trip: parseHash(buildHash(state))', () => {
  const cases = [
    { doc: null, trace: false, filter: null },
    { doc: 'parser/parser.req', trace: false, filter: null },
    { doc: 'parser/parser.req', trace: true, filter: null },
    { doc: null, trace: false, filter: { types: ['requirements', 'design'], status: 'approved' } },
    { doc: null, trace: false, filter: { types: ['adr'], status: null } },
    {
      doc: 'graph/graph.design',
      trace: false,
      filter: { types: ['design'], status: null },
    },
    {
      doc: 'a/b/c.req',
      trace: true,
      filter: { types: ['requirements'], status: 'draft' },
    },
  ];

  for (const state of cases) {
    it(`round-trips ${JSON.stringify(state)}`, () => {
      const hash = buildHash(state);
      const parsed = parseHash(hash);
      expect(parsed).toEqual(state);
    });
  }
});

describe('onHashChange', () => {
  it('returns an unsubscribe function', () => {
    const unsub = onHashChange(() => {});
    expect(typeof unsub).toBe('function');
    expect(() => unsub()).not.toThrow();
  });
});
