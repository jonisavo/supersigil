import {
  buildGraphComponentOutline,
  countCriteria,
} from './explorer-runtime-shared.js';

export const STATIC_REVISION = 'static';

function normalizeRenderDataDocuments(renderData) {
  if (Array.isArray(renderData)) {
    return renderData;
  }
  return renderData?.documents ?? [];
}

export function buildStaticExplorerAssets(
  graphData,
  renderData,
  revision = STATIC_REVISION,
) {
  const documents = graphData?.documents ?? [];
  const edges = graphData?.edges ?? [];
  const renderDataArray = normalizeRenderDataDocuments(renderData);

  const documentDetails = Object.fromEntries(
    renderDataArray.map((document) => [
      document.document_id,
      {
        revision,
        document_id: document.document_id,
        stale: document.stale ?? false,
        fences: document.fences ?? [],
        edges: document.edges ?? [],
      },
    ]),
  );

  return {
    snapshot: {
      revision,
      documents: documents.map((doc) => {
        const graphComponents = buildGraphComponentOutline(doc.components).map((component) => ({
          id: component.id,
          kind: component.kind,
          body: component.body,
          parent_component_id: component.parentComponentId ?? undefined,
          implements: component.implements,
        }));
        const coverageSummary = countCriteria(
          documentDetails[doc.id]?.fences ?? [],
        );

        return {
          id: doc.id,
          doc_type: doc.doc_type ?? null,
          status: doc.status ?? null,
          title: doc.title,
          path: doc.path ?? null,
          file_uri: doc.file_uri ?? null,
          project: doc.project ?? null,
          coverage_summary: coverageSummary,
          component_count: graphComponents.length,
          graph_components: graphComponents,
        };
      }),
      edges: [...edges],
    },
    documents: documentDetails,
  };
}

export function buildStaticExplorerDocumentFileName(documentId) {
  return `${encodeURIComponent(documentId)}.json`;
}

export function buildStaticExplorerDocumentUrl(
  documentId,
  documentsBaseUrl = '/explore/documents',
) {
  const baseUrl = documentsBaseUrl.endsWith('/')
    ? documentsBaseUrl.slice(0, -1)
    : documentsBaseUrl;
  return `${baseUrl}/${buildStaticExplorerDocumentFileName(documentId)}`;
}
