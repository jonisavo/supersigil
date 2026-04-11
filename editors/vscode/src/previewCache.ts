import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import {
  renderComponentTree,
  filterNovelEdges,
  type FenceData,
  type EdgeData,
  type LinkResolver,
} from "@supersigil/preview";
import { DocumentEntry, METHOD_DOCUMENT_COMPONENTS } from "./specExplorer";

// ---------------------------------------------------------------------------
// LSP response types
// ---------------------------------------------------------------------------

interface DocumentComponentsParams {
  uri: string;
}

interface DocumentComponentsResult {
  document_id: string;
  stale: boolean;
  fences: FenceData[];
  edges: EdgeData[];
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

interface CacheEntry {
  result: DocumentComponentsResult;
}

// ---------------------------------------------------------------------------
// PreviewCache
// ---------------------------------------------------------------------------

/**
 * Per-document cache of `documentComponents` responses.
 *
 * - On cache hit: render directly (with stale indicator when stale).
 * - On cache miss: return a loading placeholder, fire async fetch,
 *   then trigger `markdown.preview.refresh` when data arrives.
 * - On `supersigil/documentsChanged`: invalidate all entries and
 *   re-fetch for documents with open previews.
 */
export class PreviewCache {
  private cache = new Map<string, CacheEntry>();
  private pending = new Set<string>();
  private clients: Map<string, LanguageClient>;
  private documentListCache: Map<string, DocumentEntry>;
  private output: vscode.OutputChannel;

  constructor(
    clients: Map<string, LanguageClient>,
    documentListCache: Map<string, DocumentEntry>,
    output: vscode.OutputChannel,
  ) {
    this.clients = clients;
    this.documentListCache = documentListCache;
    this.output = output;
  }

  /** Clear the entire cache and re-fetch for open previews. */
  invalidateAll(): void {
    this.output.appendLine("[preview] Cache invalidated, re-fetching open previews");
    this.cache.clear();
    this.refetchOpenPreviews();
  }

  /** Clear a single document entry. */
  invalidate(uri: string): void {
    this.cache.delete(uri);
  }

  /** Get cached data for a document URI, or trigger an async fetch. */
  get(uri: string): CacheEntry | undefined {
    const entry = this.cache.get(uri);
    if (entry) return entry;

    // Cache miss: trigger async fetch
    this.fetchAsync(uri);
    return undefined;
  }

  // -------------------------------------------------------------------------
  // Async fetch
  // -------------------------------------------------------------------------

  private fetchAsync(uri: string): void {
    if (this.pending.has(uri)) return;
    this.pending.add(uri);

    const client = this.clientForUri(uri);
    if (!client?.isRunning()) {
      this.pending.delete(uri);
      return;
    }

    const params: DocumentComponentsParams = { uri };
    this.output.appendLine(`[preview] Fetching components for ${uri}`);
    client
      .sendRequest<DocumentComponentsResult>(METHOD_DOCUMENT_COMPONENTS, params)
      .then((result) => {
        this.output.appendLine(`[preview] Got ${result.fences.length} fences for ${result.document_id} (stale=${result.stale})`);
        this.cache.set(uri, { result });
        this.pending.delete(uri);
        vscode.commands.executeCommand("markdown.preview.refresh");
      })
      .catch((err) => {
        this.output.appendLine(`[preview] Fetch failed for ${uri}: ${err}`);
        this.pending.delete(uri);
      });
  }

  /**
   * Re-fetch for documents that likely have open Markdown previews.
   *
   * VS Code does not expose which documents have open Markdown previews,
   * so we use `visibleTextEditors` as a heuristic: if a Markdown file is
   * visible in an editor, its preview is likely open alongside it.
   */
  private refetchOpenPreviews(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      const doc = editor.document;
      if (
        doc.languageId === "markdown" ||
        doc.languageId === "mdx"
      ) {
        this.fetchAsync(doc.uri.toString());
      }
    }
  }

  private clientForUri(uri: string): LanguageClient | undefined {
    const vsUri = vscode.Uri.parse(uri);
    const folder = vscode.workspace.getWorkspaceFolder(vsUri);
    if (folder) {
      return this.clients.get(folder.uri.toString());
    }
    return undefined;
  }

  // -------------------------------------------------------------------------
  // Rendering
  // -------------------------------------------------------------------------

