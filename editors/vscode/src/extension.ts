import { execSync } from "child_process";
import { accessSync, constants, existsSync, statSync } from "fs";
import { homedir } from "os";
import { join } from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import {
  METHOD_DOCUMENT_LIST,
  METHOD_DOCUMENTS_CHANGED,
  SpecExplorerProvider,
  DocumentEntry,
} from "./specExplorer";
import { PreviewCache } from "./previewCache";

const clients = new Map<string, LanguageClient>();
let statusBarItem: vscode.StatusBarItem;
let specExplorer: SpecExplorerProvider;
let previewCache: PreviewCache;
let notFoundShown = false;
let outputChannel: vscode.OutputChannel;

/** Shared cache for documentList results, keyed by document ID. */
const documentListCache = new Map<string, DocumentEntry>();

/** Track fence index per-document during a single markdown-it render pass. */
const fenceIndexByUri = new Map<string, number>();

function resolveServerBinary(): string | undefined {
  const config = vscode.workspace.getConfiguration("supersigil.lsp");
  const configuredPath = config.get<string | null>("serverPath", null);

  if (configuredPath) {
    try {
      const stat = statSync(configuredPath);
      if (!stat.isFile()) {
        vscode.window.showErrorMessage(
          `Supersigil LSP server at configured path: ${configuredPath} (not a file)`,
        );
        return undefined;
      }
      accessSync(configuredPath, constants.X_OK);
      return configuredPath;
    } catch {
      const reason = existsSync(configuredPath)
        ? "path exists but is not executable"
        : "file not found";
      vscode.window.showErrorMessage(
        `Supersigil LSP server at configured path: ${configuredPath} (${reason})`,
      );
      return undefined;
    }
  }

  try {
    const cmd =
      process.platform === "win32"
        ? "where.exe supersigil-lsp"
        : "which supersigil-lsp";
    return execSync(cmd, { encoding: "utf-8" }).trim();
  } catch {
    // Not on $PATH
  }

  const home = homedir();
  const candidates = [
    join(home, ".cargo", "bin", "supersigil-lsp"),
    join(home, ".local", "bin", "supersigil-lsp"),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  if (!notFoundShown) {
    notFoundShown = true;
    vscode.window
      .showInformationMessage(
        "Supersigil LSP server not found. Install with `cargo install supersigil-lsp` or configure `supersigil.lsp.serverPath`.",
        "Open Settings",
      )
      .then((action) => {
        if (action === "Open Settings") {
          vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "supersigil.lsp.serverPath",
          );
        }
      });
  }

  return undefined;
}

function updateNoRootsContext(): void {
  vscode.commands.executeCommand(
    "setContext",
    "supersigil.noRoots",
    clients.size === 0,
  );
}

function updateStatusBar(): void {
  if (clients.size === 0) {
    statusBarItem.text = "$(warning) Supersigil";
    statusBarItem.backgroundColor = new vscode.ThemeColor(
      "statusBarItem.warningBackground",
    );
    statusBarItem.tooltip =
      "Supersigil LSP is not running. Click to restart.";
    statusBarItem.show();
    return;
  }

  let allRunning = true;
  for (const client of clients.values()) {
    if (!client.isRunning()) {
      allRunning = false;
      break;
    }
  }

  if (allRunning) {
    const label =
      clients.size === 1 ? "Supersigil" : `Supersigil (${clients.size})`;
    statusBarItem.text = label;
    statusBarItem.backgroundColor = undefined;
    statusBarItem.tooltip = `Supersigil LSP running for ${clients.size} root(s)`;
  } else {
    statusBarItem.text = "$(warning) Supersigil";
    statusBarItem.backgroundColor = new vscode.ThemeColor(
      "statusBarItem.warningBackground",
    );
    statusBarItem.tooltip =
      "Supersigil LSP: some instances are not running. Click to restart.";
  }
  statusBarItem.show();
}

/** Find the client responsible for a given file URI. */
function clientForUri(uri: vscode.Uri): LanguageClient | undefined {
  const folder = vscode.workspace.getWorkspaceFolder(uri);
  if (folder) {
    return clients.get(folder.uri.toString());
  }
  return undefined;
}

/** Find workspace folders that contain a supersigil.toml. */
function findSupersigilRoots(): vscode.WorkspaceFolder[] {
  const folders = vscode.workspace.workspaceFolders ?? [];
  return folders.filter((f) =>
    existsSync(join(f.uri.fsPath, "supersigil.toml")),
  );
}

