import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import {
  METHOD_EXPLORER_DOCUMENT,
  METHOD_EXPLORER_SNAPSHOT,
} from "./specExplorer";
import {
  OPEN_GRAPH_FILE_COMMAND,
  type OpenGraphFileTarget,
} from "./explorerLinks";

// ---------------------------------------------------------------------------
// LSP response types
// ---------------------------------------------------------------------------

export interface ExplorerDocumentSummary {
  id: string;
  doc_type: string;
  status: string | null;
  title: string;
  project: string | null;
  path: string;
  file_uri?: string | null;
  coverage_summary: { verified: number; total: number };
  component_count: number;
  graph_components: unknown[];
}

interface ExplorerSnapshotResult {
  revision: string;
  documents: ExplorerDocumentSummary[];
  edges: { from: string; to: string; kind: string }[];
}

interface ExplorerDocumentResult {
  revision: string;
  document_id: string;
  stale: boolean;
  fences: unknown[];
  edges: unknown[];
}

export interface ExplorerChangedEvent {
  rootId?: string;
  revision: string;
  changed_document_ids: string[];
  removed_document_ids: string[];
}

// ---------------------------------------------------------------------------
// Webview HTML assets
// ---------------------------------------------------------------------------

const CSS_FILES = [
  "landing-tokens.css",
  "explorer-styles.css",
  "supersigil-preview.css",
  "vscode-theme-adapter.css",
];

const JS_FILES = [
  "render-iife.js",
  "supersigil-preview.js",
  "explorer.js",
  "bootstrap.js",
];

// ---------------------------------------------------------------------------
// Multi-instance panel state
// ---------------------------------------------------------------------------

export interface ExplorerPanel {
  panel: vscode.WebviewPanel;
  folderUri: vscode.Uri;
  clientKey: string;
  pendingClientKey: string | null;
  staleWhileHidden: boolean;
  contextDirtyWhileHidden: boolean;
  ready: boolean;
  initialized: boolean;
  pendingFocusPath: string | null;
  pendingChangeEvents: Map<string, ExplorerChangedEvent>;
  availableRootsSignature: string;
}

export const openPanels: ExplorerPanel[] = [];

// ---------------------------------------------------------------------------
// openExplorerPanel — called by the command handler
// ---------------------------------------------------------------------------

export function openExplorerPanel(
  context: vscode.ExtensionContext,
  clients: Map<string, LanguageClient>,
  targetUri?: vscode.Uri,
): void {
  const activeUri = targetUri ?? vscode.window.activeTextEditor?.document.uri;
  let folderUri: vscode.Uri | null = null;
  let clientKey: string | null = null;

  if (activeUri) {
    const folder = vscode.workspace.getWorkspaceFolder(activeUri);
    if (folder) {
      const key = folder.uri.toString();
      const client = clients.get(key);
      if (client?.isRunning()) {
        folderUri = folder.uri;
        clientKey = key;
      }
    }
  }

  // Fallback: first running client
  if (!folderUri) {
    for (const [uriStr, client] of clients) {
      if (client.isRunning()) {
        folderUri = vscode.Uri.parse(uriStr);
        clientKey = uriStr;
        break;
      }
    }
  }

  // Fallback: first registered client (not yet running — panel will hydrate
  // when the client starts via refreshPanelsForClient)
  if (!folderUri) {
    for (const [uriStr] of clients) {
      folderUri = vscode.Uri.parse(uriStr);
      clientKey = uriStr;
      break;
    }
  }

  if (!folderUri || !clientKey) {
    vscode.window.showInformationMessage(
      "No Supersigil project found. Open a workspace with a supersigil.toml.",
    );
    return;
  }

  // Only auto-focus when the active file's workspace folder matches the
  // selected root. If the panel fell back to a different root, skip focus
  // to avoid matching a same-named path in the wrong workspace.
  const activeFolder = activeUri
    ? vscode.workspace.getWorkspaceFolder(activeUri)
    : undefined;
  const activeRelPath =
    activeUri &&
    activeFolder?.uri.toString() === clientKey &&
    isPotentialSpecDocumentPath(activeUri.path)
      ? vscode.workspace.asRelativePath(activeUri, false)
      : null;

  const folderName = folderNameForKey(clientKey);

  // Create panel
  const extensionUri = context.extensionUri;
  const webviewDistUri = vscode.Uri.joinPath(extensionUri, "dist", "webview");

  const panel = vscode.window.createWebviewPanel(
    "supersigil.explorer",
    `Spec Explorer (${folderName})`,
    vscode.ViewColumn.Beside,
    {
      enableCommandUris: [OPEN_GRAPH_FILE_COMMAND],
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [webviewDistUri],
    },
  );

  const nonce = generateNonce();
  panel.webview.html = getHtmlContent(panel.webview, nonce, extensionUri);

  wirePanel(panel, folderUri, clientKey, clients, activeRelPath);
}

