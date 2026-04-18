import { describe, it, expect, vi, beforeEach } from "vitest";
import { verifies } from "@supersigil/vitest";

// ---------------------------------------------------------------------------
// Mock: vscode
// ---------------------------------------------------------------------------

const mockPostMessage = vi.fn();
const mockDispose = vi.fn();
let onDidDisposeCallbacks: (() => void)[] = [];
let onDidReceiveMessageCallbacks: ((msg: unknown) => void)[] = [];
let onDidChangeViewStateCallbacks: ((e: { webviewPanel: { visible: boolean } }) => void)[] = [];

function createMockPanel() {
  const webview = {
    postMessage: mockPostMessage,
    asWebviewUri: vi.fn((uri: { path: string }) => ({
      toString: () => `https://webview.test${uri.path}`,
      path: uri.path,
    })),
    cspSource: "https://webview.test",
    options: {} as Record<string, unknown>,
    html: "",
    onDidReceiveMessage: vi.fn((cb: (msg: unknown) => void) => {
      onDidReceiveMessageCallbacks.push(cb);
      return { dispose: vi.fn() };
    }),
  };

  const panel = {
    webview,
    reveal: vi.fn(),
    dispose: mockDispose,
    title: "",
    onDidDispose: vi.fn((cb: () => void) => {
      onDidDisposeCallbacks.push(cb);
      return { dispose: vi.fn() };
    }),
    onDidChangeViewState: vi.fn((cb: (e: { webviewPanel: { visible: boolean } }) => void) => {
      onDidChangeViewStateCallbacks.push(cb);
      return { dispose: vi.fn() };
    }),
    viewColumn: undefined,
    visible: true,
  };

  return panel;
}

let panels: ReturnType<typeof createMockPanel>[] = [];

const mockShowTextDocument = vi.fn();
const mockOpenTextDocument = vi.fn().mockResolvedValue({});
const mockShowInformationMessage = vi.fn();
const mockShowErrorMessage = vi.fn();
const mockShowWarningMessage = vi.fn();

let mockActiveTextEditor: unknown = undefined;
let mockWorkspaceFolders: unknown[] | undefined = undefined;

const mockCreateWebviewPanel = vi.fn(
  (_viewType: string, title: string, _showOptions: unknown, _options?: unknown) => {
    const p = createMockPanel();
    p.title = title;
    panels.push(p);
    return p;
  },
);

