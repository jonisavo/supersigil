/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from './graph-explorer.js';

const minimalData = {
  documents: [
    { id: 'a/one', doc_type: 'requirements', status: null, title: 'One', components: [] },
    { id: 'a/two', doc_type: 'design', status: null, title: 'Two', components: [] },
  ],
  edges: [{ from: 'a/one', to: 'a/two', kind: 'traces' }],
};

function createContainer() {
  const el = document.createElement('div');
  el.style.width = '800px';
  el.style.height = '600px';
  document.body.appendChild(el);
  el.getBoundingClientRect = () => ({
    x: 0,
    y: 0,
    width: 800,
    height: 600,
    top: 0,
    right: 800,
    bottom: 600,
    left: 0,
    toJSON() {},
  });
  return el;
}

describe('mount / unmount lifecycle', () => {
  /** @type {HTMLDivElement} */
  let container;

  beforeEach(() => {
    container = createContainer();
    window.location.hash = '';
    const store = {};
    globalThis.localStorage = {
      getItem: (key) => store[key] ?? null,
      setItem: (key, val) => {
        store[key] = String(val);
      },
      removeItem: (key) => {
        delete store[key];
      },
      clear: () => {
        for (const key in store) delete store[key];
      },
      get length() {
        return Object.keys(store).length;
      },
      key: (index) => Object.keys(store)[index] ?? null,
    };
  });

  afterEach(() => {
    document.body.innerHTML = '';
    vi.restoreAllMocks();
  });

  it('unmount clears the mounted subtree and detaches global listeners', () => {
    const removeDocumentListener = vi.spyOn(document, 'removeEventListener');
    const removeWindowListener = vi.spyOn(window, 'removeEventListener');
    const handle = mount(container, minimalData);

    expect(container.children.length).toBeGreaterThan(0);

    handle.unmount();

    expect(container.children.length).toBe(0);
    const documentListenerTypes = removeDocumentListener.mock.calls.map(([type]) => type);
    expect(documentListenerTypes).toEqual(
      expect.arrayContaining(['click', 'mousemove', 'mouseup', 'keydown']),
    );
    expect(removeWindowListener.mock.calls.map(([type]) => type)).toContain('hashchange');
  });

  it('calling unmount multiple times is safe', () => {
    const handle = mount(container, minimalData);
    handle.unmount();
    expect(() => handle.unmount()).not.toThrow();
  });

  it('mount then unmount then mount does not duplicate document-level handlers', () => {
    const addSpy = vi.spyOn(document, 'addEventListener');

    const firstHandle = mount(container, minimalData);
    const firstMountCalls = addSpy.mock.calls.filter(
      ([type]) => type === 'click' || type === 'mousemove' || type === 'mouseup' || type === 'keydown',
    ).length;

    firstHandle.unmount();
    document.body.innerHTML = '';
    const nextContainer = createContainer();
    addSpy.mockClear();

    const secondHandle = mount(nextContainer, minimalData);
    const secondMountCalls = addSpy.mock.calls.filter(
      ([type]) => type === 'click' || type === 'mousemove' || type === 'mouseup' || type === 'keydown',
    ).length;

    expect(secondMountCalls).toBe(firstMountCalls);
    secondHandle.unmount();
  });

  it('refreshDetail rerenders the selected document from runtime-managed document state', () => {
    let documentState = { state: 'loading' };
    const onSelectDocument = vi.fn();
    window.__supersigilRender = {
      renderComponentTree: vi.fn(() => '<div class="runtime-rendered">Loaded detail</div>'),
    };

    const handle = mount(container, minimalData, null, null, undefined, {
      getDocumentState: (documentId) =>
        documentId === 'a/one' ? documentState : { state: 'idle' },
      onSelectDocument,
    });

    const firstNode = [...container.querySelectorAll('g.node')].find((node) =>
      node.textContent?.includes('one'),
    );
    firstNode?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onSelectDocument).toHaveBeenCalledWith('a/one');
    expect(container.querySelector('.detail-spec-loading')?.textContent).toContain(
      'Loading specification',
    );

    documentState = {
      state: 'ready',
      revision: 'rev-1',
      document: {
        revision: 'rev-1',
        document_id: 'a/one',
        stale: false,
        fences: [{ components: [{ kind: 'Criterion', verification: { state: 'verified' } }] }],
        edges: [],
      },
    };

    handle.refreshDetail();

    expect(container.querySelector('.runtime-rendered')?.textContent).toContain('Loaded detail');
    handle.unmount();
  });

  it('renders the runtime-owned root selector only when multiple roots are available', () => {
    const onSwitchRoot = vi.fn();
    const multiRootHandle = mount(container, minimalData, null, null, undefined, {
      rootContext: {
        activeRootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      },
      onSwitchRoot,
    });

    const select = /** @type {HTMLSelectElement | null} */ (container.querySelector('.root-selector'));
    expect(select).not.toBeNull();
    select.value = 'workspace-b';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    expect(onSwitchRoot).toHaveBeenCalledWith('workspace-b');
    multiRootHandle.unmount();

    container = createContainer();
    const singleRootHandle = mount(container, minimalData, null, null, undefined, {
      rootContext: {
        activeRootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      },
    });
    expect(container.querySelector('.root-selector')).toBeNull();
    singleRootHandle.unmount();
  });

  it('updates the runtime-owned root selector when runtime root context changes', () => {
    const onSwitchRoot = vi.fn();
    const handle = mount(container, minimalData, null, null, undefined, {
      rootContext: {
        activeRootId: 'workspace-a',
        availableRoots: [{ id: 'workspace-a', name: 'Workspace A' }],
      },
      onSwitchRoot,
    });

    expect(container.querySelector('.root-selector')).toBeNull();

    handle.updateRuntimeOptions?.({
      rootContext: {
        activeRootId: 'workspace-a',
        availableRoots: [
          { id: 'workspace-a', name: 'Workspace A' },
          { id: 'workspace-b', name: 'Workspace B' },
        ],
      },
    });

    const select = /** @type {HTMLSelectElement | null} */ (container.querySelector('.root-selector'));
    expect(select).not.toBeNull();
    expect([...select.options].map((option) => option.value)).toEqual([
      'workspace-a',
      'workspace-b',
    ]);

    select.value = 'workspace-b';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    expect(onSwitchRoot).toHaveBeenCalledWith('workspace-b');

    handle.updateRuntimeOptions?.({
      rootContext: {
        activeRootId: 'workspace-b',
        availableRoots: [{ id: 'workspace-b', name: 'Workspace B' }],
      },
    });

    expect(container.querySelector('.root-selector')).toBeNull();
    handle.unmount();
  });

  it('renders a runtime-owned open file control for selected documents', () => {
    const openFile = vi.fn();
    const data = {
      documents: [
        {
          ...minimalData.documents[0],
          path: 'specs/a/one.md',
          file_uri: 'file:///workspace/specs/a/one.md',
        },
        minimalData.documents[1],
      ],
      edges: minimalData.edges,
    };

    const handle = mount(container, data, null, null, undefined, { openFile });
    const firstNode = [...container.querySelectorAll('g.node')].find((node) =>
      node.textContent?.includes('one'),
    );
    firstNode?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    const button = container.querySelector('.open-file-btn');
    expect(button).not.toBeNull();
    button?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(openFile).toHaveBeenCalledWith({
      uri: 'file:///workspace/specs/a/one.md',
      line: 1,
    });
    handle.unmount();
  });
});
