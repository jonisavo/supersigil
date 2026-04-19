// @vitest-environment jsdom
import {
  describe,
  it,
  expect,
  vi,
  beforeEach,
  beforeAll,
  afterAll,
} from "vitest";
import {
  OPEN_GRAPH_FILE_COMMAND,
  buildOpenFileCommandHref,
} from "./explorerLinks";

function decodeCommandHref(href: string): { command: string; args: unknown[] } {
  const [command, encodedArgs = ""] = href.split("?");
  return {
    command,
    args: JSON.parse(decodeURIComponent(encodedArgs)),
  };
}

describe("buildOpenFileCommandHref", () => {
  it("encodes open-file targets as a command URI argument array", () => {
    const href = buildOpenFileCommandHref({
      path: "src/main.rs",
      line: 42,
      folderUri: "file:///workspace",
    });

    expect(decodeCommandHref(href)).toEqual({
      command: `command:${OPEN_GRAPH_FILE_COMMAND}`,
      args: [
        {
          path: "src/main.rs",
          line: 42,
          folderUri: "file:///workspace",
        },
      ],
    });
  });
});

type BootstrapGlobals = typeof globalThis & {
  acquireVsCodeApi?: () => {
    postMessage: (msg: unknown) => void;
    getState: () => unknown;
    setState: (state: unknown) => void;
  };
  SupersigilExplorer?: {
    createExplorerApp?: (
      container: HTMLElement,
      transport: unknown,
      options?: unknown,
    ) => { destroy: () => void } | undefined;
  };
};

type SupersigilCreateExplorerApp =
  NonNullable<BootstrapGlobals["SupersigilExplorer"]>["createExplorerApp"];

