/**
 * Supersigil Preview - Client-side interactivity
 *
 * Provides:
 * 1. Collapsible evidence sections (click .supersigil-evidence-toggle)
 * 2. Badge tooltips (title attribute, native browser tooltip)
 * 3. MutationObserver to re-initialize when new .supersigil-block nodes appear
 *
 * Zero dependencies. Works in any browser environment (VS Code webview,
 * IntelliJ JCEF, standard browsers).
 */
(function () {
  "use strict";

  // -------------------------------------------------------------------------
  // Collapsible evidence sections
  // -------------------------------------------------------------------------

  /**
   * Initialize collapsible behavior on a toggle button.
   * Skips buttons that have already been initialized.
   */
  function initToggle(button) {
    if (button.dataset.supersigilInit) return;
    button.dataset.supersigilInit = "true";

    button.addEventListener("click", function () {
      var list = button.nextElementSibling;
      if (!list || !list.classList.contains("supersigil-evidence-list")) return;

      var expanded = button.getAttribute("aria-expanded") === "true";
      button.setAttribute("aria-expanded", String(!expanded));

      if (expanded) {
        list.setAttribute("hidden", "");
      } else {
        list.removeAttribute("hidden");
      }
    });
  }

  // -------------------------------------------------------------------------
  // Initialize all existing elements
  // -------------------------------------------------------------------------

  function initAll(root) {
    var toggles = root.querySelectorAll(".supersigil-evidence-toggle");
    for (var i = 0; i < toggles.length; i++) {
      initToggle(toggles[i]);
    }
  }

  // -------------------------------------------------------------------------
  // MutationObserver for dynamically added content
  // -------------------------------------------------------------------------

  function setupObserver() {
    if (typeof MutationObserver === "undefined") return;

    var observer = new MutationObserver(function (mutations) {
      for (var i = 0; i < mutations.length; i++) {
        var addedNodes = mutations[i].addedNodes;
        for (var j = 0; j < addedNodes.length; j++) {
          var node = addedNodes[j];
          if (node.nodeType !== 1) continue; // Element nodes only

          // If the added node is a supersigil block, initialize it
          if (node.classList && node.classList.contains("supersigil-block")) {
            initAll(node);
          }

          // If the added node contains supersigil blocks, initialize them
          if (node.querySelectorAll) {
            var blocks = node.querySelectorAll(".supersigil-block");
            for (var k = 0; k < blocks.length; k++) {
              initAll(blocks[k]);
            }
          }
        }
      }
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true,
    });

    return observer;
  }

  // -------------------------------------------------------------------------
  // Boot
  // -------------------------------------------------------------------------

  function boot() {
    initAll(document);
    setupObserver();
  }

  // Run on DOMContentLoaded or immediately if already loaded
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
