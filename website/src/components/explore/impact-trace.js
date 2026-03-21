/**
 * @module impact-trace
 * Transitive downstream traversal for the graph explorer.
 */

/** @typedef {import('./graph-explorer.js').Edge} Edge */

/**
 * Compute the set of node IDs transitively downstream from a given start node.
 *
 * "Downstream" means: documents that would be affected if the start node changes.
 * An edge where `edge.to === X` means `edge.from` is downstream of X.
 * The traversal is transitive and the start node is included in the result.
 *
 * @param {Edge[]} edges - All edges in the graph.
 * @param {string} startId - The starting node ID.
 * @returns {Set<string>} The set of transitively downstream node IDs (including startId).
 */
export function traceImpact(edges, startId) {
  // Build an adjacency list: for each node X, which nodes are downstream?
  // edge.to === X  →  edge.from is downstream of X
  /** @type {Map<string, string[]>} */
  const downstream = new Map();

  for (const edge of edges) {
    const existing = downstream.get(edge.to);
    if (existing) {
      existing.push(edge.from);
    } else {
      downstream.set(edge.to, [edge.from]);
    }
  }

  // BFS from startId
  /** @type {Set<string>} */
  const visited = new Set([startId]);
  /** @type {string[]} */
  const queue = [startId];

  while (queue.length > 0) {
    const current = /** @type {string} */ (queue.shift());
    const neighbors = downstream.get(current);
    if (!neighbors) continue;
    for (const neighbor of neighbors) {
      if (!visited.has(neighbor)) {
        visited.add(neighbor);
        queue.push(neighbor);
      }
    }
  }

  return visited;
}