/**
 * Restore a serialized explorer panel after VS Code restart.
 * Called by the WebviewPanelSerializer.
 */
export function restoreExplorerPanel(
  panel: vscode.WebviewPanel,
  state: { clientKey?: string },
  clients: Map<string, LanguageClient>,
  extensionUri: vscode.Uri,
): void {
  const clientKey = state.clientKey;
  if (!clientKey) {
    panel.dispose();
    return;
  }

  // Re-set the HTML so scripts re-execute (the old JS context is dead after reload)
  const nonce = generateNonce();
  panel.webview.html = getHtmlContent(panel.webview, nonce, extensionUri);

  wirePanel(panel, vscode.Uri.parse(clientKey), clientKey, clients, null);
}

function wirePanel(
  panel: vscode.WebviewPanel,
  folderUri: vscode.Uri,
  clientKey: string,
  clients: Map<string, LanguageClient>,
  pendingFocusPath: string | null,
): void {
  const entry: ExplorerPanel = {
    panel,
    folderUri,
    clientKey,
    pendingClientKey: null,
    staleWhileHidden: false,
    contextDirtyWhileHidden: false,
    ready: false,
    initialized: false,
    pendingFocusPath,
    pendingChangeEvents: new Map(),
    availableRootsSignature: "",
  };

  openPanels.push(entry);

  panel.webview.onDidReceiveMessage((msg: {
    type: string;
    path?: string;
    uri?: string;
    line?: number;
    rootId?: string;
    requestId?: number;
    method?: string;
    params?: {
      rootId?: string;
      revision?: string;
      documentId?: string;
    };
  }) => {
    if (msg.type === "ready") {
      entry.ready = true;
      if (panel.visible) {
        void postHostReady(entry, clients);
      } else {
        entry.staleWhileHidden = true;
      }
      return;
    }
    if (msg.type === "request" && typeof msg.requestId === "number") {
      void handleTransportRequest(entry, clients, {
        requestId: msg.requestId,
        method: msg.method,
        params: msg.params,
      });
      return;
    }
    if (msg.type === "commitRoot" && typeof msg.rootId === "string") {
      commitPanelRoot(entry, msg.rootId);
      return;
    }
    if (msg.type === "openFile") {
      handleOpenFile(msg, entry.folderUri);
    }
  });

  panel.onDidChangeViewState((e) => {
    if (e.webviewPanel.visible && entry.staleWhileHidden) {
      entry.staleWhileHidden = false;
      if (!entry.initialized) {
        void postHostReady(entry, clients);
      } else {
        const hadPendingContextRefresh = entry.contextDirtyWhileHidden;
        entry.contextDirtyWhileHidden = false;
        if (hadPendingContextRefresh) {
          postHostContextChanged(entry, clients);
        }
        const pendingEvents = [...entry.pendingChangeEvents.values()];
        entry.pendingChangeEvents.clear();
        if (pendingEvents.length === 0) {
          if (!hadPendingContextRefresh) {
            postExplorerChanged(entry, emptyExplorerChangedEvent());
          }
          return;
        }
        for (const pendingEvent of pendingEvents) {
          postExplorerChanged(entry, pendingEvent);
        }
      }
    }
  });

  panel.onDidDispose(() => {
    const idx = openPanels.indexOf(entry);
    if (idx >= 0) openPanels.splice(idx, 1);
  });
}

