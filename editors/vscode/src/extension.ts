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
    statusBarItem.tooltip = "Supersigil LSP is not running. Click to restart.";
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
  context: vscode.ExtensionContext,
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
    roots.map((folder) => startClientForFolder(folder, serverPath, context)),
  );
  updateStatusBar();
}

async function stopAllClients(): Promise<void> {
  const stops = Array.from(clients.values()).map((c) => c.stop());
  await Promise.all(stops);
  clients.clear();
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    10,
  );
  statusBarItem.command = "supersigil.restartServer";
  context.subscriptions.push(statusBarItem);

  // Note: supersigil.verify is registered automatically by vscode-languageclient
  // from the LSP server's executeCommand capabilities. We only declare it in
  // package.json contributes.commands so it appears in the command palette.

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "supersigil.restartServer",
      async () => {
        await stopAllClients();
        await startAllClients(context);
      },
    ),
  );

  // React to workspace folder changes (add/remove roots).
  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(async (e) => {
      // Stop clients for removed folders.
      for (const removed of e.removed) {
        const key = removed.uri.toString();
        const client = clients.get(key);
        if (client) {
          await client.stop();
          clients.delete(key);
        }
      }

      // Start clients for added folders that have supersigil.toml.
      const serverPath = resolveServerBinary();
      if (serverPath) {
        for (const added of e.added) {
          if (existsSync(join(added.uri.fsPath, "supersigil.toml"))) {
            await startClientForFolder(added, serverPath, context);
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
