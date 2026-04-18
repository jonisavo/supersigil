import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import { METHOD_GRAPH_DATA, METHOD_DOCUMENT_COMPONENTS } from "./specExplorer";
import {
  OPEN_GRAPH_FILE_COMMAND,
  type OpenGraphFileTarget,
} from "./explorerLinks";

// ---------------------------------------------------------------------------
// LSP response types
// ---------------------------------------------------------------------------

export interface GraphDocument {
  id: string;
  doc_type: string;
  status: string | null;
  title: string;
  project: string | null;
  path: string;
  file_uri?: string | null;
  components: unknown[];
}

interface GraphDataResult {
  documents: GraphDocument[];
  edges: { from: string; to: string; kind: string }[];
}

interface DocumentComponentsResult {
  document_id: string;
  stale: boolean;
  fences: unknown[];
  edges: unknown[];
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

const CONCURRENCY_LIMIT = 10;

// ---------------------------------------------------------------------------
// Multi-instance panel state
// ---------------------------------------------------------------------------

export interface ExplorerPanel {
  panel: vscode.WebviewPanel;
  folderUri: vscode.Uri;
  clientKey: string;
  refreshGeneration: number;
  staleWhileHidden: boolean;
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
    activeUri && activeFolder?.uri.toString() === clientKey
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
    refreshGeneration: 0,
    staleWhileHidden: false,
  };

  let focusPath = pendingFocusPath;

  openPanels.push(entry);

  panel.webview.onDidReceiveMessage((msg: { type: string; path?: string; uri?: string; line?: number; folderUri?: string }) => {
    if (msg.type === "ready") {
      if (panel.visible) {
        pushData(entry, clients, { focusPath });
        focusPath = null;
      } else {
        entry.staleWhileHidden = true;
      }
      return;
    }
    if (msg.type === "switchRoot") {
      const newClientKey = msg.folderUri!;
      const newClient = clients.get(newClientKey);
      if (!newClient?.isRunning()) return;
      pushData(entry, clients, { switchToKey: newClientKey });
      return;
    }
    if (msg.type === "openFile") {
      handleOpenFile(msg, entry.folderUri);
    }
  });

  panel.onDidChangeViewState((e) => {
    if (e.webviewPanel.visible && entry.staleWhileHidden) {
      entry.staleWhileHidden = false;
      pushData(entry, clients, { focusPath });
      focusPath = null;
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
): void {
  for (const entry of openPanels) {
    if (entry.clientKey !== clientKey) continue;
    if (entry.panel.visible) {
      pushData(entry, clients);
    } else {
      entry.staleWhileHidden = true;
    }
  }
}

// ---------------------------------------------------------------------------
// pushData — fetches graph data and posts to a single panel
// ---------------------------------------------------------------------------

interface PushDataOptions {
  focusPath?: string | null;
  /** When set, perform an atomic root switch: fetch using this key,
   *  then update entry.folderUri/clientKey/title on success. */
  switchToKey?: string;
}

async function pushData(
  entry: ExplorerPanel,
  clients: Map<string, LanguageClient>,
  options?: PushDataOptions,
): Promise<void> {
  const isSwitch = !!options?.switchToKey;
  const targetKey = options?.switchToKey ?? entry.clientKey;
  const targetFolderUri = isSwitch ? vscode.Uri.parse(targetKey) : entry.folderUri;

  const client = clients.get(targetKey);
  if (!client?.isRunning()) return;

  const generation = ++entry.refreshGeneration;

  let graphData: GraphDataResult;
  try {
    graphData = await client.sendRequest<GraphDataResult>(METHOD_GRAPH_DATA);
  } catch (err) {
    vscode.window.showErrorMessage(
      `Supersigil: Failed to load graph data. ${err instanceof Error ? err.message : String(err)}`,
    );
    return;
  }

  if (generation !== entry.refreshGeneration) return;

  let focusDocumentId: string | undefined;
  if (options?.focusPath) {
    const doc = graphData.documents.find((d) => d.path === options.focusPath);
    if (doc) focusDocumentId = doc.id;
  }

  const renderData = await fetchAllComponents(client, graphData.documents, targetFolderUri);

  if (generation !== entry.refreshGeneration) return;
  if (!openPanels.includes(entry)) return;

  // Commit root switch only after data is ready
  if (isSwitch) {
    entry.folderUri = targetFolderUri;
    entry.clientKey = targetKey;
    entry.panel.title = `Spec Explorer (${folderNameForKey(targetKey)})`;
  }

  const availableRoots: Array<{ uri: string; name: string }> = [];
  for (const [uriStr, c] of clients) {
    if (c.isRunning()) {
      availableRoots.push({ uri: uriStr, name: folderNameForKey(uriStr) });
    }
  }

  entry.panel.webview.postMessage({
    type: "graphData",
    graph: graphData,
    renderData,
    currentRoot: {
      uri: isSwitch ? targetKey : entry.clientKey,
      name: folderNameForKey(isSwitch ? targetKey : entry.clientKey),
    },
    availableRoots,
    focusDocumentId,
    isRootSwitch: isSwitch,
  });
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

/**
 * Fetch documentComponents for a list of documents with bounded concurrency.
 * Individual failures are tolerated — only successful results are returned.
 */
async function fetchAllComponents(
  client: LanguageClient,
  documents: GraphDocument[],
  folderUri: vscode.Uri,
): Promise<DocumentComponentsResult[]> {
  const results: DocumentComponentsResult[] = [];
  let idx = 0;

  async function worker(): Promise<void> {
    while (idx < documents.length) {
      const doc = documents[idx++];
      const docUri = doc.file_uri
        ? vscode.Uri.parse(doc.file_uri)
        : vscode.Uri.joinPath(folderUri, doc.path);
      try {
        const result = await client.sendRequest<DocumentComponentsResult>(
          METHOD_DOCUMENT_COMPONENTS,
          { uri: docUri.toString() },
        );
        results.push(result);
      } catch (err) {
        void err;
        // Individual failures are tolerated.
      }
    }
  }

  const workers = Array.from(
    { length: Math.min(CONCURRENCY_LIMIT, documents.length) },
    () => worker(),
  );
  await Promise.all(workers);
  return results;
}