  /**
   * Render a supersigil-xml fence token to HTML.
   *
   * This is called synchronously from the markdown-it fence renderer.
   * `fenceIndex` is the 0-based index of this supersigil-xml fence
   * in the document (used for document-order correlation).
   *
   * Returns HTML string: either rendered components, a stale indicator,
   * or a loading placeholder.
   */
  renderFence(
    fenceIndex: number,
    documentUri: string,
  ): string {
    const entry = this.get(documentUri);

    if (!entry) {
      this.output.appendLine(`[preview] renderFence(${fenceIndex}, ${documentUri}): cache miss, showing loading`);
      return this.renderLoading();
    }

    const { result } = entry;
    const stale = result.stale;

    // Use document-order correlation: the nth supersigil-xml fence
    // in the markdown corresponds to the nth FenceData entry.
    const fence = fenceIndex < result.fences.length
      ? result.fences[fenceIndex]
      : undefined;

    if (!fence) {
      // Index out of range (stale data may have different fence count)
      if (result.fences.length > 0 && fenceIndex === 0) {
        // Fallback: render all fences together
        const resolver = this.createLinkResolver(documentUri);
        const html = renderComponentTree(result.fences, result.edges, resolver);
        if (stale) {
          return this.wrapStale(html);
        }
        return html;
      }
      return "";
    }

    const resolver = this.createLinkResolver(documentUri);
    const html = renderComponentTree([fence], [], resolver);
    if (stale) {
      return this.wrapStale(html);
    }
    return html;
  }

  /**
   * Render document-level edges (shown after all fences).
   * Returns empty string if no edges or data not cached yet.
   */
  renderEdges(documentUri: string): string {
    const entry = this.cache.get(documentUri);
    if (!entry || entry.result.edges.length === 0) return "";
    const resolver = this.createLinkResolver(documentUri);
    // Filter out edges already shown as components in fences.
    const novelEdges = filterNovelEdges(entry.result.fences, entry.result.edges);
    if (novelEdges.length === 0) return "";
    return renderComponentTree([], novelEdges, resolver);
  }

  // -------------------------------------------------------------------------
  // Link resolver
  // -------------------------------------------------------------------------

  private createLinkResolver(documentUri: string): LinkResolver {
    const vsUri = vscode.Uri.parse(documentUri);
    const folder = vscode.workspace.getWorkspaceFolder(vsUri);
    const workspaceRoot = folder?.uri.fsPath ?? "";
    const docListCache = this.documentListCache;

    // Use vscode:// URIs which pass through the Markdown preview's
    // click handler and are routed to the extension's registerUriHandler.
    // Format: vscode://savolainen.supersigil/<action>?<params>
    const base = "vscode://savolainen.supersigil";

    return {
      evidenceLink(file: string, line: number): string {
        const fullPath = workspaceRoot ? `${workspaceRoot}/${file}` : file;
        return `${base}/open-file?path=${encodeURIComponent(fullPath)}&line=${line}`;
      },

      documentLink(docId: string): string {
        const entry = docListCache.get(docId);
        if (!entry) return "#";
        const fullPath = workspaceRoot
          ? `${workspaceRoot}/${entry.path}`
          : entry.path;
        return `${base}/open-file?path=${encodeURIComponent(fullPath)}`;
      },

      criterionLink(docId: string, criterionId: string): string {
        return `${base}/open-criterion?doc=${encodeURIComponent(docId)}&criterion=${encodeURIComponent(criterionId)}`;
      },
    };
  }

  // -------------------------------------------------------------------------
  // Placeholder HTML
  // -------------------------------------------------------------------------

  private renderLoading(): string {
    return `<div class="supersigil-loading">
  <span class="supersigil-loading-text">Loading supersigil components\u2026</span>
</div>`;
  }

  private wrapStale(html: string): string {
    return `<div class="supersigil-stale">
  <div class="supersigil-stale-indicator" title="Data may be out of date">Stale</div>
  ${html}
</div>`;
  }

  // -------------------------------------------------------------------------
  // Document list cache management
  // -------------------------------------------------------------------------

  /** Update the document list cache (called after documentList responses). */
  updateDocumentList(documents: DocumentEntry[]): void {
    this.documentListCache.clear();
    for (const doc of documents) {
      this.documentListCache.set(doc.id, doc);
    }
  }
}
