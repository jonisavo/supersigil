/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createExplorerApp } from './explorer-app.js';

function createDeferred() {
  /** @type {(value: any) => void} */
  let resolve;
  /** @type {(reason?: unknown) => void} */
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function flushAsyncWork() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function makeSummary({
  id = 'specs/auth/req',
  doc_type = 'requirements',
  status = 'draft',
  title = 'Auth Requirements',
  path = 'specs/auth/auth.req.md',
  file_uri = null,
  project = null,
  coverage_summary = { verified: 0, total: 1 },
  component_count = 1,
  graph_components = [{ id: 'req-1', kind: 'Criterion', body: 'Authenticate users' }],
} = {}) {
  return {
    id,
    doc_type,
    status,
    title,
    path,
    file_uri,
    project,
    coverage_summary,
    component_count,
    graph_components,
  };
}

function makeSnapshot({ revision = 'rev-1', documents = [makeSummary()], edges = [] } = {}) {
  return { revision, documents, edges };
}

function makeDocument({
  revision = 'rev-1',
  document_id = 'specs/auth/req',
  stale = false,
  verificationState = 'verified',
  fences = [{ components: [{ kind: 'Criterion', verification: { state: verificationState } }] }],
  edges = [],
} = {}) {
  return {
    revision,
    document_id,
    stale,
    fences,
    edges,
  };
}

function createRendererCapture() {
  /** @type {any} */
  let mountArgs;
  const handle = {
    refreshDetail: vi.fn(),
    replaceGraphData: vi.fn(),
    updateRuntimeOptions: vi.fn(),
    destroy: vi.fn(),
  };

  return {
    handle,
    renderer: {
      mount: vi.fn((args) => {
        mountArgs = args;
        return handle;
      }),
    },
    get mountArgs() {
      return mountArgs;
    },
  };
}

function createContainer() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  return container;
}

function makeAuthAndDesignSummaries() {
  const authSummary = makeSummary();
  const designSummary = makeSummary({
    id: 'specs/auth/design',
    doc_type: 'design',
    title: 'Auth Design',
    path: 'specs/auth/auth.design.md',
    coverage_summary: { verified: 0, total: 0 },
    component_count: 0,
    graph_components: [],
  });
  return { authSummary, designSummary };
}

