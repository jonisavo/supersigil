/**
 * @module graph-explorer
 * Entry point for the interactive graph explorer component.
 * Mounts the explorer into a container element and wires up all sub-modules.
 */

import * as d3 from 'd3';
import forceInABox from 'force-in-a-box';
import { buildCoverageMap, clearDetail, renderClusterDetail, renderDetail, renderEmpty } from './detail-panel.js';
import { buildGraphComponentOutline } from './explorer-runtime-shared.js';
import {
  componentColor,
  extractFilterOptions,
  filterDocuments,
  searchDocuments,
} from './graph-data.js';
import { traceImpact } from './impact-trace.js';
import { buildHash, onHashChange, parseHash } from './url-router.js';

/**
 * @typedef {Object} GraphJSON
 * @property {DocumentNode[]} documents
 * @property {Edge[]} edges
 */

/**
 * @typedef {Object} DocumentNode
 * @property {string} id
 * @property {string|null} doc_type
 * @property {string|null} status
 * @property {string} title
 * @property {string|null} [path]
 * @property {string|null} [file_uri]
 * @property {string|null} [filePath]
 * @property {Component[]} components
 */

/**
 * @typedef {Object} Component
 * @property {string|null} id
 * @property {string} kind
 * @property {string|null} body
 * @property {Component[]} [children]
 * @property {string[]} [implements]
 */

/**
 * @typedef {Object} Edge
 * @property {string} from
 * @property {string} to
 * @property {string} kind
 */

/**
 * @typedef {Object} Cluster
 * @property {string} name
 * @property {string[]} docIds
 */

/**
 * @typedef {Object} ClusterBounds
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 */

const BASE_RADIUS = 12;
const RADIUS_SCALE = 3;
const CLUSTER_PADDING = 30;
const COMPONENT_RADIUS = 8;

/**
 * Compute the display radius for a document node.
 * Scales with the square root of the component count.
 *
 * @param {DocumentNode} doc
 * @returns {number}
 */
export function nodeRadius(doc) {
  return BASE_RADIUS + Math.sqrt(doc.components.length) * RADIUS_SCALE;
}

/**
 * Return the CSS custom property for the node stroke color based on doc_type.
 *
 * @param {DocumentNode} doc
 * @returns {string}
 */
export function nodeStrokeColor(doc) {
  switch (doc.doc_type) {
    case 'requirements':
      return 'var(--teal)';
    case 'design':
      return 'var(--green)';
    case 'adr':
      return 'var(--gold)';
    default:
      return 'var(--text-dim)';
  }
}

/**
 * Group documents into clusters by their ID prefix (everything before the last `/`).
 * Documents with no `/` in their ID form their own single-document cluster.
 * In multi-project workspaces, cluster labels include the project name.
 *
 * @param {DocumentNode[]} documents
 * @returns {Cluster[]}
 */
export function computeClusters(documents) {
  /** @type {Map<string, string[]>} */
  const groups = new Map();
  /** @type {Map<string, string>} prefix -> project name */
  const prefixProjects = new Map();

  const isMultiProject = documents.some((d) => d.project);

  for (const doc of documents) {
    const slashIdx = doc.id.lastIndexOf('/');
    const prefix = slashIdx === -1 ? doc.id : doc.id.slice(0, slashIdx);
    const existing = groups.get(prefix);
    if (existing) {
      existing.push(doc.id);
    } else {
      groups.set(prefix, [doc.id]);
    }
    if (isMultiProject && doc.project) {
      prefixProjects.set(prefix, doc.project);
    }
  }

  /** @type {Cluster[]} */
  const clusters = [];
  for (const [prefix, docIds] of groups) {
    const project = prefixProjects.get(prefix);
    const name = isMultiProject && project ? `${project} / ${prefix}` : prefix;
    clusters.push({ name, docIds, project: project || null });
  }
  return clusters;
}

/**
 * Compute the bounding rectangle for a cluster given node positions.
 *
 * @param {Cluster} cluster
 * @param {Map<string, {x: number, y: number, radius: number}>} nodePositions
 * @returns {ClusterBounds | null}
 */
export function computeClusterBounds(cluster, nodePositions) {
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  let found = false;

  for (const docId of cluster.docIds) {
    const pos = nodePositions.get(docId);
    if (!pos) continue;
    found = true;
    minX = Math.min(minX, pos.x - pos.radius);
    minY = Math.min(minY, pos.y - pos.radius);
    maxX = Math.max(maxX, pos.x + pos.radius);
    maxY = Math.max(maxY, pos.y + pos.radius);
  }

  if (!found) return null;

  const x = minX - CLUSTER_PADDING;
  const y = minY - CLUSTER_PADDING;
  return {
    x,
    y,
    width: maxX + CLUSTER_PADDING - x,
    height: maxY + CLUSTER_PADDING - y,
  };
}

/**
 * @typedef {Object} ComponentNode
 * @property {string} id           - Unique node ID: `${docId}::${componentId}`
 * @property {string} componentId  - The component's own ID
 * @property {string} kind         - Component kind (Criterion, Task, Decision, Rationale, Alternative)
 * @property {string|null} body    - Component body text
 * @property {string} parentDocId  - The parent document's ID
 * @property {string|null} parentComponentId - For Decision children, the parent Decision's component ID
 * @property {number} radius       - Display radius
 * @property {string} label        - Display label
 * @property {string[]} [implements] - For Tasks, the IDs of criteria they implement
 */

/**
 * Re-export componentColor as componentStrokeColor for backward compatibility.
 * The canonical implementation lives in graph-data.js.
 */
export const componentStrokeColor = componentColor;

/**
 * Build component nodes for drill-down from a document's components.
 * Only components with a non-null `id` and an eligible kind are included.
 * Decision children (Rationale, Alternative) are also expanded.
 *
 * @param {DocumentNode} doc
 * @returns {ComponentNode[]}
 */
export function buildComponentNodes(doc) {
  return buildGraphComponentOutline(doc.components).map((component) => ({
    id: `${doc.id}::${component.id}`,
    componentId: component.id,
    kind: component.kind,
    body: component.body,
    parentDocId: doc.id,
    parentComponentId: component.parentComponentId,
    radius: COMPONENT_RADIUS,
    label: component.displayId ? `${component.kind}: ${component.displayId}` : component.kind,
    implements: component.implements,
  }));
}

/**
 * Build internal edges for drill-down component nodes.
 * Creates edges for:
 * - Task → Criterion (via `implements` array)
 * - Decision → child (Rationale, Alternative)
 *
 * @param {ComponentNode[]} componentNodes
 * @returns {{source: string, target: string, kind: string}[]}
 */
export function buildComponentLinks(componentNodes) {
  /** @type {{source: string, target: string, kind: string}[]} */
  const result = [];

  // Index component nodes by componentId and canonical ref for lookup
  /** @type {Map<string, string>} componentId or canonical ref → node ID */
  const idMap = new Map();
  for (const node of componentNodes) {
    idMap.set(node.componentId, node.id);
    // Also index by canonical ref (e.g. "req/auth#auth-1")
    if (node.parentDocId) {
      idMap.set(`${node.parentDocId}#${node.componentId}`, node.id);
    }
  }

  for (const node of componentNodes) {
    // Task implements → Criterion links
    if (node.kind === 'Task' && node.implements) {
      for (const targetCompId of node.implements) {
        const targetNodeId = idMap.get(targetCompId);
        if (targetNodeId) {
          result.push({ source: node.id, target: targetNodeId, kind: 'implements' });
        }
      }
    }

    // Decision → child links (scoped by document to avoid cross-doc collisions)
    if (node.parentComponentId && node.parentDocId) {
      const parentNodeId = `${node.parentDocId}::${node.parentComponentId}`;
      result.push({ source: parentNodeId, target: node.id, kind: 'has_child' });
    }
  }

  return result;
}

/**
 * Extract the short label from a document ID (last segment after `/`).
 *
 * @param {string} id
 * @returns {string}
 */
function shortLabel(id) {
  const slashIdx = id.lastIndexOf('/');
  return slashIdx === -1 ? id : id.slice(slashIdx + 1);
}

