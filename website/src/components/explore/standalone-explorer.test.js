/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const createExplorerAppMock = vi.fn();

vi.mock('./explorer-app.js', () => ({
  createExplorerApp: createExplorerAppMock,
}));

describe('initStandaloneExplorer', () => {
  beforeEach(() => {
    createExplorerAppMock.mockReset();
    document.body.innerHTML = '';
    window.location.hash = '';
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('fetches a static explorer snapshot first and lazy-loads document detail', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);

    const expectedSnapshot = {
      revision: 'static',
      documents: [
        {
          id: 'specs/auth/req',
          doc_type: 'requirements',
          status: 'draft',
          title: 'Auth Requirements',
          path: 'specs/auth/auth.req.md',
          file_uri: null,
          project: null,
          coverage_summary: { verified: 1, total: 1 },
          component_count: 1,
          graph_components: [{ id: 'req-1', kind: 'Criterion', body: 'Authenticate users' }],
        },
      ],
      edges: [],
    };
    const documentDetail = {
      revision: 'static',
      document_id: 'specs/auth/req',
      stale: false,
      fences: [{ components: [{ kind: 'Criterion', verification: { state: 'verified' } }] }],
      edges: [],
    };

    const fetchImpl = vi.fn(async (url) => {
      if (url === '/explore/snapshot.json') {
        return {
          ok: true,
          json: async () => expectedSnapshot,
        };
      }
      if (url === '/explore/documents/specs%2Fauth%2Freq.json') {
        return {
          ok: true,
          json: async () => documentDetail,
        };
      }
      throw new Error(`Unexpected URL: ${url}`);
    });

    const { initStandaloneExplorer } = await import('./standalone-explorer.js');
    await initStandaloneExplorer({ container, fetchImpl });

    expect(createExplorerAppMock).toHaveBeenCalledTimes(1);
    const [, transport, options] = createExplorerAppMock.mock.calls[0];
    expect(options.repositoryInfo).toEqual({
      provider: 'github',
      repo: 'jonisavo/supersigil',
      host: 'github.com',
      mainBranch: 'main',
    });

    const snapshot = await transport.loadSnapshot('website');
    expect(snapshot).toEqual(expectedSnapshot);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(fetchImpl).toHaveBeenNthCalledWith(1, '/explore/snapshot.json');

    const detail = await transport.loadDocument({
      rootId: 'website',
      revision: 'static',
      documentId: 'specs/auth/req',
    });
    expect(detail).toEqual(documentDetail);
    expect(fetchImpl).toHaveBeenCalledTimes(2);
    expect(fetchImpl).toHaveBeenNthCalledWith(
      2,
      '/explore/documents/specs%2Fauth%2Freq.json',
    );
  });
});
