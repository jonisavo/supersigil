import { describe, expect, it } from 'vitest';

import {
  STATIC_REVISION,
  buildStaticExplorerAssets,
  buildStaticExplorerDocumentUrl,
} from './static-explorer-assets.js';

describe('buildStaticExplorerAssets', () => {
  it('builds a snapshot and revisioned document payloads for the website runtime', () => {
    const graphData = {
      documents: [
        {
          id: 'specs/auth/req',
          doc_type: 'requirements',
          status: 'draft',
          title: 'Auth Requirements',
          path: 'specs/auth/auth.req.md',
          file_uri: null,
          project: null,
          components: [{ id: 'req-1', kind: 'Criterion', body: 'Authenticate users' }],
        },
      ],
      edges: [],
    };
    const renderData = [
      {
        document_id: 'specs/auth/req',
        stale: false,
        fences: [{ components: [{ kind: 'Criterion', verification: { state: 'verified' } }] }],
        edges: [],
      },
    ];

    expect(buildStaticExplorerAssets(graphData, renderData)).toEqual({
      snapshot: {
        revision: STATIC_REVISION,
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
      },
      documents: {
        'specs/auth/req': {
          revision: STATIC_REVISION,
          document_id: 'specs/auth/req',
          stale: false,
          fences: [{ components: [{ kind: 'Criterion', verification: { state: 'verified' } }] }],
          edges: [],
        },
      },
    });
  });
});

describe('buildStaticExplorerDocumentUrl', () => {
  it('URL-encodes document ids into website asset paths', () => {
    expect(buildStaticExplorerDocumentUrl('specs/auth/req')).toBe(
      '/explore/documents/specs%2Fauth%2Freq.json',
    );
  });
});
