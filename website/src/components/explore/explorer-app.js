import { mount as mountGraphExplorer } from './graph-explorer.js';
import { buildHash, parseHash } from './url-router.js';

/**
 * Convert an ExplorerSnapshot payload into the graph shell shape expected by
 * the existing explorer renderer.
 *
 * @param {{
 *   documents?: Array<{
 *     id: string,
 *     doc_type?: string|null,
 *     status?: string|null,
 *     title: string,
 *     path?: string|null,
 *     file_uri?: string|null,
 *     project?: string|null,
 *     coverage_summary?: { verified: number, total: number } | null,
 *     graph_components?: Array<{
 *       id?: string|null,
 *       kind: string,
 *       body?: string|null,
 *       parent_component_id?: string|null,
 *       implements?: string[]
 *     }>
 *   }>,
 *   edges?: Array<{ from: string, to: string, kind: string }>
 * }} snapshot
 * @returns {{ documents: any[], edges: any[] }}
 */
export function snapshotToGraphData(snapshot) {
  return {
    documents: (snapshot.documents ?? []).map((doc) => {
      /** @type {Map<string, any>} */
      const byId = new Map();
      /** @type {any[]} */
      const topLevelComponents = [];

      for (const component of doc.graph_components ?? []) {
        if (!component.id) {
          continue;
        }

        const mapped = {
          id: component.id,
          kind: component.kind,
          body: component.body ?? null,
        };
        if (component.implements) {
          mapped.implements = component.implements;
        }
        byId.set(component.id, mapped);
      }

      for (const component of doc.graph_components ?? []) {
        if (!component.id) {
          continue;
        }

        const mapped = byId.get(component.id);
        if (component.parent_component_id) {
          const parent = byId.get(component.parent_component_id);
          if (parent) {
            parent.children ??= [];
            parent.children.push(mapped);
            continue;
          }
        }
        topLevelComponents.push(mapped);
      }

      return {
        id: doc.id,
        doc_type: doc.doc_type ?? null,
        status: doc.status ?? null,
        title: doc.title,
        path: doc.path ?? null,
        file_uri: doc.file_uri ?? null,
        project: doc.project ?? null,
        coverage_summary: doc.coverage_summary ?? null,
        components: topLevelComponents,
      };
    }),
    edges: [...(snapshot.edges ?? [])],
  };
}

function createDefaultRenderer({ repositoryInfo = null, linkResolver } = {}) {
  return {
    mount({
      container,
      graphData,
      getRenderData,
      getDocumentState,
      openFile,
      onSelectDocument,
      onSwitchRoot,
      rootContext,
    }) {
      let currentRuntimeOptions = {
        getRenderData,
        getDocumentState,
        openFile,
        onSelectDocument,
        onSwitchRoot,
        rootContext,
      };
      let currentGraphData = graphData;
      let handle = mountGraphExplorer(
        container,
        currentGraphData,
        getRenderData(),
        repositoryInfo,
        linkResolver,
        currentRuntimeOptions,
      );

      return {
        refreshDetail() {
          handle.refreshDetail?.();
        },
        updateRuntimeOptions(nextRuntimeOptions) {
          currentRuntimeOptions = {
            ...currentRuntimeOptions,
            ...nextRuntimeOptions,
          };
          handle.updateRuntimeOptions?.(nextRuntimeOptions);
        },
        replaceGraphData(nextGraphData) {
          handle.unmount();
          currentGraphData = nextGraphData;
          handle = mountGraphExplorer(
            container,
            currentGraphData,
            getRenderData(),
            repositoryInfo,
            linkResolver,
            currentRuntimeOptions,
          );
        },
        destroy() {
          handle.unmount();
        },
      };
    },
  };
}