async function startClientForFolder(
  folder: vscode.WorkspaceFolder,
  serverPath: string,
): Promise<void> {
  const key = folder.uri.toString();
  if (clients.has(key)) {
    return;
  }

  const serverOptions: ServerOptions = {
    command: serverPath,
    transport: TransportKind.stdio,
  };

  const outputChannel = vscode.window.createOutputChannel(
    `Supersigil LSP (${folder.name})`,
  );

  // Watch for .md, .mdx, and supersigil.toml changes on disk (git
  // operations, branch switches, external edits) so the LSP re-indexes.
  const mdWatcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(folder, "**/*.md"),
  );
  const mdxWatcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(folder, "**/*.mdx"),
  );
  const configWatcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(folder, "supersigil.toml"),
  );

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      {
        scheme: "file",
        language: "markdown",
        pattern: `${folder.uri.fsPath}/**/*`,
      },
      {
        scheme: "file",
        language: "mdx",
        pattern: `${folder.uri.fsPath}/**/*`,
      },
    ],
    workspaceFolder: folder,
    outputChannel,
    synchronize: {
      fileEvents: [mdWatcher, mdxWatcher, configWatcher],
    },
  };

  const client = new LanguageClient(
    `supersigil-${folder.name}`,
    `Supersigil (${folder.name})`,
    serverOptions,
    clientOptions,
  );

  client.onDidChangeState(() => updateStatusBar());

  // Refresh the Spec Explorer tree and preview cache when the LSP re-indexes.
  client.onNotification(METHOD_DOCUMENTS_CHANGED, () => {
    specExplorer?.refresh();
    previewCache?.invalidateAll();

    // Populate the shared documentListCache so link resolution and
    // goToCriterion work even before the tree view is expanded.
    if (client.isRunning()) {
      client
        .sendRequest<{ documents: DocumentEntry[] }>(METHOD_DOCUMENT_LIST)
        .then((result) => {
          previewCache?.updateDocumentList(result.documents);
        })
        .catch(() => {
          // Best-effort; tree view will also populate on expand.
        });
    }
  });

  clients.set(key, client);

  try {
    await client.start();
  } catch {
    // Status bar will reflect the error state
  }
  updateStatusBar();
  updateNoRootsContext();
  specExplorer?.refresh();
}

async function startAllClients(
  context: vscode.ExtensionContext,
): Promise<void> {
  const serverPath = resolveServerBinary();
  if (!serverPath) {
    updateStatusBar();
    return;
  }

  const roots = findSupersigilRoots();
  await Promise.all(
    roots.map((folder) => startClientForFolder(folder, serverPath)),
  );
  updateStatusBar();
}

async function stopAllClients(): Promise<void> {
  const stops = Array.from(clients.values()).map((c) => c.stop());
  await Promise.all(stops);
  clients.clear();
  updateNoRootsContext();
  specExplorer?.refresh();
}

async function showStatusMenu(
  context: vscode.ExtensionContext,
): Promise<void> {
  const items: vscode.QuickPickItem[] = [];

  if (clients.size === 0) {
    items.push({
      label: "$(circle-slash) No supersigil roots found",
      description: "No workspace folder contains supersigil.toml",
    });
  } else {
    for (const [key, client] of clients) {
      const folder = vscode.workspace.workspaceFolders?.find(
        (f) => f.uri.toString() === key,
      );
      const name = folder?.name ?? key;
      const running = client.isRunning();
      const icon = running ? "$(check)" : "$(warning)";
      const state = running ? "running" : "stopped";
      items.push({
        label: `${icon} ${name}`,
        description: state,
      });
    }
  }

  const diagCount = vscode.languages
    .getDiagnostics()
    .filter(([, diags]) => diags.some((d) => d.source === "supersigil"))
    .length;
  if (diagCount > 0) {
    items.push({ label: "", kind: vscode.QuickPickItemKind.Separator });
    items.push({
      label: `$(issues) ${diagCount} file(s) with diagnostics`,
      description: "from supersigil",
    });
  }

  items.push({ label: "", kind: vscode.QuickPickItemKind.Separator });
  items.push({
    label: "$(debug-restart) Restart Server",
    description: "Stop and restart all LSP instances",
  });
  items.push({
    label: "$(output) Show Output",
    description: "Open the LSP output channel",
  });

  const picked = await vscode.window.showQuickPick(items, {
    title: "Supersigil",
    placeHolder: "Server status and actions",
  });

  if (!picked) return;

  if (picked.label.includes("Restart Server")) {
    await stopAllClients();
    await startAllClients(context);
  } else if (picked.label.includes("Show Output")) {
    const first = clients.values().next().value;
    if (first) {
      first.outputChannel.show();
    }
  }
}

