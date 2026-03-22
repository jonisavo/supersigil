import { execSync } from "child_process";
import { existsSync } from "fs";
import { homedir } from "os";
import { join } from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
  TransportKind,
} from "vscode-languageclient/node";

const clients = new Map<string, LanguageClient>();
let statusBarItem: vscode.StatusBarItem;
let notFoundShown = false;

function resolveServerBinary(): string | undefined {
  const config = vscode.workspace.getConfiguration("supersigil.lsp");
  const configuredPath = config.get<string | null>("serverPath", null);

  if (configuredPath) {
    if (existsSync(configuredPath)) {
      return configuredPath;
    }
    vscode.window.showErrorMessage(
      `Supersigil LSP server not found at configured path: ${configuredPath}`,
    );
    return undefined;
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

  // Watch for .mdx and supersigil.toml changes on disk (git operations,
  // branch switches, external edits) so the LSP re-indexes automatically.
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
        language: "mdx",
        pattern: `${folder.uri.fsPath}/**/*`,
      },
    ],
    workspaceFolder: folder,
    outputChannel,
    synchronize: {
      fileEvents: [mdxWatcher, configWatcher],
    },
  };

  const client = new LanguageClient(
    `supersigil-${folder.name}`,
    `Supersigil (${folder.name})`,
    serverOptions,
    clientOptions,
  );

  client.onDidChangeState(() => updateStatusBar());

  clients.set(key, client);

  try {
    await client.start();
  } catch {
    // Status bar will reflect the error state
  }
  updateStatusBar();
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

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    10,
  );
  statusBarItem.command = "supersigil.showStatus";
  context.subscriptions.push(statusBarItem);

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
    }),
  );

  await startAllClients(context);
}

export async function deactivate(): Promise<void> {
  await stopAllClients();
}
