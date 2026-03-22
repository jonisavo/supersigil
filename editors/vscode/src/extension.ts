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

let client: LanguageClient | undefined;
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

  // Try $PATH lookup (may miss tools installed via cargo/mise if VS Code
  // was not launched from a shell with the full PATH)
  try {
    const cmd = process.platform === "win32"
      ? "where.exe supersigil-lsp"
      : "which supersigil-lsp";
    return execSync(cmd, { encoding: "utf-8" }).trim();
  } catch {
    // Not on $PATH
  }

  // Check common install locations that VS Code's PATH may not include
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

function setStatusBar(state: "running" | "error"): void {
  if (state === "running") {
    statusBarItem.text = "Supersigil";
    statusBarItem.backgroundColor = undefined;
    statusBarItem.tooltip = "Supersigil LSP is running";
  } else {
    statusBarItem.text = "$(warning) Supersigil";
    statusBarItem.backgroundColor = new vscode.ThemeColor(
      "statusBarItem.warningBackground",
    );
    statusBarItem.tooltip =
      "Supersigil LSP is not running. Click to restart.";
  }
  statusBarItem.show();
}

async function startClient(
  context: vscode.ExtensionContext,
): Promise<void> {
  const serverPath = resolveServerBinary();
  if (!serverPath) {
    setStatusBar("error");
    return;
  }

  const serverOptions: ServerOptions = {
    command: serverPath,
    transport: TransportKind.stdio,
  };

  const outputChannel =
    vscode.window.createOutputChannel("Supersigil LSP");

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "mdx" }],
    outputChannel,
  };

  client = new LanguageClient(
    "supersigil",
    "Supersigil",
    serverOptions,
    clientOptions,
  );

  client.onDidChangeState((e) => {
    if (e.newState === State.Running) {
      setStatusBar("running");
    } else if (e.newState === State.Stopped) {
      setStatusBar("error");
    }
  });

  try {
    await client.start();
    setStatusBar("running");
  } catch {
    setStatusBar("error");
  }
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

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "supersigil.restartServer",
      async () => {
        if (client) {
          await client.restart();
        } else {
          await startClient(context);
        }
      },
    ),
  );

  await startClient(context);
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}