// ---------------------------------------------------------------------------
// Criterion navigation command
// ---------------------------------------------------------------------------

async function goToCriterion(
  docId: string,
  criterionId: string,
): Promise<void> {
  // Look up the target document's file path from the documentList cache
  const entry = documentListCache.get(docId);
  if (!entry) {
    vscode.window.showWarningMessage(
      `Document "${docId}" not found in the spec index.`,
    );
    return;
  }

  // Find the workspace folder for this document
  const folders = vscode.workspace.workspaceFolders ?? [];
  let targetUri: vscode.Uri | undefined;
  for (const folder of folders) {
    const candidate = vscode.Uri.joinPath(folder.uri, entry.path);
    if (existsSync(candidate.fsPath)) {
      targetUri = candidate;
      break;
    }
  }

  if (!targetUri) {
    vscode.window.showWarningMessage(
      `Could not resolve file path for document "${docId}".`,
    );
    return;
  }

  // Open beside with preserveFocus so the Markdown preview stays on the source
  const doc = await vscode.workspace.openTextDocument(targetUri);
  const editor = await vscode.window.showTextDocument(doc, {
    viewColumn: vscode.ViewColumn.Beside,
    preserveFocus: true,
    preview: true,
  });

  // Search for the criterion ID in the document text to navigate to it
  const text = doc.getText();
  const searchPattern = `id="${criterionId}"`;
  const idx = text.indexOf(searchPattern);
  if (idx >= 0) {
    const pos = doc.positionAt(idx);
    const range = new vscode.Range(pos, pos);
    editor.selection = new vscode.Selection(pos, pos);
    editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
  }
}

// ---------------------------------------------------------------------------
// Markdown-it plugin: extendMarkdownIt
// ---------------------------------------------------------------------------

interface MarkdownItToken {
  type: string;
  info: string;
  content: string;
  map: [number, number] | null;
}

interface MarkdownItEnv {
  currentDocument?: vscode.Uri;
  [key: string]: unknown;
}

/**
 * Create the `extendMarkdownIt` return value for VS Code's
 * markdown.markdownItPlugins contribution.
 */
function createMarkdownItExtender(cache: PreviewCache) {
  return {
    extendMarkdownIt(md: {
      renderer: {
        rules: {
          fence: (
            tokens: MarkdownItToken[],
            idx: number,
            options: unknown,
            env: MarkdownItEnv,
            self: unknown,
          ) => string;
        };
      };
    }) {
      const defaultFence = md.renderer.rules.fence.bind(md.renderer.rules);

      md.renderer.rules.fence = (
        tokens: MarkdownItToken[],
        idx: number,
        options: unknown,
        env: MarkdownItEnv,
        self: unknown,
      ): string => {
        const token = tokens[idx];
        if (token.info.trim() === "supersigil-xml") {
          // Determine the document URI from the env
          const documentUri = resolveDocumentUri(env);
          if (!documentUri) {
            return defaultFence(tokens, idx, options, env, self);
          }

          // Track fence index for document-order correlation
          const uriStr = documentUri;
          const fenceIdx = fenceIndexByUri.get(uriStr) ?? 0;
          fenceIndexByUri.set(uriStr, fenceIdx + 1);

          // Render the fence from cached data
          const html = cache.renderFence(fenceIdx, documentUri);

          // Check if this is the last supersigil-xml fence; if so,
          // append edges and reset the fence index counter
          const remaining = countRemainingFences(tokens, idx + 1);
          if (remaining === 0) {
            const edgeHtml = cache.renderEdges(documentUri);
            fenceIndexByUri.delete(uriStr);
            return html + edgeHtml;
          }

          return html;
        }
        return defaultFence(tokens, idx, options, env, self);
      };

      return md;
    },
  };
}

/** Extract the document URI from the markdown-it env object. */
function resolveDocumentUri(env: MarkdownItEnv): string | undefined {
  // VS Code's built-in Markdown preview sets `env.currentDocument`
  // directly as a vscode.Uri, not { uri: vscode.Uri }.
  if (env.currentDocument) {
    return env.currentDocument.toString();
  }
  return undefined;
}

