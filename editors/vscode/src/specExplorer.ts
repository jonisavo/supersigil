import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

export const METHOD_DOCUMENT_LIST = "supersigil/documentList";
export const METHOD_DOCUMENT_COMPONENTS = "supersigil/documentComponents";
export const METHOD_DOCUMENTS_CHANGED = "supersigil/documentsChanged";
export const METHOD_EXPLORER_SNAPSHOT = "supersigil/explorerSnapshot";
export const METHOD_EXPLORER_DOCUMENT = "supersigil/explorerDocument";
export const METHOD_EXPLORER_CHANGED = "supersigil/explorerChanged";

// ---------------------------------------------------------------------------
// LSP response types
// ---------------------------------------------------------------------------

export interface DocumentEntry {
  id: string;
  doc_type: string;
  status: string | null;
  path: string;
  project: string | null;
}

interface DocumentListResult {
  documents: DocumentEntry[];
}

// ---------------------------------------------------------------------------
// Tree item types
// ---------------------------------------------------------------------------

interface WorkspaceRootItem {
  kind: "workspace-root";
  label: string;
  folderUri: vscode.Uri;
}

interface ProjectItem {
  kind: "project";
  label: string;
  folderUri: vscode.Uri;
}

interface GroupItem {
  kind: "group";
  label: string;
  folderUri: vscode.Uri;
  documentCount: number;
  project: string | null;
}

export interface DocumentItem {
  kind: "document";
  id: string;
  docType: string;
  status: string | null;
  path: string;
  folderUri: vscode.Uri;
  project: string | null;
}

type SpecTreeItem = WorkspaceRootItem | ProjectItem | GroupItem | DocumentItem;

// ---------------------------------------------------------------------------
// Icon and color mappings
// ---------------------------------------------------------------------------

const COLOR_PASSED = new vscode.ThemeColor("testing.iconPassed");
const COLOR_QUEUED = new vscode.ThemeColor("testing.iconQueued");
const COLOR_FAILED = new vscode.ThemeColor("testing.iconFailed");
const COLOR_WARNING = new vscode.ThemeColor("list.warningForeground");
const COLOR_DISABLED = new vscode.ThemeColor("disabledForeground");

const DOC_TYPE_ICONS: Record<string, string> = {
  requirements: "checklist",
  design: "tools",
  tasks: "tasklist",
  adr: "law",
  decision: "law",
  documentation: "book",
};

function docTypeIcon(docType: string): vscode.ThemeIcon {
  const icon = DOC_TYPE_ICONS[docType] ?? "file";
  return new vscode.ThemeIcon(icon);
}

const STABLE_STATUSES = new Set([
  "approved",
  "implemented",
  "done",
  "accepted",
]);

function statusColor(status: string | null): vscode.ThemeColor | undefined {
  if (!status) return undefined;
  if (STABLE_STATUSES.has(status)) {
    return COLOR_PASSED;
  }
  switch (status) {
    case "draft":
      return COLOR_QUEUED;
    case "superseded":
      return COLOR_DISABLED;
    default:
      return undefined;
  }
}

function isUnresolved(status: string | null): boolean {
  if (!status) return true;
  return !STABLE_STATUSES.has(status) && status !== "superseded";
}