/**
 * Create a long-lived graph explorer application that owns snapshot and
 * document-detail state for a host container.
 *
 * @param {HTMLElement} container
 * @param {{
 *   getInitialContext: () => Promise<{
 *     rootId: string,
 *     availableRoots?: Array<{ id: string, name: string }>,
 *     focusDocumentId?: string,
 *     focusDocumentPath?: string
 *   }>,
 *   loadSnapshot: (rootId: string) => Promise<any>,
 *   loadDocument: (input: { rootId: string, revision: string, documentId: string }) => Promise<any>,
 *   commitRoot?: (rootId: string) => void,
 *   subscribeChanges?: (listener: (event: any) => void) => (() => void),
 *   subscribeContext?: (listener: (context: { rootId?: string, availableRoots?: Array<{ id: string, name: string }> }) => void) => (() => void),
 *   openFile?: (target: { path?: string, uri?: string, line?: number }) => void,
 * }} transport
 * @param {{ renderer?: { mount: (args: any) => { refreshDetail?: () => void, replaceGraphData?: (graphData: any) => void, updateRuntimeOptions?: (options: any) => void, destroy: () => void } }, repositoryInfo?: any, linkResolver?: any }} [options]
 * @returns {{ destroy(): void }}
 */
export function createExplorerApp(container, transport, options = {}) {
  const renderer = options.renderer ?? createDefaultRenderer(options);

  /** @type {{ refreshDetail?: () => void, updateRuntimeOptions?: (options: any) => void, destroy: () => void } | null} */
  let renderHandle = null;
  /** @type {(() => void) | null} */
  let unsubscribeChanges = null;
  /** @type {(() => void) | null} */
  let unsubscribeContext = null;
  /** @type {string | null} */
  let activeRootId = null;
  /** @type {{ revision: string } | null} */
  let activeSnapshot = null;
  /** @type {string | null} */
  let selectedDocumentId = null;
  /** @type {Array<{ id: string, name: string }>} */
  let availableRoots = [];
  let destroyed = false;
  let snapshotRequestVersion = 0;
  let pendingRootId = null;
  let pendingRootChangeEvent = null;
  let pendingActiveRootChangeEvent = null;

  /** @type {Map<string, any>} */
  const documentCache = new Map();

  function getRenderData() {
    return [...documentCache.values()]
      .filter((entry) => entry.state === 'ready')
      .map((entry) => entry.document);
  }

  function getDocumentState(documentId) {
    return activeSnapshot ? documentCache.get(documentId) ?? { state: 'idle' } : { state: 'idle' };
  }

  function preserveReadyDocumentEntry(entry, revision) {
    const nextEntry = {
      state: 'ready',
      revision,
      document: entry.document,
      updating: true,
    };
    if (entry.promise) {
      nextEntry.promise = entry.promise;
    }
    if (entry.requestRevision) {
      nextEntry.requestRevision = entry.requestRevision;
    }
    return nextEntry;
  }

  function reconcileDocumentCache(nextRevision, changedIds, removedIds, preservedDocumentId = null) {
    const previousRevision = activeSnapshot?.revision;
    if (!previousRevision) {
      documentCache.clear();
      return;
    }

    if (previousRevision === nextRevision) {
      for (const documentId of changedIds) {
        const entry = documentCache.get(documentId);
        if (documentId === preservedDocumentId && entry?.state === 'ready') {
          documentCache.set(documentId, preserveReadyDocumentEntry(entry, nextRevision));
          continue;
        }
        documentCache.delete(documentId);
      }
      for (const documentId of removedIds) {
        documentCache.delete(documentId);
      }
      return;
    }

    const nextCache = new Map();
    for (const [documentId, entry] of documentCache.entries()) {
      if (removedIds.has(documentId)) {
        continue;
      }
      if (changedIds.has(documentId)) {
        if (documentId === preservedDocumentId && entry.state === 'ready') {
          nextCache.set(documentId, preserveReadyDocumentEntry(entry, nextRevision));
        }
        continue;
      }
      if (entry.state === 'ready') {
        if (entry.updating) {
          nextCache.set(documentId, preserveReadyDocumentEntry(entry, nextRevision));
          continue;
        }
        nextCache.set(documentId, {
          state: 'ready',
          revision: nextRevision,
          document: {
            ...entry.document,
            revision: nextRevision,
          },
        });
        continue;
      }
      if (entry.state === 'loading') {
        nextCache.set(documentId, {
          ...entry,
          revision: nextRevision,
        });
      }
    }

    documentCache.clear();
    for (const [documentId, entry] of nextCache.entries()) {
      documentCache.set(documentId, entry);
    }
  }

  function setHashFromFocusDocument(focusDocumentId) {
    const currentState = parseHash(window.location.hash);
    if (currentState.doc || !focusDocumentId) {
      return currentState.doc ?? focusDocumentId;
    }
    const hash = buildHash({ doc: focusDocumentId, trace: false, filter: currentState.filter });
    history.replaceState(null, '', hash || `${location.pathname}${location.search}`);
    return focusDocumentId;
  }

  function normalizeToIndexHash() {
    history.replaceState(null, '', `${location.pathname}${location.search}`);
  }

  function mergeChangeEvents(current, next) {
    if (!current) {
      return next;
    }
    return {
      rootId: next.rootId ?? current.rootId,
      revision: next.revision ?? current.revision,
      changed_document_ids: [
        ...new Set([
          ...(current.changed_document_ids ?? []),
          ...(next.changed_document_ids ?? []),
        ]),
      ],
      removed_document_ids: [
        ...new Set([
          ...(current.removed_document_ids ?? []),
          ...(next.removed_document_ids ?? []),
        ]),
      ],
    };
  }

  function renderExplorerError(message, error) {
    console.error(message, error);
    container.innerHTML = `<div class="explorer-error">
      <p>Unable to load explorer data.</p>
      <p><small>${error instanceof Error ? error.message : String(error)}</small></p>
    </div>`;
  }

  function isPendingDocumentLoad(entry, promise) {
    return Boolean(
      entry?.promise === promise &&
      (entry.state === 'loading' || (entry.state === 'ready' && entry.updating))
    );
  }

  function clearPendingDocumentLoad(documentId, promise, { refresh = true } = {}) {
    const current = documentCache.get(documentId);
    if (current?.state === 'loading' && current.promise === promise) {
      documentCache.delete(documentId);
    } else if (current?.state === 'ready' && current.updating && current.promise === promise) {
      documentCache.set(documentId, {
        state: 'ready',
        revision: current.revision,
        document: current.document,
      });
    } else {
      return;
    }

    if (refresh) {
      renderHandle?.refreshDetail?.();
    }
  }

  function ensureDocumentLoaded(documentId, { refresh = true } = {}) {
    if (!activeSnapshot || !activeRootId) {
      return Promise.resolve(null);
    }

    const revision = activeSnapshot.revision;
    const existing = documentCache.get(documentId);
    if (existing?.state === 'ready' && !existing.updating) {
      return Promise.resolve(existing.document);
    }
    if (existing?.state === 'loading') {
      return existing.promise;
    }
    if (existing?.state === 'ready' && existing.updating && existing.promise) {
      return existing.promise;
    }

    const rootIdAtRequest = activeRootId;
    const promise = transport.loadDocument({
      rootId: rootIdAtRequest,
      revision,
      documentId,
    });

    documentCache.set(
      documentId,
      existing?.state === 'ready'
        ? {
            state: 'ready',
            revision,
            document: existing.document,
            updating: true,
            promise,
            requestRevision: revision,
          }
        : {
            state: 'loading',
            revision,
            promise,
            requestRevision: revision,
          },
    );

    if (refresh) {
      renderHandle?.refreshDetail?.();
    }

    promise
      .then((document) => {
        if (document?.document_id !== documentId) {
          clearPendingDocumentLoad(documentId, promise);
          return document;
        }

        if (
          destroyed ||
          !activeSnapshot ||
          activeRootId !== rootIdAtRequest
        ) {
          return document;
        }

        const current = documentCache.get(documentId);
        if (!isPendingDocumentLoad(current, promise)) {
          return document;
        }

        const activeRevision = activeSnapshot.revision;
        if (document?.revision !== activeRevision) {
          const shouldRetry =
            current.requestRevision !== activeRevision && selectedDocumentId === documentId;
          clearPendingDocumentLoad(documentId, promise, { refresh: false });
          if (shouldRetry) {
            void ensureDocumentLoaded(documentId);
          } else {
            renderHandle?.refreshDetail?.();
          }
          return document;
        }

        documentCache.set(documentId, {
          state: 'ready',
          revision: activeRevision,
          document: {
            ...document,
            revision: activeRevision,
          },
        });
        renderHandle?.refreshDetail?.();
        return document;
      })
      .catch((error) => {
        const current = documentCache.get(documentId);
        if (
          destroyed ||
          !activeSnapshot ||
          activeRootId !== rootIdAtRequest ||
          !isPendingDocumentLoad(current, promise)
        ) {
          return;
        }

        documentCache.set(documentId, {
          state: 'error',
          revision: activeSnapshot.revision,
          error: error instanceof Error ? error.message : String(error),
        });
        renderHandle?.refreshDetail?.();
      });

    return promise;
  }

  async function refreshActiveRoot(event = null) {
    if (destroyed || !activeRootId || pendingRootId) {
      return;
    }
    if (event?.rootId && event.rootId !== activeRootId) {
      return;
    }

    const rootId = activeRootId;
    const requestVersion = ++snapshotRequestVersion;
    let nextSnapshot;
    try {
      nextSnapshot = await transport.loadSnapshot(rootId);
    } catch (error) {
      if (!destroyed && requestVersion === snapshotRequestVersion && !pendingRootId) {
        console.error('Explorer change refresh failed:', error);
      }
      return;
    }
    if (
      destroyed ||
      requestVersion !== snapshotRequestVersion ||
      activeRootId !== rootId ||
      pendingRootId
    ) {
      return;
    }

    const changedIds = new Set(event?.changed_document_ids ?? []);
    const removedIds = new Set(event?.removed_document_ids ?? []);
    reconcileDocumentCache(nextSnapshot.revision, changedIds, removedIds, selectedDocumentId);
    activeSnapshot = nextSnapshot;

    renderHandle?.replaceGraphData?.(snapshotToGraphData(nextSnapshot));

    if (selectedDocumentId && removedIds.has(selectedDocumentId)) {
      selectedDocumentId = null;
      normalizeToIndexHash();
      renderHandle?.refreshDetail?.();
      return;
    }

    if (selectedDocumentId) {
      const selectedState = documentCache.get(selectedDocumentId);
      if (changedIds.has(selectedDocumentId) || selectedState?.state !== 'ready') {
        void ensureDocumentLoaded(selectedDocumentId);
        return;
      }
    }

    renderHandle?.refreshDetail?.();
  }

  function updateRuntimeContext(nextContext = {}) {
    availableRoots = nextContext.availableRoots ?? availableRoots;
    if (!pendingRootId && nextContext.rootId) {
      activeRootId = nextContext.rootId;
    }
    renderHandle?.updateRuntimeOptions?.({
      rootContext: {
        activeRootId,
        availableRoots,
      },
      openFile: transport.openFile,
    });
  }

  async function switchRoot(nextRootId) {
    if (
      destroyed ||
      !nextRootId ||
      nextRootId === activeRootId ||
      nextRootId === pendingRootId ||
      !availableRoots.some((root) => root.id === nextRootId)
    ) {
      return;
    }

    const requestVersion = ++snapshotRequestVersion;
    const previousSnapshot = activeSnapshot;
    pendingRootId = nextRootId;
    let snapshot;
    try {
      snapshot = await transport.loadSnapshot(nextRootId);
    } catch {
      if (pendingRootId === nextRootId) {
        pendingRootId = null;
      }
      if (destroyed || requestVersion !== snapshotRequestVersion) {
        return;
      }

      pendingRootChangeEvent = null;
      if (pendingActiveRootChangeEvent) {
        const queuedChangeEvent = pendingActiveRootChangeEvent;
        pendingActiveRootChangeEvent = null;
        pendingRootChangeEvent = null;
        void refreshActiveRoot(queuedChangeEvent);
        return;
      }

      if (previousSnapshot) {
        renderHandle?.replaceGraphData?.(snapshotToGraphData(previousSnapshot));
        renderHandle?.refreshDetail?.();
      }
      return;
    }

    if (
      destroyed ||
      requestVersion !== snapshotRequestVersion ||
      pendingRootId !== nextRootId
    ) {
      return;
    }

    const queuedTargetRootChangeEvent = pendingRootChangeEvent;
    pendingRootId = null;
    pendingRootChangeEvent = null;
    pendingActiveRootChangeEvent = null;
    activeRootId = nextRootId;
    activeSnapshot = snapshot;
    selectedDocumentId = null;
    documentCache.clear();
    normalizeToIndexHash();
    transport.commitRoot?.(nextRootId);
    renderHandle?.updateRuntimeOptions?.({
      rootContext: {
        activeRootId,
        availableRoots,
      },
      openFile: transport.openFile,
    });
    renderHandle?.replaceGraphData?.(snapshotToGraphData(snapshot));
    if (queuedTargetRootChangeEvent) {
      void refreshActiveRoot(queuedTargetRootChangeEvent);
      return;
    }
    renderHandle?.refreshDetail?.();
  }

  void (async () => {
    try {
      const initialContext = await transport.getInitialContext();
      if (destroyed) return;

      activeRootId = initialContext.rootId;
      availableRoots = initialContext.availableRoots ?? [];
      let initialDocumentId = null;
      if (initialContext.focusDocumentId) {
        initialDocumentId = setHashFromFocusDocument(initialContext.focusDocumentId);
      }

      const requestVersion = ++snapshotRequestVersion;
      const snapshot = await transport.loadSnapshot(initialContext.rootId);
      if (
        destroyed ||
        requestVersion !== snapshotRequestVersion ||
        activeRootId !== initialContext.rootId
      ) {
        return;
      }

      activeSnapshot = snapshot;

      if (!initialDocumentId && initialContext.focusDocumentPath) {
        const focusedDocument = snapshot.documents?.find(
          (document) => document.path === initialContext.focusDocumentPath,
        );
        if (focusedDocument) {
          initialDocumentId = setHashFromFocusDocument(focusedDocument.id);
        }
      }

      if (initialDocumentId) {
        selectedDocumentId = initialDocumentId;
        ensureDocumentLoaded(initialDocumentId, { refresh: false });
      }

      renderHandle = renderer.mount({
        container,
        graphData: snapshotToGraphData(snapshot),
        getRenderData,
        getDocumentState,
        openFile: transport.openFile,
        rootContext: {
          activeRootId,
          availableRoots,
        },
        onSelectDocument(documentId) {
          selectedDocumentId = documentId;
          void ensureDocumentLoaded(documentId);
        },
        onSwitchRoot(rootId) {
          void switchRoot(rootId);
        },
      });

      if (typeof transport.subscribeContext === 'function') {
        unsubscribeContext = transport.subscribeContext((nextContext) => {
          if (destroyed) {
            return;
          }
          updateRuntimeContext(nextContext);
        });
      }

      if (typeof transport.subscribeChanges === 'function') {
        unsubscribeChanges = transport.subscribeChanges(async (event) => {
          if (destroyed || !activeRootId) {
            return;
          }

          if (pendingRootId) {
            const eventRootId = event.rootId ?? activeRootId;
            if (eventRootId === pendingRootId) {
              pendingRootChangeEvent = mergeChangeEvents(pendingRootChangeEvent, event);
            } else if (eventRootId === activeRootId) {
              pendingActiveRootChangeEvent = mergeChangeEvents(
                pendingActiveRootChangeEvent,
                event,
              );
            }
            return;
          }

          void refreshActiveRoot(event);
        });
      }
    } catch (error) {
      if (!destroyed) {
        renderExplorerError('Explorer bootstrap failed:', error);
      }
    }
  })();

  return {
    destroy() {
      if (destroyed) return;
      destroyed = true;
      unsubscribeChanges?.();
      unsubscribeContext?.();
      renderHandle?.destroy();
    },
  };
}
