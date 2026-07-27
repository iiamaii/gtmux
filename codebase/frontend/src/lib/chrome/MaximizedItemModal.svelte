<script lang="ts">
  // MaximizedItemModal — workspace 전체를 최상단에서 덮는 modal overlay.
  // sessionStore.maximizedItemId 가 null 이면 렌더링 X.
  //
  // 설계 정합:
  // - in-flow PanelNode / NoteNode / DocumentNode 는 그대로 마운트 유지. modal 의 XtermHost 는
  //   dispatcher 의 multi-subscriber (ADR-0021 D1 mirror) 로 동일 paneId fan-out
  //   → 두 xterm 인스턴스 동시 활성, 모두 PANE_OUT 수신. content 손실 없음.
  // - note 는 sessionStore.items 의 동일 entry 를 양쪽 (in-flow + modal) 이 binding.
  //   InlineEdit / textarea 의 commit 이 store 를 갱신 → 양쪽 sync.
  // - schema item.x/y/w/h 무변경. modal 은 자체 viewport-fill 영역에 렌더.

  import { tick, untrack } from 'svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { terminalPool } from '$lib/stores/terminalPool.svelte';
  import { muxStore } from '$lib/stores/mux.svelte';
  import { documentViewModeStore } from '$lib/stores/documentViewMode.svelte';
  import { documentScrollStore } from '$lib/stores/documentScroll.svelte';
  import {
    attachDocumentScroll,
    type ScrollSurfaceKind,
  } from '$lib/canvas/documentScrollController';
  import {
    buildDocumentViewState,
    flushDocumentViewStateSave,
  } from '$lib/stores/documentViewStateSaver';
  import FindBar from '$lib/common/FindBar.svelte';
  import {
    DocumentFindController,
    recordFindHandoff,
    consumeFindHandoff,
  } from '$lib/common/documentFind.svelte';
  import {
    registerDocumentFindSurface,
    type DocumentFindOpenRequest,
  } from '$lib/common/findController';
  import { fsFileUrl } from '$lib/http/fs';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import {
    DOCUMENT_EXTENSIONS,
    basename,
    fileTypeLabelForPath,
    fileStem as workspaceFileStem,
    guessMimeFromPath,
    previewMetaForPath,
    resolveWorkspacePath,
    sourceLangForDocument,
    workspaceRelativePath,
  } from '$lib/files/workspaceAssets';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import CodeViewer from '$lib/canvas/CodeViewer.svelte';
  import CanvasGlyph from '$lib/canvas/CanvasGlyph.svelte';
  import DocumentMarkdownView from '$lib/viewers/DocumentMarkdownView.svelte';
  import HtmlViewer from '$lib/viewers/HtmlViewer.svelte';
  import PdfViewer from '$lib/viewers/PdfViewer.svelte';
  import PanelDanglingOverlay from '$lib/canvas/PanelDanglingOverlay.svelte';
  import { componentSettings } from '$lib/stores/componentSettings.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import {
    renderMarkdown,
    renderHtml,
    isToggleableFileType,
    RENDERED_HTML_IFRAME_SANDBOX,
    buildRenderedHtmlSrcdoc,
    type DocumentViewMode,
  } from '$lib/canvas/documentRender';
  import type { CanvasItem, DocumentItem, NoteItem } from '$lib/types/canvas';

  const itemId = $derived(sessionStore.maximizedItemId);
  const item = $derived(itemId !== null ? sessionStore.items.get(itemId) ?? null : null);
  const isTerminal = $derived(item?.type === 'terminal');
  const isNote = $derived(item?.type === 'note');
  const isDocument = $derived(item?.type === 'document');
  const terminalPaneId = $derived(itemId !== null ? terminalPool.paneIdFor(itemId) : undefined);

  const noteAccent = $derived(item?.type === 'note' ? item.color : null);
  const documentFileName = $derived.by(() => {
    if (item?.type !== 'document') return '';
    return item.file_name ?? (item.path !== undefined ? basename(item.path) : 'document');
  });

  const headerLabel = $derived.by(() => {
    if (item === null) return '—';
    if (item.type === 'note') return item.title.length > 0 ? item.title : 'Untitled';
    if (item.type === 'document') return documentFileName.length > 0 ? documentFileName : 'document';
    // Terminal label = persisted layout item.label (ADR-0050 D3, per-panel) —
    // same derivation as PanelNode's header. The in-memory terminal_meta label
    // is no longer consulted (it was wiped every boot). Fall back to a short id
    // when no persisted label exists. (No pane_id on this item shape — id only.)
    if (item.type === 'terminal') {
      const trimmed = item.label?.trim();
      if (trimmed !== undefined && trimmed.length > 0) return trimmed;
      return itemId !== null ? itemId.slice(0, 8) : '—';
    }
    if (item.label !== undefined && item.label !== null && item.label.length > 0) return item.label;
    return itemId !== null ? itemId.slice(0, 8) : '—';
  });

  const isDead = $derived.by(() => {
    if (terminalPaneId === undefined) return false;
    return muxStore.panes.get(terminalPaneId)?.dead === true;
  });

  let titleEditing = $state(false);
  let bodyEditing = $state(false);
  let documentAssetText = $state<string | null>(null);
  let documentAssetLoading = $state(false);
  let documentAssetError = $state<string | null>(null);

  const documentHasWorkspacePath = $derived(
    item?.type === 'document' && (item.path ?? '').length > 0,
  );
  const documentHasLegacyAsset = $derived(
    item?.type === 'document' && (item.asset_id ?? '').length > 0,
  );
  const documentIsInline = $derived(
    item?.type === 'document' && !documentHasWorkspacePath && !documentHasLegacyAsset,
  );
  const documentWorkspaceAbsolute = $derived(
    item?.type === 'document' && item.path !== undefined
      ? resolveWorkspacePath(sessionStore.effectiveWorkspaceRoot, item.path)
      : null,
  );
  const documentCopyPath = $derived(documentHasWorkspacePath ? documentWorkspaceAbsolute : null);
  // ADR-0016 header-parity 2026-07-27 — the "change document" action works while
  // maximized (item id is stable; content re-derives from the store), so the
  // modal header mirrors the canvas node's change button. Gated on !locked, like
  // DocumentNode's change button.
  const documentLocked = $derived(item?.type === 'document' && item.locked === true);
  const documentRemoteSrc = $derived(
    documentWorkspaceAbsolute !== null
      ? fsFileUrl(documentWorkspaceAbsolute)
      : item?.type === 'document' && documentHasLegacyAsset
        ? `/api/assets/${item.asset_id}`
        : '',
  );

  /** ADR-0018 D10 amend ③/④ (2026-05-21) — DocumentNode 와 동일 helper 사용
   *  으로 normal / maximize 양쪽 rendering 동기화. 옛 parseDocumentText 의
   *  paragraph slice 폐기. */
  const documentText = $derived.by(() => {
    if (item?.type !== 'document') return '';
    return documentIsInline ? (item.content ?? '') : (documentAssetText ?? '');
  });
  const documentFileTypeLabel = $derived.by(() => {
    if (item?.type !== 'document') return '';
    if (documentIsInline) return 'markdown';
    return fileTypeLabelForPath(documentFileName, item.mime);
  });
  const documentSourceLang = $derived(
    sourceLangForDocument(documentFileName, documentFileTypeLabel, documentIsInline),
  );
  /** ADR-0018 D10 amend ⑥ — viewMode persist via documentViewModeStore.
   *  DocumentNode (normal) 와 같은 itemId 구독 → normal↔maximize 전환 시
   *  reset 없음. */
  const documentViewMode = $derived.by((): DocumentViewMode => {
    if (item?.type !== 'document' || itemId === null) return 'rendered';
    return documentViewModeStore.get(itemId);
  });
  const documentCanToggleView = $derived(isToggleableFileType(documentFileTypeLabel));

  /** ADR-0037 D1 UI amend 2026-07-27 — set the rendered/source view mode
   *  directly (two-button segmented control). No-op when already on `next`.
   *  ADR-0056 D3 — persist the durable mode toggle immediately. */
  function setDocumentViewModeTo(next: DocumentViewMode): void {
    if (itemId === null || documentViewMode === next) return;
    documentViewModeStore.set(itemId, next);
    void flushDocumentViewStateSave(
      itemId,
      buildDocumentViewState(next, documentScrollStore.get(itemId)),
    );
  }
  const documentHtml = $derived.by(() => {
    if (documentFileTypeLabel === 'html') return renderHtml(documentText);
    return renderMarkdown(documentText);
  });

  const documentRenderedHtmlSrcdoc = $derived(buildRenderedHtmlSrcdoc(documentText));
  const canPreviewDocumentAsset = $derived.by(() => {
    if (item?.type !== 'document' || documentIsInline) return false;
    const mime = (item.mime ?? '').toLowerCase();
    return (
      mime.startsWith('text/') ||
      mime === 'application/json' ||
      previewMetaForPath(documentFileName).kind === 'text' ||
      ['markdown', 'html'].includes(documentFileTypeLabel)
    );
  });
  /** ADR-0018 D10 amend ⑦ — PDF asset 은 browser-native PDF viewer iframe. */
  const isDocumentPdf = $derived(
    item?.type === 'document'
    && documentFileTypeLabel === 'pdf'
    && documentRemoteSrc.length > 0,
  );
  const documentPdfSrc = $derived(isDocumentPdf ? documentRemoteSrc : '');

  // svelte-flow 의 selection 변경이 item prop 의 reactive proxy 를 새 ref 로
  // 갱신할 때 effect 의 dependency 가 invalidate → fetch 재시작 blink 회피.
  // 정본 = DocumentNode 의 같은 패턴.
  const documentFetchId = $derived.by((): string => {
    if (item?.type !== 'document' || documentIsInline || !canPreviewDocumentAsset) return '';
    return documentRemoteSrc;
  });

  $effect(() => {
    const src = documentFetchId;
    if (src.length === 0) {
      documentAssetText = null;
      documentAssetLoading = false;
      documentAssetError = null;
      return;
    }

    let cancelled = false;
    documentAssetText = null;
    documentAssetError = null;
    documentAssetLoading = true;

    async function loadDocumentAsset(): Promise<void> {
      try {
        const res = await fetch(src, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: 'text/plain,application/json,*/*' },
        });
        if (!res.ok) throw new Error(`GET document source returned ${res.status}`);
        const text = await res.text();
        if (!cancelled) documentAssetText = text;
      } catch (err) {
        if (!cancelled) documentAssetError = err instanceof Error ? err.message : String(err);
      } finally {
        if (!cancelled) documentAssetLoading = false;
      }
    }

    void loadDocumentAsset();
    return () => {
      cancelled = true;
    };
  });

  // ── xterm DOM portal ────────────────────────────────────────────────────
  // Maximize 시 in-flow PanelNode 의 `[data-portal-id={itemId}]` 컨테이너 안의
  // XtermHost DOM (xterm 인스턴스 의 containerEl 트리) 을 modal 의 slot 으로
  // reparent. XtermHost 컴포넌트 자체는 PanelNode 가 계속 mount 유지 → xterm
  // 인스턴스, ResizeObserver, dispatcher 등록 그대로 보존. cleanup 시 inflow
  // 로 다시 reparent. inflow 가 사라진 edge case (session switch 등) 는 child
  // 가 modal 과 함께 GC 되도록 noop.
  let portalSlotEl: HTMLDivElement | undefined = $state(undefined);

  $effect(() => {
    if (portalSlotEl === undefined) return;
    if (!isTerminal || itemId === null) return;
    // closure capture — Svelte 5 의 `bind:this` 는 element teardown 시점에
    // outer-scope `portalSlotEl` 을 undefined 로 reset. cleanup 함수가
    // 호출되는 시점 (modal markup unmount 직전) 에 outer-scope 의 값이
    // 이미 reset 되었으면 `node.parentNode === portalSlotEl` 체크가 false
    // 가 되어 inflow 로 복귀 안 됨 → 사용자 시각: terminal 화면 빈 채로
    // 남고 새로고침 해야 복구. 본 closure 변수 `slot` 은 effect run 시점의
    // reference 를 capture 하므로 reset 와 무관하게 비교 일관.
    const slot = portalSlotEl;
    const sel = `[data-portal-id="${itemId}"]`;
    const inflowHost = document.querySelector(sel) as HTMLElement | null;
    if (inflowHost === null) return;
    // inflow 의 first child (XtermHost containerEl) 만 portalSlot 으로 이동.
    // 다중 child 가능성 (예: pending placeholder) 대비해 looper.
    const moved: ChildNode[] = [];
    while (inflowHost.firstChild) {
      const child = inflowHost.firstChild;
      slot.appendChild(child);
      moved.push(child);
    }
    return () => {
      const home = document.querySelector(sel) as HTMLElement | null;
      if (home === null) return;
      for (const node of moved) {
        if (node.parentNode === slot) {
          home.appendChild(node);
        }
      }
    };
  });

  function onRestoreClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.unmaximize();
  }

  function interceptRenderedLinks(node: HTMLElement): { destroy: () => void } {
    function onClick(e: MouseEvent): void {
      const target = e.target;
      if (!(target instanceof Element)) return;
      const anchor = target.closest('a[href]') as HTMLAnchorElement | null;
      if (anchor === null || !node.contains(anchor)) return;
      e.preventDefault();
      e.stopPropagation();
      window.open(anchor.href, '_blank', 'noopener,noreferrer');
    }
    node.addEventListener('click', onClick);
    return {
      destroy: () => node.removeEventListener('click', onClick),
    };
  }

  function blockBackdropEvent(e: Event): void {
    if (e.target !== e.currentTarget) return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (item === null) return;
    // escRouter consumers (e.g. the FindBar's p1 close, ADR-0058 D1) run on
    // the same window keydown and mark the event consumed via preventDefault
    // — respect that so one Esc doesn't close find AND unmaximize.
    if (e.defaultPrevented) return;
    if (e.key === 'Escape' && !titleEditing && !bodyEditing) {
      sessionStore.unmaximize();
    }
  }

  async function copyDocumentPath(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    const path = documentCopyPath;
    if (path === null) return;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  function initialDocumentDir(): string {
    const root = sessionStore.effectiveWorkspaceRoot;
    if (documentWorkspaceAbsolute === null) return root;
    const slash = documentWorkspaceAbsolute.lastIndexOf('/');
    return slash <= 0 ? root : documentWorkspaceAbsolute.slice(0, slash);
  }

  // ADR-0016 header-parity 2026-07-27 — mirrors DocumentNode.onLoadFileClick.
  // Swaps the maximized document's backing file in place (same item id → the
  // modal re-renders the new content). No maximize/restore round-trip needed.
  function onChangeDocumentClick(e: MouseEvent): void {
    e.stopPropagation();
    if (item === null || item.type !== 'document' || itemId === null) return;
    const targetId = itemId;
    const workspaceRoot = sessionStore.effectiveWorkspaceRoot;
    if (workspaceRoot.length === 0) {
      toastStore.show({ message: 'Workspace root is not available yet.', tone: 'error' });
      return;
    }
    filePicker.openFor(initialDocumentDir(), (absolutePath) => {
      const nextPath = workspaceRelativePath(workspaceRoot, absolutePath);
      if (nextPath === null) {
        toastStore.show({
          message: 'Document files must be inside the active project workspace.',
          tone: 'error',
        });
        return;
      }
      const nextFileName = basename(absolutePath);
      void sessionStore.applyMutation(
        (cur) => ({
          ...cur,
          items: cur.items.map((it: CanvasItem) =>
            it.id === targetId && it.type === 'document'
              ? ({
                  ...it,
                  path: nextPath,
                  asset_id: undefined,
                  label: workspaceFileStem(nextFileName),
                  file_name: nextFileName,
                  mime: guessMimeFromPath(absolutePath),
                  size_bytes: undefined,
                  content: undefined,
                } as DocumentItem)
              : it,
          ),
        }),
        {
          abortMessage: 'Document file change aborted — session reconnect failed.',
          failMessage: 'Document file change failed',
        },
      );
    }, {
      accept: { extensions: [...DOCUMENT_EXTENSIONS], description: 'document files' },
      rootKind: 'workspace',
      rootPath: workspaceRoot,
    });
  }

  async function commitNoteField(field: 'title' | 'body', next: string): Promise<void> {
    if (item === null || item.type !== 'note') return;
    if (item[field] === next) {
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === item.id && it.type === 'note'
            ? ({ ...it, [field]: next } as NoteItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Note edit aborted — session reconnect failed.',
        failMessage: 'Note commit failed',
      },
    );
    if (result.ok) {
      if (field === 'title') titleEditing = false;
      else bodyEditing = false;
    }
  }

  // ── In-document find (ADR-0058) — maximize surface ──
  let documentBodyEl = $state<HTMLElement | null>(null);
  let findBar = $state<{
    focusInput: (opts?: { selectAll?: boolean }) => void;
    prefill: (text: string) => void;
  } | null>(null);

  const findCtl = new DocumentFindController({
    getRoot: () => documentBodyEl,
    getCodeText: () => documentText,
  });

  /** Searchable views only (ADR-0058 D3/D4): markdown rendered + source view.
   *  Not html rendered (sandbox iframe) / pdf / empty. */
  const findSearchable = $derived.by((): boolean => {
    if (item?.type !== 'document' || itemId === null) return false;
    if (isDocumentPdf) return false;
    if (documentText.trim().length === 0) return false;
    if (documentViewMode === 'source') return true;
    return documentFileTypeLabel !== 'html';
  });

  function openFindSurface(req?: DocumentFindOpenRequest): void {
    findCtl.openBar();
    void tick().then(() => {
      // First line only — line-based code matching never crosses newlines
      // (ADR-0058 D2).
      const prefill = (req?.prefill ?? '').split('\n')[0]?.trim() ?? '';
      if (prefill.length > 0) findBar?.prefill(prefill);
      findBar?.focusInput({ selectAll: true });
    });
  }

  // Cmd/Ctrl+F routing target (ADR-0058 D5 branch 1) — while searchable.
  $effect(() => {
    if (!findSearchable || itemId === null) return;
    return registerDocumentFindSurface(`max:${itemId}`, openFindSurface);
  });

  // Transition close + restore handoff (ADR-0058 D1 override 2026-07-23). This
  // modal component stays mounted; only its `{#if item !== null}` content
  // unmounts on unmaximize, so this script-level effect RE-RUNS on itemId change
  // rather than tearing down. The cleanup captures the OUTGOING id and, before
  // closing, stashes a handoff so the canvas node's find resumes with the same
  // query/index. Recorded ONLY when find is still open at the transition — an
  // explicit user close (Esc/×) already set open=false, so nothing is handed off
  // (explicit close = closed everywhere). Reads in the cleanup don't establish
  // reactive deps (no tracked-read re-fire), so no untrack needed.
  $effect(() => {
    const id = itemId;
    return () => {
      if (id !== null && findCtl.open) {
        recordFindHandoff(id, findCtl.query, findCtl.currentIndex);
      }
      findCtl.close();
    };
  });

  $effect(() => {
    if (!findSearchable && findCtl.open) findCtl.close();
  });

  // Maximize handoff — consume the node's find on mount (ADR-0058 D1 override
  // 2026-07-23, survive maximize/restore). When this document was maximized
  // while its canvas node's find was open, the node stashed its query + index;
  // open the modal's find with it (after tick so documentBodyEl is mounted).
  // Depends on findSearchable so asset-backed docs (async text) consume once the
  // searchable surface exists. openWith runs deferred, so the sibling
  // itemId-close effect (sync, on mount) doesn't clobber it.
  $effect(() => {
    const id = itemId;
    if (id === null || !findSearchable) return;
    // Consume deferred into the post-flush tick() callback (not the body): the
    // node writes the handoff in its maximize effect during the same
    // maximizedItemId flush, so reading it after tick avoids a
    // consume-before-record race. documentBodyEl is also mounted by then.
    void tick().then(() =>
      untrack(() => {
        if (itemId !== id || !findSearchable) return;
        const handoff = consumeFindHandoff(id);
        if (handoff === null) return;
        findCtl.openWith(handoff.query, handoff.currentIndex);
      }),
    );
  });

  $effect(() => {
    return () => findCtl.destroy();
  });

  // ── Document scroll persistence (ADR-0056) — maximize surface ──
  const maxScrollSurfaceKind = $derived.by((): ScrollSurfaceKind | null => {
    if (item?.type !== 'document' || itemId === null) return null;
    if (isDocumentPdf) return null;
    if (documentText.trim().length === 0) return null;
    if (documentViewMode === 'source') return 'line';
    if (documentFileTypeLabel === 'html') return null; // rendered HTML = iframe
    return 'block';
  });

  // Attach on maximize open / view switch / content load.
  $effect(() => {
    const kind = maxScrollSurfaceKind;
    const host = documentBodyEl;
    void documentText; // re-run when async asset content lands (relayout)
    const id = itemId;
    if (kind === null || host === null || id === null) return;
    const el = host.querySelector<HTMLElement>(
      kind === 'line' ? '.code-viewer' : '.document-markdown-view',
    );
    if (el === null) return;
    const seed = item?.type === 'document' ? (item.view_state?.anchor ?? null) : null;
    return attachDocumentScroll({
      el,
      kind,
      itemId: id,
      seedAnchor: seed !== null && seed.kind === kind ? seed : null,
      getMode: () => documentViewMode,
    });
  });

  // ADR-0056 D4 (ii) — flush the durable view_state immediately on maximize
  // open AND close (no debounce wait) so the shared position survives the
  // surface swap + an F5 right after. The store holds the live anchor for both
  // transitions (canvas position at open, maximize position at close).
  $effect(() => {
    const id = itemId;
    if (id === null || item?.type !== 'document') return;
    // Effect keys on maximize open/close identity only — the value reads are
    // untracked so live scrolling (documentScrollStore.set during scroll) does
    // not re-fire this effect and cancel the D4 debounce (ADR-0056 D4 (ii)).
    const flush = (): void => {
      void flushDocumentViewStateSave(
        id,
        untrack(() =>
          buildDocumentViewState(documentViewModeStore.get(id), documentScrollStore.get(id)),
        ),
      );
    };
    flush();
    return () => flush();
  });
</script>

<svelte:window onkeydown={onKeyDown} />

{#if item !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="max-modal-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Maximized item"
    tabindex="-1"
    onpointerdown={blockBackdropEvent}
    onpointerup={blockBackdropEvent}
    onmousedown={blockBackdropEvent}
    onmouseup={blockBackdropEvent}
    onclick={blockBackdropEvent}
    ondblclick={blockBackdropEvent}
    oncontextmenu={blockBackdropEvent}
    onwheel={blockBackdropEvent}
  >
    <div
      class="max-card"
      class:is-note={isNote}
      style:--note-accent={noteAccent ?? 'var(--color-accent)'}
    >
      <header class="max-header">
        <!-- Type-identity glyph — unified via CanvasGlyph note/file/terminal
             (icon unification 2026-07-27, ADR-0016 정합; 13px chrome tier). -->
        {#if isNote}
          <span class="header-glyph note-glyph" aria-hidden="true">
            <CanvasGlyph name="note" size={13} />
          </span>
        {:else if isDocument}
          <span class="header-glyph" aria-hidden="true">
            <CanvasGlyph name="file" size={13} />
          </span>
        {:else}
          <span class="header-glyph" aria-hidden="true">
            <CanvasGlyph name="terminal" size={13} />
          </span>
        {/if}
        {#if isNote && titleEditing}
          <span class="header-title-edit">
            <InlineEditField
              value={item.type === 'note' ? item.title : ''}
              editing={true}
              allowEmpty={true}
              plain={true}
              placeholder="Title…"
              onCommit={(next: string) => void commitNoteField('title', next)}
              onCancel={() => (titleEditing = false)}
            />
          </span>
        {:else}
          <button
            type="button"
            class="header-title"
            ondblclick={isNote ? () => (titleEditing = true) : undefined}
            disabled={!isNote}
            title={isNote ? 'Double-click to rename' : headerLabel}
          >{headerLabel}</button>
        {/if}
        {#if isTerminal}
          <span class="max-status" aria-label="Panel status">
            <span class="led" class:dead={isDead} aria-hidden="true"></span>
            <span class="status-label">{isDead ? 'dead' : 'running'}</span>
          </span>
        {:else}
          <span class="spacer"></span>
        {/if}
        <div class="max-actions">
          {#if isDocument && documentCanToggleView}
            <!-- ADR-0037 amend / ADR-0037 D1 UI amend 2026-07-27 — [Rendered |
                 Source] segmented control leading the actions (same vocabulary
                 as SnippetsNode's .snip-mode-group + DocumentNode). Active side
                 accent-filled like the snippet modes (not an edit mode → no
                 purple). Mode persists via documentViewModeStore; the D3 flush
                 lives in setDocumentViewModeTo. -->
            <div class="max-mode-group" role="group" aria-label="Document view mode">
              <button
                type="button"
                class="max-btn max-mode-btn"
                class:is-active={documentViewMode === 'rendered'}
                aria-label="Rendered view"
                aria-pressed={documentViewMode === 'rendered'}
                title="Rendered"
                onclick={(e: MouseEvent) => { e.stopPropagation(); setDocumentViewModeTo('rendered'); }}
              >
                <!-- book-open (rendered) — visibility eye 와 겹침 회피. -->
                <CanvasGlyph name="book-open" size={13} />
              </button>
              <button
                type="button"
                class="max-btn max-mode-btn"
                class:is-active={documentViewMode === 'source'}
                aria-label="Source view"
                aria-pressed={documentViewMode === 'source'}
                title="Source"
                onclick={(e: MouseEvent) => { e.stopPropagation(); setDocumentViewModeTo('source'); }}
              >
                <!-- </> code (source) -->
                <CanvasGlyph name="code" size={13} />
              </button>
            </div>
          {/if}
          {#if isDocument && findSearchable}
            <!-- ADR-0058 D4 — magnifier opens the in-document FindBar. -->
            <button
              type="button"
              class="max-btn"
              class:is-active={findCtl.open}
              aria-label="Find in document"
              title="Find in document"
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                openFindSurface();
              }}
            >
              <CanvasGlyph name="search" size={13} />
            </button>
          {/if}
          {#if isDocument && documentCopyPath !== null}
            <button
              type="button"
              class="max-btn"
              aria-label="Copy path"
              title="Copy path"
              onclick={(e) => void copyDocumentPath(e)}
            >
              <CanvasGlyph name="copy" size={13} />
            </button>
          {/if}
          {#if isDocument && !documentLocked}
            <!-- ADR-0016 header-parity 2026-07-27 — change the backing document
                 file in place (works while maximized; item id stable). Mirrors
                 DocumentNode's change button. -->
            <button
              type="button"
              class="max-btn"
              aria-label="Change document"
              title="Change document"
              onclick={onChangeDocumentClick}
            >
              <CanvasGlyph name="change" size={13} />
            </button>
          {/if}
          <!-- The modal IS the maximized state, so the restore control always
               shows the lucide minimize (corner-brackets-in) glyph in active
               state — icon system unification 2026-07-27 (ADR-0016 정합). -->
          <button
            type="button"
            class="max-btn is-active"
            aria-label="Restore"
            title="Restore (Esc)"
            onclick={onRestoreClick}
          >
            <CanvasGlyph name="restore-max" size={13} />
          </button>
        </div>
      </header>

      <div class="max-body">
        {#if isTerminal}
          {#if terminalPaneId !== undefined}
            <!-- xterm DOM portal target — in-flow PanelNode 의 xterm 컨테이너
                 DOM 이 maximize 동안 본 div 로 reparent (JS appendChild).
                 단일 xterm 인스턴스 가 in-flow ↔ modal 사이를 이동 — state /
                 scroll buffer / dispatcher 등록 모두 보존. -->
            <div class="xterm-portal-slot" bind:this={portalSlotEl}></div>
          {:else}
            <div class="max-pending" role="status" aria-live="polite">
              <div class="pending-title">Terminal stream connecting…</div>
              <div class="pending-hint">Waiting for spawn handshake.</div>
            </div>
          {/if}
          {#if itemId !== null}
            <PanelDanglingOverlay terminalId={itemId} />
          {/if}
        {:else if isNote && item.type === 'note'}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!--
            R6 (ADR-0018 D9 amend, batch-5 Grill #13): MaximizedItemModal 안의
            note body 도 NoteNode 와 동일하게 host wrapper 전체가 dblclick zone.
            padding / empty area 어디서든 dblclick → body editing 진입.
          -->
          <div
            class="note-body-host"
            style:--note-content-scale={componentSettings.noteScale}
            ondblclick={() => (bodyEditing = true)}
          >
            {#if bodyEditing}
              <InlineEditTextarea
                value={item.body}
                editing={true}
                allowEmpty={true}
                plain={true}
                placeholder="Body…"
                onCommit={(next: string) => void commitNoteField('body', next)}
                onCancel={() => (bodyEditing = false)}
              />
            {:else}
              <pre
                class="note-body-text"
                class:empty={item.body.length === 0}
              >{item.body.length > 0 ? item.body : 'Double-click to add body'}</pre>
            {/if}
          </div>
        {:else if isDocument && item.type === 'document'}
          {#if findCtl.open && findSearchable}
            <!-- ADR-0058 D1 — floating find overlay, anchored to .max-body
                 (stays put while .document-body-host scrolls). -->
            <FindBar
              bind:this={findBar}
              matchCount={findCtl.count}
              currentIndex={findCtl.currentIndex}
              capped={findCtl.capped}
              initialQuery={findCtl.query}
              onQueryChange={(q: string) => findCtl.setQuery(q)}
              onNavigate={(dir: 1 | -1) => findCtl.navigate(dir)}
              onClose={() => findCtl.close()}
            />
          {/if}
          <article
            bind:this={documentBodyEl}
            class="document-body-host nowheel"
            data-find-surface={findSearchable ? `max:${itemId}` : undefined}
            onwheel={(e) => e.stopPropagation()}
          >
            {#if isDocumentPdf}
              <!-- ADR-0018 D10 amend ⑦ — PDF iframe (browser-native viewer).
                   sandbox 미지정: PDF plugin 의 same-origin 요구. -->
              <PdfViewer
                src={documentPdfSrc}
                title={documentFileName}
              />
            {:else if !documentIsInline && documentAssetLoading}
              <div class="document-empty">Loading preview…</div>
            {:else if !documentIsInline && !canPreviewDocumentAsset}
              <div class="document-asset-summary">
                <div class="document-eyebrow">Document file</div>
                <h1>{documentFileName}</h1>
                <p>Preview is not available for this document type.</p>
              </div>
            {:else if !documentIsInline && documentAssetError !== null}
              <div class="document-asset-summary">
                <div class="document-eyebrow">Document file</div>
                <h1>{documentFileName}</h1>
                <p>{documentAssetError}</p>
              </div>
            {:else if documentText.length === 0}
              <div class="document-empty">Empty document</div>
            {:else}
              <div class="document-eyebrow">{documentIsInline ? 'Inline document' : 'Document file'}</div>
              <!-- ADR-0018 D10 amend ③/④/⑤ + ADR-0037 — DocumentNode 와 동일
                   markdown/html/source rendering. -->
              {#if documentViewMode === 'source'}
                <div
                  class="document-source-view"
                  style:--document-content-scale={componentSettings.documentScale}
                >
                  <CodeViewer text={documentText} lang={documentSourceLang} filename={documentFileName} />
                </div>
              {:else if documentFileTypeLabel === 'html'}
                <HtmlViewer
                  title={documentFileName}
                  srcdoc={documentRenderedHtmlSrcdoc}
                  sandbox={RENDERED_HTML_IFRAME_SANDBOX}
                />
              {:else}
                <div class="document-markdown-host" use:interceptRenderedLinks>
                  <DocumentMarkdownView
                    html={documentHtml}
                    label={documentFileName}
                    eyebrow={documentIsInline ? 'Inline document' : 'Document file'}
                    scale={componentSettings.documentScale}
                  />
                </div>
              {/if}
            {/if}
          </article>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .max-modal-backdrop {
    position: absolute;
    inset: 0;
    background: transparent;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    z-index: var(--z-modal);
    display: flex;
    align-items: stretch;
    justify-content: stretch;
  }

  .max-card {
    flex: 1 1 auto;
    margin: var(--space-12);
    background: var(--color-surface);
    color: var(--color-fg);
    display: grid;
    grid-template-rows: 36px 1fr;
    overflow: hidden;
    font-family: var(--font-sans);
    box-shadow: 0 20px 48px rgba(0,0,0,.22), 0 0 0 1px var(--color-border);
    border-radius: var(--radius-md);
  }
  .max-card.is-note {
    border-left: 2px solid var(--note-accent);
  }

  .max-header {
    display: flex; align-items: center; gap: 10px;
    padding: 0 6px 0 12px;
    background: var(--color-surface-2);
    border-bottom: 1px solid var(--color-border);
    height: 36px;
    user-select: none;
  }

  .header-glyph {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--color-fg);
    opacity: .8;
  }
  .header-glyph.note-glyph {
    color: var(--note-accent);
    opacity: 1;
  }

  /* Header title — NoteNode-anchored micro-label family (icon system
     unification 2026-07-27, ADR-0016 정합): mono · 9.5px · 540 · 0.6px.
     NO uppercase — the title is a filename / terminal id / note title
     (case-bearing content). */
  .header-title {
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: var(--weight-semibold);
    letter-spacing: 0.6px;
    color: var(--color-fg);
    background: transparent;
    border: 0;
    padding: 0;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
    text-align: left;
    cursor: text;
  }
  .header-title:disabled {
    cursor: default;
  }

  .header-title-edit {
    flex: 0 1 auto;
    min-width: 120px;
  }

  .max-status {
    display: flex; align-items: center; gap: 6px;
    margin-left: auto;
    margin-right: 4px;
    font-family: var(--font-mono);
    font-size: 9px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    flex-shrink: 0;
  }
  .max-status .led {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 28%, transparent);
  }
  .max-status .led.dead {
    background: var(--color-danger);
    box-shadow: none;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .max-actions {
    display: flex; align-items: center; gap: 1px;
    flex-shrink: 0;
  }
  .max-btn {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    border: 0;
    background: transparent;
    border-radius: var(--radius-sm);
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
  }
  .max-btn:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }
  /* Toggle-ON standard (SoT §3) — a persistent "view/surface active" toggle
     (find open, maximized→restore) tints its icon with the rail current-tab
     accent, mirroring .rail-btn.active. The [Rendered|Source] mode segment
     keeps its accent FILL via the more-specific .max-mode-btn override below.
     Accent is theme-agnostic → reads on light + dark. */
  .max-btn.is-active,
  .max-btn.is-active:hover {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
  }

  /* ADR-0037 D1 UI amend 2026-07-27 — [Rendered | Source] segmented control.
     Faint container + inter-button gap = one unit (matches SnippetsNode's
     .snip-mode-group + DocumentNode). Buttons drop to 22×22 inside the group so
     the segmentation reads tight; active side uses the neutral highlight above
     (not an edit mode → no purple). */
  .max-mode-group {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 1px;
    background: var(--color-glass-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    /* Mode-group ↔ neighbouring buttons = 4px (SoT §1: group-adjacency gap).
       3px side margin + the cluster's 1px flex gap = 4px to the next button;
       plain button↔button stays 1px (2026-07-27 refinement). */
    margin: 0 3px;
  }
  .max-mode-btn {
    width: 22px;
    height: 22px;
  }
  /* Active side accent-filled like the snippet modes (2026-07-27), kept on
     hover. */
  .max-mode-btn.is-active,
  .max-mode-btn.is-active:hover:not(:disabled) {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .max-body {
    background: var(--color-bg);
    overflow: hidden;
    position: relative;
    min-height: 0;
  }

  /* xterm DOM portal target — in-flow PanelNode 의 xterm 컨테이너 가 본 div
     안으로 이동. flex 로 width/height 100% 채움 (xterm 의 ResizeObserver 가
     fit() 자동 호출 → cell 크기 재계산). */
  .xterm-portal-slot {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--xterm-bg);
  }
  :global(.xterm-portal-slot > :first-child) {
    flex: 1 1 auto;
    min-height: 0;
  }

  .max-pending {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
    color: var(--color-fg-muted);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.2px;
  }
  .pending-title { color: var(--color-fg); }
  .pending-hint { color: var(--color-fg-subtle); font-size: 10px; margin-top: 4px; }

  .note-body-host {
    width: 100%; height: 100%;
    background: var(--color-surface);
    padding: 24px 36px;
    overflow: auto;
  }
  .note-body-text {
    margin: 0;
    font-family: var(--font-sans);
	    font-size: calc(var(--text-lg) * var(--note-content-scale, 1));
    line-height: 1.55;
    letter-spacing: 0;
    color: var(--color-fg);
    white-space: pre-wrap;
    word-break: break-word;
    cursor: text;
  }
  .note-body-text.empty {
    color: var(--color-fg-subtle);
    font-style: italic;
  }

  .document-body-host {
    width: 100%;
    height: 100%;
    background: var(--color-surface);
    padding: 42px 58px;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-width: thin;
  }

  .document-eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    margin-bottom: 18px;
  }

  .document-body-host h1 {
    margin: 0 0 18px;
    font-size: 34px;
    font-weight: var(--weight-semibold);
    line-height: 1.12;
    color: var(--color-fg);
  }

  .document-body-host p {
    margin: 0 0 10px;
    max-width: 80ch;
    font-size: 14px;
    line-height: 1.6;
    color: var(--color-fg-muted);
    overflow-wrap: anywhere;
  }

  .document-source-view {
    flex: 1 1 auto;
    min-height: 0;
    height: 100%;
    --code-viewer-font-size: calc(13px * var(--document-content-scale, 1));
    --code-viewer-line-height: 1.6;
    --code-viewer-gutter-width: 42px;
    --code-viewer-padding: 12px 0;
  }
  .document-body-host:has(.document-source-view) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .document-body-host:has(.document-source-view) .document-eyebrow {
    display: none;
  }

  .document-body-host:has(:global(.document-markdown-view)) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .document-body-host:has(:global(.document-markdown-view)) .document-eyebrow {
    display: none;
  }

  .document-markdown-host {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* ADR-0037 amend — rendered HTML is isolated in a sandboxed iframe. */
  .document-body-host:has(:global(.html-viewer-frame)) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .document-body-host:has(:global(.html-viewer-frame)) .document-eyebrow {
    display: none;
  }

  /* ADR-0018 D10 amend ⑦ (2026-05-22) — PDF iframe (browser-native viewer).
     rendered HTML iframe 과 달리 height auto-fit 안 함 — PDF plugin 의
     internal scroll + multi-page nav 사용. host 100% 채우고 padding 제거. */
  .document-body-host:has(:global(.pdf-viewer-frame)) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .document-asset-summary h1 {
    margin: 0 0 12px;
  }

  .document-empty {
    height: 100%;
    display: grid;
    place-items: center;
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.7px;
    text-transform: uppercase;
    color: var(--color-fg-subtle);
  }
</style>