/** Count remaining supersigil-xml fence tokens after the given index. */
function countRemainingFences(
  tokens: MarkdownItToken[],
  startIdx: number,
): number {
  let count = 0;
  for (let i = startIdx; i < tokens.length; i++) {
    if (
      tokens[i].type === "fence" &&
      tokens[i].info.trim() === "supersigil-xml"
    ) {
      count++;
    }
  }
  return count;
}

// ---------------------------------------------------------------------------
// Activation
// ---------------------------------------------------------------------------

export async function activate(
  context: vscode.ExtensionContext,
): Promise<ReturnType<typeof createMarkdownItExtender>> {
  outputChannel = vscode.window.createOutputChannel("Supersigil");
  context.subscriptions.push(outputChannel);

  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    10,
  );
  statusBarItem.command = "supersigil.showStatus";
  context.subscriptions.push(statusBarItem);

  // Initialize preview cache
  previewCache = new PreviewCache(clients, documentListCache, outputChannel);

  context.subscriptions.push(
    vscode.commands.registerCommand("supersigil.showStatus", () =>
      showStatusMenu(context),
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "supersigil.restartServer",
      async () => {
        await stopAllClients();
        await startAllClients(context);
      },
    ),
  );

  // Spec Explorer tree view
  specExplorer = new SpecExplorerProvider(clients);
  context.subscriptions.push(specExplorer);
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider(
      "supersigil.specExplorer",
      specExplorer,
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("supersigil.init", () => {
      const terminal = vscode.window.createTerminal("Supersigil Init");
      terminal.show();
      terminal.sendText("supersigil init");
    }),
  );

  // Register supersigil.verify ourselves instead of letting each language
  // client auto-register it (which fails for the second client with
  // "command already exists"). Routes to the client for the active file.
  context.subscriptions.push(
    vscode.commands.registerCommand("supersigil.verify", async () => {
      const editor = vscode.window.activeTextEditor;
      const client = editor ? clientForUri(editor.document.uri) : undefined;
      if (client?.isRunning()) {
        await client.sendRequest("workspace/executeCommand", {
          command: "supersigil.verify",
        });
      } else {
        vscode.window.showWarningMessage(
          "Supersigil LSP server is not running for this workspace.",
        );
      }
    }),
  );

  // Criterion navigation command
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "supersigil.goToCriterion",
      async (docId: string, criterionId: string) => {
        await goToCriterion(docId, criterionId);
      },
    ),
  );

  // URI handler for vscode://savolainen.supersigil/... links.
  // Used by the Markdown preview where command: URIs are blocked.
  context.subscriptions.push(
    vscode.window.registerUriHandler({
      async handleUri(uri: vscode.Uri) {
        outputChannel.appendLine(`[uri] Handling: ${uri.toString()}`);
        const params = new URLSearchParams(uri.query);

        switch (uri.path) {
          case "/open-file": {
            const filePath = params.get("path");
            if (!filePath) return;
            const line = parseInt(params.get("line") ?? "1", 10);
            const fileUri = vscode.Uri.file(filePath);
            const doc = await vscode.workspace.openTextDocument(fileUri);
            const selection = new vscode.Range(
              Math.max(0, line - 1), 0,
              Math.max(0, line - 1), 0,
            );
            await vscode.window.showTextDocument(doc, {
              selection,
              viewColumn: vscode.ViewColumn.Beside,
              preserveFocus: true,
              preview: true,
            });
            break;
          }
          case "/open-criterion": {
            const docId = params.get("doc");
            const criterionId = params.get("criterion");
            if (docId && criterionId) {
              await goToCriterion(docId, criterionId);
            }
            break;
          }
          default:
            outputChannel.appendLine(`[uri] Unknown path: ${uri.path}`);
        }
      },
    }),
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(async (e) => {
      for (const removed of e.removed) {
        const key = removed.uri.toString();
        const client = clients.get(key);
        if (client) {
          await client.stop();
          clients.delete(key);
        }
      }

      const serverPath = resolveServerBinary();
      if (serverPath) {
        for (const added of e.added) {
          if (existsSync(join(added.uri.fsPath, "supersigil.toml"))) {
            await startClientForFolder(added, serverPath);
          }
        }
      }
      updateStatusBar();
      updateNoRootsContext();
      specExplorer?.refresh();
    }),
  );

  await startAllClients(context);
  updateNoRootsContext();

  // Return the markdown-it plugin for VS Code's built-in Markdown preview
  return createMarkdownItExtender(previewCache);
}

export async function deactivate(): Promise<void> {
  await stopAllClients();
}