// ---------------------------------------------------------------------------
// refreshPanelsForClient — called from documentsChanged handler
// ---------------------------------------------------------------------------

export function refreshPanelsForClient(
  clientKey: string,
  clients: Map<string, LanguageClient>,
  event: ExplorerChangedEvent = emptyExplorerChangedEvent(),
): void {
  for (const entry of openPanels) {
    const rootsChanged = availableRootsSignature(availableRootsForClients(clients))
      !== entry.availableRootsSignature;
    if (rootsChanged) {
      if (!entry.ready) {
        continue;
      }
      if (!entry.panel.visible) {
        entry.staleWhileHidden = true;
        entry.contextDirtyWhileHidden = true;
      } else if (entry.initialized) {
        postHostContextChanged(entry, clients);
      }
    }
    if (entry.clientKey !== clientKey && entry.pendingClientKey !== clientKey) {
      continue;
    }
    if (!entry.ready) continue;
    if (!entry.panel.visible) {
      entry.staleWhileHidden = true;
      queuePendingChangeEvent(entry, {
        ...event,
        rootId: clientKey,
      });
      continue;
    }
    if (!entry.initialized) {
      void postHostReady(entry, clients);
      continue;
    }
    postExplorerChanged(entry, event, clientKey);
  }
}

function emptyExplorerChangedEvent(): ExplorerChangedEvent {
  return {
    revision: "",
    changed_document_ids: [],
    removed_document_ids: [],
  };
}

function mergeExplorerChangedEvents(
  current: ExplorerChangedEvent | null,
  next: ExplorerChangedEvent,
): ExplorerChangedEvent {
  if (!current) {
    return next;
  }
  const currentHasRevision = Boolean(current.revision);
  const nextHasRevision = Boolean(next.revision);
  if (!currentHasRevision && !nextHasRevision) {
    return {
      ...emptyExplorerChangedEvent(),
      rootId: next.rootId ?? current.rootId,
    };
  }
  if (!currentHasRevision) {
    return next;
  }
  if (!nextHasRevision) {
    return current;
  }
  return {
    rootId: next.rootId ?? current.rootId,
    revision: next.revision,
    changed_document_ids: [
      ...new Set([
        ...current.changed_document_ids,
        ...next.changed_document_ids,
      ]),
    ],
    removed_document_ids: [
      ...new Set([
        ...current.removed_document_ids,
        ...next.removed_document_ids,
      ]),
    ],
  };
}

function queuePendingChangeEvent(
  entry: ExplorerPanel,
  event: ExplorerChangedEvent,
): void {
  const rootId = event.rootId ?? entry.clientKey;
  const current = entry.pendingChangeEvents.get(rootId) ?? null;
  entry.pendingChangeEvents.set(
    rootId,
    mergeExplorerChangedEvents(current, {
      ...event,
      rootId,
    }),
  );
}

function availableRootsForClients(
  clients: Map<string, LanguageClient>,
): Array<{ id: string; name: string }> {
  const availableRoots: Array<{ id: string; name: string }> = [];
  for (const [uriStr, client] of clients) {
    if (client.isRunning()) {
      availableRoots.push({ id: uriStr, name: folderNameForKey(uriStr) });
    }
  }
  return availableRoots;
}

function availableRootsSignature(
  availableRoots: Array<{ id: string; name: string }>,
): string {
  return availableRoots
    .map((root) => `${root.id}\u0000${root.name}`)
    .join("\n");
}

