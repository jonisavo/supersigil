export const GRAPH_COMPONENT_KINDS = new Set([
  'Criterion',
  'Task',
  'Decision',
  'Rationale',
  'Alternative',
]);

/**
 * Count criteria and their verification states from rendered fences.
 *
 * @param {any[]} fences
 * @returns {{ total: number, verified: number }}
 */
export function countCriteria(fences) {
  let total = 0;
  let verified = 0;

  function visit(components) {
    for (const component of components ?? []) {
      if (component.kind === 'Criterion') {
        total += 1;
        if (component.verification?.state === 'verified') {
          verified += 1;
        }
      }
      visit(component.children);
    }
  }

  for (const fence of fences ?? []) {
    visit(fence.components);
  }

  return { total, verified };
}

/**
 * Flatten graph-visible components into the runtime outline shared by the
 * website snapshot builder and drilldown renderer.
 *
 * @param {Array<{ id?: string | null, kind: string, body?: string | null, children?: any[], implements?: string[] }>} components
 * @returns {Array<{ id: string, kind: string, body: string | null, parentComponentId: string | null, displayId: string | null, implements?: string[] }>}
 */
export function buildGraphComponentOutline(components) {
  /** @type {Array<{ id: string, kind: string, body: string | null, parentComponentId: string | null, displayId: string | null, implements?: string[] }>} */
  const result = [];

  for (const component of components ?? []) {
    if (!component?.id || !GRAPH_COMPONENT_KINDS.has(component.kind)) {
      continue;
    }

    result.push({
      id: component.id,
      kind: component.kind,
      body: component.body ?? null,
      parentComponentId: null,
      displayId: component.id,
      implements: component.implements,
    });

    if (component.kind === 'Decision') {
      for (let index = 0; index < (component.children ?? []).length; index += 1) {
        const child = component.children[index];
        if (!GRAPH_COMPONENT_KINDS.has(child.kind)) {
          continue;
        }

        result.push({
          id: child.id ?? `${component.id}-${child.kind.toLowerCase()}-${index}`,
          kind: child.kind,
          body: child.body ?? null,
          parentComponentId: component.id,
          displayId: child.id ?? null,
          implements: child.implements,
        });
      }
    }
  }

  return result;
}
