/// <reference lib="dom" />
import {
  buildCriterionLink,
  buildDocumentLink,
  buildOpenFileCommandHref,
  type OpenGraphFileTarget,
} from "./explorerLinks";

export const EVIDENCE_SCHEME = "supersigil-evidence";

export function createLinkResolver(folderUri?: string) {
  return {
    evidenceLink: (file: string, line: number) =>
      buildOpenFileCommandHref({ path: file, line, folderUri }),
    documentLink: buildDocumentLink,
    criterionLink: buildCriterionLink,
  };
}

export const linkResolver = {
  evidenceLink: (file: string, line: number) =>
    `${EVIDENCE_SCHEME}:${encodeURIComponent(file)}?line=${line}`,
  documentLink: buildDocumentLink,
  criterionLink: buildCriterionLink,
};

export function parseEvidenceHref(
  href: string,
): { path: string; line: number | undefined } | null {
  const prefix = EVIDENCE_SCHEME + ":";
  if (!href.startsWith(prefix)) return null;

  const encoded = href.slice(prefix.length);
  const [filePart, query] = encoded.split("?");
  const line = query
    ? new URLSearchParams(query).get("line")
    : undefined;
  return {
    path: decodeURIComponent(filePart),
    line: line ? parseInt(line, 10) : undefined,
  };
}

interface DocumentOpenFileInfo {
  path: string;
  uri?: string;
}

function buildDocumentOpenFileTarget(
  info: DocumentOpenFileInfo,
  folderUri?: string,
): OpenGraphFileTarget | null {
  if (info.uri) {
    return { uri: info.uri };
  }

  if (!folderUri) {
    return null;
  }

  return { path: info.path, folderUri };
}

export function injectOpenFileButton(
  container: HTMLElement,
  fileByDocId: Map<string, DocumentOpenFileInfo>,
  folderUri?: string,
): void {
  const header = container.querySelector(".detail-panel-header");
  if (!header || header.querySelector(".open-file-btn")) return;

  const titleEl = header.querySelector(".detail-panel-title");
  if (!titleEl) return;

  const docId = titleEl.textContent?.trim();
  if (!docId) return;

  const info = fileByDocId.get(docId);
  if (!info) return;

  const target = buildDocumentOpenFileTarget(info, folderUri);
  if (!target) return;

  const btn = document.createElement("a");
  btn.className = "open-file-btn";
  btn.textContent = "Open File";
  btn.title = `Open ${info.path}`;
  btn.setAttribute("href", buildOpenFileCommandHref(target));
  header.insertBefore(btn, header.querySelector(".detail-panel-close"));
}

// ---------------------------------------------------------------------------
// Webview bootstrap (runs only inside the VS Code webview)
// ---------------------------------------------------------------------------

declare function acquireVsCodeApi(): {
  postMessage(msg: unknown): void;
  getState(): unknown;
  setState(state: unknown): void;
};

declare const SupersigilExplorer: {
  mount(
    container: HTMLElement,
    data: unknown,
    renderData: unknown,
    repositoryInfo: unknown,
    linkResolver?: unknown,
  ): { unmount: () => void } | undefined;
};

interface GraphDocument {
  id: string;
  path: string;
  file_uri?: string | null;
}

interface GraphData {
  documents: GraphDocument[];
  edges: unknown[];
}

if (typeof acquireVsCodeApi === "function") {
  const vscode = acquireVsCodeApi();
  const container = document.getElementById("explorer")!;

  let fileByDocId: Map<string, DocumentOpenFileInfo>;
  let currentUnmount: (() => void) | null = null;
  let detailObserver: MutationObserver | null = null;
  let currentRootUri: string | undefined;

  function mountExplorer(
    graph: GraphData,
    renderData: unknown[],
    focusDocumentId?: string,
    isRootSwitch?: boolean,
  ) {
    fileByDocId = new Map(
      graph.documents.map((d) => [
        d.id,
        {
          path: d.path,
          uri: d.file_uri ?? undefined,
        },
      ]),
    );

    // Hash selection: root switch clears state, focus sets target, otherwise preserve
    let targetHash: string | null;
    if (isRootSwitch) {
      targetHash = null;
    } else if (focusDocumentId) {
      targetHash = linkResolver.documentLink(focusDocumentId);
    } else {
      targetHash = window.location.hash || null;
    }

    // Clear hash before mount so the router doesn't pick up stale state
    if (targetHash === null) {
      history.replaceState(null, "", location.pathname + location.search);
    }

    if (detailObserver) {
      detailObserver.disconnect();
      detailObserver = null;
    }
    if (currentUnmount) currentUnmount();

    // Set hash before mount so the explorer's initial state parser picks it up
    if (targetHash) {
      window.location.hash = targetHash;
    }

    container.innerHTML = "";
    lastRootSelectorKey = "";
    const runtimeLinkResolver = createLinkResolver(currentRootUri);
    const handle = SupersigilExplorer.mount(
      container,
      graph,
      renderData,
      null,
      runtimeLinkResolver,
    );
    currentUnmount = handle?.unmount ?? null;

    observeDetailPanel();

    // Inject button for any detail panel already rendered by mount's hash restore
    injectOpenFileButton(container, fileByDocId, currentRootUri);
  }

  let lastRootSelectorKey = "";

  function updateRootSelector(
    currentRoot: { uri: string; name: string },
    availableRoots: Array<{ uri: string; name: string }>,
  ) {
    // Skip rebuild if roots and selection haven't changed
    const key = `${currentRoot.uri}|${availableRoots.map((r) => r.uri).join(",")}`;
    if (key === lastRootSelectorKey) {
      return;
    }
    lastRootSelectorKey = key;

    const existing = container.querySelector(".root-selector");
    if (existing) existing.remove();

    if (availableRoots.length <= 1) return;

    const bar = container.querySelector(".explorer-bar");
    if (!bar) return;

    const select = document.createElement("select");
    select.className = "root-selector";
    for (const root of availableRoots) {
      const opt = document.createElement("option");
      opt.value = root.uri;
      opt.textContent = root.name;
      opt.selected = root.uri === currentRoot.uri;
      select.appendChild(opt);
    }
    select.addEventListener("change", () => {
      vscode.postMessage({ type: "switchRoot", folderUri: select.value });
    });
    bar.prepend(select);
  }

  function observeDetailPanel() {
    detailObserver = new MutationObserver((mutations) => {
      // Only act when nodes are actually added (skip attribute-only mutations
      // from d3 simulation ticks which fire at 60fps)
      if (mutations.some((m) => m.addedNodes.length > 0)) {
        injectOpenFileButton(container, fileByDocId, currentRootUri);
      }
    });
    detailObserver.observe(container, { childList: true, subtree: true });
  }

  container.addEventListener("click", (e) => {
    const anchor = (e.target as HTMLElement).closest("a");
    if (!anchor) return;

    const href = anchor.getAttribute("href") ?? "";
    const parsed = parseEvidenceHref(href);
    if (parsed) {
      e.preventDefault();
      vscode.postMessage({
        type: "openFile",
        path: parsed.path,
        line: parsed.line,
      });
    }
  });

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (msg.type === "graphData") {
      currentRootUri = msg.currentRoot?.uri;
      mountExplorer(
        msg.graph,
        msg.renderData,
        msg.focusDocumentId,
        msg.isRootSwitch,
      );
      updateRootSelector(msg.currentRoot, msg.availableRoots);
      // Save state for webview serialization (VS Code restart recovery)
      if (msg.currentRoot) {
        vscode.setState({ clientKey: msg.currentRoot.uri });
      }
    }
  });

  vscode.postMessage({ type: "ready" });
}
