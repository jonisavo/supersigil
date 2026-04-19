import { createExplorerApp } from './explorer-app.js';
import {
  STATIC_REVISION,
  buildStaticExplorerDocumentUrl,
} from './static-explorer-assets.js';

export const DEFAULT_WEBSITE_REPOSITORY_INFO = {
  provider: 'github',
  repo: 'jonisavo/supersigil',
  host: 'github.com',
  mainBranch: 'main',
};

export function createStaticExplorerTransport({
  snapshot,
  fetchImpl = fetch,
  documentsBaseUrl = '/explore/documents',
}) {
  return {
    async getInitialContext() {
      return {
        rootId: 'website',
        availableRoots: [{ id: 'website', name: 'Website' }],
      };
    },
    async loadSnapshot() {
      return snapshot;
    },
    async loadDocument({ documentId }) {
      const response = await fetchImpl(
        buildStaticExplorerDocumentUrl(documentId, documentsBaseUrl),
      );

      if (response.ok) {
        const document = await response.json();
        return {
          revision: document.revision ?? snapshot.revision ?? STATIC_REVISION,
          ...document,
        };
      }

      if (response.status === 404) {
        return {
          revision: snapshot.revision ?? STATIC_REVISION,
          document_id: documentId,
          stale: false,
          fences: [],
          edges: [],
        };
      }

      throw new Error(
        `Failed to load explorer document ${documentId}: ${response.status} ${response.statusText}`,
      );
    },
    subscribeChanges() {
      return () => {};
    },
  };
}

export async function initStandaloneExplorer({
  container,
  fetchImpl = fetch,
  repositoryInfo = DEFAULT_WEBSITE_REPOSITORY_INFO,
  snapshotUrl = '/explore/snapshot.json',
  documentsBaseUrl = '/explore/documents',
} = {}) {
  try {
    const snapshotResponse = await fetchImpl(snapshotUrl);

    if (!snapshotResponse.ok) {
      throw new Error(
        `Failed to load explorer snapshot: ${snapshotResponse.status} ${snapshotResponse.statusText}`,
      );
    }

    const snapshot = await snapshotResponse.json();
    const transport = createStaticExplorerTransport({
      snapshot,
      fetchImpl,
      documentsBaseUrl,
    });

    return createExplorerApp(container, transport, { repositoryInfo });
  } catch (error) {
    console.error('Spec explorer failed to load:', error);
    if (container) {
      container.innerHTML = `<div class="explorer-error">
        <p>Unable to load explorer data.</p>
        <p><small>${error instanceof Error ? error.message : String(error)}</small></p>
      </div>`;
    }
    return null;
  }
}
