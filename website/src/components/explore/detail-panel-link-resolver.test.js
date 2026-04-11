/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it } from 'vitest';
import { renderDetail } from './detail-panel.js';

describe('renderDetail linkResolver parameter', () => {
  const node = {
    id: 'doc/a',
    doc_type: 'requirements',
    status: 'approved',
    title: 'A',
    components: [],
  };

  const renderData = [
    {
      document_id: 'doc/a',
      fences: [{ components: [{ kind: 'Criterion', verification: { state: 'verified' } }] }],
      edges: [],
    },
  ];

  afterEach(() => {
    delete window.__supersigilRender;
  });

  it('uses provided linkResolver instead of creating one from repositoryInfo', () => {
    let capturedResolver = null;
    window.__supersigilRender = {
      renderComponentTree: (_fences, _edges, resolver) => {
        capturedResolver = resolver;
        return '<div>rendered</div>';
      },
    };

    const customResolver = {
      evidenceLink: (file, line) => `vscode://file/${file}#${line}`,
      documentLink: (docId) => `#/doc/${encodeURIComponent(docId)}`,
      criterionLink: (docId, _criterionId) => `#/doc/${encodeURIComponent(docId)}`,
    };

    const container = document.createElement('div');
    renderDetail(container, node, [], renderData, null, customResolver);

    expect(capturedResolver).toBe(customResolver);
  });

  it('falls back to repositoryInfo-based resolver when linkResolver is not provided', () => {
    let capturedResolver = null;
    window.__supersigilRender = {
      renderComponentTree: (_fences, _edges, resolver) => {
        capturedResolver = resolver;
        return '<div>rendered</div>';
      },
    };

    const repoInfo = {
      provider: 'github',
      repo: 'org/repo',
      host: 'github.com',
      mainBranch: 'main',
    };

    const container = document.createElement('div');
    renderDetail(container, node, [], renderData, repoInfo);

    expect(capturedResolver).not.toBeNull();
    expect(capturedResolver.evidenceLink('src/main.rs', 42)).toBe(
      'https://github.com/org/repo/blob/main/src/main.rs#L42',
    );
    expect(capturedResolver.documentLink('doc/b')).toBe('#/doc/doc%2Fb');
  });

  it('custom linkResolver evidence links appear in rendered HTML', () => {
    window.__supersigilRender = {
      renderComponentTree: (_fences, _edges, resolver) => {
        const link = resolver.evidenceLink('src/lib.rs', 10);
        return `<a href="${link}">evidence</a>`;
      },
    };

    const customResolver = {
      evidenceLink: (file, line) => `vscode://file/${file}#${line}`,
      documentLink: (docId) => `#/doc/${encodeURIComponent(docId)}`,
      criterionLink: (docId, _criterionId) => `#/doc/${encodeURIComponent(docId)}`,
    };

    const container = document.createElement('div');
    renderDetail(container, node, [], renderData, null, customResolver);

    expect(container.innerHTML).toContain('vscode://file/src/lib.rs#10');
  });
});
