(function () {
  "use strict";

  var state = {
    booted: false,
    currentApp: null,
    container: null,
    initialContext: null,
    initialContextPromise: null,
    resolveInitialContext: null,
    changeListeners: [],
  };

  var EVIDENCE_PREFIX = "#supersigil-action:";

  function getContainer() {
    if (!state.container || !document.contains(state.container)) {
      state.container = document.getElementById("explorer") || document.body;
    }
    return state.container;
  }

  function getExplorer() {
    return window.SupersigilExplorer && typeof window.SupersigilExplorer.createExplorerApp === "function"
      ? window.SupersigilExplorer
      : null;
  }

  function escapeActionValue(value) {
    return String(value).replace(/\\/g, "\\\\").replace(/:/g, "\\:");
  }

  function buildOpenFileAction(path, line) {
    return "open-file:" + escapeActionValue(path) + ":" + String(line);
  }

  function buildOpenFileUriAction(uri, line) {
    return "open-file-uri:" + escapeActionValue(uri) + ":" + String(line);
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

  function sendRequest(method, params) {
    return new Promise(function (resolve, reject) {
      if (typeof window.__supersigilQuery !== "function") {
        reject(new Error("Supersigil query bridge is unavailable"));
        return;
      }

      window.__supersigilQuery(
        JSON.stringify({
          method: method,
          params: params || {},
        }),
        function (response) {
          resolve(parsePayload(response));
        },
        function (_code, message) {
          reject(new Error(message || "Supersigil query failed"));
        }
      );
    });
  }

  function ensureInitialContextPromise() {
    if (state.initialContext) {
      return Promise.resolve(state.initialContext);
    }
    if (!state.initialContextPromise) {
      state.initialContextPromise = new Promise(function (resolve) {
        state.resolveInitialContext = resolve;
      });
    }
    return state.initialContextPromise;
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

  function ensureBooted() {
    if (state.booted) return;
    state.booted = true;
    attachEvidenceClickHandler();
  }

  function createTransport() {
    return {
      getInitialContext: function () {
        return ensureInitialContextPromise();
      },
      loadSnapshot: function (rootId) {
        return sendRequest("loadSnapshot", { rootId: rootId });
      },
      loadDocument: function (input) {
        return sendRequest("loadDocument", input);
      },
      subscribeChanges: function (listener) {
        state.changeListeners.push(listener);
        return function () {
          state.changeListeners = state.changeListeners.filter(function (candidate) {
            return candidate !== listener;
          });
        };
      },
      openFile: function (target) {
        if (!target) return;
        if (target.path) {
          sendAction(buildOpenFileAction(target.path, target.line || 1));
          return;
        }
        if (target.uri) {
          sendAction(buildOpenFileUriAction(target.uri, target.line || 1));
        }
      },
    };
  }

  function boot() {
    ensureBooted();
    if (state.currentApp) return;

    var explorer = getExplorer();
    if (!explorer) return;

    var container = getContainer();
    if (!container) return;

    state.currentApp = explorer.createExplorerApp(container, createTransport(), {
      linkResolver: {
        evidenceLink: function (path, line) {
          return EVIDENCE_PREFIX + buildOpenFileAction(path, line);
        },
        documentLink: function (docId) {
          return "#/doc/" + encodeURIComponent(docId);
        },
        criterionLink: function (docId, _criterionId) {
          return "#/doc/" + encodeURIComponent(docId);
        },
      },
    }) || null;

    sendAction("ready");
  }

  window.__supersigilHostReady = function (initialContext) {
    ensureBooted();

    state.initialContext = parsePayload(initialContext);
    if (state.resolveInitialContext) {
      state.resolveInitialContext(state.initialContext);
      state.resolveInitialContext = null;
      state.initialContextPromise = null;
    }
  };

  window.__supersigilExplorerChanged = function (event) {
    var payload = parsePayload(event);
    for (var i = 0; i < state.changeListeners.length; i += 1) {
      state.changeListeners[i](payload);
    }
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