async function postHostReady(
  entry: ExplorerPanel,
  clients: Map<string, LanguageClient>,
): Promise<void> {
  const client = clients.get(entry.clientKey);
  if (!client?.isRunning()) return;
  if (!openPanels.includes(entry)) return;

  entry.initialized = true;
  entry.contextDirtyWhileHidden = false;
  entry.pendingChangeEvents.clear();
  const availableRoots = availableRootsForClients(clients);
  entry.availableRootsSignature = availableRootsSignature(availableRoots);

  entry.panel.webview.postMessage({
    type: "hostReady",
    initialContext: {
      rootId: entry.clientKey,
      availableRoots,
      focusDocumentPath: entry.pendingFocusPath ?? undefined,
    },
  });
  entry.pendingFocusPath = null;
}

function postHostContextChanged(
  entry: ExplorerPanel,
  clients: Map<string, LanguageClient>,
): void {
  const availableRoots = availableRootsForClients(clients);
  entry.availableRootsSignature = availableRootsSignature(availableRoots);
  entry.panel.webview.postMessage({
    type: "hostContextChanged",
    context: {
      rootId: entry.clientKey,
      availableRoots,
    },
  });
}

function postExplorerChanged(
  entry: ExplorerPanel,
  event: ExplorerChangedEvent,
  rootId: string = event.rootId ?? entry.clientKey,
): void {
  entry.panel.webview.postMessage({
    type: "explorerChanged",
    event: {
      ...event,
      rootId,
    },
  });
}

function commitPanelRoot(entry: ExplorerPanel, targetKey: string): void {
  entry.pendingClientKey = null;
  if (targetKey === entry.clientKey) {
    return;
  }
  entry.folderUri = vscode.Uri.parse(targetKey);
  entry.clientKey = targetKey;
  entry.panel.title = `Spec Explorer (${folderNameForKey(targetKey)})`;
}

async function handleTransportRequest(
  entry: ExplorerPanel,
  clients: Map<string, LanguageClient>,
  msg: {
    requestId: number;
    method?: string;
    params?: {
      rootId?: string;
      revision?: string;
      documentId?: string;
    };
  },
): Promise<void> {
  const targetKey = msg.params?.rootId ?? entry.clientKey;
  try {
    if (msg.method === "loadSnapshot") {
      const client = clients.get(targetKey);
      if (!client?.isRunning()) {
        throw new Error(`No running Supersigil client for ${targetKey}`);
      }
      if (targetKey !== entry.clientKey) {
        entry.pendingClientKey = targetKey;
      }

      const snapshot = await client.sendRequest<ExplorerSnapshotResult>(
        METHOD_EXPLORER_SNAPSHOT,
      );
      if (!openPanels.includes(entry)) return;

      entry.panel.webview.postMessage({
        type: "response",
        requestId: msg.requestId,
        result: snapshot,
      });
      return;
    }

    if (msg.method === "loadDocument") {
      const client = clients.get(targetKey);
      if (!client?.isRunning()) {
        throw new Error(`No running Supersigil client for ${targetKey}`);
      }

      const document = await client.sendRequest<ExplorerDocumentResult>(
        METHOD_EXPLORER_DOCUMENT,
        {
          document_id: msg.params?.documentId ?? "",
          revision: msg.params?.revision ?? "",
        },
      );
      if (!openPanels.includes(entry)) return;

      entry.panel.webview.postMessage({
        type: "response",
        requestId: msg.requestId,
        result: document,
      });
      return;
    }

    entry.panel.webview.postMessage({
      type: "response",
      requestId: msg.requestId,
      error: `Unsupported explorer request: ${msg.method ?? "unknown"}`,
    });
  } catch (err) {
    if (msg.method === "loadSnapshot" && entry.pendingClientKey === targetKey) {
      entry.pendingClientKey = null;
    }
    entry.panel.webview.postMessage({
      type: "response",
      requestId: msg.requestId,
      error: err instanceof Error ? err.message : String(err),
    });
  }
}

// ---------------------------------------------------------------------------
// handleOpenFile
// ---------------------------------------------------------------------------

export function openGraphFile(target: OpenGraphFileTarget): void {
  openGraphFileWithOptions(target, {});
}