describe("bootstrap webview runtime", () => {
  let bootstrapContainer: HTMLElement;
  const postMessage = vi.fn();
  const setState = vi.fn();
  let createExplorerAppImpl: SupersigilCreateExplorerApp;

  beforeAll(async () => {
    document.body.innerHTML = '<div id="explorer"></div>';
    bootstrapContainer = document.getElementById("explorer")!;

    const globals = globalThis as BootstrapGlobals;
    globals.acquireVsCodeApi = () => ({
      postMessage,
      getState: () => undefined,
      setState,
    });
    globals.SupersigilExplorer = {
      createExplorerApp: (...args) => createExplorerAppImpl?.(...args),
    };

    vi.resetModules();
    await import("./explorerBootstrap");
  });

  beforeEach(() => {
    bootstrapContainer.innerHTML = "";
    postMessage.mockClear();
    setState.mockClear();
    history.replaceState(null, "", location.pathname + location.search);
    createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() }));
  });

  afterAll(() => {
    const globals = globalThis as BootstrapGlobals;
    delete globals.acquireVsCodeApi;
    delete globals.SupersigilExplorer;
  });

  it("creates a shared explorer app and routes transport traffic over postMessage", async () => {
    let capturedTransport:
      | {
          getInitialContext: () => Promise<unknown>;
          loadSnapshot: (rootId: string) => Promise<unknown>;
          commitRoot: (rootId: string) => void;
          loadDocument: (input: {
        rootId: string;
        revision: string;
        documentId: string;
      }) => Promise<unknown>;
      subscribeChanges: (
        listener: (event: unknown) => void,
      ) => () => void;
      subscribeContext: (
        listener: (context: unknown) => void,
      ) => () => void;
      openFile: (target: { path?: string; uri?: string; line?: number }) => void;
    }
      | undefined;
    let capturedOptions:
      | {
          linkResolver?: { evidenceLink: (file: string, line: number) => string };
        }
      | undefined;
    const changeListener = vi.fn();

    createExplorerAppImpl = vi.fn((container, transport, options) => {
      expect(container).toBe(bootstrapContainer);
      capturedTransport = transport as typeof capturedTransport;
      capturedOptions = options as typeof capturedOptions;
      return { destroy: vi.fn() };
    });

    vi.resetModules();
    await import("./explorerBootstrap");

    expect(createExplorerAppImpl).toHaveBeenCalledTimes(1);
    expect(postMessage).toHaveBeenCalledWith({ type: "ready" });

    const initialContextPromise = capturedTransport!.getInitialContext();
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "hostReady",
          initialContext: {
            rootId: "file:///ws-a",
            availableRoots: [
              { id: "file:///ws-a", name: "workspace-a" },
              { id: "file:///ws-b", name: "workspace-b" },
            ],
            focusDocumentPath: "specs/proj/requirements.md",
          },
        },
      }),
    );

    await expect(initialContextPromise).resolves.toEqual({
      rootId: "file:///ws-a",
      availableRoots: [
        { id: "file:///ws-a", name: "workspace-a" },
        { id: "file:///ws-b", name: "workspace-b" },
      ],
      focusDocumentPath: "specs/proj/requirements.md",
    });
    expect(setState).toHaveBeenCalledWith({ clientKey: "file:///ws-a" });

    const snapshotPromise = capturedTransport!.loadSnapshot("file:///ws-b");
    const requestMessage = postMessage.mock.calls.find(
      ([message]) =>
        (message as { type?: string; method?: string }).type === "request" &&
        (message as { method?: string }).method === "loadSnapshot",
    )?.[0] as
      | {
          requestId: number;
          type: string;
        }
      | undefined;
    expect(requestMessage).toBeDefined();

    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "response",
          requestId: requestMessage!.requestId,
          result: { revision: "rev-2", documents: [], edges: [] },
        },
      }),
    );

    await expect(snapshotPromise).resolves.toEqual({
      revision: "rev-2",
      documents: [],
      edges: [],
    });
    expect(setState).toHaveBeenLastCalledWith({ clientKey: "file:///ws-a" });

    expect(
      decodeCommandHref(capturedOptions!.linkResolver!.evidenceLink("src/main.rs", 42)),
    ).toEqual({
      command: `command:${OPEN_GRAPH_FILE_COMMAND}`,
      args: [
        {
          path: "src/main.rs",
          line: 42,
          folderUri: "file:///ws-a",
        },
      ],
    });

    capturedTransport!.commitRoot("file:///ws-b");

    expect(setState).toHaveBeenLastCalledWith({ clientKey: "file:///ws-b" });
    expect(
      decodeCommandHref(capturedOptions!.linkResolver!.evidenceLink("src/main.rs", 42)),
    ).toEqual({
      command: `command:${OPEN_GRAPH_FILE_COMMAND}`,
      args: [
        {
          path: "src/main.rs",
          line: 42,
          folderUri: "file:///ws-b",
        },
      ],
    });

    const unsubscribe = capturedTransport!.subscribeChanges(changeListener);
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "explorerChanged",
          event: {
            revision: "rev-3",
            changed_document_ids: ["proj/requirements"],
            removed_document_ids: [],
          },
        },
      }),
    );
    expect(changeListener).toHaveBeenCalledWith({
      revision: "rev-3",
      changed_document_ids: ["proj/requirements"],
      removed_document_ids: [],
    });
    unsubscribe();

    const contextListener = vi.fn();
    const unsubscribeContext = capturedTransport!.subscribeContext(contextListener);
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "hostContextChanged",
          context: {
            rootId: "file:///ws-a",
            availableRoots: [
              { id: "file:///ws-a", name: "workspace-a" },
              { id: "file:///ws-b", name: "workspace-b" },
              { id: "file:///ws-c", name: "workspace-c" },
            ],
          },
        },
      }),
    );
    expect(contextListener).toHaveBeenCalledWith({
      rootId: "file:///ws-a",
      availableRoots: [
        { id: "file:///ws-a", name: "workspace-a" },
        { id: "file:///ws-b", name: "workspace-b" },
        { id: "file:///ws-c", name: "workspace-c" },
      ],
    });
    unsubscribeContext();
  });

  it("keeps the previous root state when a snapshot switch request fails", async () => {
    let capturedTransport:
      | {
          getInitialContext: () => Promise<unknown>;
          loadSnapshot: (rootId: string) => Promise<unknown>;
          commitRoot: (rootId: string) => void;
        }
      | undefined;
    let capturedOptions:
      | {
          linkResolver?: { evidenceLink: (file: string, line: number) => string };
        }
      | undefined;

    createExplorerAppImpl = vi.fn((container, transport, options) => {
      expect(container).toBe(bootstrapContainer);
      capturedTransport = transport as typeof capturedTransport;
      capturedOptions = options as typeof capturedOptions;
      return { destroy: vi.fn() };
    });

    vi.resetModules();
    await import("./explorerBootstrap");

    const initialContextPromise = capturedTransport!.getInitialContext();
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "hostReady",
          initialContext: {
            rootId: "file:///ws-a",
            availableRoots: [
              { id: "file:///ws-a", name: "workspace-a" },
              { id: "file:///ws-b", name: "workspace-b" },
            ],
          },
        },
      }),
    );

    await expect(initialContextPromise).resolves.toEqual({
      rootId: "file:///ws-a",
      availableRoots: [
        { id: "file:///ws-a", name: "workspace-a" },
        { id: "file:///ws-b", name: "workspace-b" },
      ],
    });

    const snapshotPromise = capturedTransport!.loadSnapshot("file:///ws-b");
    const requestMessage = [...postMessage.mock.calls]
      .reverse()
      .find((call) => {
        const [message] = call as unknown[];
        return (
          (message as { type?: string; method?: string }).type === "request" &&
          (message as { method?: string }).method === "loadSnapshot"
        );
      })?.[0] as
      | {
          requestId: number;
          type: string;
        }
      | undefined;
    expect(requestMessage).toBeDefined();

    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "response",
          requestId: requestMessage!.requestId,
          error: "No running Supersigil client for file:///ws-b",
        },
      }),
    );

    await expect(snapshotPromise).rejects.toThrow(
      "No running Supersigil client for file:///ws-b",
    );

    expect(
      setState.mock.calls.some(
        ([state]) => (state as { clientKey?: string }).clientKey === "file:///ws-b",
      ),
    ).toBe(false);
    expect(
      decodeCommandHref(capturedOptions!.linkResolver!.evidenceLink("src/main.rs", 42)),
    ).toEqual({
      command: `command:${OPEN_GRAPH_FILE_COMMAND}`,
      args: [
        {
          path: "src/main.rs",
          line: 42,
          folderUri: "file:///ws-a",
        },
      ],
    });
  });
});
