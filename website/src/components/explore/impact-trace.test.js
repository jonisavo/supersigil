import { describe, expect, it } from 'vitest';
import { traceImpact } from './impact-trace.js';

describe('traceImpact', () => {
  it('returns only the start node when there are no edges', () => {
    const edges = [];
    const result = traceImpact(edges, 'A');
    expect(result).toBeInstanceOf(Set);
    expect(result).toEqual(new Set(['A']));
  });

  it('traces a linear chain downstream', () => {
    // A ← B ← C  (B implements A, C implements B)
    // edges: from=B to=A means B depends on / implements A
    // so A's downstream = {A, B, C}
    const edges = [
      { from: 'B', to: 'A', kind: 'Implements' },
      { from: 'C', to: 'B', kind: 'Implements' },
    ];
    const result = traceImpact(edges, 'A');
    expect(result).toEqual(new Set(['A', 'B', 'C']));
  });

  it('traces a diamond graph downstream', () => {
    // A is implemented by B and C; D implements both B and C
    const edges = [
      { from: 'B', to: 'A', kind: 'Implements' },
      { from: 'C', to: 'A', kind: 'DependsOn' },
      { from: 'D', to: 'B', kind: 'Implements' },
      { from: 'D', to: 'C', kind: 'DependsOn' },
    ];
    const result = traceImpact(edges, 'A');
    expect(result).toEqual(new Set(['A', 'B', 'C', 'D']));
  });

  it('does not reach disconnected subgraphs', () => {
    const edges = [
      { from: 'B', to: 'A', kind: 'Implements' },
      { from: 'D', to: 'C', kind: 'Implements' },
    ];
    const result = traceImpact(edges, 'A');
    expect(result).toEqual(new Set(['A', 'B']));
  });

  it('handles cycles without infinite loop', () => {
    const edges = [
      { from: 'B', to: 'A', kind: 'Implements' },
      { from: 'A', to: 'B', kind: 'DependsOn' },
    ];
    const result = traceImpact(edges, 'A');
    expect(result).toEqual(new Set(['A', 'B']));
  });

  it('follows References edges', () => {
    const edges = [{ from: 'B', to: 'A', kind: 'References' }];
    const result = traceImpact(edges, 'A');
    expect(result).toEqual(new Set(['A', 'B']));
  });

  it('traces from a mid-chain node', () => {
    const edges = [
      { from: 'B', to: 'A', kind: 'Implements' },
      { from: 'C', to: 'B', kind: 'Implements' },
      { from: 'D', to: 'C', kind: 'Implements' },
    ];
    // Tracing from B: B's downstream is C and D, not A
    const result = traceImpact(edges, 'B');
    expect(result).toEqual(new Set(['B', 'C', 'D']));
  });
});