/**
 * Mount the graph explorer into the given container element.
 *
 * @param {HTMLElement} container - The DOM element to render the explorer into.
 * @param {GraphJSON} data - The graph data to visualise.
 * @param {any[]} [renderData] - The render data array from render-data.json.
 * @param {{ provider: string, repo: string, host: string, mainBranch: string } | null} [repositoryInfo] - Repository info for evidence links.
 * @param {Object} [linkResolver] - Optional pre-built link resolver. When provided, passed to renderDetail instead of creating one from repositoryInfo.
 * @param {{
 *   getRenderData?: () => any[],
 *   getDocumentState?: (documentId: string) => any,
 *   openFile?: (target: { path?: string, uri?: string, line?: number }) => void,
 *   onSelectDocument?: (documentId: string) => void,
 *   onSwitchRoot?: (rootId: string) => void,
 *   rootContext?: {
 *     activeRootId: string,
 *     availableRoots: Array<{ id: string, name: string }>
 *   },
 * }} [runtimeOptions]
 * @returns {{ unmount(): void, refreshDetail(): void, updateRuntimeOptions(nextRuntimeOptions: any): void }}
 */
export function mount(container, data, renderData, repositoryInfo, linkResolver, runtimeOptions) {
  const { documents, edges } = data;
  let currentRuntimeOptions = runtimeOptions ?? {};

  function resolveRenderData() {
    return currentRuntimeOptions.getRenderData?.() ?? renderData ?? [];
  }

  function resolveDocumentState(documentId) {
    return currentRuntimeOptions.getDocumentState?.(documentId) ?? null;
  }

  function openFileTargetForNode(node) {
    if (!currentRuntimeOptions.openFile) {
      return null;
    }

    const line = 1;
    if (node.filePath) {
      return { path: node.filePath, line };
    }
    if (node.file_uri) {
      return { uri: node.file_uri, line };
    }
    if (node.path) {
      return { path: node.path, line };
    }
    return null;
  }

  const coverageMap = buildCoverageMap(resolveRenderData(), data);

  function badgeColor(docId) {
    const cov = coverageMap.get(docId);
    if (!cov) return null;
    if (cov.verified === cov.total) return 'var(--green)';
    if (cov.verified === 0) return 'var(--red)';
    return 'var(--gold)';
  }

  function appendVerificationBadges(selection) {
    selection.each(function (d) {
      if (/** @type {any} */ (d).componentId) return;
      const color = badgeColor(/** @type {any} */ (d).id);
      if (!color) return;
      const offset = /** @type {any} */ (d).radius * Math.SQRT1_2;
      d3.select(this)
        .append('circle')
        .attr('class', 'verification-badge')
        .attr('cx', offset)
        .attr('cy', -offset)
        .attr('r', 4)
        .attr('fill', color)
        .attr('stroke', 'var(--bg-deep)')
        .attr('stroke-width', 1.5);
    });
  }

  if (!container || typeof container.getBoundingClientRect !== 'function') {
    return { unmount() {} };
  }

  /** @param {any} endpoint @returns {string} */
  function edgeEndpointId(endpoint) {
    return typeof endpoint === 'string' ? endpoint : endpoint.id;
  }

  /** Resolve an edge endpoint to the document ID used for filter visibility. */
  function edgeFilterId(endpoint) {
    if (typeof endpoint === 'object' && endpoint.parentDocId) {
      return endpoint.parentDocId;
    }
    return edgeEndpointId(endpoint);
  }

  // ===== URL deep-linking guard =====
  /** When true, suppress hash updates (we're applying state from a hash). */
  let isRestoringState = false;

  // ===== Filter state =====
  /** @type {{ types: Set<string>, status: string|null }} */
  const filterState = { types: new Set(), status: null };

  // Extract filter options from data
  const filterOptions = extractFilterOptions(documents);

  // ===== Filter bar UI =====
  const explorerBar = document.createElement('div');
  explorerBar.className = 'explorer-bar';
  /** @type {HTMLSelectElement | null} */
  let rootSelector = null;

  function syncRootSelector() {
    const availableRoots = currentRuntimeOptions.rootContext?.availableRoots ?? [];
    if (!currentRuntimeOptions.onSwitchRoot || availableRoots.length <= 1) {
      rootSelector?.remove();
      rootSelector = null;
      return;
    }

    if (!rootSelector) {
      rootSelector = document.createElement('select');
      rootSelector.className = 'root-selector';
      rootSelector.setAttribute('aria-label', 'Switch workspace root');
      rootSelector.addEventListener('change', () => {
        currentRuntimeOptions.onSwitchRoot?.(rootSelector?.value ?? '');
      });
      explorerBar.insertBefore(rootSelector, explorerBar.firstChild);
    }

    rootSelector.replaceChildren();
    for (const root of availableRoots) {
      const option = document.createElement('option');
      option.value = root.id;
      option.textContent = root.name;
      option.selected = root.id === currentRuntimeOptions.rootContext?.activeRootId;
      rootSelector.appendChild(option);
    }
  }

  syncRootSelector();

  // Type filter — custom dropdown multiselect
  const typeDropdown = document.createElement('div');
  typeDropdown.className = 'filter-multiselect';

  const typeToggle = document.createElement('button');
  typeToggle.className = 'filter-dropdown-toggle filter-multiselect-toggle';
  typeDropdown.appendChild(typeToggle);

  const typeMenu = document.createElement('div');
  typeMenu.className = 'filter-dropdown-menu filter-multiselect-menu';

  /** Update the toggle button label based on current selection */
  function updateTypeLabel() {
    if (filterState.types.size === 0) {
      typeToggle.textContent = 'All types \u25BE';
    } else {
      const names = [...filterState.types].sort().join(', ');
      typeToggle.textContent = `${names} \u25BE`;
    }
  }

  for (const docType of filterOptions.types) {
    const item = document.createElement('label');
    item.className = 'filter-dropdown-item filter-multiselect-item';

    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.value = docType;
    checkbox.className = 'filter-multiselect-checkbox';

    const label = document.createElement('span');
    label.textContent = docType;

    item.appendChild(checkbox);
    item.appendChild(label);
    typeMenu.appendChild(item);

    checkbox.addEventListener('change', () => {
      if (checkbox.checked) {
        filterState.types.add(docType);
      } else {
        filterState.types.delete(docType);
      }
      updateTypeLabel();
      applyFilters();
      syncHashToUrl();
    });
  }

  typeDropdown.appendChild(typeMenu);
  updateTypeLabel();

  // Toggle menu open/close — close the other dropdown first
  typeToggle.addEventListener('click', (e) => {
    e.stopPropagation();
    statusDropdown?.classList.remove('open');
    typeDropdown.classList.toggle('open');
  });

  /** @param {MouseEvent} e */
  function handleTypeDropdownClose(e) {
    if (!typeDropdown.contains(/** @type {Node} */ (e.target))) {
      typeDropdown.classList.remove('open');
    }
  }
  document.addEventListener('click', handleTypeDropdownClose);

  explorerBar.appendChild(typeDropdown);

  // Status dropdown — custom single-select matching type multiselect styling
  /** @type {HTMLDivElement|null} */
  let statusDropdown = null;
  /** @type {((e: MouseEvent) => void)|null} */
  let handleStatusDropdownClose = null;
  if (filterOptions.statuses.length > 0) {
    statusDropdown = document.createElement('div');
    statusDropdown.className = 'filter-select';

    const statusToggle = document.createElement('button');
    statusToggle.className = 'filter-dropdown-toggle filter-select-toggle';
    statusToggle.textContent = 'All statuses \u25BE';
    statusDropdown.appendChild(statusToggle);

    const statusMenu = document.createElement('div');
    statusMenu.className = 'filter-dropdown-menu filter-select-menu';

    const allStatuses = ['all', ...filterOptions.statuses];
    for (const status of allStatuses) {
      const item = document.createElement('div');
      item.className = 'filter-dropdown-item filter-select-item';
      if (status === 'all') item.classList.add('active');
      item.dataset.value = status;
      item.textContent = status === 'all' ? 'All statuses' : status;
      item.addEventListener('click', () => {
        filterState.status = status === 'all' ? null : status;
        statusToggle.textContent = `${status === 'all' ? 'All statuses' : status} \u25BE`;
        for (const el of statusMenu.querySelectorAll('.filter-select-item')) {
          el.classList.remove('active');
        }
        item.classList.add('active');
        statusDropdown.classList.remove('open');
        applyFilters();
        syncHashToUrl();
      });
      statusMenu.appendChild(item);
    }

    statusDropdown.appendChild(statusMenu);

    statusToggle.addEventListener('click', (e) => {
      e.stopPropagation();
      // Close the type dropdown if open
      typeDropdown.classList.remove('open');
      statusDropdown.classList.toggle('open');
    });

    handleStatusDropdownClose = /** @param {MouseEvent} e */ (e) => {
      if (!statusDropdown.contains(/** @type {Node} */ (e.target))) {
        statusDropdown.classList.remove('open');
      }
    };
    document.addEventListener('click', handleStatusDropdownClose);

    explorerBar.appendChild(statusDropdown);
  }

  // ===== Search input =====
  const searchWrapper = document.createElement('div');
  searchWrapper.className = 'explorer-search';
  searchWrapper.style.position = 'relative';

  const searchIcon = document.createElement('span');
  searchIcon.className = 'explorer-search-icon';
  searchIcon.textContent = '/';
  searchWrapper.appendChild(searchIcon);

  const searchInput = document.createElement('input');
  searchInput.type = 'text';
  searchInput.placeholder = 'Search docs...';
  searchWrapper.appendChild(searchInput);

  const searchResults = document.createElement('div');
  searchResults.className = 'search-results';
  searchWrapper.appendChild(searchResults);

  explorerBar.appendChild(searchWrapper);

  /** @type {number} Index of the currently highlighted search result (-1 = none) */
  let searchHighlight = -1;

  /** @type {DocumentNode[]} Currently displayed (capped) search results */
  let currentSearchResults = [];

  /** @type {number|null} Debounce timer for closing on blur */
  let blurTimer = null;

  /**
   * Render the search results dropdown.
   * @param {DocumentNode[]} results
   */
  function renderSearchResults(results) {
    searchResults.innerHTML = '';
    searchHighlight = -1;

    if (results.length === 0) {
      searchResults.classList.remove('open');
      return;
    }

    currentSearchResults = results.slice(0, 8);
    for (let i = 0; i < currentSearchResults.length; i++) {
      const doc = currentSearchResults[i];
      const item = document.createElement('div');
      item.className = 'search-result-item';
      item.dataset.index = String(i);

      const idSpan = document.createElement('span');
      idSpan.className = 'search-result-id';
      idSpan.textContent = doc.id;

      const titleSpan = document.createElement('span');
      titleSpan.className = 'search-result-title';
      titleSpan.textContent = doc.title ?? '';

      item.appendChild(idSpan);
      item.appendChild(titleSpan);

      item.addEventListener('mousedown', (e) => {
        e.preventDefault(); // prevent blur from firing before click
        pickSearchResult(doc);
      });

      item.addEventListener('mouseenter', () => {
        setSearchHighlight(i);
      });

      searchResults.appendChild(item);
    }

    searchResults.classList.add('open');
  }

  /**
   * Set the highlighted search result index and update visual state.
   * @param {number} idx
   */
  function setSearchHighlight(idx) {
    const items = searchResults.querySelectorAll('.search-result-item');
    for (const el of items) {
      el.classList.remove('highlighted');
    }
    searchHighlight = idx;
    if (idx >= 0 && idx < items.length) {
      items[idx].classList.add('highlighted');
    }
  }

  /**
   * Pick a search result: zoom to node, open detail panel, clear search.
   * @param {DocumentNode} doc
   */
  function pickSearchResult(doc) {
    // Find the simulation node for this document
    const simNode = nodes.find((n) => n.id === doc.id);
    if (simNode && simNode.x != null && simNode.y != null) {
      clearTrace();
      selectNode(simNode);

      // Zoom to the node (use current SVG dimensions, not stale mount-time values)
      const svgEl = svg.node();
      const currentWidth = svgEl ? svgEl.clientWidth || width : width;
      const currentHeight = svgEl ? svgEl.clientHeight || height : height;
      const scale = 1.5;
      const tx = currentWidth / 2 - simNode.x * scale;
      const ty = currentHeight / 2 - simNode.y * scale;
      svg
        .transition()
        .duration(500)
        .call(/** @type {any} */ (zoom).transform, d3.zoomIdentity.translate(tx, ty).scale(scale));
    }

    // Clear search
    searchInput.value = '';
    searchResults.innerHTML = '';
    searchResults.classList.remove('open');
    searchHighlight = -1;
  }

  function closeSearch() {
    searchResults.innerHTML = '';
    searchResults.classList.remove('open');
    searchHighlight = -1;
  }

  searchInput.addEventListener('input', () => {
    const query = searchInput.value;
    const results = searchDocuments(documents, query);
    renderSearchResults(results);
  });

  searchInput.addEventListener('keydown', (event) => {
    const items = searchResults.querySelectorAll('.search-result-item');
    const count = items.length;

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (count > 0) {
        setSearchHighlight(searchHighlight < count - 1 ? searchHighlight + 1 : 0);
      }
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (count > 0) {
        setSearchHighlight(searchHighlight > 0 ? searchHighlight - 1 : count - 1);
      }
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (searchHighlight >= 0 && searchHighlight < count) {
        const doc = currentSearchResults[searchHighlight];
        if (doc) pickSearchResult(doc);
      }
    } else if (event.key === 'Escape') {
      closeSearch();
      searchInput.blur();
    }
  });

  searchInput.addEventListener('blur', () => {
    // Delay closing to allow mousedown on results to fire
    blurTimer = window.setTimeout(() => {
      closeSearch();
    }, 150);
  });

  searchInput.addEventListener('focus', () => {
    if (blurTimer != null) {
      clearTimeout(blurTimer);
      blurTimer = null;
    }
    // Re-show results if there's a query
    const query = searchInput.value;
    if (query.trim()) {
      const results = searchDocuments(documents, query);
      renderSearchResults(results);
    }
  });

  container.classList.add('explorer');
  container.prepend(explorerBar);

  // ===== Resizable split pane layout =====
  const splitContainer = document.createElement('div');
  splitContainer.className = 'explorer-split';
  container.appendChild(splitContainer);

  const graphPane = document.createElement('div');
  graphPane.className = 'explorer-split-graph';
  splitContainer.appendChild(graphPane);

  const divider = document.createElement('div');
  divider.className = 'explorer-split-divider';
  splitContainer.appendChild(divider);

  const specPane = document.createElement('div');
  specPane.className = 'explorer-split-panel';
  splitContainer.appendChild(specPane);

  // Restore saved split position from localStorage
  const SPLIT_KEY = 'supersigil-explorer-split';
  const savedSplit = localStorage.getItem(SPLIT_KEY);
  if (savedSplit) {
    const pct = Number.parseFloat(savedSplit);
    if (pct > 10 && pct < 90) {
      graphPane.style.flex = `0 0 ${pct}%`;
      specPane.style.width = `${100 - pct}%`;
    }
  }

  // Resizable divider drag logic
  let isDragging = false;
  divider.addEventListener('mousedown', (e) => {
    e.preventDefault();
    isDragging = true;
    divider.classList.add('dragging');
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  });

  let lastSplitPct = savedSplit ? Number.parseFloat(savedSplit) : 0;

  /** @param {MouseEvent} e */
  function handleDividerMousemove(e) {
    if (!isDragging) return;
    const splitRect = splitContainer.getBoundingClientRect();
    const x = e.clientX - splitRect.left;
    const totalWidth = splitRect.width;
    const pct = (x / totalWidth) * 100;
    const clamped = Math.max(20, Math.min(80, pct));
    graphPane.style.flex = `0 0 ${clamped}%`;
    specPane.style.width = `${100 - clamped}%`;
    lastSplitPct = clamped;
  }
  document.addEventListener('mousemove', handleDividerMousemove);

  function handleDividerMouseup() {
    if (!isDragging) return;
    isDragging = false;
    divider.classList.remove('dragging');
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    localStorage.setItem(SPLIT_KEY, String(lastSplitPct));
  }
  document.addEventListener('mouseup', handleDividerMouseup);

  // Wrap SVG in a canvas container for proper flex layout
  const canvasDiv = document.createElement('div');
  canvasDiv.className = 'explorer-canvas';
  graphPane.appendChild(canvasDiv);

  const rect = canvasDiv.getBoundingClientRect();
  const screenWidth = rect.width || 800;
  const screenHeight = rect.height || 600;

  // Use a large virtual canvas so the treemap has room to partition clusters.
  // On small screens (mobile), this prevents clusters from being crammed into
  // tiny cells. The zoom transform will fit the content on initial load.
  const MIN_SIM_SIZE = 1600;
  const width = Math.max(screenWidth, MIN_SIM_SIZE);
  const height = Math.max(screenHeight, MIN_SIM_SIZE * 0.75);

  const clusters = computeClusters(documents);

  // Map from doc ID to cluster name for quick lookup
  /** @type {Map<string, string>} */
  const docClusterMap = new Map();
  for (const cluster of clusters) {
    for (const docId of cluster.docIds) {
      docClusterMap.set(docId, cluster.name);
    }
  }

  // Build simulation nodes with radius pre-computed and group for forceInABox.
  // Group by prefix cluster name so forceInABox creates tight prefix groups.
  /** @type {(DocumentNode & d3.SimulationNodeDatum & {radius: number, group: string})[]} */
  const nodes = documents.map((doc) => ({
    ...doc,
    radius: nodeRadius(doc),
    group: docClusterMap.get(doc.id) ?? doc.id,
  }));

  // Build simulation links
  /** @type {d3.SimulationLinkDatum<typeof nodes[number]>[]} */
  const links = edges.map((e) => ({
    source: e.from,
    target: e.to,
    kind: e.kind,
  }));

  // Create SVG inside the canvas container
  const svg = d3
    .select(canvasDiv)
    .append('svg')
    .attr('width', '100%')
    .attr('height', '100%')
    .attr('viewBox', `0 0 ${width} ${height}`)
    .style('background', 'var(--bg-deep)');

  // Zoom layer
  const zoomLayer = svg.append('g').attr('class', 'zoom-layer');

  // Sub-groups in rendering order
  const clustersGroup = zoomLayer.append('g').attr('class', 'clusters');
  const edgesGroup = zoomLayer.append('g').attr('class', 'edges');
  const nodesGroup = zoomLayer.append('g').attr('class', 'nodes');

  // Edge legend (HTML overlay, positioned in top-left of canvas)
  const legendEl = document.createElement('div');
  legendEl.className = 'edge-legend';
  legendEl.innerHTML = [
    { label: 'Implements', cls: 'legend-implements' },
    { label: 'DependsOn', cls: 'legend-dependson' },
    { label: 'References', cls: 'legend-references' },
  ]
    .map(
      (item) =>
        `<div class="edge-legend-item"><span class="edge-legend-line ${item.cls}"></span><span>${item.label}</span></div>`,
    )
    .join('');
  canvasDiv.appendChild(legendEl);

  // Render edges — color/dash-coded by kind, no labels by default.
  // Default opacity set via style (not attr) so selection dimming works.
  let edgeLines = edgesGroup
    .selectAll('line.edge')
    .data(links)
    .join('line')
    .attr('class', 'edge')
    .attr('data-kind', (d) => d.kind)
    .attr('data-idx', (_d, i) => i)
    .style('opacity', (d) => (d.kind === 'References' ? 0.3 : 0.4))
    .on('mouseenter', function () {
      const idx = +this.getAttribute('data-idx');
      d3.select(edgeLabels.nodes()[idx]).classed('visible', true);
    })
    .on('mouseleave', function () {
      const idx = +this.getAttribute('data-idx');
      d3.select(edgeLabels.nodes()[idx]).classed('visible', false);
    });

  // Edge labels — hidden by default (CSS opacity: 0), shown on hover or selection
  let edgeLabels = edgesGroup
    .selectAll('text.edge-label')
    .data(links)
    .join('text')
    .attr('class', 'edge-label')
    .attr('font-size', 9)
    .attr('font-family', 'var(--font-mono)')
    .attr('text-anchor', 'middle')
    .attr('dy', -4)
    .text((d) => d.kind);

  // Render nodes
  let nodeGroups = nodesGroup
    .selectAll('g.node')
    .data(nodes)
    .join('g')
    .attr('class', 'node')
    .style('cursor', 'grab');

  nodeGroups
    .append('circle')
    .attr('r', (d) => d.radius)
    .attr('fill', 'var(--bg-card)')
    .attr('stroke', (d) => nodeStrokeColor(d))
    .attr('stroke-width', 2);

  appendVerificationBadges(nodeGroups);

  nodeGroups
    .append('text')
    .attr('fill', 'var(--text)')
    .attr('font-size', 11)
    .attr('font-family', 'var(--font-body)')
    .attr('text-anchor', 'middle')
    .attr('dy', (d) => d.radius + 14)
    .text((d) => shortLabel(d.id));

  // Cluster rects and labels (will be updated on tick)
  // Project color palette for cluster stroke/label in multi-project mode
  const projectColors = {};
  const palette = [
    'var(--gold-dim)', 'var(--teal)', '#8b5cf6', '#f59e0b',
    '#ec4899', '#06b6d4', '#84cc16', '#f97316',
  ];
  let colorIdx = 0;
  for (const c of clusters) {
    if (c.project && !projectColors[c.project]) {
      projectColors[c.project] = palette[colorIdx % palette.length];
      colorIdx++;
    }
  }
  function clusterColor(d) {
    return d.project ? (projectColors[d.project] || 'var(--gold-dim)') : 'var(--gold-dim)';
  }

  const clusterRects = clustersGroup
    .selectAll('rect.cluster')
    .data(clusters)
    .join('rect')
    .attr('class', 'cluster')
    .attr('fill', 'none')
    .attr('stroke', clusterColor)
    .attr('stroke-width', 1.5)
    .attr('stroke-dasharray', '6 4')
    .attr('rx', 8)
    .attr('ry', 8);

  const clusterLabels = clustersGroup
    .selectAll('text.cluster-label')
    .data(clusters)
    .join('text')
    .attr('class', 'cluster-label')
    .attr('fill', clusterColor)
    .attr('font-size', 10)
    .attr('font-family', 'var(--font-mono)')
    .text((d) => d.name);

  // Project-level visual grouping: labels and colors only (no enclosing rects,
  // as force-based project separation doesn't produce clean non-overlapping regions).

  // ===== Filter application =====

  /**
   * Apply current filter state: compute visible set, adjust node and edge opacity.
   * Filtered-out nodes fade to near-invisible rather than being removed (req-6-3).
   */
  function applyFilters() {
    const visibleSet = filterDocuments(documents, filterState);

    // Adjust node opacity
    nodesGroup.selectAll('g.node').attr('opacity', (d) => {
      const id = /** @type {any} */ (d).id;
      // Component nodes follow their parent document's visibility
      const parentDocId = /** @type {any} */ (d).parentDocId;
      const checkId = parentDocId ?? id;
      return visibleSet.has(checkId) ? 1 : 0.08;
    });

    // Adjust edge opacity: fully visible if both endpoints visible, near-invisible otherwise
    edgesGroup.selectAll('line.edge').attr('stroke-opacity', (d) => {
      const sourceId = edgeFilterId(d.source);
      const targetId = edgeFilterId(d.target);
      return visibleSet.has(sourceId) && visibleSet.has(targetId) ? 0.6 : 0.03;
    });

    // Adjust edge label opacity to match
    edgesGroup.selectAll('text.edge-label').attr('opacity', (d) => {
      const sourceId = edgeFilterId(d.source);
      const targetId = edgeFilterId(d.target);
      return visibleSet.has(sourceId) && visibleSet.has(targetId) ? 1 : 0.03;
    });
  }

  // ===== Cluster drill-down state =====

  /** @type {Set<string>} IDs of documents currently expanded */
  const expandedDocs = new Set();
  /** @type {Map<string, ComponentNode[]>} docId → component nodes currently in simulation */
  const expandedComponentNodes = new Map();

  /**
   * Collect all active nodes (document + expanded component nodes).
   * @returns {any[]}
   */
  function allActiveNodes() {
    const result = [...nodes];
    for (const compNodes of expandedComponentNodes.values()) {
      result.push(...compNodes);
    }
    return result;
  }

  // forceInABox: treemap-based grouping force that assigns non-overlapping
  // regions to each cluster, then nudges nodes toward their group's focus.
  const groupingForce = forceInABox()
    .strength(0.1)
    .template('treemap')
    .groupBy('group')
    .links(links)
    .linkStrengthInterCluster(0.01)
    .linkStrengthIntraCluster(0.1)
    .size([width, height]);

  // Force simulation
  const simulation = d3
    .forceSimulation(nodes)
    .force('group', groupingForce)
    .force(
      'link',
      d3
        .forceLink(links)
        .id((d) => /** @type {typeof nodes[number]} */ (d).id)
        .distance(80)
        .strength(groupingForce.getLinkStrength),
    )
    .force('charge', d3.forceManyBody().strength(-200))
    .force(
      'collide',
      d3.forceCollide().radius((d) => /** @type {typeof nodes[number]} */ (d).radius + 4),
    )
    .on('tick', ticked);

  function ticked() {
    // Update edges
    edgeLines
      .attr('x1', (d) => /** @type {any} */ (d.source).x)
      .attr('y1', (d) => /** @type {any} */ (d.source).y)
      .attr('x2', (d) => /** @type {any} */ (d.target).x)
      .attr('y2', (d) => /** @type {any} */ (d.target).y);

    edgeLabels
      .attr('x', (d) => /** @type {any} */ (d.source.x + /** @type {any} */ (d.target).x) / 2)
      .attr('y', (d) => /** @type {any} */ (d.source.y + /** @type {any} */ (d.target).y) / 2);

    // Update nodes
    nodeGroups.attr('transform', (d) => `translate(${d.x},${d.y})`);

    // Update cluster bounds (include component nodes for expanded clusters)
    /** @type {Map<string, {x: number, y: number, radius: number}>} */
    const nodePositions = new Map();
    for (const node of allActiveNodes()) {
      nodePositions.set(node.id, {
        x: node.x ?? 0,
        y: node.y ?? 0,
        radius: node.radius,
      });
    }

    // Compute bounds once per cluster, reuse for rects and labels
    /** @type {Map<string, ClusterBounds>} */
    const boundsMap = new Map();
    for (const cluster of clusters) {
      const bounds = computeClusterBounds(cluster, nodePositions);
      if (bounds) boundsMap.set(cluster.name, bounds);
    }

    clusterRects.each(function (cluster) {
      const bounds = boundsMap.get(cluster.name);
      if (bounds) {
        d3.select(this)
          .attr('x', bounds.x)
          .attr('y', bounds.y)
          .attr('width', bounds.width)
          .attr('height', bounds.height)
          .attr('visibility', 'visible');
      } else {
        d3.select(this).attr('visibility', 'hidden');
      }
    });

    clusterLabels.each(function (cluster) {
      const bounds = boundsMap.get(cluster.name);
      if (bounds) {
        d3.select(this)
          .attr('x', bounds.x + 8)
          .attr('y', bounds.y + 14)
          .attr('visibility', 'visible');
      } else {
        d3.select(this).attr('visibility', 'hidden');
      }
    });

  }

  // Zoom behavior
  const zoom = d3
    .zoom()
    .scaleExtent([0.1, 4])
    .on('zoom', (event) => {
      zoomLayer.attr('transform', event.transform);
    });

  svg.call(/** @type {any} */ (zoom));

  // Drag behavior
  const drag = d3
    .drag()
    .on('start', (event, d) => {
      if (!event.active) simulation.alphaTarget(0.3).restart();
      d.fx = d.x;
      d.fy = d.y;
    })
    .on('drag', (event, d) => {
      d.fx = event.x;
      d.fy = event.y;
    })
    .on('end', (event, d) => {
      if (!event.active) simulation.alphaTarget(0);
      d.fx = null;
      d.fy = null;
    });

  nodeGroups.call(/** @type {any} */ (drag));

  // ===== Cluster drill-down (expand/collapse on double-click) =====

  /**
   * Re-bind all edges (document-level + component-level) to the SVG and simulation.
   */
  function rebindEdges() {
    // Merge document links with component links computed across ALL expanded
    // documents so that cross-document Task→Criterion links are resolved.
    const allComponentNodes = [];
    for (const compNodes of expandedComponentNodes.values()) {
      allComponentNodes.push(...compNodes);
    }
    const allLinks = [...links, ...buildComponentLinks(allComponentNodes)];

    // Update grouping force with new links
    groupingForce.links(allLinks);

    // Update simulation link force with adaptive strength from grouping force
    simulation.force(
      'link',
      d3
        .forceLink(allLinks)
        .id((d) => /** @type {any} */ (d).id)
        .distance((d) =>
          /** @type {any} */ (d).kind === 'has_child' ||
          /** @type {any} */ (d).kind === 'implements'
            ? 40
            : 100,
        )
        .strength(groupingForce.getLinkStrength),
    );

    // Re-binddata for edge lines
    edgeLines = edgesGroup
      .selectAll('line.edge')
      .data(
        allLinks,
        (d) =>
          `${/** @type {any} */ (d).source?.id ?? d.source}-${/** @type {any} */ (d).target?.id ?? d.target}-${/** @type {any} */ (d).kind}`,
      )
      .join(
        (enter) =>
          enter
            .append('line')
            .attr('class', 'edge')
            .attr('data-kind', (d) => d.kind)
            .attr('stroke-width', (d) =>
              d.kind === 'has_child' || d.kind === 'implements' ? 0.8 : 1,
            )
            .style('opacity', 0)
            .call((sel) => sel.transition().duration(300).style('opacity', 0.4)),
        (update) => update,
        (exit) => exit.transition().duration(200).style('opacity', 0).remove(),
      );

    // Re-bind edge labels
    edgeLabels = edgesGroup
      .selectAll('text.edge-label')
      .data(
        allLinks,
        (d) =>
          `${/** @type {any} */ (d).source?.id ?? d.source}-${/** @type {any} */ (d).target?.id ?? d.target}-${/** @type {any} */ (d).kind}`,
      )
      .join(
        (enter) =>
          enter
            .append('text')
            .attr('class', 'edge-label')
            .attr('font-size', 9)
            .attr('font-family', 'var(--font-mono)')
            .attr('text-anchor', 'middle')
            .attr('dy', -4)
            .text((d) => d.kind),
        (update) => update,
        (exit) => exit.remove(),
      );
  }

  /**
   * Re-bind all nodes (document-level + component-level) to the SVG and simulation.
   */
  function rebindNodes() {
    // Merge document nodes with all expanded component nodes
    const allNodes = [...nodes];
    for (const compNodes of expandedComponentNodes.values()) {
      allNodes.push(.../** @type {any[]} */ (compNodes));
    }

    // Update simulation nodes
    simulation.nodes(allNodes);

    // Update collide force to include component nodes
    simulation.force(
      'collide',
      d3
        .forceCollide()
        .radius((d) => /** @type {any} */ (d).radius + /** @type {any} */ (d.componentId ? 4 : 8)),
    );

    // Re-bind node groups with enter/exit transitions
    nodeGroups = nodesGroup
      .selectAll('g.node')
      .data(allNodes, (d) => /** @type {any} */ (d).id)
      .join(
        (enter) => {
          const g = enter
            .append('g')
            .attr('class', (d) =>
              /** @type {any} */ (d).componentId ? 'node component-node' : 'node',
            )
            .style('cursor', 'grab')
            .attr('opacity', 0);

          g.append('circle')
            .attr('r', (d) => /** @type {any} */ (d).radius)
            .attr('fill', 'var(--bg-card)')
            .attr('stroke', (d) => {
              const cn = /** @type {any} */ (d);
              return cn.componentId ? componentColor(cn.kind) : nodeStrokeColor(cn);
            })
            .attr('stroke-width', (d) => (/** @type {any} */ (d).componentId ? 1.5 : 2));

          g.append('text')
            .attr('fill', 'var(--text)')
            .attr('font-size', (d) => (/** @type {any} */ (d).componentId ? 9 : 11))
            .attr('font-family', 'var(--font-body)')
            .attr('text-anchor', 'middle')
            .attr('dy', (d) => /** @type {any} */ (d).radius + 12)
            .text((d) => {
              const cn = /** @type {any} */ (d);
              return cn.label ?? shortLabel(cn.id);
            });

          appendVerificationBadges(g);

          g.transition().duration(300).attr('opacity', 1);

          // Wire up drag for new nodes
          g.call(/** @type {any} */ (drag));

          // Wire up dblclick for document nodes (not component nodes)
          g.filter((d) => !(/** @type {any} */ (d).componentId)).on('dblclick', handleDblClick);

          return g;
        },
        (update) => update,
        (exit) => exit.transition().duration(200).attr('opacity', 0).remove(),
      );
  }

  /**
   * Handle double-click on a document node to expand/collapse its components.
   *
   * @param {MouseEvent} event
   * @param {typeof nodes[number]} d
   */
  function handleDblClick(event, d) {
    event.stopPropagation();

    const docId = d.id;

    if (expandedDocs.has(docId)) {
      // Collapse: remove component node IDs from cluster tracking
      const collapsingNodes = expandedComponentNodes.get(docId);
      if (collapsingNodes) {
        const clusterName = docClusterMap.get(docId);
        const cluster = clusterName ? clusters.find((c) => c.name === clusterName) : null;
        for (const cn of collapsingNodes) {
          docClusterMap.delete(cn.id);
          if (cluster) {
            const idx = cluster.docIds.indexOf(cn.id);
            if (idx !== -1) cluster.docIds.splice(idx, 1);
          }
        }
      }
      expandedDocs.delete(docId);
      expandedComponentNodes.delete(docId);
    } else {
      // Expand
      expandedDocs.add(docId);

      const compNodes = buildComponentNodes(d);
      // Position component nodes near the parent
      for (const cn of compNodes) {
        /** @type {any} */ (cn).x = (d.x ?? 0) + (Math.random() - 0.5) * 40;
        /** @type {any} */ (cn).y = (d.y ?? 0) + (Math.random() - 0.5) * 40;
      }

      expandedComponentNodes.set(docId, compNodes);

      // Add component node IDs to cluster tracking so cluster bounds include them
      const clusterName = docClusterMap.get(docId);
      if (clusterName) {
        for (const cn of compNodes) {
          docClusterMap.set(cn.id, clusterName);
          /** @type {any} */ (cn).group = clusterName;
        }
        // Add component node IDs to the cluster's docIds so bounds include them
        const cluster = clusters.find((c) => c.name === clusterName);
        if (cluster) {
          for (const cn of compNodes) {
            cluster.docIds.push(cn.id);
          }
        }
      }
    }

    rebindNodes();
    rebindEdges();
    applyFilters();

    // Reheat simulation
    simulation.alpha(0.5).restart();
  }

  // Wire up initial dblclick handlers
  nodeGroups.on('dblclick', handleDblClick);

  // ===== Detail sidebar (single-click selection) =====

  // Create the detail panel container inside the spec pane
  const detailPanel = document.createElement('div');
  detailPanel.className = 'detail-panel';
  specPane.appendChild(detailPanel);

  // Show the empty state by default
  renderEmpty(detailPanel, data, resolveRenderData());

  /** @type {string|null} Currently selected node ID */
  let selectedNodeId = null;

  /** @type {string|null} Currently selected cluster name */
  let selectedClusterName = null;

  /**
   * Show edge labels only for edges connected to the given node ID.
   * Pass null to hide all edge labels.
   * @param {string|null} nodeId
   */
  function showEdgeLabelsFor(nodeId) {
    edgeLabels.classed('visible', (/** @type {any} */ d) => {
      if (!nodeId) return false;
      const srcId = edgeEndpointId(d.source);
      const tgtId = edgeEndpointId(d.target);
      return srcId === nodeId || tgtId === nodeId;
    });
    // Connected edges: full opacity + thicker; unrelated: nearly invisible.
    // Uses inline style (not attr) so it overrides CSS specificity.
    edgeLines.each(function (/** @type {any} */ d) {
      const srcId = edgeEndpointId(d.source);
      const tgtId = edgeEndpointId(d.target);
      const connected = nodeId && (srcId === nodeId || tgtId === nodeId);
      const el = d3.select(this);
      el.style('opacity', nodeId ? (connected ? 0.9 : 0.06) : null);
      el.style('stroke-width', nodeId ? (connected ? '2.5px' : null) : null);
    });
  }

  /**
   * Clear cluster selection visuals (highlight ring, node glow, edge dimming).
   */
  function deselectCluster() {
    if (!selectedClusterName) return;
    clusterRects.classed('cluster-selected', false);
    nodesGroup.selectAll('g.node').classed('in-selected-cluster', false);
    // Reset edge styling
    edgeLines.each(function () {
      d3.select(this).style('opacity', null).style('stroke-width', null);
    });
    clearDetail(detailPanel);
    renderEmpty(detailPanel, data, resolveRenderData());
    selectedClusterName = null;
  }

  /**
   * Clear the current selection (node or cluster): remove highlights, show empty state.
   */
  function clearSelection() {
    if (selectedClusterName) {
      deselectCluster();
      syncHashToUrl();
      return;
    }
    if (selectedNodeId) {
      clearTrace();
      nodesGroup.selectAll('g.node').classed('node-selected', false);
      showEdgeLabelsFor(null);
      clearDetail(detailPanel);
      renderEmpty(detailPanel, data, resolveRenderData());
      selectedNodeId = null;
      syncHashToUrl();
    }
  }

  /**
   * Smoothly pan and zoom the view to center on the given point.
   *
   * @param {number} x - World X coordinate.
   * @param {number} y - World Y coordinate.
   * @param {number} targetScale - Desired zoom scale.
   */
  function panToPoint(x, y, targetScale) {
    // D3 zoom transform: screen_point = translate + viewBox_point * scale
    // The SVG viewBox adds its own scaling (viewBox coords → screen pixels).
    // The zoom transform is layered on top, so scale=1 means "viewBox default".
    //
    // To place viewBox point (x,y) at screen position (sx,sy) with zoom k:
    //   tx = sx - x * k    (where tx, x are in viewBox coords; sx in screen px)
    //
    // But since the zoomLayer transform is applied INSIDE the viewBox,
    // everything is in viewBox coordinates. The SVG viewBox scaling maps
    // the result to screen pixels automatically.
    //
    // So: tx = viewBox_visible_center_x - x * k
    const isMobile = screenWidth < 600;

    // Visible center in viewBox coords (the graph pane takes up the left portion)
    const cx = width / 2;
    const cy = isMobile ? (height * 0.55) / 2 : height / 2;

    const tx = cx - x * targetScale;
    const ty = cy - y * targetScale;
    svg
      .transition()
      .duration(400)
      .call(
        /** @type {any} */ (zoom).transform,
        d3.zoomIdentity.translate(tx, ty).scale(targetScale),
      );
  }

  /**
   * Smoothly pan the view to center on a node.
   * @param {typeof nodes[number]} d
   */
  function centerOnNode(d) {
    if (d.x == null || d.y == null) return;
    const isMobile = screenWidth < 600;
    panToPoint(d.x, d.y, isMobile ? 3 : 1.2);
  }

  /**
   * Select a node: apply gold ring, center view, open sidebar with detail.
   *
   * @param {typeof nodes[number]} d - The node datum.
   */
  function selectNode(d) {
    // Clear cluster selection if active
    if (selectedClusterName) {
      deselectCluster();
    }

    clearTrace();

    // Remove previous selection ring
    nodesGroup.selectAll('g.node').classed('node-selected', false);

    // Apply gold ring to the clicked node
    nodesGroup
      .selectAll('g.node')
      .filter((nd) => /** @type {any} */ (nd).id === d.id)
      .classed('node-selected', true);

    // Show labels only for this node's edges
    showEdgeLabelsFor(d.id);

    // Center the view on the selected node
    centerOnNode(d);

    currentRuntimeOptions.onSelectDocument?.(d.id);
    renderDetail(
      detailPanel,
      d,
      edges,
      resolveRenderData(),
      repositoryInfo ?? null,
      linkResolver,
      resolveDocumentState(d.id),
      openFileTargetForNode(d),
    );
    selectedNodeId = d.id;
    syncHashToUrl();
  }

  // Close sidebar on close button click, and handle trace button
  detailPanel.addEventListener('click', (event) => {
    const target = /** @type {HTMLElement} */ (event.target);
    if (target.closest('.open-file-btn')) {
      const button = /** @type {HTMLElement} */ (target.closest('.open-file-btn'));
      currentRuntimeOptions.openFile?.({
        path: button.dataset.path || undefined,
        uri: button.dataset.uri || undefined,
        line: button.dataset.line ? Number(button.dataset.line) : undefined,
      });
      return;
    }
    if (target.closest('.detail-panel-close')) {
      clearSelection();
      return;
    }
    if (target.closest('.detail-panel-trace-btn')) {
      const btn = /** @type {HTMLElement} */ (target.closest('.detail-panel-trace-btn'));
      const docId = btn.dataset.docId;
      if (docId) {
        if (activeTraceSet) {
          // Toggle off: clear existing trace
          clearTrace();
          btn.classList.remove('active');
        } else {
          activateTrace(docId);
          btn.classList.add('active');
        }
      }
    }
    // Navigate to a linked document when clicking an edge target
    const edgeTarget = target.closest('.edge-list-target');
    if (edgeTarget) {
      const docId = /** @type {HTMLElement} */ (edgeTarget).dataset.docId;
      if (docId) {
        const simNode = nodes.find((n) => n.id === docId);
        if (simNode) {
          selectNode(simNode);
        }
      }
    }

    // Intercept document links in the spec content (#/doc/...)
    const anchor = target.closest('a[href^="#/doc/"]');
    if (anchor) {
      event.preventDefault();
      const href = /** @type {HTMLAnchorElement} */ (anchor).getAttribute('href');
      if (href) {
        const docId = decodeURIComponent(href.replace('#/doc/', ''));
        const simNode = nodes.find((n) => n.id === docId);
        if (simNode) {
          clearTrace();
          selectNode(simNode);
        }
      }
    }
  });

  // ===== Impact trace state =====

  /** @type {Set<string>|null} Currently traced node IDs, or null if no trace active */
  let activeTraceSet = null;

  /**
   * Activate an impact trace from the given document ID.
   *
   * @param {string} startId
   */
  function activateTrace(startId) {
    activeTraceSet = traceImpact(edges, startId);
    applyTrace();
    syncHashToUrl();
  }

  /**
   * Clear the active impact trace, restoring normal opacity.
   */
  function clearTrace() {
    if (!activeTraceSet) return;
    activeTraceSet = null;

    // Restore all node opacity (re-apply filters will handle filtered state)
    nodesGroup.selectAll('g.node').attr('opacity', 1);

    // Restore default edge opacity and clear inline styles
    edgesGroup
      .selectAll('line.edge')
      .style('opacity', null)
      .style('stroke-width', null)
      .attr('stroke', null);
    edgesGroup.selectAll('text.edge-label').classed('visible', false);

    // Re-apply filters so filtered-out nodes stay dimmed
    applyFilters();

    // If a node is still selected, re-apply its edge highlighting
    if (selectedNodeId) {
      showEdgeLabelsFor(selectedNodeId);
    }

    syncHashToUrl();
  }

  /**
   * Apply trace visual styles: traced subgraph at full opacity with gold edges,
   * everything else dimmed.
   */
  function applyTrace() {
    if (!activeTraceSet) return;
    const traced = activeTraceSet;

    // Respect active filters: filtered-out nodes stay dimmed even if traced
    const visibleSet = filterDocuments(documents, filterState);

    // Nodes: traced + visible at full opacity, filtered-out at 0.08, non-traced at 0.05
    nodesGroup.selectAll('g.node').attr('opacity', (d) => {
      const id = /** @type {any} */ (d).id;
      const parentDocId = /** @type {any} */ (d).parentDocId;
      const checkId = parentDocId ?? id;
      if (!visibleSet.has(checkId)) return 0.08;
      return traced.has(id) ? 1 : 0.05;
    });

    // Edges: traced edges get gold stroke + full opacity, others nearly invisible
    edgesGroup.selectAll('line.edge').each(function (d) {
      const sourceId = edgeEndpointId(d.source);
      const targetId = edgeEndpointId(d.target);
      const isTraced = traced.has(sourceId) && traced.has(targetId);

      d3.select(this)
        .attr('stroke', isTraced ? 'var(--gold)' : null)
        .style('opacity', isTraced ? 0.8 : 0.03);
    });

    // Edge labels: show for traced edges only
    edgesGroup.selectAll('text.edge-label').classed('visible', (d) => {
      const sourceId = edgeEndpointId(d.source);
      const targetId = edgeEndpointId(d.target);
      return traced.has(sourceId) && traced.has(targetId);
    });
  }

  // ===== URL hash synchronisation =====

  /**
   * Build the current view state and push it to the URL hash.
   * Uses replaceState to avoid cluttering browser history.
   * Skipped when we are restoring state from a hash (guard flag).
   */
  function syncHashToUrl() {
    if (isRestoringState) return;

    /** @type {import('./url-router.js').RouterState} */
    const state = {
      doc: selectedNodeId,
      trace: activeTraceSet !== null && selectedNodeId !== null,
      filter: null,
    };

    if (filterState.types.size > 0 || filterState.status !== null) {
      state.filter = {
        types: [...filterState.types],
        status: filterState.status,
      };
    }

    const hash = buildHash(state);
    history.replaceState(null, '', hash || location.pathname + location.search);
  }

  /**
   * Restore view state from a parsed RouterState.
   *
   * @param {import('./url-router.js').RouterState} state
   */
  function restoreStateFromHash(state) {
    isRestoringState = true;
    try {
      // --- Restore filter state ---
      if (state.filter) {
        // Set type filter state
        filterState.types.clear();
        for (const t of state.filter.types) {
          filterState.types.add(t);
        }
        // Update multiselect checkboxes
        const checkboxes = container.querySelectorAll('.filter-multiselect-checkbox');
        for (const cb of checkboxes) {
          /** @type {HTMLInputElement} */ (cb).checked = filterState.types.has(
            /** @type {HTMLInputElement} */ (cb).value,
          );
        }
        updateTypeLabel();
        // Set status dropdown
        filterState.status = state.filter.status || null;
        const statusVal = filterState.status || 'all';
        const toggle = container.querySelector('.filter-select-toggle');
        if (toggle)
          toggle.textContent = `${statusVal === 'all' ? 'All statuses' : statusVal} \u25BE`;
        for (const el of container.querySelectorAll('.filter-select-item')) {
          el.classList.toggle(
            'active',
            /** @type {HTMLElement} */ (el).dataset.value === statusVal,
          );
        }
      } else {
        // Clear filters
        filterState.types.clear();
        filterState.status = null;
        const checkboxes = container.querySelectorAll('.filter-multiselect-checkbox');
        for (const cb of checkboxes) {
          /** @type {HTMLInputElement} */ (cb).checked = false;
        }
        updateTypeLabel();
        const toggle = container.querySelector('.filter-select-toggle');
        if (toggle) toggle.textContent = 'All statuses \u25BE';
        for (const el of container.querySelectorAll('.filter-select-item')) {
          el.classList.toggle('active', /** @type {HTMLElement} */ (el).dataset.value === 'all');
        }
      }
      applyFilters();

      // --- Restore doc selection ---
      if (state.doc) {
        const simNode = nodes.find((n) => n.id === state.doc);
        if (simNode) {
          selectNode(simNode);

          // Zoom to the node
          const svgEl = svg.node();
          const currentWidth = svgEl ? svgEl.clientWidth || width : width;
          const currentHeight = svgEl ? svgEl.clientHeight || height : height;
          const scale = 1.5;
          const tx = currentWidth / 2 - (simNode.x ?? 0) * scale;
          const ty = currentHeight / 2 - (simNode.y ?? 0) * scale;
          svg
            .transition()
            .duration(500)
            .call(
              /** @type {any} */ (zoom).transform,
              d3.zoomIdentity.translate(tx, ty).scale(scale),
            );

          // Activate trace if requested
          if (state.trace) {
            activateTrace(state.doc);
          }
        }
      } else {
        clearTrace();
        clearSelection();
      }
    } finally {
      isRestoringState = false;
    }
  }

  /** @param {KeyboardEvent} event */
  function handleKeydown(event) {
    if (event.key === 'Escape') {
      clearTrace();
      clearSelection();
    }

    if (
      event.key === '/' &&
      !event.ctrlKey &&
      !event.metaKey &&
      !(event.target instanceof HTMLInputElement) &&
      !(event.target instanceof HTMLTextAreaElement) &&
      !(event.target instanceof HTMLSelectElement)
    ) {
      event.preventDefault();
      searchInput.focus();
    }
  }
  document.addEventListener('keydown', handleKeydown);

  // ===== Cluster selection (single-click on cluster rect or label) =====

  /**
   * Select a cluster: highlight its nodes, show cross-cluster edges, open detail.
   *
   * @param {Cluster} cluster - The cluster data.
   */
  function selectCluster(cluster) {
    // Clear node selection if active
    if (selectedNodeId) {
      clearTrace();
      nodesGroup.selectAll('g.node').classed('node-selected', false);
      showEdgeLabelsFor(null);
      clearDetail(detailPanel);
      selectedNodeId = null;
    }

    // Clear previous cluster visual state (but don't clearDetail — we're
    // about to render new content, and clearDetail's async cleanup would
    // wipe it)
    if (selectedClusterName) {
      clusterRects.classed('cluster-selected', false);
      nodesGroup.selectAll('g.node').classed('in-selected-cluster', false);
      edgeLines.each(function () {
        d3.select(this).style('opacity', null).style('stroke-width', null);
      });
      selectedClusterName = null;
    }

    selectedClusterName = cluster.name;

    // Highlight cluster rect
    clusterRects.classed('cluster-selected', (d) => d.name === cluster.name);

    // Highlight nodes in the cluster
    const clusterDocSet = new Set(cluster.docIds);
    nodesGroup.selectAll('g.node').classed('in-selected-cluster', (d) => {
      const id = /** @type {any} */ (d).id;
      return clusterDocSet.has(id);
    });

    // Highlight cross-cluster edges (one endpoint inside, other outside)
    edgeLines.each(function (/** @type {any} */ d) {
      const srcId = edgeEndpointId(d.source);
      const tgtId = edgeEndpointId(d.target);
      const srcInside = clusterDocSet.has(srcId);
      const tgtInside = clusterDocSet.has(tgtId);
      const isCrossCluster = srcInside !== tgtInside;
      const el = d3.select(this);
      el.style('opacity', isCrossCluster ? 0.9 : 0.06);
      el.style('stroke-width', isCrossCluster ? '2.5px' : null);
    });

    // Center view on cluster centroid
    const clusterNodes = nodes.filter((n) => clusterDocSet.has(n.id));
    if (clusterNodes.length > 0) {
      const cx = clusterNodes.reduce((s, n) => s + (n.x ?? 0), 0) / clusterNodes.length;
      const cy = clusterNodes.reduce((s, n) => s + (n.y ?? 0), 0) / clusterNodes.length;
      const isMobile = screenWidth < 600;
      panToPoint(cx, cy, isMobile ? 2.5 : 1.0);
    }

    // Render cluster detail in sidebar
    const clusterDocs = documents.filter((doc) => clusterDocSet.has(doc.id));
    renderClusterDetail(detailPanel, cluster.name, clusterDocs, edges);
  }

  // Cluster click handler via event delegation on clustersGroup
  d3.select(clustersGroup.node()).on('click', (event) => {
    const target = /** @type {Element} */ (event.target);
    // Match clicks on cluster rects or labels
    const clusterRect = target.closest('rect.cluster');
    const clusterLabel = target.closest('text.cluster-label');
    const el = clusterRect || clusterLabel;
    if (!el) return;

    event.stopPropagation();

    const datum = /** @type {Cluster} */ (d3.select(el).datum());
    if (!datum) return;

    // Toggle: clicking the same cluster again deselects
    if (selectedClusterName === datum.name) {
      deselectCluster();
      syncHashToUrl();
    } else {
      selectCluster(datum);
      syncHashToUrl();
    }
  });

  // Clear trace on SVG background click (not on a node or cluster)
  svg.on('click', (event) => {
    const target = /** @type {Element} */ (event.target);
    // Only clear on background click — ignore clicks on nodes, edges, or clusters
    if (
      target.closest('g.node') ||
      target.closest('line.edge') ||
      target.closest('rect.cluster') ||
      target.closest('text.cluster-label')
    )
      return;
    clearTrace();
    clearSelection();
  });

  // Use SVG-level event delegation so click handling survives node rebinds
  d3.select(nodesGroup.node()).on('click', (event) => {
    const target = /** @type {Element} */ (event.target);
    const nodeEl = target.closest('g.node');
    if (!nodeEl) return;

    const datum = /** @type {typeof nodes[number]} */ (d3.select(nodeEl).datum());
    if (!datum || /** @type {any} */ (datum).componentId) return;

    event.stopPropagation();

    // Clear cluster selection if active
    if (selectedClusterName) {
      deselectCluster();
    }

    if (selectedNodeId === datum.id) {
      clearSelection();
    } else {
      selectNode(datum);
    }
  });

  // ===== Restore state from URL hash on initial load =====
  const initialState = parseHash(location.hash);
  if (initialState.doc || initialState.filter) {
    // Apply filter and selection immediately (these don't need stable positions).
    // But skip the zoom — positions are still being computed.
    restoreStateFromHash(initialState);

    // Defer the zoom to the selected node until positions are meaningful.
    if (initialState.doc) {
      const focusNode = nodes.find((n) => n.id === initialState.doc);
      if (focusNode) {
        let zoomApplied = false;
        const zoomToFocus = () => {
          if (zoomApplied) return;
          if (focusNode.x == null || focusNode.y == null) return;
          zoomApplied = true;
          simulation.on('tick.initialZoom', null);
          simulation.on('end.initialZoom', null);
          centerOnNode(focusNode);
        };
        simulation.on('tick.initialZoom', () => {
          if (simulation.alpha() < 0.3) zoomToFocus();
        });
        simulation.on('end.initialZoom', zoomToFocus);
      }
    }
  }

  // ===== Listen for hashchange (back/forward, manual URL edits) =====
  const unsubscribeHashChange = onHashChange((state) => {
    restoreStateFromHash(state);
  });

  // ===== Unmount =====
  let unmounted = false;

  function unmount() {
    if (unmounted) return;
    unmounted = true;

    simulation.stop();

    document.removeEventListener('click', handleTypeDropdownClose);
    if (handleStatusDropdownClose) {
      document.removeEventListener('click', handleStatusDropdownClose);
    }
    document.removeEventListener('mousemove', handleDividerMousemove);
    document.removeEventListener('mouseup', handleDividerMouseup);
    document.removeEventListener('keydown', handleKeydown);

    unsubscribeHashChange();
    container.replaceChildren();
  }

  function refreshDetail() {
    if (selectedClusterName) {
      const selectedCluster = clusters.find((cluster) => cluster.name === selectedClusterName);
      if (selectedCluster) {
        const selectedIds = new Set(selectedCluster.docIds);
        const clusterDocs = documents.filter((doc) => selectedIds.has(doc.id));
        renderClusterDetail(detailPanel, selectedCluster.name, clusterDocs, edges);
      }
      return;
    }

    if (selectedNodeId) {
      const selectedNode = nodes.find((node) => node.id === selectedNodeId);
      if (selectedNode) {
        renderDetail(
          detailPanel,
          selectedNode,
          edges,
          resolveRenderData(),
          repositoryInfo ?? null,
          linkResolver,
          resolveDocumentState(selectedNode.id),
          openFileTargetForNode(selectedNode),
        );
        return;
      }
    }

    renderEmpty(detailPanel, data, resolveRenderData());
  }

  function updateRuntimeOptions(nextRuntimeOptions) {
    currentRuntimeOptions = {
      ...currentRuntimeOptions,
      ...nextRuntimeOptions,
    };
    syncRootSelector();
    if (selectedNodeId || selectedClusterName) {
      refreshDetail();
    }
  }

  return { unmount, refreshDetail, updateRuntimeOptions };
}
