/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import process from 'node:process';

const BRIDGE_PATH = resolve(
  process.cwd(),
  '../editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js',
);
const STYLES_PATH = resolve(process.cwd(), 'src/components/explore/styles.css');

function loadBridge({
  createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() })),
  query = undefined,
  action = undefined,
} = {}) {
  document.body.innerHTML = '<div id="explorer"></div>';
  window.location.hash = '';

  delete window.__supersigilHostReady;
  delete window.__supersigilExplorerChanged;
  delete window.__supersigilQuery;
  delete window.__supersigilAction;
  delete window.SupersigilExplorer;

  if (query !== undefined) {
    window.__supersigilQuery = query;
  }
  if (action !== undefined) {
    window.__supersigilAction = action;
  }

  window.SupersigilExplorer = {
    createExplorerApp: createExplorerAppImpl,
  };

  const script = readFileSync(BRIDGE_PATH, 'utf8');
  window.eval(script);
  document.dispatchEvent(new Event('DOMContentLoaded'));

  return createExplorerAppImpl;
}

afterEach(() => {
  document.body.innerHTML = '';
  delete window.__supersigilHostReady;
  delete window.__supersigilExplorerChanged;
  delete window.__supersigilQuery;
  delete window.__supersigilAction;
  delete window.SupersigilExplorer;
  window.location.hash = '';
  vi.restoreAllMocks();
});

describe('explorer bridge', () => {
  // supersigil: intellij-graph-explorer-bridge-routing
  // supersigil: intellij-graph-explorer-state-preservation
  it('boots the shared runtime, resolves host context, and forwards transport requests', async () => {
    const actionSpy = vi.fn();
    const querySpy = vi.fn((payload, onSuccess) => {
      const request = JSON.parse(payload);
      if (request.method === 'loadSnapshot') {
        onSuccess(
          JSON.stringify({
            revision: 'rev-1',
            documents: [{ id: 'docs/spec-a', title: 'Spec A', path: 'specs/spec-a.md' }],
            edges: [],
          }),
        );
        return;
      }

      onSuccess(
        JSON.stringify({
          revision: request.params.revision,
          document_id: request.params.documentId,
          stale: false,
          fences: [],
          edges: [],
        }),
      );
    });
    const createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() }));

    loadBridge({
      createExplorerAppImpl,
      query: querySpy,
      action: actionSpy,
    });

    expect(createExplorerAppImpl).toHaveBeenCalledTimes(1);
    expect(actionSpy).toHaveBeenCalledWith('ready');
    expect(window.__supersigilHostReady).toBeTypeOf('function');
    expect(window.__supersigilExplorerChanged).toBeTypeOf('function');

    const [, transport, options] = createExplorerAppImpl.mock.calls[0];
    expect(options.linkResolver.documentLink('docs/spec-b')).toBe('#/doc/docs%2Fspec-b');
    expect(options.linkResolver.criterionLink('docs/spec-b', 'criterion-1')).toBe(
      '#/doc/docs%2Fspec-b',
    );

    window.__supersigilHostReady({
      rootId: 'workspace',
      availableRoots: [{ id: 'workspace', name: 'Workspace' }],
      focusDocumentId: 'docs/spec-a',
    });

    await expect(transport.getInitialContext()).resolves.toEqual({
      rootId: 'workspace',
      availableRoots: [{ id: 'workspace', name: 'Workspace' }],
      focusDocumentId: 'docs/spec-a',
    });
    await expect(transport.loadSnapshot('workspace')).resolves.toEqual({
      revision: 'rev-1',
      documents: [{ id: 'docs/spec-a', title: 'Spec A', path: 'specs/spec-a.md' }],
      edges: [],
    });
    await expect(
      transport.loadDocument({
        rootId: 'workspace',
        revision: 'rev-1',
        documentId: 'docs/spec-a',
      }),
    ).resolves.toEqual({
      revision: 'rev-1',
      document_id: 'docs/spec-a',
      stale: false,
      fences: [],
      edges: [],
    });

    expect(querySpy).toHaveBeenNthCalledWith(
      1,
      JSON.stringify({
        method: 'loadSnapshot',
        params: { rootId: 'workspace' },
      }),
      expect.any(Function),
      expect.any(Function),
    );
    expect(querySpy).toHaveBeenNthCalledWith(
      2,
      JSON.stringify({
        method: 'loadDocument',
        params: {
          rootId: 'workspace',
          revision: 'rev-1',
          documentId: 'docs/spec-a',
        },
      }),
      expect.any(Function),
      expect.any(Function),
    );
  });

  // supersigil: intellij-graph-explorer-open-file-navigation
  // supersigil: intellij-graph-explorer-evidence-navigation
  // supersigil: intellij-graph-explorer-navigation-paths
  it('encodes evidence links and forwards runtime open-file actions', async () => {
    const actionSpy = vi.fn();
    const createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() }));

    loadBridge({
      createExplorerAppImpl,
      action: actionSpy,
    });

    const [, transport, options] = createExplorerAppImpl.mock.calls[0];
    const href = options.linkResolver.evidenceLink('C:\\Users\\specs\\file.md', 7);
    expect(href).toBe(
      '#supersigil-action:open-file:C\\:\\\\Users\\\\specs\\\\file.md:7',
    );

    const anchor = document.createElement('a');
    anchor.href = href;
    anchor.textContent = 'Evidence';
    document.body.appendChild(anchor);
    anchor.click();

    expect(actionSpy).toHaveBeenCalledWith('ready');
    expect(actionSpy).toHaveBeenCalledWith(
      'open-file:C\\:\\\\Users\\\\specs\\\\file.md:7',
    );

    transport.openFile({ path: '/workspace/specs/spec-a.md', line: 1 });
    expect(actionSpy).toHaveBeenCalledWith('open-file:/workspace/specs/spec-a.md:1');
  });

  it('forwards explorerChanged events to runtime subscribers', () => {
    const createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() }));

    loadBridge({
      createExplorerAppImpl,
    });

    const [, transport] = createExplorerAppImpl.mock.calls[0];
    const listener = vi.fn();
    const unsubscribe = transport.subscribeChanges(listener);

    window.__supersigilExplorerChanged({
      revision: 'rev-2',
      changed_document_ids: ['docs/spec-a'],
      removed_document_ids: [],
    });

    expect(listener).toHaveBeenCalledWith({
      revision: 'rev-2',
      changed_document_ids: ['docs/spec-a'],
      removed_document_ids: [],
    });

    unsubscribe();
    window.__supersigilExplorerChanged({
      revision: 'rev-3',
      changed_document_ids: ['docs/spec-b'],
      removed_document_ids: [],
    });

    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('does not crash when __supersigilQuery is missing and rejects transport requests', async () => {
    const createExplorerAppImpl = vi.fn(() => ({ destroy: vi.fn() }));

    expect(() =>
      loadBridge({
        createExplorerAppImpl,
      }),
    ).not.toThrow();

    const [, transport] = createExplorerAppImpl.mock.calls[0];
    await expect(transport.loadSnapshot('workspace')).rejects.toThrow(
      'Supersigil query bridge is unavailable',
    );
  });

  it('styles the open file control as a right-aligned header action button', () => {
    const styles = readFileSync(STYLES_PATH, 'utf8');

    expect(styles).toContain('.open-file-btn');
    expect(styles).toContain('margin-left: auto;');
    expect(styles).toContain('cursor: pointer;');
    expect(styles).toContain('.open-file-btn:hover');
  });
});