export function openGraphFileWithOptions(
  target: OpenGraphFileTarget,
  showOptions: vscode.TextDocumentShowOptions,
): void {
  if (target.uri) {
    handleOpenFile(target, undefined, showOptions);
    return;
  }

  if (!target.path || !target.folderUri) {
    return;
  }

  handleOpenFile(target, vscode.Uri.parse(target.folderUri), showOptions);
}

function handleOpenFile(
  msg: { path?: string; uri?: string; line?: number },
  folderUri?: vscode.Uri,
  showOptions: vscode.TextDocumentShowOptions = {},
): void {
  if (msg.uri) {
    const fileUri = vscode.Uri.parse(msg.uri);
    if (!fileUri.toString().startsWith("file:")) {
      vscode.window.showWarningMessage(
        `Supersigil: Blocked navigation to unsupported URI: ${msg.uri}`,
      );
      return;
    }
    openFileAtUri(fileUri, msg, showOptions);
    return;
  }

  if (!msg.path || !folderUri) return;
  if (!isPathSafe(msg.path)) {
    vscode.window.showWarningMessage(
      `Supersigil: Blocked navigation to unsafe path: ${msg.path}`,
    );
    return;
  }

  const fileUri = vscode.Uri.joinPath(folderUri, msg.path);
  if (!fileUri.fsPath.startsWith(folderUri.fsPath)) {
    vscode.window.showWarningMessage(
      `Supersigil: Blocked navigation outside workspace: ${msg.path}`,
    );
    return;
  }
  openFileAtUri(fileUri, msg, showOptions);
}

function openFileAtUri(
  fileUri: vscode.Uri,
  msg: { path?: string; line?: number },
  showOptions: vscode.TextDocumentShowOptions,
): void {
  const openOpts: vscode.TextDocumentShowOptions = { ...showOptions };
  if (msg.line !== undefined && Number.isFinite(msg.line) && msg.line > 0) {
    const line = msg.line - 1;
    const pos = new vscode.Position(line, 0);
    openOpts.selection = new vscode.Range(pos, pos);
  }
  vscode.workspace.openTextDocument(fileUri).then(
    (doc) => vscode.window.showTextDocument(doc, openOpts),
    () => vscode.window.showWarningMessage(`Could not open file: ${msg.path}`),
  );
}

// ---------------------------------------------------------------------------
// getHtmlContent
// ---------------------------------------------------------------------------

function getHtmlContent(
  webview: vscode.Webview,
  nonce: string,
  extensionUri: vscode.Uri,
): string {
  const distUri = vscode.Uri.joinPath(extensionUri, "dist", "webview");

  const cssLinks = CSS_FILES.map((file) => {
    const uri = webview.asWebviewUri(vscode.Uri.joinPath(distUri, file));
    return `<link rel="stylesheet" href="${uri}">`;
  }).join("\n    ");

  const scriptTags = JS_FILES.map((file) => {
    const uri = webview.asWebviewUri(vscode.Uri.joinPath(distUri, file));
    return `<script nonce="${nonce}" src="${uri}"></script>`;
  }).join("\n    ");

  return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <!-- 'unsafe-inline' for style-src is required because d3 sets inline styles on SVG elements -->
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}'; img-src ${webview.cspSource} data:;">
    ${cssLinks}
    <title>Spec Explorer</title>
</head>
<body>
    <div id="explorer" style="height: 100vh;"></div>
    ${scriptTags}
</body>
</html>`;
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

function generateNonce(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let result = "";
  for (let i = 0; i < 32; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result;
}

function folderNameForKey(key: string): string {
  return vscode.workspace.workspaceFolders?.find(
    (f) => f.uri.toString() === key,
  )?.name ?? "workspace";
}

/** Reject paths that attempt to escape the workspace root. */
function isPathSafe(p: string): boolean {
  if (p.startsWith("/") || p.startsWith("\\")) return false;
  const segments = p.split(/[/\\]/);
  return !segments.includes("..");
}

function isPotentialSpecDocumentPath(path: string): boolean {
  return path.endsWith(".md") || path.endsWith(".mdx");
}
