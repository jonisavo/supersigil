"use strict";
var SupersigilExplorer = (() => {
  var __create = Object.create;
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getProtoOf = Object.getPrototypeOf;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __commonJS = (cb, mod) => function __require() {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  };
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
    // If the importer is in node compatibility mode or this is not an ESM
    // file that has been converted to a CommonJS file using a Babel-
    // compatible transform (i.e. "__esModule" has not been set), then set
    // "default" to the CommonJS "module.exports" for node compatibility.
    isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
    mod
  ));
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // src/components/explore/d3-global.cjs
  var require_d3_global = __commonJS({
    "src/components/explore/d3-global.cjs"(exports, module) {
      "use strict";
      module.exports = globalThis.d3;
    }
  });

  // src/components/explore/graph-explorer.js
  var graph_explorer_exports = {};
  __export(graph_explorer_exports, {
    buildComponentLinks: () => buildComponentLinks,
    buildComponentNodes: () => buildComponentNodes,
    componentStrokeColor: () => componentStrokeColor,
    computeClusterBounds: () => computeClusterBounds,
    computeClusters: () => computeClusters,
    mount: () => mount,
    nodeRadius: () => nodeRadius,
    nodeStrokeColor: () => nodeStrokeColor
  });
  var d32 = __toESM(require_d3_global(), 1);

  // node_modules/.pnpm/force-in-a-box@1.0.2/node_modules/force-in-a-box/dist/forceInABox.esm.js
  var d3 = __toESM(require_d3_global(), 1);
  function forceInABox() {
    function constant(_) {
      return () => _;
    }
    function index(d) {
      return d.index;
    }
    let id = index, nodes = [], links = [], tree, size = [100, 100], forceNodeSize = constant(1), forceCharge = constant(-1), forceLinkDistance = constant(100), forceLinkStrength = constant(0.1), foci = {}, linkStrengthIntraCluster = 0.1, linkStrengthInterCluster = 1e-3, templateNodes = [], offset = [0, 0], templateForce, groupBy = function(d) {
      return d.cluster;
    }, template = "treemap", enableGrouping = true, strength = 0.1;
    function force(alpha) {
      if (!enableGrouping) {
        return force;
      }
      if (template === "force") {
        templateForce.tick();
        getFocisFromTemplate();
      }
      for (let i = 0, n = nodes.length, node, k = alpha * strength; i < n; ++i) {
        node = nodes[i];
        node.vx += (foci[groupBy(node)].x - node.x) * k;
        node.vy += (foci[groupBy(node)].y - node.y) * k;
      }
    }
    function initialize() {
      if (!nodes) return;
      if (template === "treemap") {
        initializeWithTreemap();
      } else {
        initializeWithForce();
      }
    }
    force.initialize = function(_) {
      nodes = _;
      initialize();
    };
    function getLinkKey(l) {
      let sourceID = groupBy(l.source), targetID = groupBy(l.target);
      return sourceID <= targetID ? sourceID + "~" + targetID : targetID + "~" + sourceID;
    }
    function computeClustersNodeCounts(nodes2) {
      let clustersCounts = /* @__PURE__ */ new Map(), tmpCount = {};
      nodes2.forEach(function(d) {
        if (!clustersCounts.has(groupBy(d))) {
          clustersCounts.set(groupBy(d), { count: 0, sumforceNodeSize: 0 });
        }
      });
      nodes2.forEach(function(d) {
        tmpCount = clustersCounts.get(groupBy(d));
        tmpCount.count = tmpCount.count + 1;
        tmpCount.sumforceNodeSize = tmpCount.sumforceNodeSize + Math.PI * (forceNodeSize(d) * forceNodeSize(d)) * 1.3;
        clustersCounts.set(groupBy(d), tmpCount);
      });
      return clustersCounts;
    }
    function computeClustersLinkCounts(links2) {
      let dClusterLinks = /* @__PURE__ */ new Map(), clusterLinks = [];
      links2.forEach(function(l) {
        let key = getLinkKey(l), count;
        if (dClusterLinks.has(key)) {
          count = dClusterLinks.get(key);
        } else {
          count = 0;
        }
        count += 1;
        dClusterLinks.set(key, count);
      });
      dClusterLinks.forEach(function(value, key) {
        let source, target;
        source = key.split("~")[0];
        target = key.split("~")[1];
        if (source !== void 0 && target !== void 0) {
          clusterLinks.push({
            source,
            target,
            count: value
          });
        }
      });
      return clusterLinks;
    }
    function getGroupsGraph() {
      let gnodes = [], glinks = [], dNodes = /* @__PURE__ */ new Map(), c, i, cc, clustersCounts, clustersLinks;
      clustersCounts = computeClustersNodeCounts(nodes);
      clustersLinks = computeClustersLinkCounts(links);
      for (c of clustersCounts.keys()) {
        cc = clustersCounts.get(c);
        gnodes.push({
          id: c,
          size: cc.count,
          r: Math.sqrt(cc.sumforceNodeSize / Math.PI)
        });
        dNodes.set(c, i);
      }
      clustersLinks.forEach(function(l) {
        let source = dNodes.get(l.source), target = dNodes.get(l.target);
        if (source !== void 0 && target !== void 0) {
          glinks.push({
            source,
            target,
            count: l.count
          });
        }
      });
      return { nodes: gnodes, links: glinks };
    }
    function getGroupsTree() {
      let children = [], c, cc, clustersCounts;
      clustersCounts = computeClustersNodeCounts(force.nodes());
      for (c of clustersCounts.keys()) {
        cc = clustersCounts.get(c);
        children.push({ id: c, size: cc.count });
      }
      return { id: "clustersTree", children };
    }
    function getFocisFromTemplate() {
      foci.none = { x: 0, y: 0 };
      templateNodes.forEach(function(d) {
        if (template === "treemap") {
          foci[d.data.id] = {
            x: d.x0 + (d.x1 - d.x0) / 2 - offset[0],
            y: d.y0 + (d.y1 - d.y0) / 2 - offset[1]
          };
        } else {
          foci[d.id] = {
            x: d.x - offset[0],
            y: d.y - offset[1]
          };
        }
      });
      return foci;
    }
    function initializeWithTreemap() {
      let treemap2 = d3.treemap().size(force.size());
      tree = d3.hierarchy(getGroupsTree()).sum(function(d) {
        return d.size;
      }).sort(function(a, b) {
        return b.height - a.height || b.value - a.value;
      });
      templateNodes = treemap2(tree).leaves();
      getFocisFromTemplate();
    }
    function checkLinksAsObjects() {
      let linkCount = 0;
      if (nodes.length === 0) return;
      links.forEach(function(link) {
        let source, target;
        if (!nodes) return;
        source = link.source;
        target = link.target;
        if (typeof link.source !== "object") source = nodes[link.source];
        if (typeof link.target !== "object") target = nodes[link.target];
        if (source === void 0 || target === void 0) {
          throw Error(
            "Error setting links, couldnt find nodes for a link (see it on the console)"
          );
        }
        link.source = source;
        link.target = target;
        link.index = linkCount++;
      });
    }
    function initializeWithForce() {
      let net;
      if (!nodes || !nodes.length) {
        return;
      }
      if (nodes && nodes.length > 0) {
        if (groupBy(nodes[0]) === void 0) {
          throw Error(
            "Couldnt find the grouping attribute for the nodes. Make sure to set it up with forceInABox.groupBy('clusterAttr') before calling .links()"
          );
        }
      }
      checkLinksAsObjects();
      net = getGroupsGraph();
      templateForce = d3.forceSimulation(net.nodes).force("x", d3.forceX(size[0] / 2).strength(0.1)).force("y", d3.forceY(size[1] / 2).strength(0.1)).force(
        "collide",
        d3.forceCollide(function(d) {
          return d.r;
        }).iterations(4)
      ).force("charge", d3.forceManyBody().strength(forceCharge)).force(
        "links",
        d3.forceLink(net.nodes.length ? net.links : []).distance(forceLinkDistance).strength(forceLinkStrength)
      );
      templateNodes = templateForce.nodes();
      getFocisFromTemplate();
    }
    function drawTreemap(container) {
      container.selectAll("circle.cell").remove();
      container.selectAll("line.cell").remove();
      container.selectAll("rect.cell").data(templateNodes).enter().append("svg:rect").attr("class", "cell").attr("x", function(d) {
        return d.x0;
      }).attr("y", function(d) {
        return d.y0;
      }).attr("width", function(d) {
        return d.x1 - d.x0;
      }).attr("height", function(d) {
        return d.y1 - d.y0;
      });
    }
    function drawGraph(container) {
      container.selectAll("rect.cell").remove();
      let templateLinksSel = container.selectAll("line.cell").data(templateForce.force("links").links());
      templateLinksSel.enter().append("line").attr("class", "cell").merge(templateLinksSel).attr("x2", function(d) {
        return d.source.x;
      }).attr("y2", function(d) {
        return d.source.y;
      }).attr("x1", function(d) {
        return d.target.x;
      }).attr("y1", function(d) {
        return d.target.y;
      }).style("stroke-width", "1px").style("stroke-opacity", "0.5");
      let templateNodesSel = container.selectAll("circle.cell").data(templateForce.nodes());
      templateNodesSel.enter().append("svg:circle").attr("class", "cell").merge(templateNodesSel).attr("cx", function(d) {
        return d.x;
      }).attr("cy", function(d) {
        return d.y;
      }).attr("r", function(d) {
        return d.r;
      });
      templateForce.on("tick", () => {
        drawGraph(container);
      }).restart();
      templateNodesSel.exit().remove();
      templateLinksSel.exit().remove();
    }
    force.drawTemplate = function(container) {
      if (template === "treemap") {
        drawTreemap(container);
      } else {
        drawGraph(container);
      }
      return force;
    };
    force.drawTreemap = force.drawTemplate;
    force.deleteTemplate = function(container) {
      container.selectAll(".cell").remove();
      if (templateForce) {
        templateForce.on("tick", null).restart();
      }
      return force;
    };
    force.template = function(x) {
      if (!arguments.length) return template;
      template = x;
      initialize();
      return force;
    };
    force.groupBy = function(x) {
      if (!arguments.length) return groupBy;
      if (typeof x === "string") {
        groupBy = function(d) {
          return d[x];
        };
        return force;
      }
      groupBy = x;
      return force;
    };
    force.enableGrouping = function(x) {
      if (!arguments.length) return enableGrouping;
      enableGrouping = x;
      return force;
    };
    force.strength = function(x) {
      if (!arguments.length) return strength;
      strength = x;
      return force;
    };
    force.getLinkStrength = function(e) {
      if (enableGrouping) {
        if (groupBy(e.source) === groupBy(e.target)) {
          if (typeof linkStrengthIntraCluster === "function") {
            return linkStrengthIntraCluster(e);
          } else {
            return linkStrengthIntraCluster;
          }
        } else {
          if (typeof linkStrengthInterCluster === "function") {
            return linkStrengthInterCluster(e);
          } else {
            return linkStrengthInterCluster;
          }
        }
      } else {
        if (typeof linkStrengthIntraCluster === "function") {
          return linkStrengthIntraCluster(e);
        } else {
          return linkStrengthIntraCluster;
        }
      }
    };
    force.id = function(_) {
      return arguments.length ? (id = _, force) : id;
    };
    force.size = function(_) {
      return arguments.length ? (size = _, force) : size;
    };
    force.linkStrengthInterCluster = function(_) {
      return arguments.length ? (linkStrengthInterCluster = _, force) : linkStrengthInterCluster;
    };
    force.linkStrengthIntraCluster = function(_) {
      return arguments.length ? (linkStrengthIntraCluster = _, force) : linkStrengthIntraCluster;
    };
    force.nodes = function(_) {
      return arguments.length ? (nodes = _, force) : nodes;
    };
    force.links = function(_) {
      if (!arguments.length) return links;
      if (_ === null) links = [];
      else links = _;
      initialize();
      return force;
    };
    force.forceNodeSize = function(_) {
      return arguments.length ? (forceNodeSize = typeof _ === "function" ? _ : constant(+_), initialize(), force) : forceNodeSize;
    };
    force.nodeSize = force.forceNodeSize;
    force.forceCharge = function(_) {
      return arguments.length ? (forceCharge = typeof _ === "function" ? _ : constant(+_), initialize(), force) : forceCharge;
    };
    force.forceLinkDistance = function(_) {
      return arguments.length ? (forceLinkDistance = typeof _ === "function" ? _ : constant(+_), initialize(), force) : forceLinkDistance;
    };
    force.forceLinkStrength = function(_) {
      return arguments.length ? (forceLinkStrength = typeof _ === "function" ? _ : constant(+_), initialize(), force) : forceLinkStrength;
    };
    force.offset = function(_) {
      return arguments.length ? (offset = typeof _ === "function" ? _ : constant(+_), force) : offset;
    };
    force.getFocis = getFocisFromTemplate;
    return force;
  }

  // src/components/explore/graph-data.js
  function extractFilterOptions(documents) {
    const typeSet = /* @__PURE__ */ new Set();
    const statusSet = /* @__PURE__ */ new Set();
    for (const doc of documents) {
      if (doc.doc_type != null) typeSet.add(doc.doc_type);
      if (doc.status != null) statusSet.add(doc.status);
    }
    return {
      types: [...typeSet].sort(),
      statuses: [...statusSet].sort()
    };
  }
  function filterDocuments(documents, filters) {
    const { types, status } = filters;
    const hasTypeFilter = types && types.size > 0;
    const hasStatusFilter = status != null && status !== "all";
    const visible = /* @__PURE__ */ new Set();
    for (const doc of documents) {
      if (hasTypeFilter && (doc.doc_type == null || !types.has(doc.doc_type))) continue;
      if (hasStatusFilter && doc.status !== status) continue;
      visible.add(doc.id);
    }
    return visible;
  }
  function componentColor(kind) {
    switch (kind) {
      case "Criterion":
        return "var(--teal)";
      case "Task":
        return "var(--green)";
      case "Decision":
        return "var(--gold)";
      case "Rationale":
      case "Alternative":
        return "var(--text-muted)";
      default:
        return "var(--text-dim)";
    }
  }
  function searchDocuments(documents, query) {
    const q = query.trim().toLowerCase();
    if (q === "") return [];
    const scored = [];
    for (const doc of documents) {
      const idLower = doc.id.toLowerCase();
      const titleLower = (doc.title ?? "").toLowerCase();
      const idMatch = idLower.includes(q);
      const titleMatch = titleLower.includes(q);
      if (!idMatch && !titleMatch) continue;
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

  // src/components/explore/detail-panel.js
  function escHtml(str) {
    if (!str) return "";
    return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  function edgeListItemHtml(arrow, peerId, kind) {
    return `<li class="edge-list-item"><span class="edge-list-kind">${arrow}</span><span class="edge-list-target" data-doc-id="${escHtml(peerId)}">${escHtml(peerId)}</span><span class="edge-list-kind">${escHtml(kind)}</span></li>`;
  }
  function buildEdgeGroups(edges, docId) {
    const incoming = [];
    const outgoing = [];
    for (const edge of edges) {
      if (edge.to === docId) {
        incoming.push(edge);
      }
      if (edge.from === docId) {
        outgoing.push(edge);
      }
    }
    return { incoming, outgoing };
  }
  function buildBadgeClass(category, value) {
    if (category === "type") {
      const typeValue = value ?? "unknown";
      return `badge badge-type-${typeValue}`;
    }
    const statusValue = value ?? "draft";
    return `badge badge-status-${statusValue}`;
  }
  function countCriteria(fences) {
    let total = 0;
    let verified = 0;
    function visit(components) {
      for (const comp of components) {
        if (comp.kind === "Criterion") {
          total++;
          const state = comp.verification?.state ?? "unverified";
          if (state === "verified") verified++;
        }
        if (comp.children) visit(comp.children);
      }
    }
    for (const fence of fences) {
      if (fence.components) visit(fence.components);
    }
    return { total, verified };
  }
  function createExplorerLinkResolver(repositoryUrl) {
    return {
      evidenceLink: (file, line) => `${repositoryUrl}/blob/main/${file}#L${line}`,
      documentLink: (docId) => `#/doc/${encodeURIComponent(docId)}`,
      criterionLink: (docId, _criterionId) => `#/doc/${encodeURIComponent(docId)}`
    };
  }
  function renderDetail(container, node, edges, renderData, repositoryUrl) {
    cancelPendingClear(container);
    const { incoming, outgoing } = buildEdgeGroups(edges, node.id);
    let edgesHtml = "";
    if (incoming.length > 0 || outgoing.length > 0) {
      const edgeItems = [];
      for (const edge of incoming) {
        edgeItems.push(edgeListItemHtml("\u2190", edge.from, edge.kind));
      }
      for (const edge of outgoing) {
        edgeItems.push(edgeListItemHtml("\u2192", edge.to, edge.kind));
      }
      edgesHtml = `<div class="detail-section"><div class="detail-section-label">Edges</div><ul class="edge-list">${edgeItems.join("")}</ul></div>`;
    }
    const typeClass = buildBadgeClass("type", node.doc_type);
    const statusClass = buildBadgeClass("status", node.status);
    const typeLabel = node.doc_type ?? "unknown";
    const statusLabel = node.status ?? "draft";
    const renderDoc = renderData?.find((d) => d.document_id === node.id);
    let coverageHtml = "";
    if (renderDoc) {
      const { total, verified } = countCriteria(renderDoc.fences);
      if (total > 0) {
        const pct = Math.round(verified / total * 100);
        coverageHtml = `<div class="detail-section"><div class="detail-coverage"><span class="detail-coverage-bar"><span class="detail-coverage-bar-fill" style="width: ${pct}%"></span></span><span class="detail-coverage-text">${verified}/${total} criteria verified (${pct}%)</span></div></div>`;
      }
    }
    const traceBtn = `<div class="detail-section"><button class="detail-panel-trace-btn" data-doc-id="${escHtml(node.id)}">Trace impact</button></div>`;
    let specContentHtml = "";
    if (renderDoc && typeof window !== "undefined" && window.__supersigilRender) {
      const linkResolver = createExplorerLinkResolver(repositoryUrl);
      try {
        specContentHtml = window.__supersigilRender.renderComponentTree(
          renderDoc.fences,
          renderDoc.edges,
          linkResolver
        );
      } catch (err) {
        console.error("Failed to render spec content:", err);
        specContentHtml = '<p class="detail-spec-empty">Failed to render spec content.</p>';
      }
    }
    const specSection = specContentHtml ? `<div class="detail-spec-content">${specContentHtml}</div>` : "";
    container.innerHTML = `<div class="detail-panel-header"><div class="detail-panel-title">${escHtml(node.id)}</div><button class="detail-panel-close" aria-label="Close">\u2715</button></div><div class="detail-panel-body"><div class="detail-section"><span class="${typeClass}">${escHtml(typeLabel)}</span> <span class="${statusClass}">${escHtml(statusLabel)}</span></div>${coverageHtml}${traceBtn}${edgesHtml}${specSection}</div>`;
    container.classList.add("open");
  }
  function buildClusterEdgeGroups(edges, clusterDocIds) {
    const incoming = [];
    const outgoing = [];
    for (const edge of edges) {
      const fromInside = clusterDocIds.has(edge.from);
      const toInside = clusterDocIds.has(edge.to);
      if (!fromInside && toInside) {
        incoming.push(edge);
      } else if (fromInside && !toInside) {
        outgoing.push(edge);
      }
    }
    return { incoming, outgoing };
  }
  function renderClusterDetail(container, clusterName, documents, edges) {
    cancelPendingClear(container);
    const docIdSet = new Set(documents.map((d) => d.id));
    const docCount = documents.length;
    const typeCounts = /* @__PURE__ */ new Map();
    for (const doc of documents) {
      const t = doc.doc_type ?? "unknown";
      typeCounts.set(t, (typeCounts.get(t) ?? 0) + 1);
    }
    const typeBadges = [...typeCounts.entries()].sort((a, b) => b[1] - a[1]).map(
      ([t, count]) => `<span class="${buildBadgeClass("type", t)}">${count} ${escHtml(t)}</span>`
    ).join(" ");
    const statusCounts = /* @__PURE__ */ new Map();
    for (const doc of documents) {
      const s = doc.status ?? "draft";
      statusCounts.set(s, (statusCounts.get(s) ?? 0) + 1);
    }
    const statusBadges = [...statusCounts.entries()].sort((a, b) => b[1] - a[1]).map(
      ([s, count]) => `<span class="${buildBadgeClass("status", s)}">${count} ${escHtml(s)}</span>`
    ).join(" ");
    const compKindCounts = /* @__PURE__ */ new Map();
    for (const doc of documents) {
      for (const comp of doc.components) {
        if (comp.id) {
          compKindCounts.set(comp.kind, (compKindCounts.get(comp.kind) ?? 0) + 1);
        }
      }
    }
    let componentsHtml = "";
    if (compKindCounts.size > 0) {
      const compItems = [...compKindCounts.entries()].sort((a, b) => b[1] - a[1]).map(
        ([kind, count]) => `<li class="component-item"><span class="component-item-kind" style="border-color: ${componentColor(kind)}; color: ${componentColor(kind)}">${escHtml(kind)}</span><span class="component-item-id">${count}</span></li>`
      ).join("");
      componentsHtml = `<div class="detail-section"><div class="detail-section-label">Components</div><ul class="component-list">${compItems}</ul></div>`;
    }
    const { incoming, outgoing } = buildClusterEdgeGroups(edges, docIdSet);
    let edgesHtml = "";
    if (incoming.length > 0 || outgoing.length > 0) {
      const edgeItems = [];
      if (incoming.length > 0) {
        edgeItems.push(
          `<li class="edge-list-item" style="margin-bottom: 0.25rem"><strong style="font-size: 0.75rem; color: var(--text-muted)">Incoming (${incoming.length})</strong></li>`
        );
        for (const edge of incoming) {
          edgeItems.push(edgeListItemHtml("\u2190", edge.from, edge.kind));
        }
      }
      if (outgoing.length > 0) {
        edgeItems.push(
          `<li class="edge-list-item" style="margin-top: 0.5rem; margin-bottom: 0.25rem"><strong style="font-size: 0.75rem; color: var(--text-muted)">Outgoing (${outgoing.length})</strong></li>`
        );
        for (const edge of outgoing) {
          edgeItems.push(edgeListItemHtml("\u2192", edge.to, edge.kind));
        }
      }
      edgesHtml = `<div class="detail-section"><div class="detail-section-label">Cross-cluster Edges</div><ul class="edge-list">${edgeItems.join("")}</ul></div>`;
    }
    container.innerHTML = `<div class="detail-panel-header"><div class="detail-panel-title">${escHtml(clusterName)}</div><button class="detail-panel-close" aria-label="Close">\u2715</button></div><div class="detail-panel-body"><div class="detail-section"><div class="detail-section-label">Summary</div><div class="detail-section-content">${docCount} documents</div></div><div class="detail-section"><div class="detail-section-label">Types</div><div class="detail-section-content">${typeBadges}</div></div><div class="detail-section"><div class="detail-section-label">Status</div><div class="detail-section-content">${statusBadges}</div></div>${componentsHtml}${edgesHtml}</div>`;
    container.classList.add("open");
  }
  function cancelPendingClear(container) {
    const timer = (
      /** @type {any} */
      container._clearTimer
    );
    if (timer) {
      clearTimeout(timer);
      container._clearTimer = null;
    }
    const handler = (
      /** @type {any} */
      container._clearHandler
    );
    if (handler) {
      container.removeEventListener("transitionend", handler);
      container._clearHandler = null;
    }
  }
  function clearDetail(container) {
    cancelPendingClear(container);
    container.classList.remove("open");
    container.innerHTML = "";
  }
  function renderEmpty(container, graphData, renderData) {
    if (!graphData || !graphData.documents || graphData.documents.length === 0) {
      container.innerHTML = `<div class="detail-spec-empty"><div>Select a document in the graph<br/>to view its specification</div><div class="detail-spec-empty-hint">Click a node or use / to search</div></div>`;
      return;
    }
    const coverageMap = /* @__PURE__ */ new Map();
    if (renderData) {
      for (const doc of renderData) {
        const cov = countCriteria(doc.fences || []);
        if (cov.total > 0) {
          coverageMap.set(doc.document_id, cov);
        }
      }
    }
    const isMultiProject = graphData.documents.some((d) => d.project);
    const typeOrder = { requirements: 0, design: 1, tasks: 2, adr: 3, documentation: 4 };
    function docPrefix(id) {
      const slashIdx = id.indexOf("/");
      return slashIdx > 0 ? id.substring(0, slashIdx) : id;
    }
    function docSuffix(id) {
      const slashIdx = id.indexOf("/");
      return slashIdx > 0 ? id.substring(slashIdx + 1) : id;
    }
    const tree = /* @__PURE__ */ new Map();
    for (const doc of graphData.documents) {
      const project = isMultiProject ? doc.project || "(ungrouped)" : "(all)";
      const prefix = docPrefix(doc.id);
      if (!tree.has(project)) tree.set(project, /* @__PURE__ */ new Map());
      const prefixMap = tree.get(project);
      if (!prefixMap.has(prefix)) prefixMap.set(prefix, []);
      prefixMap.get(prefix).push(doc);
    }
    const sortedProjects = [...tree.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    let globalTotal = 0;
    let globalVerified = 0;
    for (const { total, verified } of coverageMap.values()) {
      globalTotal += total;
      globalVerified += verified;
    }
    const globalPct = globalTotal > 0 ? Math.round(globalVerified / globalTotal * 100) : 0;
    let html = `<div class="detail-panel-header"><div class="detail-panel-title">Spec Index</div></div>`;
    html += `<div class="detail-panel-body">`;
    if (globalTotal > 0) {
      html += `<div class="detail-index-coverage">${globalVerified}/${globalTotal} criteria verified (${globalPct}%)<div class="detail-coverage-bar"><div class="detail-coverage-fill" style="width:${globalPct}%"></div></div></div>`;
    }
    html += `<div class="detail-index-hint">Click a document to view its specification</div>`;
    function renderDocList(docs) {
      let out = "";
      docs.sort((a, b) => {
        const ta = typeOrder[a.doc_type] ?? 5;
        const tb = typeOrder[b.doc_type] ?? 5;
        return ta !== tb ? ta - tb : a.id.localeCompare(b.id);
      });
      for (const doc of docs) {
        const cov = coverageMap.get(doc.id);
        const covLabel = cov ? ` ${cov.verified}/${cov.total}` : "";
        const typeLabel = doc.doc_type || "";
        const statusLabel = doc.status || "";
        out += `<a href="#/doc/${encodeURIComponent(doc.id)}" class="detail-index-doc">`;
        out += `<span class="detail-index-doc-id">${escHtml(docSuffix(doc.id))}</span>`;
        out += `<span class="detail-index-doc-meta">`;
        if (typeLabel) out += `<span class="detail-badge detail-badge-type-${escHtml(typeLabel)}">${escHtml(typeLabel)}</span>`;
        if (statusLabel) out += `<span class="detail-badge detail-badge-status">${escHtml(statusLabel)}</span>`;
        if (covLabel) out += `<span class="detail-index-doc-cov">${covLabel}</span>`;
        out += `</span>`;
        out += `</a>`;
      }
      return out;
    }
    for (const [project, prefixMap] of sortedProjects) {
      const sortedPrefixes = [...prefixMap.entries()].sort((a, b) => a[0].localeCompare(b[0]));
      const totalDocs = [...prefixMap.values()].reduce((n, d) => n + d.length, 0);
      if (isMultiProject) {
        html += `<details class="detail-index-project" open>`;
        html += `<summary class="detail-index-project-title">${escHtml(project)} <span class="detail-index-group-count">${totalDocs}</span></summary>`;
      }
      for (const [prefix, docs] of sortedPrefixes) {
        html += `<details class="detail-index-group" open>`;
        html += `<summary class="detail-index-group-title">${escHtml(prefix)} <span class="detail-index-group-count">${docs.length}</span></summary>`;
        html += renderDocList(docs);
        html += `</details>`;
      }
      if (isMultiProject) {
        html += `</details>`;
      }
    }
    html += `</div>`;
    container.innerHTML = html;
  }

  // src/components/explore/impact-trace.js
  function traceImpact(edges, startId) {
    const downstream = /* @__PURE__ */ new Map();
    for (const edge of edges) {
      const existing = downstream.get(edge.to);
      if (existing) {
        existing.push(edge.from);
      } else {
        downstream.set(edge.to, [edge.from]);
      }
    }
    const visited = /* @__PURE__ */ new Set([startId]);
    const queue = [startId];
    while (queue.length > 0) {
      const current = (
        /** @type {string} */
        queue.shift()
      );
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

  // src/components/explore/url-router.js
  function defaultState() {
    return { doc: null, trace: false, filter: null };
  }
  function parseFilterSegment(filterStr) {
    if (!filterStr) return null;
    let types = [];
    let status = null;
    const pairs = filterStr.split(";");
    for (const pair of pairs) {
      const colonIdx = pair.indexOf(":");
      if (colonIdx === -1) continue;
      const key = pair.slice(0, colonIdx);
      const value = pair.slice(colonIdx + 1);
      if (key === "type") {
        types = value.split(",").filter((v) => v.length > 0);
      } else if (key === "status") {
        status = value || null;
      }
    }
    if (types.length === 0 && status === null) return null;
    return { types, status };
  }
  function buildFilterSegment(filter) {
    const parts = [];
    if (filter.types.length > 0) {
      parts.push(`type:${filter.types.join(",")}`);
    }
    if (filter.status) {
      parts.push(`status:${filter.status}`);
    }
    return parts.join(";");
  }
  function parseHash(hash) {
    const state = defaultState();
    let path = hash.startsWith("#") ? hash.slice(1) : hash;
    if (path.startsWith("/")) path = path.slice(1);
    if (!path) return state;
    if (path.startsWith("doc/")) {
      const rest = path.slice(4);
      const traceIdx = rest.indexOf("/trace");
      const filterIdx = rest.indexOf("/filter/");
      let docEnd = rest.length;
      if (traceIdx !== -1 && filterIdx !== -1) {
        docEnd = Math.min(traceIdx, filterIdx);
      } else if (traceIdx !== -1) {
        docEnd = traceIdx;
      } else if (filterIdx !== -1) {
        docEnd = filterIdx;
      }
      let actualTraceIdx = -1;
      let searchFrom = 0;
      while (searchFrom < rest.length) {
        const idx = rest.indexOf("/trace", searchFrom);
        if (idx === -1) break;
        const afterTrace = idx + 6;
        if (afterTrace === rest.length || rest.slice(afterTrace).startsWith("/filter/")) {
          actualTraceIdx = idx;
          break;
        }
        searchFrom = idx + 1;
      }
      let actualFilterIdx = -1;
      if (actualTraceIdx !== -1) {
        const afterTrace = rest.slice(actualTraceIdx + 6);
        if (afterTrace.startsWith("/filter/")) {
          actualFilterIdx = actualTraceIdx + 6 + 8;
        }
        docEnd = actualTraceIdx;
      } else {
        searchFrom = 0;
        while (searchFrom < rest.length) {
          const idx = rest.indexOf("/filter/", searchFrom);
          if (idx === -1) break;
          actualFilterIdx = idx + 8;
          docEnd = idx;
          break;
        }
      }
      state.doc = rest.slice(0, docEnd) || null;
      state.trace = actualTraceIdx !== -1;
      if (actualFilterIdx !== -1) {
        state.filter = parseFilterSegment(rest.slice(actualFilterIdx));
      }
    } else if (path.startsWith("filter/")) {
      const filterStr = path.slice(7);
      state.filter = parseFilterSegment(filterStr);
    }
    return state;
  }
  function buildHash(state) {
    let hash = "";
    if (state.doc) {
      hash += `/doc/${state.doc}`;
      if (state.trace) {
        hash += "/trace";
      }
    }
    if (state.filter) {
      const filterStr = buildFilterSegment(state.filter);
      if (filterStr) {
        hash += `/filter/${filterStr}`;
      }
    }
    return hash ? `#${hash}` : "";
  }
  function onHashChange(callback) {
    if (typeof window === "undefined") return () => {
    };
    function handler(_event) {
      const state = parseHash(location.hash);
      callback(state);
    }
    window.addEventListener("hashchange", handler);
    return () => window.removeEventListener("hashchange", handler);
  }

  // src/components/explore/graph-explorer.js
  var BASE_RADIUS = 12;
  var RADIUS_SCALE = 3;
  var CLUSTER_PADDING = 30;
  var COMPONENT_RADIUS = 8;
  var DRILLDOWN_KINDS = /* @__PURE__ */ new Set(["Criterion", "Task", "Decision", "Rationale", "Alternative"]);
  function nodeRadius(doc) {
    return BASE_RADIUS + Math.sqrt(doc.components.length) * RADIUS_SCALE;
  }
  function nodeStrokeColor(doc) {
    switch (doc.doc_type) {
      case "requirements":
        return "var(--teal)";
      case "design":
        return "var(--green)";
      case "adr":
        return "var(--gold)";
      default:
        return "var(--text-dim)";
    }
  }
  function computeClusters(documents) {
    const groups = /* @__PURE__ */ new Map();
    const prefixProjects = /* @__PURE__ */ new Map();
    const isMultiProject = documents.some((d) => d.project);
    for (const doc of documents) {
      const slashIdx = doc.id.lastIndexOf("/");
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
    const clusters = [];
    for (const [prefix, docIds] of groups) {
      const project = prefixProjects.get(prefix);
      const name = isMultiProject && project ? `${project} / ${prefix}` : prefix;
      clusters.push({ name, docIds, project: project || null });
    }
    return clusters;
  }
  function computeClusterBounds(cluster, nodePositions) {
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
      height: maxY + CLUSTER_PADDING - y
    };
  }
  var componentStrokeColor = componentColor;
  function buildComponentNodes(doc) {
    const result = [];
    for (const comp of doc.components) {
      if (!comp.id || !DRILLDOWN_KINDS.has(comp.kind)) continue;
      result.push({
        id: `${doc.id}::${comp.id}`,
        componentId: comp.id,
        kind: comp.kind,
        body: comp.body ?? null,
        parentDocId: doc.id,
        parentComponentId: null,
        radius: COMPONENT_RADIUS,
        label: `${comp.kind}: ${comp.id}`,
        implements: comp.implements
      });
      if (comp.kind === "Decision" && comp.children) {
        for (let ci = 0; ci < comp.children.length; ci++) {
          const child = comp.children[ci];
          if (!DRILLDOWN_KINDS.has(child.kind)) continue;
          const childId = child.id ?? `${comp.id}-${child.kind.toLowerCase()}-${ci}`;
          result.push({
            id: `${doc.id}::${childId}`,
            componentId: childId,
            kind: child.kind,
            body: child.body ?? null,
            parentDocId: doc.id,
            parentComponentId: comp.id,
            radius: COMPONENT_RADIUS,
            label: child.id ? `${child.kind}: ${child.id}` : child.kind
          });
        }
      }
    }
    return result;
  }
  function buildComponentLinks(componentNodes) {
    const result = [];
    const idMap = /* @__PURE__ */ new Map();
    for (const node of componentNodes) {
      idMap.set(node.componentId, node.id);
      if (node.parentDocId) {
        idMap.set(`${node.parentDocId}#${node.componentId}`, node.id);
      }
    }
    for (const node of componentNodes) {
      if (node.kind === "Task" && node.implements) {
        for (const targetCompId of node.implements) {
          const targetNodeId = idMap.get(targetCompId);
          if (targetNodeId) {
            result.push({ source: node.id, target: targetNodeId, kind: "implements" });
          }
        }
      }
      if (node.parentComponentId && node.parentDocId) {
        const parentNodeId = `${node.parentDocId}::${node.parentComponentId}`;
        result.push({ source: parentNodeId, target: node.id, kind: "has_child" });
      }
    }
    return result;
  }
  function shortLabel(id) {
    const slashIdx = id.lastIndexOf("/");
    return slashIdx === -1 ? id : id.slice(slashIdx + 1);
  }
  function mount(container, data, renderData) {
    const { documents, edges } = data;
    if (!container || typeof container.getBoundingClientRect !== "function") return;
    const repositoryUrl = "https://github.com/supersigil/supersigil";
    function edgeEndpointId(endpoint) {
      return typeof endpoint === "string" ? endpoint : endpoint.id;
    }
    function edgeFilterId(endpoint) {
      if (typeof endpoint === "object" && endpoint.parentDocId) {
        return endpoint.parentDocId;
      }
      return edgeEndpointId(endpoint);
    }
    let isRestoringState = false;
    const filterState = { types: /* @__PURE__ */ new Set(), status: null };
    const filterOptions = extractFilterOptions(documents);
    const explorerBar = document.createElement("div");
    explorerBar.className = "explorer-bar";
    const typeDropdown = document.createElement("div");
    typeDropdown.className = "filter-multiselect";
    const typeToggle = document.createElement("button");
    typeToggle.className = "filter-dropdown-toggle filter-multiselect-toggle";
    typeDropdown.appendChild(typeToggle);
    const typeMenu = document.createElement("div");
    typeMenu.className = "filter-dropdown-menu filter-multiselect-menu";
    function updateTypeLabel() {
      if (filterState.types.size === 0) {
        typeToggle.textContent = "All types \u25BE";
      } else {
        const names = [...filterState.types].sort().join(", ");
        typeToggle.textContent = `${names} \u25BE`;
      }
    }
    for (const docType of filterOptions.types) {
      const item = document.createElement("label");
      item.className = "filter-dropdown-item filter-multiselect-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = docType;
      checkbox.className = "filter-multiselect-checkbox";
      const label = document.createElement("span");
      label.textContent = docType;
      item.appendChild(checkbox);
      item.appendChild(label);
      typeMenu.appendChild(item);
      checkbox.addEventListener("change", () => {
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
    typeToggle.addEventListener("click", (e) => {
      e.stopPropagation();
      statusDropdown?.classList.remove("open");
      typeDropdown.classList.toggle("open");
    });
    document.addEventListener("click", (e) => {
      if (!typeDropdown.contains(
        /** @type {Node} */
        e.target
      )) {
        typeDropdown.classList.remove("open");
      }
    });
    explorerBar.appendChild(typeDropdown);
    let statusDropdown = null;
    if (filterOptions.statuses.length > 0) {
      statusDropdown = document.createElement("div");
      statusDropdown.className = "filter-select";
      const statusToggle = document.createElement("button");
      statusToggle.className = "filter-dropdown-toggle filter-select-toggle";
      statusToggle.textContent = "All statuses \u25BE";
      statusDropdown.appendChild(statusToggle);
      const statusMenu = document.createElement("div");
      statusMenu.className = "filter-dropdown-menu filter-select-menu";
      const allStatuses = ["all", ...filterOptions.statuses];
      for (const status of allStatuses) {
        const item = document.createElement("div");
        item.className = "filter-dropdown-item filter-select-item";
        if (status === "all") item.classList.add("active");
        item.dataset.value = status;
        item.textContent = status === "all" ? "All statuses" : status;
        item.addEventListener("click", () => {
          filterState.status = status === "all" ? null : status;
          statusToggle.textContent = `${status === "all" ? "All statuses" : status} \u25BE`;
          for (const el of statusMenu.querySelectorAll(".filter-select-item")) {
            el.classList.remove("active");
          }
          item.classList.add("active");
          statusDropdown.classList.remove("open");
          applyFilters();
          syncHashToUrl();
        });
        statusMenu.appendChild(item);
      }
      statusDropdown.appendChild(statusMenu);
      statusToggle.addEventListener("click", (e) => {
        e.stopPropagation();
        typeDropdown.classList.remove("open");
        statusDropdown.classList.toggle("open");
      });
      document.addEventListener("click", (e) => {
        if (!statusDropdown.contains(
          /** @type {Node} */
          e.target
        )) {
          statusDropdown.classList.remove("open");
        }
      });
      explorerBar.appendChild(statusDropdown);
    }
    const searchWrapper = document.createElement("div");
    searchWrapper.className = "explorer-search";
    searchWrapper.style.position = "relative";
    const searchIcon = document.createElement("span");
    searchIcon.className = "explorer-search-icon";
    searchIcon.textContent = "/";
    searchWrapper.appendChild(searchIcon);
    const searchInput = document.createElement("input");
    searchInput.type = "text";
    searchInput.placeholder = "Search docs...";
    searchWrapper.appendChild(searchInput);
    const searchResults = document.createElement("div");
    searchResults.className = "search-results";
    searchWrapper.appendChild(searchResults);
    explorerBar.appendChild(searchWrapper);
    let searchHighlight = -1;
    let currentSearchResults = [];
    let blurTimer = null;
    function renderSearchResults(results) {
      searchResults.innerHTML = "";
      searchHighlight = -1;
      if (results.length === 0) {
        searchResults.classList.remove("open");
        return;
      }
      currentSearchResults = results.slice(0, 8);
      for (let i = 0; i < currentSearchResults.length; i++) {
        const doc = currentSearchResults[i];
        const item = document.createElement("div");
        item.className = "search-result-item";
        item.dataset.index = String(i);
        const idSpan = document.createElement("span");
        idSpan.className = "search-result-id";
        idSpan.textContent = doc.id;
        const titleSpan = document.createElement("span");
        titleSpan.className = "search-result-title";
        titleSpan.textContent = doc.title ?? "";
        item.appendChild(idSpan);
        item.appendChild(titleSpan);
        item.addEventListener("mousedown", (e) => {
          e.preventDefault();
          pickSearchResult(doc);
        });
        item.addEventListener("mouseenter", () => {
          setSearchHighlight(i);
        });
        searchResults.appendChild(item);
      }
      searchResults.classList.add("open");
    }
    function setSearchHighlight(idx) {
      const items = searchResults.querySelectorAll(".search-result-item");
      for (const el of items) {
        el.classList.remove("highlighted");
      }
      searchHighlight = idx;
      if (idx >= 0 && idx < items.length) {
        items[idx].classList.add("highlighted");
      }
    }
    function pickSearchResult(doc) {
      const simNode = nodes.find((n) => n.id === doc.id);
      if (simNode && simNode.x != null && simNode.y != null) {
        clearTrace();
        selectNode(simNode);
        const svgEl = svg.node();
        const currentWidth = svgEl ? svgEl.clientWidth || width : width;
        const currentHeight = svgEl ? svgEl.clientHeight || height : height;
        const scale = 1.5;
        const tx = currentWidth / 2 - simNode.x * scale;
        const ty = currentHeight / 2 - simNode.y * scale;
        svg.transition().duration(500).call(
          /** @type {any} */
          zoom2.transform,
          d32.zoomIdentity.translate(tx, ty).scale(scale)
        );
      }
      searchInput.value = "";
      searchResults.innerHTML = "";
      searchResults.classList.remove("open");
      searchHighlight = -1;
    }
    function closeSearch() {
      searchResults.innerHTML = "";
      searchResults.classList.remove("open");
      searchHighlight = -1;
    }
    searchInput.addEventListener("input", () => {
      const query = searchInput.value;
      const results = searchDocuments(documents, query);
      renderSearchResults(results);
    });
    searchInput.addEventListener("keydown", (event) => {
      const items = searchResults.querySelectorAll(".search-result-item");
      const count = items.length;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        if (count > 0) {
          setSearchHighlight(searchHighlight < count - 1 ? searchHighlight + 1 : 0);
        }
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        if (count > 0) {
          setSearchHighlight(searchHighlight > 0 ? searchHighlight - 1 : count - 1);
        }
      } else if (event.key === "Enter") {
        event.preventDefault();
        if (searchHighlight >= 0 && searchHighlight < count) {
          const doc = currentSearchResults[searchHighlight];
          if (doc) pickSearchResult(doc);
        }
      } else if (event.key === "Escape") {
        closeSearch();
        searchInput.blur();
      }
    });
    searchInput.addEventListener("blur", () => {
      blurTimer = window.setTimeout(() => {
        closeSearch();
      }, 150);
    });
    searchInput.addEventListener("focus", () => {
      if (blurTimer != null) {
        clearTimeout(blurTimer);
        blurTimer = null;
      }
      const query = searchInput.value;
      if (query.trim()) {
        const results = searchDocuments(documents, query);
        renderSearchResults(results);
      }
    });
    container.classList.add("explorer");
    container.prepend(explorerBar);
    const splitContainer = document.createElement("div");
    splitContainer.className = "explorer-split";
    container.appendChild(splitContainer);
    const graphPane = document.createElement("div");
    graphPane.className = "explorer-split-graph";
    splitContainer.appendChild(graphPane);
    const divider = document.createElement("div");
    divider.className = "explorer-split-divider";
    splitContainer.appendChild(divider);
    const specPane = document.createElement("div");
    specPane.className = "explorer-split-panel";
    splitContainer.appendChild(specPane);
    const SPLIT_KEY = "supersigil-explorer-split";
    const savedSplit = localStorage.getItem(SPLIT_KEY);
    if (savedSplit) {
      const pct = Number.parseFloat(savedSplit);
      if (pct > 10 && pct < 90) {
        graphPane.style.flex = `0 0 ${pct}%`;
        specPane.style.width = `${100 - pct}%`;
      }
    }
    let isDragging = false;
    divider.addEventListener("mousedown", (e) => {
      e.preventDefault();
      isDragging = true;
      divider.classList.add("dragging");
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    });
    document.addEventListener("mousemove", (e) => {
      if (!isDragging) return;
      const splitRect = splitContainer.getBoundingClientRect();
      const x = e.clientX - splitRect.left;
      const totalWidth = splitRect.width;
      const pct = x / totalWidth * 100;
      const clamped = Math.max(20, Math.min(80, pct));
      graphPane.style.flex = `0 0 ${clamped}%`;
      specPane.style.width = `${100 - clamped}%`;
      localStorage.setItem(SPLIT_KEY, String(clamped));
    });
    document.addEventListener("mouseup", () => {
      if (!isDragging) return;
      isDragging = false;
      divider.classList.remove("dragging");
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    });
    const canvasDiv = document.createElement("div");
    canvasDiv.className = "explorer-canvas";
    graphPane.appendChild(canvasDiv);
    const rect = canvasDiv.getBoundingClientRect();
    const screenWidth = rect.width || 800;
    const screenHeight = rect.height || 600;
    const MIN_SIM_SIZE = 1600;
    const width = Math.max(screenWidth, MIN_SIM_SIZE);
    const height = Math.max(screenHeight, MIN_SIM_SIZE * 0.75);
    const clusters = computeClusters(documents);
    const docClusterMap = /* @__PURE__ */ new Map();
    for (const cluster of clusters) {
      for (const docId of cluster.docIds) {
        docClusterMap.set(docId, cluster.name);
      }
    }
    const isMultiProject = documents.some((d) => d.project);
    const nodes = documents.map((doc) => ({
      ...doc,
      radius: nodeRadius(doc),
      group: docClusterMap.get(doc.id) ?? doc.id
    }));
    const links = edges.map((e) => ({
      source: e.from,
      target: e.to,
      kind: e.kind
    }));
    const svg = d32.select(canvasDiv).append("svg").attr("width", "100%").attr("height", "100%").attr("viewBox", `0 0 ${width} ${height}`).style("background", "var(--bg-deep)");
    const zoomLayer = svg.append("g").attr("class", "zoom-layer");
    const clustersGroup = zoomLayer.append("g").attr("class", "clusters");
    const edgesGroup = zoomLayer.append("g").attr("class", "edges");
    const nodesGroup = zoomLayer.append("g").attr("class", "nodes");
    const legendEl = document.createElement("div");
    legendEl.className = "edge-legend";
    legendEl.innerHTML = [
      { label: "Implements", cls: "legend-implements" },
      { label: "DependsOn", cls: "legend-dependson" },
      { label: "References", cls: "legend-references" }
    ].map(
      (item) => `<div class="edge-legend-item"><span class="edge-legend-line ${item.cls}"></span><span>${item.label}</span></div>`
    ).join("");
    canvasDiv.appendChild(legendEl);
    let edgeLines = edgesGroup.selectAll("line.edge").data(links).join("line").attr("class", "edge").attr("data-kind", (d) => d.kind).attr("data-idx", (_d, i) => i).style("opacity", (d) => d.kind === "References" ? 0.3 : 0.4).on("mouseenter", function() {
      const idx = +this.getAttribute("data-idx");
      d32.select(edgeLabels.nodes()[idx]).classed("visible", true);
    }).on("mouseleave", function() {
      const idx = +this.getAttribute("data-idx");
      d32.select(edgeLabels.nodes()[idx]).classed("visible", false);
    });
    let edgeLabels = edgesGroup.selectAll("text.edge-label").data(links).join("text").attr("class", "edge-label").attr("font-size", 9).attr("font-family", "var(--font-mono)").attr("text-anchor", "middle").attr("dy", -4).text((d) => d.kind);
    let nodeGroups = nodesGroup.selectAll("g.node").data(nodes).join("g").attr("class", "node").style("cursor", "grab");
    nodeGroups.append("circle").attr("r", (d) => d.radius).attr("fill", "var(--bg-card)").attr("stroke", (d) => nodeStrokeColor(d)).attr("stroke-width", 2);
    nodeGroups.append("text").attr("fill", "var(--text)").attr("font-size", 11).attr("font-family", "var(--font-body)").attr("text-anchor", "middle").attr("dy", (d) => d.radius + 14).text((d) => shortLabel(d.id));
    const projectColors = {};
    const palette = [
      "var(--gold-dim)",
      "var(--teal)",
      "#8b5cf6",
      "#f59e0b",
      "#ec4899",
      "#06b6d4",
      "#84cc16",
      "#f97316"
    ];
    let colorIdx = 0;
    for (const c of clusters) {
      if (c.project && !projectColors[c.project]) {
        projectColors[c.project] = palette[colorIdx % palette.length];
        colorIdx++;
      }
    }
    function clusterColor(d) {
      return d.project ? projectColors[d.project] || "var(--gold-dim)" : "var(--gold-dim)";
    }
    const clusterRects = clustersGroup.selectAll("rect.cluster").data(clusters).join("rect").attr("class", "cluster").attr("fill", "none").attr("stroke", clusterColor).attr("stroke-width", 1.5).attr("stroke-dasharray", "6 4").attr("rx", 8).attr("ry", 8);
    const clusterLabels = clustersGroup.selectAll("text.cluster-label").data(clusters).join("text").attr("class", "cluster-label").attr("fill", clusterColor).attr("font-size", 10).attr("font-family", "var(--font-mono)").text((d) => d.name);
    function applyFilters() {
      const visibleSet = filterDocuments(documents, filterState);
      nodesGroup.selectAll("g.node").attr("opacity", (d) => {
        const id = (
          /** @type {any} */
          d.id
        );
        const parentDocId = (
          /** @type {any} */
          d.parentDocId
        );
        const checkId = parentDocId ?? id;
        return visibleSet.has(checkId) ? 1 : 0.08;
      });
      edgesGroup.selectAll("line.edge").attr("stroke-opacity", (d) => {
        const sourceId = edgeFilterId(d.source);
        const targetId = edgeFilterId(d.target);
        return visibleSet.has(sourceId) && visibleSet.has(targetId) ? 0.6 : 0.03;
      });
      edgesGroup.selectAll("text.edge-label").attr("opacity", (d) => {
        const sourceId = edgeFilterId(d.source);
        const targetId = edgeFilterId(d.target);
        return visibleSet.has(sourceId) && visibleSet.has(targetId) ? 1 : 0.03;
      });
    }
    const expandedDocs = /* @__PURE__ */ new Set();
    const expandedComponentNodes = /* @__PURE__ */ new Map();
    function allActiveNodes() {
      const result = [...nodes];
      for (const compNodes of expandedComponentNodes.values()) {
        result.push(...compNodes);
      }
      return result;
    }
    const groupingForce = forceInABox().strength(0.1).template("treemap").groupBy("group").links(links).linkStrengthInterCluster(0.01).linkStrengthIntraCluster(0.1).size([width, height]);
    const simulation = d32.forceSimulation(nodes).force("group", groupingForce).force(
      "link",
      d32.forceLink(links).id((d) => (
        /** @type {typeof nodes[number]} */
        d.id
      )).distance(80).strength(groupingForce.getLinkStrength)
    ).force("charge", d32.forceManyBody().strength(-200)).force(
      "collide",
      d32.forceCollide().radius((d) => (
        /** @type {typeof nodes[number]} */
        d.radius + 4
      ))
    ).on("tick", ticked);
    function ticked() {
      edgeLines.attr("x1", (d) => (
        /** @type {any} */
        d.source.x
      )).attr("y1", (d) => (
        /** @type {any} */
        d.source.y
      )).attr("x2", (d) => (
        /** @type {any} */
        d.target.x
      )).attr("y2", (d) => (
        /** @type {any} */
        d.target.y
      ));
      edgeLabels.attr("x", (d) => (
        /** @type {any} */
        (d.source.x + /** @type {any} */
        d.target.x) / 2
      )).attr("y", (d) => (
        /** @type {any} */
        (d.source.y + /** @type {any} */
        d.target.y) / 2
      ));
      nodeGroups.attr("transform", (d) => `translate(${d.x},${d.y})`);
      const nodePositions = /* @__PURE__ */ new Map();
      for (const node of allActiveNodes()) {
        nodePositions.set(node.id, {
          x: node.x ?? 0,
          y: node.y ?? 0,
          radius: node.radius
        });
      }
      const boundsMap = /* @__PURE__ */ new Map();
      for (const cluster of clusters) {
        const bounds = computeClusterBounds(cluster, nodePositions);
        if (bounds) boundsMap.set(cluster.name, bounds);
      }
      clusterRects.each(function(cluster) {
        const bounds = boundsMap.get(cluster.name);
        if (bounds) {
          d32.select(this).attr("x", bounds.x).attr("y", bounds.y).attr("width", bounds.width).attr("height", bounds.height).attr("visibility", "visible");
        } else {
          d32.select(this).attr("visibility", "hidden");
        }
      });
      clusterLabels.each(function(cluster) {
        const bounds = boundsMap.get(cluster.name);
        if (bounds) {
          d32.select(this).attr("x", bounds.x + 8).attr("y", bounds.y + 14).attr("visibility", "visible");
        } else {
          d32.select(this).attr("visibility", "hidden");
        }
      });
    }
    const zoom2 = d32.zoom().scaleExtent([0.1, 4]).on("zoom", (event) => {
      zoomLayer.attr("transform", event.transform);
    });
    svg.call(
      /** @type {any} */
      zoom2
    );
    const drag2 = d32.drag().on("start", (event, d) => {
      if (!event.active) simulation.alphaTarget(0.3).restart();
      d.fx = d.x;
      d.fy = d.y;
    }).on("drag", (event, d) => {
      d.fx = event.x;
      d.fy = event.y;
    }).on("end", (event, d) => {
      if (!event.active) simulation.alphaTarget(0);
      d.fx = null;
      d.fy = null;
    });
    nodeGroups.call(
      /** @type {any} */
      drag2
    );
    function rebindEdges() {
      const allComponentNodes = [];
      for (const compNodes of expandedComponentNodes.values()) {
        allComponentNodes.push(...compNodes);
      }
      const allLinks = [...links, ...buildComponentLinks(allComponentNodes)];
      groupingForce.links(allLinks);
      simulation.force(
        "link",
        d32.forceLink(allLinks).id((d) => (
          /** @type {any} */
          d.id
        )).distance(
          (d) => (
            /** @type {any} */
            d.kind === "has_child" || /** @type {any} */
            d.kind === "implements" ? 40 : 100
          )
        ).strength(groupingForce.getLinkStrength)
      );
      edgeLines = edgesGroup.selectAll("line.edge").data(
        allLinks,
        (d) => `${/** @type {any} */
        d.source?.id ?? d.source}-${/** @type {any} */
        d.target?.id ?? d.target}-${/** @type {any} */
        d.kind}`
      ).join(
        (enter) => enter.append("line").attr("class", "edge").attr("data-kind", (d) => d.kind).attr(
          "stroke-width",
          (d) => d.kind === "has_child" || d.kind === "implements" ? 0.8 : 1
        ).style("opacity", 0).call((sel) => sel.transition().duration(300).style("opacity", 0.4)),
        (update) => update,
        (exit) => exit.transition().duration(200).style("opacity", 0).remove()
      );
      edgeLabels = edgesGroup.selectAll("text.edge-label").data(
        allLinks,
        (d) => `${/** @type {any} */
        d.source?.id ?? d.source}-${/** @type {any} */
        d.target?.id ?? d.target}-${/** @type {any} */
        d.kind}`
      ).join(
        (enter) => enter.append("text").attr("class", "edge-label").attr("font-size", 9).attr("font-family", "var(--font-mono)").attr("text-anchor", "middle").attr("dy", -4).text((d) => d.kind),
        (update) => update,
        (exit) => exit.remove()
      );
    }
    function rebindNodes() {
      const allNodes = [...nodes];
      for (const compNodes of expandedComponentNodes.values()) {
        allNodes.push(.../** @type {any[]} */
        compNodes);
      }
      simulation.nodes(allNodes);
      simulation.force(
        "collide",
        d32.forceCollide().radius((d) => (
          /** @type {any} */
          d.radius + /** @type {any} */
          (d.componentId ? 4 : 8)
        ))
      );
      nodeGroups = nodesGroup.selectAll("g.node").data(allNodes, (d) => (
        /** @type {any} */
        d.id
      )).join(
        (enter) => {
          const g = enter.append("g").attr(
            "class",
            (d) => (
              /** @type {any} */
              d.componentId ? "node component-node" : "node"
            )
          ).style("cursor", "grab").attr("opacity", 0);
          g.append("circle").attr("r", (d) => (
            /** @type {any} */
            d.radius
          )).attr("fill", "var(--bg-card)").attr("stroke", (d) => {
            const cn = (
              /** @type {any} */
              d
            );
            return cn.componentId ? componentColor(cn.kind) : nodeStrokeColor(cn);
          }).attr("stroke-width", (d) => (
            /** @type {any} */
            d.componentId ? 1.5 : 2
          ));
          g.append("text").attr("fill", "var(--text)").attr("font-size", (d) => (
            /** @type {any} */
            d.componentId ? 9 : 11
          )).attr("font-family", "var(--font-body)").attr("text-anchor", "middle").attr("dy", (d) => (
            /** @type {any} */
            d.radius + 12
          )).text((d) => {
            const cn = (
              /** @type {any} */
              d
            );
            return cn.label ?? shortLabel(cn.id);
          });
          g.transition().duration(300).attr("opacity", 1);
          g.call(
            /** @type {any} */
            drag2
          );
          g.filter((d) => !/** @type {any} */
          d.componentId).on("dblclick", handleDblClick);
          return g;
        },
        (update) => update,
        (exit) => exit.transition().duration(200).attr("opacity", 0).remove()
      );
    }
    function handleDblClick(event, d) {
      event.stopPropagation();
      const docId = d.id;
      if (expandedDocs.has(docId)) {
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
        expandedDocs.add(docId);
        const compNodes = buildComponentNodes(d);
        for (const cn of compNodes) {
          cn.x = (d.x ?? 0) + (Math.random() - 0.5) * 40;
          cn.y = (d.y ?? 0) + (Math.random() - 0.5) * 40;
        }
        expandedComponentNodes.set(docId, compNodes);
        const clusterName = docClusterMap.get(docId);
        if (clusterName) {
          for (const cn of compNodes) {
            docClusterMap.set(cn.id, clusterName);
            cn.group = clusterName;
          }
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
      simulation.alpha(0.5).restart();
    }
    nodeGroups.on("dblclick", handleDblClick);
    const detailPanel = document.createElement("div");
    detailPanel.className = "detail-panel";
    specPane.appendChild(detailPanel);
    renderEmpty(detailPanel, data, renderData);
    let selectedNodeId = null;
    let selectedClusterName = null;
    function showEdgeLabelsFor(nodeId) {
      edgeLabels.classed("visible", (d) => {
        if (!nodeId) return false;
        const srcId = edgeEndpointId(d.source);
        const tgtId = edgeEndpointId(d.target);
        return srcId === nodeId || tgtId === nodeId;
      });
      edgeLines.each(function(d) {
        const srcId = edgeEndpointId(d.source);
        const tgtId = edgeEndpointId(d.target);
        const connected = nodeId && (srcId === nodeId || tgtId === nodeId);
        const el = d32.select(this);
        el.style("opacity", nodeId ? connected ? 0.9 : 0.06 : null);
        el.style("stroke-width", nodeId ? connected ? "2.5px" : null : null);
      });
    }
    function deselectCluster() {
      if (!selectedClusterName) return;
      clusterRects.classed("cluster-selected", false);
      nodesGroup.selectAll("g.node").classed("in-selected-cluster", false);
      edgeLines.each(function() {
        d32.select(this).style("opacity", null).style("stroke-width", null);
      });
      clearDetail(detailPanel);
      renderEmpty(detailPanel, data, renderData);
      selectedClusterName = null;
    }
    function clearSelection() {
      if (selectedClusterName) {
        deselectCluster();
        syncHashToUrl();
        return;
      }
      if (selectedNodeId) {
        clearTrace();
        nodesGroup.selectAll("g.node").classed("node-selected", false);
        showEdgeLabelsFor(null);
        clearDetail(detailPanel);
        renderEmpty(detailPanel, data, renderData);
        selectedNodeId = null;
        syncHashToUrl();
      }
    }
    function panToPoint(x, y, targetScale) {
      const isMobile = screenWidth < 600;
      const cx = width / 2;
      const cy = isMobile ? height * 0.55 / 2 : height / 2;
      const tx = cx - x * targetScale;
      const ty = cy - y * targetScale;
      svg.transition().duration(400).call(
        /** @type {any} */
        zoom2.transform,
        d32.zoomIdentity.translate(tx, ty).scale(targetScale)
      );
    }
    function centerOnNode(d) {
      if (d.x == null || d.y == null) return;
      const isMobile = screenWidth < 600;
      panToPoint(d.x, d.y, isMobile ? 3 : 1.2);
    }
    function selectNode(d) {
      if (selectedClusterName) {
        deselectCluster();
      }
      nodesGroup.selectAll("g.node").classed("node-selected", false);
      nodesGroup.selectAll("g.node").filter((nd) => (
        /** @type {any} */
        nd.id === d.id
      )).classed("node-selected", true);
      showEdgeLabelsFor(d.id);
      centerOnNode(d);
      renderDetail(detailPanel, d, edges, renderData ?? [], repositoryUrl);
      selectedNodeId = d.id;
      syncHashToUrl();
    }
    detailPanel.addEventListener("click", (event) => {
      const target = (
        /** @type {HTMLElement} */
        event.target
      );
      if (target.closest(".detail-panel-close")) {
        clearSelection();
        return;
      }
      if (target.closest(".detail-panel-trace-btn")) {
        const btn = (
          /** @type {HTMLElement} */
          target.closest(".detail-panel-trace-btn")
        );
        const docId = btn.dataset.docId;
        if (docId) {
          if (activeTraceSet) {
            clearTrace();
            btn.classList.remove("active");
          } else {
            activateTrace(docId);
            btn.classList.add("active");
          }
        }
      }
      const edgeTarget = target.closest(".edge-list-target");
      if (edgeTarget) {
        const docId = (
          /** @type {HTMLElement} */
          edgeTarget.dataset.docId
        );
        if (docId) {
          const simNode = nodes.find((n) => n.id === docId);
          if (simNode) {
            selectNode(simNode);
          }
        }
      }
      const anchor = target.closest('a[href^="#/doc/"]');
      if (anchor) {
        event.preventDefault();
        const href = (
          /** @type {HTMLAnchorElement} */
          anchor.getAttribute("href")
        );
        if (href) {
          const docId = decodeURIComponent(href.replace("#/doc/", ""));
          const simNode = nodes.find((n) => n.id === docId);
          if (simNode) {
            clearTrace();
            selectNode(simNode);
          }
        }
      }
    });
    let activeTraceSet = null;
    function activateTrace(startId) {
      activeTraceSet = traceImpact(edges, startId);
      applyTrace();
      syncHashToUrl();
    }
    function clearTrace() {
      if (!activeTraceSet) return;
      activeTraceSet = null;
      nodesGroup.selectAll("g.node").attr("opacity", 1);
      edgesGroup.selectAll("line.edge").style("opacity", null).style("stroke-width", null).attr("stroke", null);
      edgesGroup.selectAll("text.edge-label").classed("visible", false);
      applyFilters();
      if (selectedNodeId) {
        showEdgeLabelsFor(selectedNodeId);
      }
      syncHashToUrl();
    }
    function applyTrace() {
      if (!activeTraceSet) return;
      const traced = activeTraceSet;
      const visibleSet = filterDocuments(documents, filterState);
      nodesGroup.selectAll("g.node").attr("opacity", (d) => {
        const id = (
          /** @type {any} */
          d.id
        );
        const parentDocId = (
          /** @type {any} */
          d.parentDocId
        );
        const checkId = parentDocId ?? id;
        if (!visibleSet.has(checkId)) return 0.08;
        return traced.has(id) ? 1 : 0.05;
      });
      edgesGroup.selectAll("line.edge").each(function(d) {
        const sourceId = edgeEndpointId(d.source);
        const targetId = edgeEndpointId(d.target);
        const isTraced = traced.has(sourceId) && traced.has(targetId);
        d32.select(this).attr("stroke", isTraced ? "var(--gold)" : null).style("opacity", isTraced ? 0.8 : 0.03);
      });
      edgesGroup.selectAll("text.edge-label").classed("visible", (d) => {
        const sourceId = edgeEndpointId(d.source);
        const targetId = edgeEndpointId(d.target);
        return traced.has(sourceId) && traced.has(targetId);
      });
    }
    function syncHashToUrl() {
      if (isRestoringState) return;
      const state = {
        doc: selectedNodeId,
        trace: activeTraceSet !== null && selectedNodeId !== null,
        filter: null
      };
      if (filterState.types.size > 0 || filterState.status !== null) {
        state.filter = {
          types: [...filterState.types],
          status: filterState.status
        };
      }
      const hash = buildHash(state);
      history.replaceState(null, "", hash || location.pathname + location.search);
    }
    function restoreStateFromHash(state) {
      isRestoringState = true;
      try {
        if (state.filter) {
          filterState.types.clear();
          for (const t of state.filter.types) {
            filterState.types.add(t);
          }
          const checkboxes = container.querySelectorAll(".filter-multiselect-checkbox");
          for (const cb of checkboxes) {
            cb.checked = filterState.types.has(
              /** @type {HTMLInputElement} */
              cb.value
            );
          }
          updateTypeLabel();
          filterState.status = state.filter.status || null;
          const statusVal = filterState.status || "all";
          const toggle = container.querySelector(".filter-select-toggle");
          if (toggle)
            toggle.textContent = `${statusVal === "all" ? "All statuses" : statusVal} \u25BE`;
          for (const el of container.querySelectorAll(".filter-select-item")) {
            el.classList.toggle(
              "active",
              /** @type {HTMLElement} */
              el.dataset.value === statusVal
            );
          }
        } else {
          filterState.types.clear();
          filterState.status = null;
          const checkboxes = container.querySelectorAll(".filter-multiselect-checkbox");
          for (const cb of checkboxes) {
            cb.checked = false;
          }
          updateTypeLabel();
          const toggle = container.querySelector(".filter-select-toggle");
          if (toggle) toggle.textContent = "All statuses \u25BE";
          for (const el of container.querySelectorAll(".filter-select-item")) {
            el.classList.toggle(
              "active",
              /** @type {HTMLElement} */
              el.dataset.value === "all"
            );
          }
        }
        applyFilters();
        if (state.doc) {
          const simNode = nodes.find((n) => n.id === state.doc);
          if (simNode) {
            selectNode(simNode);
            const svgEl = svg.node();
            const currentWidth = svgEl ? svgEl.clientWidth || width : width;
            const currentHeight = svgEl ? svgEl.clientHeight || height : height;
            const scale = 1.5;
            const tx = currentWidth / 2 - (simNode.x ?? 0) * scale;
            const ty = currentHeight / 2 - (simNode.y ?? 0) * scale;
            svg.transition().duration(500).call(
              /** @type {any} */
              zoom2.transform,
              d32.zoomIdentity.translate(tx, ty).scale(scale)
            );
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
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        clearTrace();
        clearSelection();
      }
      if (event.key === "/" && !event.ctrlKey && !event.metaKey && !(event.target instanceof HTMLInputElement) && !(event.target instanceof HTMLTextAreaElement) && !(event.target instanceof HTMLSelectElement)) {
        event.preventDefault();
        searchInput.focus();
      }
    });
    function selectCluster(cluster) {
      if (selectedNodeId) {
        clearTrace();
        nodesGroup.selectAll("g.node").classed("node-selected", false);
        showEdgeLabelsFor(null);
        clearDetail(detailPanel);
        selectedNodeId = null;
      }
      if (selectedClusterName) {
        clusterRects.classed("cluster-selected", false);
        nodesGroup.selectAll("g.node").classed("in-selected-cluster", false);
        edgeLines.each(function() {
          d32.select(this).style("opacity", null).style("stroke-width", null);
        });
        selectedClusterName = null;
      }
      selectedClusterName = cluster.name;
      clusterRects.classed("cluster-selected", (d) => d.name === cluster.name);
      const clusterDocSet = new Set(cluster.docIds);
      nodesGroup.selectAll("g.node").classed("in-selected-cluster", (d) => {
        const id = (
          /** @type {any} */
          d.id
        );
        return clusterDocSet.has(id);
      });
      edgeLines.each(function(d) {
        const srcId = edgeEndpointId(d.source);
        const tgtId = edgeEndpointId(d.target);
        const srcInside = clusterDocSet.has(srcId);
        const tgtInside = clusterDocSet.has(tgtId);
        const isCrossCluster = srcInside !== tgtInside;
        const el = d32.select(this);
        el.style("opacity", isCrossCluster ? 0.9 : 0.06);
        el.style("stroke-width", isCrossCluster ? "2.5px" : null);
      });
      const clusterNodes = nodes.filter((n) => clusterDocSet.has(n.id));
      if (clusterNodes.length > 0) {
        const cx = clusterNodes.reduce((s, n) => s + (n.x ?? 0), 0) / clusterNodes.length;
        const cy = clusterNodes.reduce((s, n) => s + (n.y ?? 0), 0) / clusterNodes.length;
        const isMobile = screenWidth < 600;
        panToPoint(cx, cy, isMobile ? 2.5 : 1);
      }
      const clusterDocs = documents.filter((doc) => clusterDocSet.has(doc.id));
      renderClusterDetail(detailPanel, cluster.name, clusterDocs, edges);
    }
    d32.select(clustersGroup.node()).on("click", (event) => {
      const target = (
        /** @type {Element} */
        event.target
      );
      const clusterRect = target.closest("rect.cluster");
      const clusterLabel = target.closest("text.cluster-label");
      const el = clusterRect || clusterLabel;
      if (!el) return;
      event.stopPropagation();
      const datum = (
        /** @type {Cluster} */
        d32.select(el).datum()
      );
      if (!datum) return;
      if (selectedClusterName === datum.name) {
        deselectCluster();
        syncHashToUrl();
      } else {
        selectCluster(datum);
        syncHashToUrl();
      }
    });
    svg.on("click", (event) => {
      const target = (
        /** @type {Element} */
        event.target
      );
      if (target.closest("g.node") || target.closest("line.edge") || target.closest("rect.cluster") || target.closest("text.cluster-label"))
        return;
      clearTrace();
      clearSelection();
    });
    d32.select(nodesGroup.node()).on("click", (event) => {
      const target = (
        /** @type {Element} */
        event.target
      );
      const nodeEl = target.closest("g.node");
      if (!nodeEl) return;
      const datum = (
        /** @type {typeof nodes[number]} */
        d32.select(nodeEl).datum()
      );
      if (!datum || /** @type {any} */
      datum.componentId) return;
      event.stopPropagation();
      if (selectedClusterName) {
        deselectCluster();
      }
      if (selectedNodeId === datum.id) {
        clearSelection();
      } else {
        selectNode(datum);
      }
    });
    const initialState = parseHash(location.hash);
    if (initialState.doc || initialState.filter) {
      let initialRestored = false;
      const restoreOnce = () => {
        if (initialRestored) return;
        initialRestored = true;
        simulation.on("end.restore", null);
        restoreStateFromHash(initialState);
      };
      simulation.on("end.restore", restoreOnce);
      const fallbackTimer = setTimeout(restoreOnce, 800);
      const origRestore = restoreOnce;
      const restoreAndCleanup = () => {
        clearTimeout(fallbackTimer);
        origRestore();
      };
      simulation.on("end.restore", restoreAndCleanup);
    }
    onHashChange((state) => {
      restoreStateFromHash(state);
    });
  }
  return __toCommonJS(graph_explorer_exports);
})();
