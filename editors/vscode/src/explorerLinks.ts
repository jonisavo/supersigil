export const OPEN_GRAPH_FILE_COMMAND = "supersigil.openGraphFile";

export interface OpenGraphFileTarget {
  path?: string;
  uri?: string;
  line?: number;
  folderUri?: string;
}

export function buildOpenFileCommandHref(target: OpenGraphFileTarget): string {
  return `command:${OPEN_GRAPH_FILE_COMMAND}?${encodeURIComponent(JSON.stringify([target]))}`;
}

export function buildDocumentLink(docId: string): string {
  return `#/doc/${encodeURIComponent(docId)}`;
}

export function buildCriterionLink(docId: string, _criterionId: string): string {
  return buildDocumentLink(docId);
}