function iconForDocument(
  docType: string,
  status: string | null,
  diagnosticSeverity: vscode.DiagnosticSeverity | undefined,
): vscode.ThemeIcon {
  if (diagnosticSeverity === vscode.DiagnosticSeverity.Error) {
    return new vscode.ThemeIcon(
      "error",
      COLOR_FAILED,
    );
  }
  if (diagnosticSeverity === vscode.DiagnosticSeverity.Warning) {
    return new vscode.ThemeIcon(
      "warning",
      COLOR_WARNING,
    );
  }
  const base = docTypeIcon(docType);
  const color = statusColor(status);
  return color ? new vscode.ThemeIcon(base.id, color) : base;
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/** Sort comparator that puts nulls last, strings alphabetically. */
function nullsLastCompare(a: string | null, b: string | null): number {
  if (a === null) return 1;
  if (b === null) return -1;
  return a.localeCompare(b);
}

function worstDiagnosticSeverity(
  uri: vscode.Uri,
): vscode.DiagnosticSeverity | undefined {
  const diagnostics = vscode.languages
    .getDiagnostics(uri)
    .filter((d) => d.source === "supersigil");
  if (diagnostics.length === 0) return undefined;
  let worst = vscode.DiagnosticSeverity.Information;
  for (const d of diagnostics) {
    if (d.severity < worst) worst = d.severity;
  }
  return worst;
}

// ---------------------------------------------------------------------------
// Grouping logic (pure data transformation)
// ---------------------------------------------------------------------------

export interface GroupedDocuments {
  workspaceRoots: Map<
    string,
    {
      folderUri: vscode.Uri;
      folderName: string;
      projects: Map<
        string | null,
        Map<string | null, DocumentItem[]>
      >;
    }
  >;
  multiRoot: boolean;
}

export function groupDocuments(
  documentsByFolder: Map<
    string,
    { folderUri: vscode.Uri; folderName: string; documents: DocumentEntry[] }
  >,
): GroupedDocuments {
  const workspaceRoots = new Map<
    string,
    {
      folderUri: vscode.Uri;
      folderName: string;
      projects: Map<string | null, Map<string | null, DocumentItem[]>>;
    }
  >();

  for (const [key, { folderUri, folderName, documents }] of documentsByFolder) {
    const projects = new Map<
      string | null,
      Map<string | null, DocumentItem[]>
    >();

    for (const doc of documents) {
      const project = doc.project ?? null;
      if (!projects.has(project)) {
        projects.set(project, new Map());
      }
      const groups = projects.get(project)!;

      const slashIndex = doc.id.indexOf("/");
      const prefix = slashIndex >= 0 ? doc.id.substring(0, slashIndex) : null;

      if (!groups.has(prefix)) {
        groups.set(prefix, []);
      }

      groups.get(prefix)!.push({
        kind: "document",
        id: doc.id,
        docType: doc.doc_type,
        status: doc.status,
        path: doc.path,
        folderUri,
        project,
      });
    }

    workspaceRoots.set(key, { folderUri, folderName, projects });
  }

  return {
    workspaceRoots,
    multiRoot: workspaceRoots.size > 1,
  };
}

// ---------------------------------------------------------------------------
// TreeDataProvider
// ---------------------------------------------------------------------------

export class SpecExplorerProvider
  implements vscode.TreeDataProvider<SpecTreeItem>
{
  private _onDidChangeTreeData = new vscode.EventEmitter<
    SpecTreeItem | undefined | null
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private clients: Map<string, LanguageClient>;
  private cachedData: GroupedDocuments | null = null;
  private diagnosticsListener: vscode.Disposable | null = null;
  /** Tracks worst supersigil diagnostic severity per URI to detect changes. */
  private diagnosticSeverityByUri = new Map<
    string,
    vscode.DiagnosticSeverity
  >();
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(clients: Map<string, LanguageClient>) {
    this.clients = clients;
    this.diagnosticsListener = vscode.languages.onDidChangeDiagnostics(
      (e) => {
        let needsRefresh = false;
        for (const uri of e.uris) {
          const key = uri.toString();
          const severity = worstDiagnosticSeverity(uri);
          const previous = this.diagnosticSeverityByUri.get(key);

          if (severity !== undefined) {
            if (previous !== severity) {
              this.diagnosticSeverityByUri.set(key, severity);
              needsRefresh = true;
            }
          } else if (previous !== undefined) {
            this.diagnosticSeverityByUri.delete(key);
            needsRefresh = true;
          }
        }
        if (needsRefresh) {
          this.debouncedRefreshIcons();
        }
      },
    );
  }

  refresh(): void {
    this.cachedData = null;
    this.scheduleRefresh();
  }

  dispose(): void {
    this.diagnosticsListener?.dispose();
    this._onDidChangeTreeData.dispose();
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
  }

  /** Debounce tree invalidation to collapse rapid-fire updates. */
  private scheduleRefresh(): void {
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      this._onDidChangeTreeData.fire(undefined);
    }, 150);
  }

  /** Icon-only refresh (no cache invalidation). */
  private debouncedRefreshIcons(): void {
    this.scheduleRefresh();
  }

  async getTreeItem(element: SpecTreeItem): Promise<vscode.TreeItem> {
    switch (element.kind) {
      case "workspace-root": {
        const item = new vscode.TreeItem(
          element.label,
          vscode.TreeItemCollapsibleState.Collapsed,
        );
        item.iconPath = new vscode.ThemeIcon("root-folder");
        item.contextValue = "workspaceRoot";
        return item;
      }
      case "project": {
        const item = new vscode.TreeItem(
          element.label,
          vscode.TreeItemCollapsibleState.Collapsed,
        );
        const projectColor = this.worstChildColor(
          this.getDocumentsForProject(element.folderUri, element.label),
        );
        item.iconPath = new vscode.ThemeIcon("tag", projectColor);
        item.contextValue = "project";
        return item;
      }
      case "group": {
        const item = new vscode.TreeItem(
          element.label,
          vscode.TreeItemCollapsibleState.Collapsed,
        );
        const groupColor = this.worstChildColor(
          this.getDocumentsForGroup(
            element.folderUri,
            element.project,
            element.label,
          ),
        );
        item.iconPath = new vscode.ThemeIcon("folder", groupColor);
        item.description = `${element.documentCount} documents`;
        item.contextValue = "group";
        return item;
      }
      case "document": {
        const fileUri = vscode.Uri.joinPath(element.folderUri, element.path);
        const severity = worstDiagnosticSeverity(fileUri);
        const icon = iconForDocument(element.docType, element.status, severity);

        const slashIndex = element.id.indexOf("/");
        const label = slashIndex >= 0 ? element.id.substring(slashIndex + 1) : element.id;

        const item = new vscode.TreeItem(
          label,
          vscode.TreeItemCollapsibleState.None,
        );
        item.iconPath = icon;
        item.tooltip = element.id;
        item.description = element.status
          ? `${element.docType} · ${element.status}`
          : element.docType;
        item.command = {
          command: "vscode.open",
          title: "Open Document",
          arguments: [fileUri],
        };
        item.contextValue = "document";
        item.resourceUri = fileUri;
        return item;
      }
    }
  }

  async getChildren(element?: SpecTreeItem): Promise<SpecTreeItem[]> {
    if (!element) {
      return this.getRootChildren();
    }

    const data = this.cachedData;
    if (!data) return [];

    switch (element.kind) {
      case "workspace-root": {
        const root = data.workspaceRoots.get(element.folderUri.toString());
        if (!root) return [];
        return this.getProjectOrGroupChildren(root.projects, element.folderUri);
      }
      case "project": {
        const root = data.workspaceRoots.get(element.folderUri.toString());
        if (!root) return [];
        const groups = root.projects.get(element.label) ?? new Map();
        return this.getGroupChildren(groups, element.folderUri);
      }
      case "group": {
        const root = data.workspaceRoots.get(element.folderUri.toString());
        if (!root) return [];
        const groups =
          root.projects.get(element.project) ?? new Map();
        return groups.get(element.label) ?? [];
      }
      case "document":
        return [];
    }
  }

  private async getRootChildren(): Promise<SpecTreeItem[]> {
    const entries = await Promise.all(
      [...this.clients.entries()]
        .filter(([, client]) => client.isRunning())
        .map(async ([key, client]) => {
          const folder = vscode.workspace.workspaceFolders?.find(
            (f) => f.uri.toString() === key,
          );
          if (!folder) return null;

          try {
            const result = await client.sendRequest<DocumentListResult>(
              METHOD_DOCUMENT_LIST,
            );
            return { key, folderUri: folder.uri, folderName: folder.name, documents: result.documents };
          } catch (err) {
            console.warn(`[Supersigil] documentList request failed for ${folder.name}:`, err);
            return null;
          }
        }),
    );

    const documentsByFolder = new Map<
      string,
      { folderUri: vscode.Uri; folderName: string; documents: DocumentEntry[] }
    >();
    for (const entry of entries) {
      if (entry) {
        documentsByFolder.set(entry.key, {
          folderUri: entry.folderUri,
          folderName: entry.folderName,
          documents: entry.documents,
        });
      }
    }

    this.cachedData = groupDocuments(documentsByFolder);

    if (this.cachedData.multiRoot) {
      const roots: WorkspaceRootItem[] = [];
      for (const [, root] of this.cachedData.workspaceRoots) {
        roots.push({
          kind: "workspace-root",
          label: root.folderName,
          folderUri: root.folderUri,
        });
      }
      return roots;
    }

    const [root] = this.cachedData.workspaceRoots.values();
    if (!root) return [];
    return this.getProjectOrGroupChildren(root.projects, root.folderUri);
  }

  private getProjectOrGroupChildren(
    projects: Map<string | null, Map<string | null, DocumentItem[]>>,
    folderUri: vscode.Uri,
  ): SpecTreeItem[] {
    const hasProjects = [...projects.keys()].some((k) => k !== null);

    if (hasProjects) {
      const items: ProjectItem[] = [];
      for (const projectName of [...projects.keys()].filter((k): k is string => k !== null).sort()) {
        items.push({
          kind: "project",
          label: projectName,
          folderUri,
        });
      }
      const ungrouped = projects.get(null);
      if (ungrouped) {
        return [
          ...items,
          ...this.getGroupChildren(ungrouped, folderUri),
        ];
      }
      return items;
    }

    const groups = projects.values().next().value;
    if (!groups) return [];
    return this.getGroupChildren(groups, folderUri);
  }

  private getGroupChildren(
    groups: Map<string | null, DocumentItem[]>,
    folderUri: vscode.Uri,
  ): SpecTreeItem[] {
    const items: SpecTreeItem[] = [];
    for (const prefix of [...groups.keys()].sort(nullsLastCompare)) {
      const docs = groups.get(prefix)!;
      if (prefix === null) {
        items.push(...docs);
      } else {
        items.push({
          kind: "group",
          label: prefix,
          folderUri,
          documentCount: docs.length,
          project: docs[0]?.project ?? null,
        });
      }
    }
    return items;
  }

  private getDocumentsForProject(
    folderUri: vscode.Uri,
    projectName: string,
  ): DocumentItem[] {
    const root = this.cachedData?.workspaceRoots.get(folderUri.toString());
    if (!root) return [];
    const groups = root.projects.get(projectName);
    if (!groups) return [];
    return [...groups.values()].flat();
  }

  private getDocumentsForGroup(
    folderUri: vscode.Uri,
    project: string | null,
    prefix: string,
  ): DocumentItem[] {
    const root = this.cachedData?.workspaceRoots.get(folderUri.toString());
    if (!root) return [];
    const groups = root.projects.get(project);
    if (!groups) return [];
    return groups.get(prefix) ?? [];
  }

  private worstChildColor(docs: DocumentItem[]): vscode.ThemeColor | undefined {
    let hasUnresolved = false;
    for (const doc of docs) {
      const fileUri = vscode.Uri.joinPath(doc.folderUri, doc.path);
      const severity = worstDiagnosticSeverity(fileUri);
      if (severity === vscode.DiagnosticSeverity.Error) {
        return COLOR_FAILED;
      }
      if (severity === vscode.DiagnosticSeverity.Warning) {
        return COLOR_WARNING;
      }
      if (isUnresolved(doc.status)) {
        hasUnresolved = true;
      }
    }
    if (hasUnresolved) {
      return COLOR_QUEUED;
    }
    return undefined;
  }
}
