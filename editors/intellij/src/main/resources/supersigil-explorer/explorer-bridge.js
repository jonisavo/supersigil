(function () {
  "use strict";

  var state = {
    booted: false,
    observer: null,
    currentHandle: null,
    docPathById: new Map(),
    container: null,
  };

  var EVIDENCE_PREFIX = "#supersigil-action:";

  function getContainer() {
    if (!state.container || !document.contains(state.container)) {
      state.container = document.getElementById("explorer") || document.body;
    }
    return state.container;
  }

  function getExplorer() {
    return window.SupersigilExplorer && typeof window.SupersigilExplorer.mount === "function"
      ? window.SupersigilExplorer
      : null;
  }

  function escapeActionValue(value) {
    return String(value).replace(/\\/g, "\\\\").replace(/:/g, "\\:");
  }

  function buildOpenFileAction(path, line) {
    return "open-file:" + escapeActionValue(path) + ":" + String(line);
  }

  function applyHash(hash) {
    if (hash) {
      window.location.hash = hash;
      return;
    }

    history.replaceState(null, "", window.location.pathname + window.location.search);
  }

  function parsePayload(json) {
    if (json && typeof json === "object") return json;
    if (typeof json !== "string") return {};
    try {
      return JSON.parse(json);
    } catch (e) {
      return {};
    }
  }

  function getGraphData(payload) {
    return payload.graphData || payload.graph || payload.data?.graphData || payload.data?.graph || null;
  }

  function getRenderData(payload) {
    return payload.renderData || payload.render || payload.data?.renderData || payload.data?.render || [];
  }

  function getDocumentPath(doc) {
    if (!doc || typeof doc !== "object") return null;
    return (
      doc.filePath ||
      doc.file_path ||
      doc.sourcePath ||
      doc.source_path ||
      doc.path ||
      doc.file ||
      null
    );
  }

  function buildDocumentPathMap(graphData) {
    var map = new Map();
    var documents = graphData && Array.isArray(graphData.documents) ? graphData.documents : [];
    for (var i = 0; i < documents.length; i++) {
      var doc = documents[i];
      if (!doc || !doc.id) continue;
      var path = getDocumentPath(doc);
      if (path) {
        map.set(doc.id, path);
      }
    }
    return map;
  }

  function findAnchor(target) {
    if (!target) return null;
    if (typeof target.closest === "function") {
      return target.closest("a");
    }
    while (target && target.tagName !== "A") {
      target = target.parentElement;
    }
    return target && target.tagName === "A" ? target : null;
  }

  function sendAction(action) {
    if (typeof window.__supersigilAction === "function") {
      window.__supersigilAction(action);
    }
  }

  function attachEvidenceClickHandler() {
    document.addEventListener(
      "click",
      function (event) {
        var anchor = findAnchor(event.target);
        if (!anchor) return;

        var href = anchor.getAttribute("href");
        if (!href || href.indexOf(EVIDENCE_PREFIX) !== 0) return;

        event.preventDefault();
        event.stopPropagation();

        var action = href.substring(EVIDENCE_PREFIX.length);
        try {
          action = decodeURIComponent(action);
        } catch (e) {
          // Keep the raw action when it is not percent-encoded.
        }

        sendAction(action);
      },
      true
    );
  }

  function injectOpenFileButton(container) {
    var headers = container.querySelectorAll(".detail-panel-header");
    for (var i = 0; i < headers.length; i++) {
      var header = headers[i];
      if (header.querySelector(".open-file-btn")) continue;

      var titleEl = header.querySelector(".detail-panel-title");
      if (!titleEl) continue;

      var docId = (titleEl.textContent || "").trim();
      if (!docId) continue;

      var path = state.docPathById.get(docId);
      if (!path) continue;

      var button = document.createElement("button");
      button.type = "button";
      button.className = "open-file-btn";
      button.textContent = "Open File";
      button.title = "Open " + path;
      (function (buttonPath) {
        button.addEventListener("click", function () {
          sendAction(buildOpenFileAction(buttonPath, 1));
        });
      })(path);

      var closeButton = header.querySelector(".detail-panel-close");
      if (closeButton) {
        header.insertBefore(button, closeButton);
      } else {
        header.appendChild(button);
      }
    }
  }

  function setupObserver() {
    if (state.observer || typeof MutationObserver === "undefined") return;

    var container = getContainer();
    if (!container || !container.nodeType) return;

    state.observer = new MutationObserver(function () {
      injectOpenFileButton(container);
    });

    state.observer.observe(container, {
      childList: true,
      subtree: true,
    });
  }

  function ensureBooted() {
    if (state.booted) return;
    state.booted = true;
    attachEvidenceClickHandler();
    setupObserver();
  }

  function mountExplorer(graphData, renderData) {
    var explorer = getExplorer();
    if (!explorer) return;

    var container = getContainer();
    if (!container) return;

    var preservedHash = window.location.hash;

    if (state.currentHandle && typeof state.currentHandle.unmount === "function") {
      state.currentHandle.unmount();
    }

    applyHash(preservedHash);
    container.innerHTML = "";
    state.docPathById = buildDocumentPathMap(graphData);

    var handle = explorer.mount(container, graphData, renderData, null, {
      evidenceLink: function (path, line) {
        return EVIDENCE_PREFIX + buildOpenFileAction(path, line);
      },
      documentLink: function (docId) {
        return "#/doc/" + encodeURIComponent(docId);
      },
      criterionLink: function (docId, _criterionId) {
        return "#/doc/" + encodeURIComponent(docId);
      },
    });

    state.currentHandle = handle || null;
    injectOpenFileButton(container);
  }

  window.__supersigilReceiveData = function (json) {
    ensureBooted();

    var payload = parsePayload(json);
    var graphData = getGraphData(payload);
    var renderData = getRenderData(payload);

    if (!graphData) return;

    mountExplorer(graphData, renderData);
  };

  function boot() {
    ensureBooted();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
