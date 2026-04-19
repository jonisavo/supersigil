/// <reference lib="dom" />
import {
  buildCriterionLink,
  buildDocumentLink,
  buildOpenFileCommandHref,
} from "./explorerLinks";

// ---------------------------------------------------------------------------
// Webview bootstrap (runs only inside the VS Code webview)
// ---------------------------------------------------------------------------

declare function acquireVsCodeApi(): {
  postMessage(msg: unknown): void;
  getState(): unknown;
  setState(state: unknown): void;
};

declare const SupersigilExplorer: {
  createExplorerApp(
    container: HTMLElement,
    transport: unknown,
    options?: unknown,
  ): { destroy: () => void } | undefined;
  mount?(
    container: HTMLElement,
    data: unknown,
    renderData: unknown,
    repositoryInfo: unknown,
    linkResolver?: unknown,
    runtimeOptions?: unknown,
  ): { unmount: () => void } | undefined;
}

if (typeof acquireVsCodeApi === "function") {
  const vscode = acquireVsCodeApi();
  const container = document.getElementById("explorer")!;

  let currentRootUri: string | undefined;
  let nextRequestId = 1;

  const pendingRequests = new Map<
    number,
    {
      resolve: (value: unknown) => void;
      reject: (reason?: unknown) => void;
    }
  >();
  const changeListeners = new Set<(event: unknown) => void>();
  const contextListeners = new Set<(context: unknown) => void>();

  let resolveInitialContext: ((value: unknown) => void) | null = null;
  const initialContextPromise = new Promise<unknown>((resolve) => {
    resolveInitialContext = resolve;
  });

  const runtimeLinkResolver = {
    evidenceLink(file: string, line: number) {
      return buildOpenFileCommandHref({
        path: file,
        line,
        folderUri: currentRootUri,
      });
    },
    documentLink: buildDocumentLink,
    criterionLink: buildCriterionLink,
  };

  function setCurrentRoot(rootId: string | undefined) {
    currentRootUri = rootId;
    if (rootId) {
      vscode.setState({ clientKey: rootId });
    }
  }

  function sendRequest(method: string, params: unknown) {
    const requestId = nextRequestId++;
    const promise = new Promise<unknown>((resolve, reject) => {
      pendingRequests.set(requestId, { resolve, reject });
    });
    vscode.postMessage({
      type: "request",
      requestId,
      method,
      params,
    });
    return promise;
  }

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (msg.type === "hostReady") {
      setCurrentRoot(msg.initialContext?.rootId);
      resolveInitialContext?.(msg.initialContext);
      resolveInitialContext = null;
      return;
    }

    if (msg.type === "response") {
      const pending = pendingRequests.get(msg.requestId);
      if (!pending) {
        return;
      }
      pendingRequests.delete(msg.requestId);
      if (msg.error) {
        pending.reject(new Error(String(msg.error)));
      } else {
        pending.resolve(msg.result);
      }
      return;
    }

    if (msg.type === "explorerChanged") {
      for (const listener of changeListeners) {
        listener(msg.event);
      }
    }

    if (msg.type === "hostContextChanged") {
      if (typeof msg.context?.rootId === "string") {
        setCurrentRoot(msg.context.rootId);
      }
      for (const listener of contextListeners) {
        listener(msg.context);
      }
    }
  });

  SupersigilExplorer.createExplorerApp?.(
    container,
    {
      getInitialContext() {
        return initialContextPromise;
      },
      loadSnapshot(rootId: string) {
        return sendRequest("loadSnapshot", { rootId });
      },
      commitRoot(rootId: string) {
        setCurrentRoot(rootId);
        vscode.postMessage({
          type: "commitRoot",
          rootId,
        });
      },
      loadDocument(input: {
        rootId: string;
        revision: string;
        documentId: string;
      }) {
        return sendRequest("loadDocument", input);
      },
      subscribeChanges(listener: (event: unknown) => void) {
        changeListeners.add(listener);
        return () => {
          changeListeners.delete(listener);
        };
      },
      subscribeContext(listener: (context: unknown) => void) {
        contextListeners.add(listener);
        return () => {
          contextListeners.delete(listener);
        };
      },
      openFile(target: { path?: string; uri?: string; line?: number }) {
        vscode.postMessage({
          type: "openFile",
          path: target.path,
          uri: target.uri,
          line: target.line,
        });
      },
    },
    {
      linkResolver: runtimeLinkResolver,
    },
  );

  vscode.postMessage({ type: "ready" });
}
