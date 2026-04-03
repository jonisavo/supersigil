/**
 * Supersigil Bridge for IntelliJ JCEF Markdown Preview
 *
 * This script is injected into the JCEF-based Markdown preview by
 * SupersigilPreviewExtensionProvider. It:
 *
 * 1. Imports renderComponentTree from the co-injected render.js module.
 * 2. On page load and MutationObserver updates, requests component data
 *    from the JVM side via the injected query function.
 * 3. Finds <code class="language-supersigil-xml"> elements and replaces
 *    them with rendered components.
 * 4. Produces javascript: URIs for navigation that call back to the JVM.
 *
 * The JVM side injects two global functions before this script runs:
 * - window.__supersigilQuery(request, onSuccess, onFailure)
 *   Sends a JSON request string to the JVM and receives a JSON response.
 * - window.__supersigilAction(action)
 *   Sends a navigation action string to the JVM (fire-and-forget).
 *
 * Communication protocol:
 * - Query request:  { "type": "documentComponents", "uri": "<uri>" }
 *   Query response: DocumentComponentsResult JSON
 * - Action string:  "open-file:<path>:<line>" or
 *                   "open-criterion:<docId>:<criterionId>"
 */
(function () {
  "use strict";

  // -------------------------------------------------------------------------
  // renderComponentTree import (loaded as a sibling script via ES module)
  // We wait for it to be available on window.__supersigilRender
  // -------------------------------------------------------------------------

  // The render.js is loaded as an ES module; we re-export it to window.
  // Since JCEF may not support ES modules natively in injected scripts,
  // render.js is adapted: the Kotlin side wraps it to expose the function
  // on window.__supersigilRender.

  // -------------------------------------------------------------------------
  // State
  // -------------------------------------------------------------------------

  var lastData = null;
  var currentUri = null;
  var rendering = false;

  // -------------------------------------------------------------------------
  // Link resolver for IntelliJ
  // -------------------------------------------------------------------------

  function createLinkResolver() {
    // Use data-supersigil-action attributes instead of javascript: URIs
    // (JCEF blocks javascript: protocol in link clicks). The bridge
    // attaches click handlers to intercept these.
    return {
      evidenceLink: function (file, line) {
        return "#" + "supersigil-action:" + encodeAction("open-file:" + file + ":" + line);
      },
      documentLink: function (docId) {
        return "#" + "supersigil-action:" + encodeAction("open-criterion:" + docId + ":");
      },
      criterionLink: function (docId, criterionId) {
        return "#" + "supersigil-action:" + encodeAction("open-criterion:" + docId + ":" + criterionId);
      }
    };
  }

  /**
   * Encode a value for embedding in an action hash.
   */
  function encodeAction(value) {
    return encodeURIComponent(value);
  }

  /**
   * Attach a delegated click handler to intercept supersigil action links.
   */
  function attachClickHandler() {
    document.addEventListener("click", function (e) {
      var target = e.target;
      while (target && target.tagName !== "A") {
        target = target.parentElement;
      }
      if (!target) return;

      var href = target.getAttribute("href");
      if (!href || href.indexOf("#supersigil-action:") !== 0) return;

      e.preventDefault();
      e.stopPropagation();

      var encoded = href.substring("#supersigil-action:".length);
      var action = decodeURIComponent(encoded);

      if (window.__supersigilAction) {
        window.__supersigilAction(action);
      }
    }, true);
  }

  // -------------------------------------------------------------------------
  // DOM replacement
  // -------------------------------------------------------------------------

  /**
   * Find all <code class="language-supersigil-xml"> elements in the
   * document and replace each with rendered component HTML.
   */
  function replaceCodeFences(data) {
    if (!data || !data.fences) return;
    if (!window.__supersigilRender || !window.__supersigilRender.renderComponentTree) return;

    var renderComponentTree = window.__supersigilRender.renderComponentTree;
    var resolver = createLinkResolver();

    // Find all supersigil-xml code elements. The Markdown preview renders
    // fenced code blocks as <pre><code class="language-supersigil-xml">
    var codeElements = document.querySelectorAll("code.language-supersigil-xml");

    for (var i = 0; i < codeElements.length; i++) {
      var codeEl = codeElements[i];
      var preEl = codeEl.parentElement;
      if (!preEl || preEl.tagName !== "PRE") continue;

      // Skip already-replaced elements
      if (preEl.dataset.supersigilReplaced) continue;

      var fence = i < data.fences.length ? data.fences[i] : null;
      if (!fence) continue;

      // Render components for this fence
      // Include edges only after the last fence
      var edges = (i === codeElements.length - 1) ? (data.edges || []) : [];
      var html = renderComponentTree([fence], edges, resolver);

      if (html) {
        var container = document.createElement("div");
        container.className = "supersigil-rendered";
        container.innerHTML = html;

        // Mark as stale if needed
        if (data.stale) {
          container.classList.add("supersigil-stale");
        }

        preEl.dataset.supersigilReplaced = "true";
        preEl.style.display = "none";
        preEl.parentElement.insertBefore(container, preEl.nextSibling);
      }
    }
  }

  // -------------------------------------------------------------------------
  // Data fetching
  // -------------------------------------------------------------------------

  /**
   * Detect the current document URI from the page.
   * IntelliJ's markdown preview includes the file path in the page context.
   */
  function detectDocumentUri() {
    // IntelliJ's JCEF markdown preview sets the document URI on the page.
    // We can access it via the injected __supersigilDocUri variable set by
    // the resource provider, or extract it from the page URL.
    if (window.__supersigilDocUri) {
      return window.__supersigilDocUri;
    }
    // Fallback: try to extract from window.location or page metadata
    return null;
  }

  function fetchAndRender() {
    if (rendering) return;
    var uri = detectDocumentUri();
    if (!uri) return;

    currentUri = uri;
    rendering = true;

    if (!window.__supersigilQuery) {
      rendering = false;
      return;
    }

    var request = JSON.stringify({ type: "documentComponents", uri: uri });

    window.__supersigilQuery(
      request,
      function (response) {
        rendering = false;
        try {
          var data = JSON.parse(response);
          lastData = data;
          replaceCodeFences(data);
        } catch (e) {
          // Ignore parse errors
        }
      },
      function (errorCode, errorMessage) {
        rendering = false;
      }
    );
  }

  // -------------------------------------------------------------------------
  // MutationObserver
  // -------------------------------------------------------------------------

  function setupObserver() {
    if (typeof MutationObserver === "undefined") return;

    var debounceTimer = null;

    var observer = new MutationObserver(function (mutations) {
      // Check if any added nodes contain supersigil-xml code elements
      var hasNewFences = false;
      for (var i = 0; i < mutations.length; i++) {
        var addedNodes = mutations[i].addedNodes;
        for (var j = 0; j < addedNodes.length; j++) {
          var node = addedNodes[j];
          if (node.nodeType !== 1) continue;

          if (node.querySelector && node.querySelector("code.language-supersigil-xml")) {
            hasNewFences = true;
            break;
          }
          if (node.classList && node.classList.contains("language-supersigil-xml")) {
            hasNewFences = true;
            break;
          }
        }
        if (hasNewFences) break;
      }

      if (hasNewFences) {
        // Debounce: wait for the preview to finish updating
        if (debounceTimer) clearTimeout(debounceTimer);
        debounceTimer = setTimeout(function () {
          debounceTimer = null;
          // If we have cached data, try rendering with it first
          if (lastData) {
            replaceCodeFences(lastData);
          }
          // Then fetch fresh data
          fetchAndRender();
        }, 100);
      }
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true
    });

    return observer;
  }

  // -------------------------------------------------------------------------
  // Boot
  // -------------------------------------------------------------------------

  function boot() {
    // Attach click handler for supersigil action links
    attachClickHandler();
    // Initial render with any cached data
    fetchAndRender();
    // Watch for DOM changes (preview updates)
    setupObserver();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