describe('createExplorerApp', () => {
  afterEach(() => {
    document.body.innerHTML = '';
    window.location.hash = '';
    vi.restoreAllMocks();
  });

  it('mounts from snapshot data and starts loading the focused document without waiting for workspace detail batches', async () => {
    const summary = makeSummary({
      file_uri: 'file:///workspace/specs/auth/auth.req.md',
      coverage_summary: { verified: 1, total: 2 },
    });
    const snapshot = makeSnapshot({ documents: [summary] });
    const loadDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
        focusDocumentId: summary.id,
      })),
      loadSnapshot: vi.fn(async () => snapshot),
      loadDocument: vi.fn(() => loadDocumentDeferred.promise),
      subscribeChanges: vi.fn(() => () => {}),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    expect(rendererCapture.renderer.mount).toHaveBeenCalledTimes(1);
    expect(rendererCapture.mountArgs.graphData).toEqual({
      documents: [
        {
          id: summary.id,
          doc_type: summary.doc_type,
          status: summary.status,
          title: summary.title,
          path: summary.path,
          file_uri: summary.file_uri,
          project: summary.project,
          coverage_summary: summary.coverage_summary,
          components: summary.graph_components,
        },
      ],
      edges: [],
    });
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('loading');
    expect(transport.loadDocument).toHaveBeenCalledWith({
      rootId: 'workspace-a',
      revision: snapshot.revision,
      documentId: summary.id,
    });

    loadDocumentDeferred.resolve(makeDocument({ revision: snapshot.revision, document_id: summary.id }));
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).document.document_id).toBe(summary.id);
    expect(rendererCapture.handle.refreshDetail).toHaveBeenCalled();
  });

  it('resolves an initial focus document from the focused path after the snapshot loads', async () => {
    const summary = makeSummary({ file_uri: 'file:///workspace/specs/auth/auth.req.md' });
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
        focusDocumentPath: summary.path,
      })),
      loadSnapshot: vi.fn(async () => makeSnapshot({ documents: [summary] })),
      loadDocument: vi.fn().mockResolvedValue(makeDocument({ document_id: summary.id })),
      subscribeChanges: vi.fn(() => () => {}),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    expect(window.location.hash).toBe('#/doc/specs/auth/req');
    expect(transport.loadDocument).toHaveBeenCalledWith({
      rootId: 'workspace-a',
      revision: 'rev-1',
      documentId: summary.id,
    });
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('ready');
  });

  it('coalesces duplicate interactive document loads for the active revision', async () => {
    const summary = makeSummary();
    const loadDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi.fn(async () => makeSnapshot({ documents: [summary] })),
      loadDocument: vi.fn(() => loadDocumentDeferred.promise),
      subscribeChanges: vi.fn(() => () => {}),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(summary.id);
    rendererCapture.mountArgs.onSelectDocument(summary.id);

    expect(transport.loadDocument).toHaveBeenCalledTimes(1);
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('loading');

    loadDocumentDeferred.resolve(makeDocument({ document_id: summary.id }));
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(summary.id);

    expect(transport.loadDocument).toHaveBeenCalledTimes(1);
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('ready');
  });

  it('drops mismatched detail responses without leaving the document stuck loading', async () => {
    const summary = makeSummary();
    const loadDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi.fn(async () => makeSnapshot({ documents: [summary] })),
      loadDocument: vi.fn(() => loadDocumentDeferred.promise),
      subscribeChanges: vi.fn(() => () => {}),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(summary.id);
    expect(rendererCapture.mountArgs.getDocumentState(summary.id).state).toBe('loading');

    loadDocumentDeferred.resolve(
      makeDocument({ document_id: 'specs/other/req', revision: 'rev-1' }),
    );
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(summary.id)).toEqual({ state: 'idle' });
  });

  it('updates available roots when host context changes after mount', async () => {
    const authSummary = makeSummary();
    const billingSummary = makeSummary({
      id: 'specs/billing/req',
      title: 'Billing Requirements',
      path: 'specs/billing/billing.req.md',
    });
    const rendererCapture = createRendererCapture();
    /** @type {(context: { rootId?: string, availableRoots?: Array<{ id: string, name: string }> }) => void} */
    let contextListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-a', documents: [authSummary] }))
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-b', documents: [billingSummary] })),
      loadDocument: vi.fn(),
      subscribeChanges: vi.fn(() => () => {}),
      subscribeContext: vi.fn((listener) => {
        contextListener = listener;
        return () => {};
      }),
      commitRoot: vi.fn(),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    contextListener({
      rootId: 'workspace-a',
      availableRoots: [
        { id: 'workspace-a', name: 'Workspace A' },
        { id: 'workspace-b', name: 'Workspace B' },
      ],
    });
    await flushAsyncWork();

    expect(rendererCapture.handle.updateRuntimeOptions).toHaveBeenCalledWith({
      rootContext: {
        activeRootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      },
      openFile: undefined,
    });

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenNthCalledWith(2, 'workspace-b');
    expect(transport.commitRoot).toHaveBeenCalledWith('workspace-b');
  });

  it('accepts a newer detail response once the active snapshot has advanced to that revision', async () => {
    const { authSummary, designSummary } = makeAuthAndDesignSummaries();
    const loadDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(
          makeSnapshot({ revision: 'rev-1', documents: [authSummary, designSummary] }),
        )
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-2',
            documents: [
              authSummary,
              { ...designSummary, status: 'approved' },
            ],
          }),
        ),
      loadDocument: vi.fn(() => loadDocumentDeferred.promise),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('loading');

    changeListener({
      revision: 'rev-2',
      changed_document_ids: [designSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    loadDocumentDeferred.resolve(
      makeDocument({ revision: 'rev-2', document_id: authSummary.id }),
    );
    await flushAsyncWork();

    expect(transport.loadDocument).toHaveBeenCalledTimes(1);
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).document.revision).toBe('rev-2');
  });

  it('retries a carried-forward detail load after the original request rejects', async () => {
    const { authSummary, designSummary } = makeAuthAndDesignSummaries();
    const rejectedDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, rootId?: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(
          makeSnapshot({ revision: 'rev-1', documents: [authSummary, designSummary] }),
        )
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-2',
            documents: [
              authSummary,
              { ...designSummary, status: 'approved' },
            ],
          }),
        ),
      loadDocument: vi
        .fn()
        .mockImplementationOnce(() => rejectedDocumentDeferred.promise)
        .mockResolvedValueOnce(makeDocument({ revision: 'rev-2', document_id: authSummary.id })),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('loading');

    changeListener({
      revision: 'rev-2',
      changed_document_ids: [designSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    rejectedDocumentDeferred.reject(new Error('network timeout'));
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id)).toEqual({
      state: 'error',
      revision: 'rev-2',
      error: 'network timeout',
    });

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    await flushAsyncWork();

    expect(transport.loadDocument).toHaveBeenCalledTimes(2);
    expect(transport.loadDocument).toHaveBeenNthCalledWith(2, {
      rootId: 'workspace-a',
      revision: 'rev-2',
      documentId: authSummary.id,
    });
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
  });

  it('retries a carried-forward detail load when the original response resolves stale', async () => {
    const { authSummary, designSummary } = makeAuthAndDesignSummaries();
    const staleDocumentDeferred = createDeferred();
    const refreshedDocumentDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, rootId?: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(
          makeSnapshot({ revision: 'rev-1', documents: [authSummary, designSummary] }),
        )
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-2',
            documents: [
              authSummary,
              { ...designSummary, status: 'approved' },
            ],
          }),
        ),
      loadDocument: vi
        .fn()
        .mockImplementationOnce(() => staleDocumentDeferred.promise)
        .mockImplementationOnce(() => refreshedDocumentDeferred.promise),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('loading');

    changeListener({
      revision: 'rev-2',
      changed_document_ids: [designSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    staleDocumentDeferred.resolve(makeDocument({ revision: 'rev-1', document_id: authSummary.id }));
    await flushAsyncWork();

    expect(transport.loadDocument).toHaveBeenCalledTimes(2);
    expect(transport.loadDocument).toHaveBeenNthCalledWith(2, {
      rootId: 'workspace-a',
      revision: 'rev-2',
      documentId: authSummary.id,
    });
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('loading');

    refreshedDocumentDeferred.resolve(
      makeDocument({ revision: 'rev-2', document_id: authSummary.id }),
    );
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).document.revision).toBe('rev-2');
  });

  it('preserves selected detail while a changed document reloads across revisions', async () => {
    const { authSummary, designSummary } = makeAuthAndDesignSummaries();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const changedDocumentDeferred = createDeferred();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-1', documents: [authSummary, designSummary] }))
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-2',
            documents: [
              { ...authSummary, status: 'approved', coverage_summary: { verified: 1, total: 1 } },
              { ...designSummary, status: 'approved' },
            ],
          }),
        )
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-3',
            documents: [
              { ...authSummary, status: 'approved', coverage_summary: { verified: 1, total: 1 } },
              { ...designSummary, status: 'approved' },
            ],
          }),
        ),
      loadDocument: vi
        .fn()
        .mockResolvedValueOnce(makeDocument({ revision: 'rev-1', document_id: authSummary.id, verificationState: 'unverified' }))
        .mockImplementationOnce(() => changedDocumentDeferred.promise),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    await flushAsyncWork();
    const initialDocument = rendererCapture.mountArgs.getDocumentState(authSummary.id).document;
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');

    changeListener({
      revision: 'rev-2',
      changed_document_ids: [authSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenCalledWith({
      documents: [
        {
          id: authSummary.id,
          doc_type: authSummary.doc_type,
          status: 'approved',
          title: authSummary.title,
          path: authSummary.path,
          file_uri: authSummary.file_uri,
          project: authSummary.project,
          coverage_summary: { verified: 1, total: 1 },
          components: authSummary.graph_components,
        },
        {
          id: designSummary.id,
          doc_type: designSummary.doc_type,
          status: 'approved',
          title: designSummary.title,
          path: designSummary.path,
          file_uri: designSummary.file_uri,
          project: designSummary.project,
          coverage_summary: designSummary.coverage_summary,
          components: designSummary.graph_components,
        },
      ],
      edges: [],
    });
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id)).toMatchObject({
      state: 'ready',
      revision: 'rev-2',
      updating: true,
      document: initialDocument,
    });
    expect(transport.loadDocument).toHaveBeenNthCalledWith(2, {
      rootId: 'workspace-a',
      revision: 'rev-2',
      documentId: authSummary.id,
    });

    changedDocumentDeferred.resolve(
      makeDocument({ revision: 'rev-2', document_id: authSummary.id }),
    );
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).document.revision).toBe('rev-2');

    changeListener({
      revision: 'rev-3',
      changed_document_ids: [designSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(3);
    expect(transport.loadDocument).toHaveBeenCalledTimes(2);
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).document.revision).toBe('rev-3');
  });

  it('preserves the current snapshot and detail state when a root switch fails', async () => {
    const authSummary = makeSummary();
    const authSnapshot = makeSnapshot({ revision: 'rev-a', documents: [authSummary] });
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(authSnapshot)
        .mockRejectedValueOnce(new Error('workspace-b unavailable')),
      loadDocument: vi
        .fn()
        .mockResolvedValueOnce(
          makeDocument({ revision: 'rev-a', document_id: authSummary.id }),
        ),
      subscribeChanges: vi.fn(() => () => {}),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    await flushAsyncWork();
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenNthCalledWith(2, 'workspace-b');
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenCalledWith({
      documents: [
        {
          id: authSummary.id,
          doc_type: authSummary.doc_type,
          status: authSummary.status,
          title: authSummary.title,
          path: authSummary.path,
          file_uri: authSummary.file_uri,
          project: authSummary.project,
          coverage_summary: authSummary.coverage_summary,
          components: authSummary.graph_components,
        },
      ],
      edges: [],
    });
    expect(rendererCapture.handle.updateRuntimeOptions).not.toHaveBeenCalled();
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');
    expect(transport.loadDocument).toHaveBeenCalledTimes(1);
  });

  it('does not let old-root refreshes cancel an in-flight root switch', async () => {
    const authSummary = makeSummary();
    const billingSummary = makeSummary({
      id: 'specs/billing/req',
      status: 'approved',
      title: 'Billing Requirements',
      path: 'specs/billing/billing.req.md',
      coverage_summary: { verified: 2, total: 2 },
      graph_components: [{ id: 'req-2', kind: 'Criterion', body: 'Capture invoices' }],
    });
    const switchDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-a', documents: [authSummary] }))
        .mockImplementationOnce(() => switchDeferred.promise),
      loadDocument: vi.fn(),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
      commitRoot: vi.fn(),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    changeListener({
      rootId: 'workspace-a',
      revision: 'rev-a-2',
      changed_document_ids: [authSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);

    switchDeferred.resolve(makeSnapshot({ revision: 'rev-b', documents: [billingSummary] }));
    await flushAsyncWork();

    expect(rendererCapture.handle.updateRuntimeOptions).toHaveBeenCalledWith({
      rootContext: {
        activeRootId: 'workspace-b',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      },
      openFile: undefined,
    });
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenLastCalledWith({
      documents: [
        {
          id: billingSummary.id,
          doc_type: billingSummary.doc_type,
          status: billingSummary.status,
          title: billingSummary.title,
          path: billingSummary.path,
          file_uri: billingSummary.file_uri,
          project: billingSummary.project,
          coverage_summary: billingSummary.coverage_summary,
          components: billingSummary.graph_components,
        },
      ],
      edges: [],
    });
    expect(transport.commitRoot).toHaveBeenCalledWith('workspace-b');
  });

  it('refreshes the target root after a queued target-root change arrives during switch commit', async () => {
    const authSummary = makeSummary();
    const billingSummary = makeSummary({
      id: 'specs/billing/req',
      status: 'approved',
      title: 'Billing Requirements',
      path: 'specs/billing/billing.req.md',
      coverage_summary: { verified: 2, total: 2 },
      graph_components: [{ id: 'req-2', kind: 'Criterion', body: 'Capture invoices' }],
    });
    const refreshedBillingSummary = {
      ...billingSummary,
      status: 'implemented',
      coverage_summary: { verified: 3, total: 3 },
    };
    const switchDeferred = createDeferred();
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, rootId?: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-a', documents: [authSummary] }))
        .mockImplementationOnce(() => switchDeferred.promise)
        .mockResolvedValueOnce(
          makeSnapshot({ revision: 'rev-b-2', documents: [refreshedBillingSummary] }),
        ),
      loadDocument: vi.fn(),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
      commitRoot: vi.fn(),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    changeListener({
      rootId: 'workspace-b',
      revision: 'rev-b-2',
      changed_document_ids: [billingSummary.id],
      removed_document_ids: [],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);

    switchDeferred.resolve(makeSnapshot({ revision: 'rev-b', documents: [billingSummary] }));
    await flushAsyncWork();
    await flushAsyncWork();

    expect(transport.commitRoot).toHaveBeenCalledWith('workspace-b');
    expect(transport.loadSnapshot).toHaveBeenCalledTimes(3);
    expect(transport.loadSnapshot).toHaveBeenNthCalledWith(3, 'workspace-b');
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenLastCalledWith({
      documents: [
        {
          id: refreshedBillingSummary.id,
          doc_type: refreshedBillingSummary.doc_type,
          status: refreshedBillingSummary.status,
          title: refreshedBillingSummary.title,
          path: refreshedBillingSummary.path,
          file_uri: refreshedBillingSummary.file_uri,
          project: refreshedBillingSummary.project,
          coverage_summary: refreshedBillingSummary.coverage_summary,
          components: refreshedBillingSummary.graph_components,
        },
      ],
      edges: [],
    });
  });

  it('ignores old-root invalidations that arrive after the new root is locally committed', async () => {
    const sharedSummary = makeSummary({
      id: 'specs/shared/req',
      title: 'Shared Requirements',
      path: 'specs/shared/shared.req.md',
    });
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, rootId?: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-a',
            documents: [{ ...sharedSummary, status: 'draft' }],
          }),
        )
        .mockResolvedValueOnce(
          makeSnapshot({
            revision: 'rev-b',
            documents: [{ ...sharedSummary, status: 'approved' }],
          }),
        ),
      loadDocument: vi.fn().mockResolvedValue(
        makeDocument({ revision: 'rev-b', document_id: sharedSummary.id }),
      ),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
      commitRoot: vi.fn(),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    rendererCapture.mountArgs.onSelectDocument(sharedSummary.id);
    await flushAsyncWork();

    expect(rendererCapture.mountArgs.getDocumentState(sharedSummary.id).state).toBe('ready');
    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);

    changeListener({
      rootId: 'workspace-a',
      revision: 'rev-a-2',
      changed_document_ids: [],
      removed_document_ids: [sharedSummary.id],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);
    expect(rendererCapture.mountArgs.getDocumentState(sharedSummary.id).state).toBe('ready');
    expect(rendererCapture.mountArgs.getDocumentState(sharedSummary.id).document.revision).toBe('rev-b');
  });

  it('clears the selected document and normalizes the hash when a change event removes it', async () => {
    const authSummary = makeSummary();
    const designSummary = makeSummary({
      id: 'specs/auth/design',
      doc_type: 'design',
      title: 'Auth Design',
      path: 'specs/auth/auth.design.md',
      coverage_summary: { verified: 0, total: 0 },
      component_count: 0,
      graph_components: [],
    });
    const rendererCapture = createRendererCapture();
    /** @type {(event: { revision: string, changed_document_ids: string[], removed_document_ids: string[] }) => void} */
    let changeListener;
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
        focusDocumentId: authSummary.id,
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-1', documents: [authSummary, designSummary] }))
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-2', documents: [designSummary] })),
      loadDocument: vi.fn().mockResolvedValue(
        makeDocument({ revision: 'rev-1', document_id: authSummary.id, verificationState: 'unverified' }),
      ),
      subscribeChanges: vi.fn((listener) => {
        changeListener = listener;
        return () => {};
      }),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    expect(window.location.hash).toBe('#/doc/specs/auth/req');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');

    changeListener({
      revision: 'rev-2',
      changed_document_ids: [],
      removed_document_ids: [authSummary.id],
    });
    await flushAsyncWork();

    expect(transport.loadSnapshot).toHaveBeenCalledTimes(2);
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenCalledWith({
      documents: [
        {
          id: designSummary.id,
          doc_type: designSummary.doc_type,
          status: designSummary.status,
          title: designSummary.title,
          path: designSummary.path,
          file_uri: designSummary.file_uri,
          project: designSummary.project,
          coverage_summary: designSummary.coverage_summary,
          components: [],
        },
      ],
      edges: [],
    });
    expect(window.location.hash).toBe('');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('idle');
    expect(rendererCapture.handle.refreshDetail).toHaveBeenCalled();
  });

  it('switches roots inside the shared runtime and resets revision-scoped detail state', async () => {
    const authSummary = makeSummary();
    const billingSummary = makeSummary({
      id: 'specs/billing/req',
      status: 'approved',
      title: 'Billing Requirements',
      path: 'specs/billing/billing.req.md',
      coverage_summary: { verified: 2, total: 2 },
      graph_components: [{ id: 'req-2', kind: 'Criterion', body: 'Capture invoices' }],
    });
    const rendererCapture = createRendererCapture();
    const transport = {
      getInitialContext: vi.fn(async () => ({
        rootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      })),
      loadSnapshot: vi
        .fn()
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-a', documents: [authSummary] }))
        .mockResolvedValueOnce(makeSnapshot({ revision: 'rev-b', documents: [billingSummary] })),
      loadDocument: vi
        .fn()
        .mockResolvedValueOnce(makeDocument({ revision: 'rev-a', document_id: authSummary.id, verificationState: 'unverified' }))
        .mockResolvedValueOnce(makeDocument({ revision: 'rev-b', document_id: billingSummary.id })),
      subscribeChanges: vi.fn(() => () => {}),
      commitRoot: vi.fn(),
    };

    createExplorerApp(createContainer(), transport, { renderer: rendererCapture.renderer });
    await flushAsyncWork();

    expect(rendererCapture.renderer.mount).toHaveBeenCalledTimes(1);
    expect(rendererCapture.mountArgs.rootContext).toEqual({
      activeRootId: 'workspace-a',
      availableRoots: [
        { id: 'workspace-a', name: 'Workspace A' },
        { id: 'workspace-b', name: 'Workspace B' },
      ],
    });

    rendererCapture.mountArgs.onSelectDocument(authSummary.id);
    await flushAsyncWork();
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('ready');

    rendererCapture.mountArgs.onSwitchRoot('workspace-b');
    await flushAsyncWork();

    expect(rendererCapture.renderer.mount).toHaveBeenCalledTimes(1);
    expect(transport.loadSnapshot).toHaveBeenNthCalledWith(2, 'workspace-b');
    expect(rendererCapture.handle.replaceGraphData).toHaveBeenCalledWith({
      documents: [
        {
          id: billingSummary.id,
          doc_type: billingSummary.doc_type,
          status: billingSummary.status,
          title: billingSummary.title,
          path: billingSummary.path,
          file_uri: billingSummary.file_uri,
          project: billingSummary.project,
          coverage_summary: billingSummary.coverage_summary,
          components: billingSummary.graph_components,
        },
      ],
      edges: [],
    });
    expect(transport.commitRoot).toHaveBeenCalledWith('workspace-b');
    expect(rendererCapture.mountArgs.getDocumentState(authSummary.id).state).toBe('idle');

    rendererCapture.mountArgs.onSelectDocument(billingSummary.id);
    await flushAsyncWork();

    expect(transport.loadDocument).toHaveBeenNthCalledWith(2, {
      rootId: 'workspace-b',
      revision: 'rev-b',
      documentId: billingSummary.id,
    });
    expect(rendererCapture.mountArgs.getDocumentState(billingSummary.id).state).toBe('ready');
  });
});
