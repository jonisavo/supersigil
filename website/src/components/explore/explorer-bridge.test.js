/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const BRIDGE_PATH = resolve(
  process.cwd(),
  '../editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js',
);

function loadBridge({ mountImpl, query = undefined, action = undefined } = {}) {
  document.body.innerHTML = '<div id="explorer"></div>';
  window.location.hash = '';

  delete window.__supersigilReceiveData;
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
    mount: mountImpl,
  };

  const script = readFileSync(BRIDGE_PATH, 'utf8');
  window.eval(script);
  document.dispatchEvent(new Event('DOMContentLoaded'));
}

afterEach(() => {
  document.body.innerHTML = '';
  delete window.__supersigilReceiveData;
  delete window.__supersigilQuery;
  delete window.__supersigilAction;
  delete window.SupersigilExplorer;
  window.location.hash = '';
  vi.restoreAllMocks();
});

const STYLES_PATH = resolve(process.cwd(), 'src/components/explore/styles.css');

describe('explorer bridge', () => {
  // supersigil: intellij-graph-explorer-bridge-routing
  // supersigil: intellij-graph-explorer-open-file-navigation
  // supersigil: intellij-graph-explorer-evidence-navigation
  // supersigil: intellij-graph-explorer-state-preservation
  it('mounts data with interceptable links and preserves hash across remounts', async () => {
    const actionSpy = vi.fn();
    const unmountSpy = vi.fn(() => {
      window.location.hash = '#/doc/changed';
    });

    let mountCount = 0;
    const hashesSeenAtMount = [];
    const mountSpy = vi.fn((container, graph, renderData, _repositoryInfo, linkResolver) => {
      mountCount += 1;
      hashesSeenAtMount.push(window.location.hash);
      expect(container).toBe(document.getElementById('explorer'));
      expect(graph.documents[0].id).toBe('docs/spec-a');
      expect(renderData).toEqual([{ document_id: 'docs/spec-a', fences: [] }]);
      expect(linkResolver.evidenceLink('src/specs/spec-a.md', 12)).toBe(
        '#supersigil-action:open-file:src/specs/spec-a.md:12',
      );
      expect(linkResolver.documentLink('docs/spec-b')).toBe('#/doc/docs%2Fspec-b');
      expect(linkResolver.criterionLink('docs/spec-b', 'criterion-1')).toBe(
        '#/doc/docs%2Fspec-b',
      );

      container.innerHTML = `
        <a class="evidence-link" href="#supersigil-action:open-file:src/specs/spec-a.md:12">
          Evidence
        </a>
        <div class="detail-panel-header">
          <div class="detail-panel-title">docs/spec-a</div>
          <button class="detail-panel-close" aria-label="Close">x</button>
        </div>
      `;

      return { unmount: unmountSpy };
    });

    loadBridge({
      mountImpl: mountSpy,
      action: actionSpy,
    });

    expect(window.__supersigilReceiveData).toBeTypeOf('function');

    const payload = JSON.stringify({
      graphData: {
        documents: [{ id: 'docs/spec-a', path: 'src/specs/spec-a.md' }],
        edges: [],
      },
      renderData: [{ document_id: 'docs/spec-a', fences: [] }],
    });

    window.location.hash = '#/doc/docs%2Fspec-a';
    window.__supersigilReceiveData(payload);
    await Promise.resolve();

    expect(mountSpy).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.open-file-btn')).not.toBeNull();

    document.querySelector('.evidence-link').click();
    expect(actionSpy).toHaveBeenCalledWith('open-file:src/specs/spec-a.md:12');

    document.querySelector('.open-file-btn').click();
    expect(actionSpy).toHaveBeenCalledWith('open-file:src/specs/spec-a.md:1');

    window.location.hash = '#/doc/docs%2Fspec-a/trace';
    window.__supersigilReceiveData(payload);
    await Promise.resolve();

    expect(unmountSpy).toHaveBeenCalledTimes(1);
    expect(mountSpy).toHaveBeenCalledTimes(2);
    expect(window.location.hash).toBe('#/doc/docs%2Fspec-a/trace');
    expect(hashesSeenAtMount[0]).toBe('#/doc/docs%2Fspec-a');
    expect(hashesSeenAtMount[1]).toBe('#/doc/docs%2Fspec-a/trace');
    expect(mountCount).toBe(2);
  });

  it('encodes windows evidence links and forwards escaped actions', async () => {
    const actionSpy = vi.fn();
    let capturedResolver = null;

    const mountSpy = vi.fn((_container, _graph, _renderData, _repositoryInfo, linkResolver) => {
      capturedResolver = linkResolver;
      return { unmount: vi.fn() };
    });

    loadBridge({
      mountImpl: mountSpy,
      action: actionSpy,
    });

    window.__supersigilReceiveData(
      JSON.stringify({
        graphData: {
          documents: [{ id: 'docs/windows', path: 'C:\\Users\\specs\\file.md' }],
          edges: [],
        },
        renderData: [],
      }),
    );

    await Promise.resolve();

    const href = capturedResolver.evidenceLink('C:\\Users\\specs\\file.md', 7);
    expect(href).toBe(
      '#supersigil-action:open-file:C\\:\\\\Users\\\\specs\\\\file.md:7',
    );

    const anchor = document.createElement('a');
    anchor.href = href;
    document.body.appendChild(anchor);
    anchor.click();

    expect(actionSpy).toHaveBeenCalledWith(
      'open-file:C\\:\\\\Users\\\\specs\\\\file.md:7',
    );
  });

  it('does not duplicate open file buttons when observer runs again', async () => {
    const mountSpy = vi.fn((container) => {
      container.innerHTML = `
        <div class="detail-panel-header">
          <div class="detail-panel-title">docs/spec-a</div>
          <button class="detail-panel-close" aria-label="Close">x</button>
        </div>
      `;

      return { unmount: vi.fn() };
    });

    loadBridge({
      mountImpl: mountSpy,
    });

    window.__supersigilReceiveData(
      JSON.stringify({
        graphData: {
          documents: [{ id: 'docs/spec-a', path: 'src/specs/spec-a.md' }],
          edges: [],
        },
        renderData: [],
      }),
    );

    await Promise.resolve();

    const header = document.querySelector('.detail-panel-header');
    const extra = document.createElement('span');
    extra.textContent = 'refresh';
    header.appendChild(extra);
    await Promise.resolve();

    expect(document.querySelectorAll('.open-file-btn')).toHaveLength(1);
  });

  // supersigil: intellij-graph-explorer-navigation-paths
  it('open file button prefers resolved absolute document paths when available', async () => {
    const actionSpy = vi.fn();

    const mountSpy = vi.fn((container) => {
      container.innerHTML = `
        <div class="detail-panel-header">
          <div class="detail-panel-title">docs/spec-a</div>
          <button class="detail-panel-close" aria-label="Close">x</button>
        </div>
      `;

      return { unmount: vi.fn() };
    });

    loadBridge({
      mountImpl: mountSpy,
      action: actionSpy,
    });

    window.__supersigilReceiveData(
      JSON.stringify({
        graphData: {
          documents: [
            {
              id: 'docs/spec-a',
              path: 'src/specs/spec-a.md',
              filePath: '/workspace/src/specs/spec-a.md',
            },
          ],
          edges: [],
        },
        renderData: [],
      }),
    );

    await Promise.resolve();

    document.querySelector('.open-file-btn').click();
    expect(actionSpy).toHaveBeenCalledWith('open-file:/workspace/src/specs/spec-a.md:1');
  });

  it('does not crash when __supersigilQuery is missing', () => {
    expect(() =>
      loadBridge({
        mountImpl: vi.fn(() => ({ unmount: vi.fn() })),
      }),
    ).not.toThrow();

    expect(window.__supersigilReceiveData).toBeTypeOf('function');
  });

  it('styles the open file control as a right-aligned header action button', () => {
    const styles = readFileSync(STYLES_PATH, 'utf8');

    expect(styles).toContain('.open-file-btn');
    expect(styles).toContain('margin-left: auto;');
    expect(styles).toContain('cursor: pointer;');
    expect(styles).toContain('.open-file-btn:hover');
  });
});