vi.mock("vscode", () => ({
  window: {
    createWebviewPanel: (
      viewType: string,
      title: string,
      showOptions: unknown,
      options?: unknown,
    ) => mockCreateWebviewPanel(viewType, title, showOptions, options),
    showTextDocument: (doc: unknown, options?: unknown) =>
      mockShowTextDocument(doc, options),
    showInformationMessage: (...args: unknown[]) => mockShowInformationMessage(...args),
    showErrorMessage: (...args: unknown[]) => mockShowErrorMessage(...args),
    showWarningMessage: (...args: unknown[]) => mockShowWarningMessage(...args),
    get activeTextEditor() {
      return mockActiveTextEditor;
    },
  },
  workspace: {
    openTextDocument: (uri: unknown) => mockOpenTextDocument(uri),
    getWorkspaceFolder: (uri: { toString: () => string }) => {
      if (!mockWorkspaceFolders) return undefined;
      return mockWorkspaceFolders.find((f: unknown) => {
        const folder = f as { uri: { toString: () => string; fsPath: string } };
        return uri.toString().startsWith(folder.uri.toString());
      });
    },
    get workspaceFolders() {
      return mockWorkspaceFolders;
    },
    asRelativePath: (uri: { toString: () => string }, _includeWorkspace?: boolean) => {
      // Simple mock: extract path after workspace root
      const uriStr = uri.toString();
      if (mockWorkspaceFolders) {
        for (const f of mockWorkspaceFolders) {
          const folder = f as { uri: { toString: () => string } };
          const prefix = folder.uri.toString() + "/";
          if (uriStr.startsWith(prefix)) {
            return uriStr.slice(prefix.length);
          }
        }
      }
      return uriStr;
    },
  },
  ViewColumn: {
    Beside: 2,
  },
  Uri: {
    parse: (uriStr: string) => ({
      path: uriStr.replace(/^file:\/\//, ""),
      fsPath: uriStr.replace(/^file:\/\//, ""),
      toString: () => uriStr,
    }),
    joinPath: (base: { path: string; fsPath: string }, ...segments: string[]) => {
      const joined = base.path + "/" + segments.join("/");
      return {
        path: joined,
        fsPath: joined,
        toString: () => `file://${joined}`,
      };
    },
  },
  Selection: class {
    constructor(
      public anchor: { line: number; character: number },
      public active: { line: number; character: number },
    ) {}
  },
  Position: class {
    constructor(
      public line: number,
      public character: number,
    ) {}
  },
  Range: class {
    constructor(
      public start: unknown,
      public end: unknown,
    ) {}
  },
  ThemeColor: class {
    constructor(public id: string) {}
  },
  ThemeIcon: class {
    constructor(
      public id: string,
      public color?: unknown,
    ) {}
  },
  DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
  TreeItem: class {
    constructor(
      public label: string,
      public collapsibleState?: number,
    ) {}
  },
  TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
  EventEmitter: class {
    event = () => {};
    fire() {}
    dispose() {}
  },
  languages: {
    getDiagnostics: () => [],
    onDidChangeDiagnostics: () => ({ dispose: () => {} }),
  },
}));

vi.mock("vscode-languageclient/node", () => ({
  LanguageClient: class {},
}));

// ---------------------------------------------------------------------------
// Import under test (after mocks)
// ---------------------------------------------------------------------------

import {
  openExplorerPanel,
  openGraphFile,
  refreshPanelsForClient,
  openPanels,
  restoreExplorerPanel,
} from "./explorerWebview";
import { OPEN_GRAPH_FILE_COMMAND } from "./explorerLinks";
import type { GraphDocument } from "./explorerWebview";
import type { LanguageClient } from "vscode-languageclient/node";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeMockClient(
  running: boolean,
  sendRequest?: (method: string, params?: unknown) => Promise<unknown>,
): LanguageClient {
  return {
    isRunning: () => running,
    sendRequest: sendRequest ?? vi.fn(),
  } as unknown as LanguageClient;
}

const METHOD_GRAPH_DATA = "supersigil/graphData";
const METHOD_DOCUMENT_COMPONENTS = "supersigil/documentComponents";

function makeGraphDataResponse(): {
  documents: GraphDocument[];
  edges: { from: string; to: string; kind: string }[];
} {
  return {
    documents: [
      {
        id: "proj/requirements",
        doc_type: "requirements",
        status: "draft",
        title: "Requirements",
        project: "proj",
        path: "specs/proj/requirements.md",
        components: [],
      },
      {
        id: "proj/design",
        doc_type: "design",
        status: "approved",
        title: "Design",
        project: "proj",
        path: "specs/proj/design.md",
        components: [],
      },
    ],
    edges: [
      { from: "proj/requirements", to: "proj/design", kind: "traces" },
    ],
  };
}

function makeDocumentComponentsResponse(docId: string) {
  return {
    document_id: docId,
    stale: false,
    fences: [{ byte_range: [0, 100], components: [] }],
    edges: [],
  };
}

function makeMockExtensionContext() {
  return {
    extensionUri: {
      path: "/ext",
      fsPath: "/ext",
      toString: () => "file:///ext",
    },
    subscriptions: [],
  } as unknown as import("vscode").ExtensionContext;
}

function makeStandardSendRequest() {
  return vi.fn().mockImplementation((method: string, params?: unknown) => {
    if (method === METHOD_GRAPH_DATA) return Promise.resolve(makeGraphDataResponse());
    if (method === METHOD_DOCUMENT_COMPONENTS) {
      const p = params as { uri: string };
      if (p.uri.includes("requirements")) {
        return Promise.resolve(makeDocumentComponentsResponse("proj/requirements"));
      }
      return Promise.resolve(makeDocumentComponentsResponse("proj/design"));
    }
    return Promise.reject(new Error("unknown method"));
  });
}

/** Simulate the bootstrap sending a 'ready' message for the latest panel. */
async function sendReady(panelIndex = -1): Promise<void> {
  const idx = panelIndex >= 0 ? panelIndex : onDidReceiveMessageCallbacks.length - 1;
  onDidReceiveMessageCallbacks[idx]?.({ type: "ready" });
  await new Promise((r) => setTimeout(r, 0));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("openExplorerPanel", () => {
  let clients: Map<string, LanguageClient>;

  beforeEach(() => {
    vi.clearAllMocks();
    clients = new Map();
    panels = [];
    onDidDisposeCallbacks = [];
    onDidReceiveMessageCallbacks = [];
    onDidChangeViewStateCallbacks = [];
    mockActiveTextEditor = undefined;
    mockWorkspaceFolders = undefined;
    // Clear the openPanels array
    openPanels.length = 0;
  });

  describe("root resolution", () => {
    it("resolves root from active editor workspace folder", verifies("vscode-explorer-webview/req#req-2-5"), async () => {
      const sendRequest = makeStandardSendRequest();
      const activeClient = makeMockClient(true, sendRequest);
      const otherClient = makeMockClient(true, vi.fn());

      clients.set("file:///active-root", activeClient);
      clients.set("file:///other-root", otherClient);

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///active-root", fsPath: "/active-root", path: "/active-root" }, name: "active" },
        { uri: { toString: () => "file:///other-root", fsPath: "/other-root", path: "/other-root" }, name: "other" },
      ];

      mockActiveTextEditor = {
        document: {
          uri: {
            toString: () => "file:///active-root/specs/proj/requirements.md",
            path: "/active-root/specs/proj/requirements.md",
            fsPath: "/active-root/specs/proj/requirements.md",
          },
        },
      };

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      // Should use the active editor's client
      expect(sendRequest).toHaveBeenCalledWith(METHOD_GRAPH_DATA);
    });

    it("falls back to first running client when no active editor", async () => {
      const sendRequest = makeStandardSendRequest();

      const stoppedClient = makeMockClient(false);
      const runningClient = makeMockClient(true, sendRequest);

      clients.set("file:///stopped", stoppedClient);
      clients.set("file:///running", runningClient);

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      expect(sendRequest).toHaveBeenCalledWith(METHOD_GRAPH_DATA);
    });

    it("creates panel with stopped client (hydrates when client starts)", () => {
      const stoppedClient = makeMockClient(false);
      clients.set("file:///stopped", stoppedClient);

      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).toHaveBeenCalledTimes(1);
      // No data pushed since client isn't running
      expect(mockPostMessage).not.toHaveBeenCalled();
    });

    it("shows info message when no clients exist at all", () => {
      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).not.toHaveBeenCalled();
      expect(mockShowInformationMessage).toHaveBeenCalled();
    });

    it("shows info message when a workspace has no registered Supersigil client", () => {
      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///plain-workspace", fsPath: "/plain-workspace", path: "/plain-workspace" }, name: "plain" },
      ];

      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).not.toHaveBeenCalled();
      expect(mockShowInformationMessage).toHaveBeenCalledWith(
        "No Supersigil project found. Open a workspace with a supersigil.toml.",
      );
    });
  });

  describe("multi-instance panel creation", () => {
    it("creates a new panel on each invocation", verifies("vscode-explorer-webview/req#req-2-2"), async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).toHaveBeenCalledTimes(2);
      expect(openPanels).toHaveLength(2);
    });

    it("enables the open-graph-file command URI in the webview options", () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).toHaveBeenCalledWith(
        "supersigil.explorer",
        expect.any(String),
        expect.anything(),
        expect.objectContaining({
          enableCommandUris: [OPEN_GRAPH_FILE_COMMAND],
        }),
      );
    });

    it("panel title includes folder name", verifies("vscode-explorer-webview/req#req-2-6"), () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///workspace", fsPath: "/workspace", path: "/workspace" }, name: "my-project" },
      ];

      openExplorerPanel(makeMockExtensionContext(), clients);

      expect(mockCreateWebviewPanel).toHaveBeenCalledWith(
        "supersigil.explorer",
        "Spec Explorer (my-project)",
        expect.anything(),
        expect.anything(),
      );
    });

    it("removes panel from openPanels on dispose", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      expect(openPanels).toHaveLength(1);

      // Simulate panel dispose
      onDidDisposeCallbacks[0]?.();
      expect(openPanels).toHaveLength(0);
    });
  });

  describe("focusDocumentId", () => {
    it("resolves focusDocumentId from active file path", verifies("vscode-explorer-webview/req#req-2-7"), async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///workspace", fsPath: "/workspace", path: "/workspace" }, name: "ws" },
      ];

      mockActiveTextEditor = {
        document: {
          uri: {
            toString: () => "file:///workspace/specs/proj/requirements.md",
            path: "/workspace/specs/proj/requirements.md",
            fsPath: "/workspace/specs/proj/requirements.md",
          },
        },
      };

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      expect(mockPostMessage).toHaveBeenCalledTimes(1);
      const message = mockPostMessage.mock.calls[0][0];
      expect(message.focusDocumentId).toBe("proj/requirements");
    });

    it("focusDocumentId is undefined when active file is not a spec document", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///workspace", fsPath: "/workspace", path: "/workspace" }, name: "ws" },
      ];

      mockActiveTextEditor = {
        document: {
          uri: {
            toString: () => "file:///workspace/src/main.rs",
            path: "/workspace/src/main.rs",
            fsPath: "/workspace/src/main.rs",
          },
        },
      };

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      const message = mockPostMessage.mock.calls[0][0];
      expect(message.focusDocumentId).toBeUndefined();
    });
  });

  describe("switchRoot", () => {
    it("validates against running clients and ignores stopped clients", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));
      clients.set("file:///stopped", makeMockClient(false));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      mockPostMessage.mockClear();
      sendRequest.mockClear();

      // Try to switch to a stopped client
      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({ type: "switchRoot", folderUri: "file:///stopped" });

      await new Promise((r) => setTimeout(r, 0));

      // Should not have sent any request
      expect(sendRequest).not.toHaveBeenCalled();
      expect(mockPostMessage).not.toHaveBeenCalled();
    });

    it("updates folderUri and clientKey on switchRoot", verifies("vscode-explorer-webview/req#req-5-3"), async () => {
      const sendRequestA = makeStandardSendRequest();
      const sendRequestB = makeStandardSendRequest();
      clients.set("file:///workspace-a", makeMockClient(true, sendRequestA));
      clients.set("file:///workspace-b", makeMockClient(true, sendRequestB));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///workspace-a", fsPath: "/workspace-a", path: "/workspace-a" }, name: "project-a" },
        { uri: { toString: () => "file:///workspace-b", fsPath: "/workspace-b", path: "/workspace-b" }, name: "project-b" },
      ];

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      const entry = openPanels[0];
      expect(entry.clientKey).toBe("file:///workspace-a");

      mockPostMessage.mockClear();

      // Switch to workspace-b
      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({ type: "switchRoot", folderUri: "file:///workspace-b" });

      await new Promise((r) => setTimeout(r, 0));

      expect(entry.clientKey).toBe("file:///workspace-b");
      expect(entry.folderUri.toString()).toBe("file:///workspace-b");
      expect(panels[0].title).toBe("Spec Explorer (project-b)");
    });

    it("posts graphData with isRootSwitch true on switchRoot", async () => {
      const sendRequestA = makeStandardSendRequest();
      const sendRequestB = makeStandardSendRequest();
      clients.set("file:///workspace-a", makeMockClient(true, sendRequestA));
      clients.set("file:///workspace-b", makeMockClient(true, sendRequestB));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      mockPostMessage.mockClear();

      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({ type: "switchRoot", folderUri: "file:///workspace-b" });

      await new Promise((r) => setTimeout(r, 0));

      expect(mockPostMessage).toHaveBeenCalledTimes(1);
      const message = mockPostMessage.mock.calls[0][0];
      expect(message.isRootSwitch).toBe(true);
    });
  });

  describe("refreshPanelsForClient", () => {
    it("only refreshes panels matching the given clientKey", async () => {
      const sendRequestA = makeStandardSendRequest();
      const sendRequestB = makeStandardSendRequest();
      clients.set("file:///workspace-a", makeMockClient(true, sendRequestA));
      clients.set("file:///workspace-b", makeMockClient(true, sendRequestB));

      // Create two panels, one for each workspace
      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady(0);

      // Force the second panel to use workspace-b by switching root
      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady(1);

      // Switch second panel to workspace-b
      const secondCb = onDidReceiveMessageCallbacks[1];
      secondCb({ type: "switchRoot", folderUri: "file:///workspace-b" });
      await new Promise((r) => setTimeout(r, 0));

      mockPostMessage.mockClear();
      sendRequestA.mockClear();
      sendRequestB.mockClear();

      // Refresh for workspace-a only
      refreshPanelsForClient("file:///workspace-a", clients);
      await new Promise((r) => setTimeout(r, 0));

      // Only the first panel's client should have been called
      expect(sendRequestA).toHaveBeenCalledWith(METHOD_GRAPH_DATA);
      expect(sendRequestB).not.toHaveBeenCalledWith(METHOD_GRAPH_DATA);
    });

    it("does not refresh hidden panels", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      // Mark panel as hidden
      panels[0].visible = false;

      mockPostMessage.mockClear();
      sendRequest.mockClear();

      refreshPanelsForClient("file:///workspace", clients);
      await new Promise((r) => setTimeout(r, 0));

      expect(sendRequest).not.toHaveBeenCalled();
    });
  });

  describe("onDidChangeViewState", () => {
    it("triggers refresh when panel becomes visible after being stale", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      // Mark the panel as stale (simulates documentsChanged while hidden)
      openPanels[0].staleWhileHidden = true;

      mockPostMessage.mockClear();
      sendRequest.mockClear();

      // Simulate panel becoming visible
      const viewStateCb = onDidChangeViewStateCallbacks[0];
      viewStateCb({ webviewPanel: { visible: true } });
      await new Promise((r) => setTimeout(r, 0));

      expect(sendRequest).toHaveBeenCalledWith(METHOD_GRAPH_DATA);
      expect(mockPostMessage).toHaveBeenCalledTimes(1);
    });

    it("does not refresh when panel becomes visible without being stale", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      mockPostMessage.mockClear();
      sendRequest.mockClear();

      // Simulate panel becoming visible (not stale)
      const viewStateCb = onDidChangeViewStateCallbacks[0];
      viewStateCb({ webviewPanel: { visible: true } });
      await new Promise((r) => setTimeout(r, 0));

      expect(sendRequest).not.toHaveBeenCalled();
      expect(mockPostMessage).not.toHaveBeenCalled();
    });
  });

  describe("pushData message shape", () => {
    it("includes currentRoot, availableRoots, focusDocumentId, and isRootSwitch", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///workspace", fsPath: "/workspace", path: "/workspace" }, name: "my-ws" },
      ];

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      const message = mockPostMessage.mock.calls[0][0];
      expect(message.type).toBe("graphData");
      expect(message.graph).toBeDefined();
      expect(message.renderData).toBeDefined();
      expect(message.currentRoot).toEqual({ uri: "file:///workspace", name: "my-ws" });
      expect(message.availableRoots).toEqual([{ uri: "file:///workspace", name: "my-ws" }]);
      expect(message.isRootSwitch).toBe(false);
    });

    it("lists all running clients in availableRoots", async () => {
      const sendRequest = makeStandardSendRequest();
      clients.set("file:///ws-a", makeMockClient(true, sendRequest));
      clients.set("file:///ws-b", makeMockClient(true, vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA) return Promise.resolve(makeGraphDataResponse());
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      })));
      clients.set("file:///ws-stopped", makeMockClient(false));

      mockWorkspaceFolders = [
        { uri: { toString: () => "file:///ws-a", fsPath: "/ws-a", path: "/ws-a" }, name: "a" },
        { uri: { toString: () => "file:///ws-b", fsPath: "/ws-b", path: "/ws-b" }, name: "b" },
        { uri: { toString: () => "file:///ws-stopped", fsPath: "/ws-stopped", path: "/ws-stopped" }, name: "stopped" },
      ];

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      const message = mockPostMessage.mock.calls[0][0];
      expect(message.availableRoots).toHaveLength(2);
      expect(message.availableRoots.map((r: { name: string }) => r.name)).toEqual(["a", "b"]);
    });

    it("assembles graph and render data and posts correct message shape", async () => {
      const graphData = makeGraphDataResponse();

      const sendRequest = vi.fn().mockImplementation((method: string, params?: unknown) => {
        if (method === METHOD_GRAPH_DATA) return Promise.resolve(graphData);
        if (method === METHOD_DOCUMENT_COMPONENTS) {
          const p = params as { uri: string };
          if (p.uri.includes("requirements")) {
            return Promise.resolve(makeDocumentComponentsResponse("proj/requirements"));
          }
          return Promise.resolve(makeDocumentComponentsResponse("proj/design"));
        }
        return Promise.reject(new Error("unknown method"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      expect(mockPostMessage).toHaveBeenCalledTimes(1);

      const message = mockPostMessage.mock.calls[0][0];
      expect(message.type).toBe("graphData");
      expect(message.graph).toBe(graphData);
      expect(message.renderData).toHaveLength(2);
      expect(message.renderData[0].document_id).toBeDefined();
      expect(message.renderData[1].document_id).toBeDefined();
    });

    it("fetches documentComponents for each document in parallel", async () => {
      const graphData = makeGraphDataResponse();

      const sendRequest = vi.fn().mockImplementation((method: string, params?: unknown) => {
        if (method === METHOD_GRAPH_DATA) return Promise.resolve(graphData);
        if (method === METHOD_DOCUMENT_COMPONENTS) {
          const p = params as { uri: string };
          const docId = p.uri.includes("requirements")
            ? "proj/requirements"
            : "proj/design";
          return Promise.resolve(makeDocumentComponentsResponse(docId));
        }
        return Promise.reject(new Error("unknown method"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      // graphData + 2 documentComponents requests
      expect(sendRequest).toHaveBeenCalledTimes(3);
      expect(sendRequest).toHaveBeenCalledWith(
        METHOD_DOCUMENT_COMPONENTS,
        expect.objectContaining({ uri: expect.stringContaining("requirements") }),
      );
      expect(sendRequest).toHaveBeenCalledWith(
        METHOD_DOCUMENT_COMPONENTS,
        expect.objectContaining({ uri: expect.stringContaining("design") }),
      );
    });

    it("tolerates partial failure in documentComponents batch", async () => {
      const graphData = makeGraphDataResponse();

      const sendRequest = vi.fn().mockImplementation((method: string, params?: unknown) => {
        if (method === METHOD_GRAPH_DATA) return Promise.resolve(graphData);
        if (method === METHOD_DOCUMENT_COMPONENTS) {
          const p = params as { uri: string };
          if (p.uri.includes("requirements")) {
            return Promise.reject(new Error("LSP error for requirements"));
          }
          return Promise.resolve(makeDocumentComponentsResponse("proj/design"));
        }
        return Promise.reject(new Error("unknown method"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      expect(mockPostMessage).toHaveBeenCalledTimes(1);

      const message = mockPostMessage.mock.calls[0][0];
      expect(message.type).toBe("graphData");
      expect(message.graph).toBe(graphData);
      // Only 1 succeeded (design), requirements failed
      expect(message.renderData).toHaveLength(1);
      expect(message.renderData[0].document_id).toBe("proj/design");
    });
  });

  describe("handleMessage", () => {
    it("resolves openFile path against workspace root and opens document", async () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      // Simulate webview message
      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({
        type: "openFile",
        path: "specs/proj/requirements.md",
      });

      // Wait for async processing
      await vi.waitFor(() => {
        expect(mockOpenTextDocument).toHaveBeenCalledTimes(1);
      });

      const openUri = mockOpenTextDocument.mock.calls[0][0];
      expect(openUri.path).toContain("specs/proj/requirements.md");

      expect(mockShowTextDocument).toHaveBeenCalledTimes(1);
      const showArgs = mockShowTextDocument.mock.calls[0];
      // No line specified, so no selection
      expect(showArgs[1]).not.toHaveProperty("selection");
    });

    it("opens document by explicit file URI when provided", async () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({
        type: "openFile",
        path: "../shared/specs/requirements.md",
        uri: "file:///shared/specs/requirements.md",
      });

      await vi.waitFor(() => {
        expect(mockOpenTextDocument).toHaveBeenCalledTimes(1);
      });

      const openUri = mockOpenTextDocument.mock.calls[0][0];
      expect(openUri.toString()).toBe("file:///shared/specs/requirements.md");
      expect(mockShowWarningMessage).not.toHaveBeenCalled();
    });

    it("opens document with line selection when line is provided", async () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({
        type: "openFile",
        path: "src/main.rs",
        line: 42,
      });

      await vi.waitFor(() => {
        expect(mockShowTextDocument).toHaveBeenCalledTimes(1);
      });

      const showArgs = mockShowTextDocument.mock.calls[0];
      expect(showArgs[1]).toHaveProperty("selection");
    });

    it("ignores unknown message types", () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      // This should not throw
      const lastCb = onDidReceiveMessageCallbacks[onDidReceiveMessageCallbacks.length - 1];
      lastCb({ type: "unknownType" });

      expect(mockOpenTextDocument).not.toHaveBeenCalled();
    });

    it("uses file_uri for documentComponents requests when present", async () => {
      const graphData = makeGraphDataResponse();
      graphData.documents[0] = {
        ...graphData.documents[0],
        path: "../shared/specs/proj/requirements.md",
        file_uri: "file:///shared/specs/proj/requirements.md",
      };

      const sendRequest = vi.fn().mockImplementation((method: string, params?: unknown) => {
        if (method === METHOD_GRAPH_DATA) return Promise.resolve(graphData);
        if (method === METHOD_DOCUMENT_COMPONENTS) {
          const p = params as { uri: string };
          if (p.uri.includes("shared/specs/proj/requirements.md")) {
            return Promise.resolve(makeDocumentComponentsResponse("proj/requirements"));
          }
          return Promise.resolve(makeDocumentComponentsResponse("proj/design"));
        }
        return Promise.reject(new Error("unknown method"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);
      await sendReady();

      expect(sendRequest).toHaveBeenCalledWith(
        METHOD_DOCUMENT_COMPONENTS,
        expect.objectContaining({ uri: "file:///shared/specs/proj/requirements.md" }),
      );
    });
  });

  describe("openGraphFile", () => {
    it("resolves a relative path against the provided folder URI", async () => {
      openGraphFile({
        path: "specs/proj/requirements.md",
        folderUri: "file:///workspace",
      });

      await vi.waitFor(() => {
        expect(mockOpenTextDocument).toHaveBeenCalledTimes(1);
      });

      const openUri = mockOpenTextDocument.mock.calls[0][0];
      expect(openUri.path).toContain("specs/proj/requirements.md");
      expect(mockShowWarningMessage).not.toHaveBeenCalled();
    });

    it("opens an explicit file URI without needing a folder URI", async () => {
      openGraphFile({
        uri: "file:///shared/specs/requirements.md",
        line: 7,
      });

      await vi.waitFor(() => {
        expect(mockShowTextDocument).toHaveBeenCalledTimes(1);
      });

      const openUri = mockOpenTextDocument.mock.calls[0][0];
      expect(openUri.toString()).toBe("file:///shared/specs/requirements.md");
      expect(mockShowTextDocument.mock.calls[0][1]).toHaveProperty("selection");
    });
  });

  describe("getHtmlContent", () => {
    it("generates HTML with nonce-based CSP and resource URIs", () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      const html = panels[0].webview.html;

      // CSP meta tag
      expect(html).toContain("Content-Security-Policy");
      expect(html).toContain("'unsafe-inline'");
      expect(html).toContain("nonce-");
      expect(html).toContain(panels[0].webview.cspSource);

      // Resource URIs (4 CSS files, 4 JS files)
      expect(html).toContain("landing-tokens.css");
      expect(html).toContain("explorer-styles.css");
      expect(html).toContain("supersigil-preview.css");
      expect(html).toContain("vscode-theme-adapter.css");
      expect(html).toContain("render-iife.js");
      expect(html).toContain("supersigil-preview.js");
      expect(html).toContain("explorer.js");
      expect(html).toContain("bootstrap.js");

      // Explorer container
      expect(html).toContain('id="explorer"');
    });

    it("uses webview.asWebviewUri for all resources", () => {
      const sendRequest = vi.fn().mockImplementation((method: string) => {
        if (method === METHOD_GRAPH_DATA)
          return Promise.resolve({ documents: [], edges: [] });
        return Promise.resolve(makeDocumentComponentsResponse("any"));
      });

      clients.set("file:///workspace", makeMockClient(true, sendRequest));

      openExplorerPanel(makeMockExtensionContext(), clients);

      // 4 CSS + 4 JS = 8 calls to asWebviewUri
      expect(panels[0].webview.asWebviewUri).toHaveBeenCalledTimes(8);
    });
  });
});

describe("restoreExplorerPanel", () => {
  let clients: Map<string, LanguageClient>;

  beforeEach(() => {
    vi.clearAllMocks();
    clients = new Map();
    panels = [];
    onDidDisposeCallbacks = [];
    onDidReceiveMessageCallbacks = [];
    onDidChangeViewStateCallbacks = [];
    mockActiveTextEditor = undefined;
    mockWorkspaceFolders = undefined;
    openPanels.length = 0;
  });

  it("disposes panels that do not have serialized client state", () => {
    const panel = createMockPanel();

    restoreExplorerPanel(
      panel as never,
      {},
      clients,
      makeMockExtensionContext().extensionUri,
    );

    expect(mockDispose).toHaveBeenCalledTimes(1);
    expect(openPanels).toHaveLength(0);
  });

  it("rehydrates a restored panel and handles ready/openFile messages", async () => {
    const sendRequest = makeStandardSendRequest();
    clients.set("file:///workspace", makeMockClient(true, sendRequest));

    const panel = createMockPanel();

    restoreExplorerPanel(
      panel as never,
      { clientKey: "file:///workspace" },
      clients,
      makeMockExtensionContext().extensionUri,
    );

    expect(openPanels).toHaveLength(1);
    expect(panel.webview.html).toContain("bootstrap.js");

    const callback = onDidReceiveMessageCallbacks[0];
    callback?.({ type: "ready" });
    await new Promise((r) => setTimeout(r, 0));

    expect(mockPostMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: "graphData" }),
    );

    callback?.({
      type: "openFile",
      path: "specs/proj/requirements.md",
    });

    await vi.waitFor(() => {
      expect(mockOpenTextDocument).toHaveBeenCalled();
    });
  });
});
