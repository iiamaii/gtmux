<script lang="ts">
  /**
   * DocumentNode — SvelteFlow custom node for `type: "document"`.
   *
   * 정본 시안: `ref/frontend-design/components-v5 §02` — `.shape-document`.
   *
   * 정본 spec:
   * - ADR-0018 D4 amend ② (2026-05-17) — 두 mode: (a) asset-based / (b)
   *   inline-stored. BE schema.rs 가 Item::Document 에 `asset_id: Option`
   *   + `content: Option` ship (DOCUMENT_INLINE_MAX_BYTES 64 KB).
   * - plan-0011 FE Slice-A2 — 본 컴포넌트가 inline-stored mode 의 v3 시안
   *   정합 placeholder. 추후 InlineEdit wire 는 별 후속.
   *
   * 시안 구조 (grid 30px / 1fr / 26px):
   *   1. doc-head — file svg + filename + sep + size + right (Edited Nm ago)
   *   2. doc-body — eyebrow + h2 + p (markdown content 의 첫 줄 = h2)
   *   3. doc-foot — page-dot indicator + count + right (md · UTF-8 · LF)
   *
   * 현 단계: read-only display. content 의 첫 줄 (# 로 시작하면 strip) 을
   * h2 로, 나머지를 p 로 분할 렌더. 인라인 편집 진입 (더블 클릭) 은 후속.
   */

  import { NodeResizer, useSvelteFlow } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { documentViewModeStore } from '$lib/stores/documentViewMode.svelte';
  import InlineEditField from '$lib/common/InlineEditField.svelte';
  import InlineEditTextarea from '$lib/common/InlineEditTextarea.svelte';
  import { toastStore } from '$lib/ui/toast-store.svelte';
  import { copyTextToSystemClipboard } from '$lib/clipboard/textClipboard';
  import { filePicker } from '$lib/stores/filePicker.svelte';
  import { fsFileUrl } from '$lib/http/fs';
  import { UnauthorizedError } from '$lib/http/sessions';
  import CodeViewer from './CodeViewer.svelte';
  import {
    DOCUMENT_EXTENSIONS,
    basename,
    fileTypeLabelForPath,
    fileStem as workspaceFileStem,
    guessMimeFromPath,
    previewMetaForPath,
    resolveWorkspacePath,
    shikiLangForPath,
    workspaceRelativePath,
  } from '$lib/files/workspaceAssets';
  import {
    renderMarkdown,
    renderHtml,
    isToggleableFileType,
    getNextViewMode,
    getNextViewModeLabel,
    RENDERED_HTML_IFRAME_SANDBOX,
    buildRenderedHtmlSrcdoc,
  } from './documentRender';
  import type { CanvasItem, DocumentItem } from '$lib/types/canvas';
  import {
    constrainResizeAspectIfShift,
    scheduleLiveAspectResize,
  } from './resizeConstraint';

  /** Inline content cap = ADR-0018 D4 amend ② / BE DOCUMENT_INLINE_MAX_BYTES. */
  const DOCUMENT_INLINE_MAX_BYTES = 64 * 1024;

  interface DocumentNodeData {
    id: string;
    x: number;
    y: number;
    w: number;
    h: number;
    visibility: boolean;
    locked: boolean;
    minimized?: boolean;
    label?: string;
    path?: string;
    asset_id?: string;
    file_name?: string;
    content?: string;
    mime?: string;
    size_bytes?: number;
    /** Canvas.svelte group selection proxy. Descendants must not show own controls. */
    group_selected?: boolean;
  }

  let {
    data,
    dragging = false,
  }: {
    data: DocumentNodeData;
    id?: string;
    type?: string;
    width?: number;
    height?: number;
    dragHandle?: string;
    sourcePosition?: unknown;
    targetPosition?: unknown;
    /** Svelte Flow 가 drag 중에만 true 로 set. iframe pointer-events 차단의
     *  reactive 신호 — drag 중 mouse 가 PDF / HTML iframe 위로 들어가
     *  iframe 의 plugin / sandbox 가 event capture → 부모 drag mousemove/up
     *  미도달 회귀 회피 (2026-05-22 사용자 보고, PDF 주로). */
    dragging?: boolean;
    zIndex?: number;
    selectable?: boolean;
    deletable?: boolean;
    draggable?: boolean;
    parentId?: string;
  } = $props();

  const { updateNode } = useSvelteFlow();
  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(sessionStore.M.has(data.id) && data.group_selected !== true);
  const isMultiM = $derived(isInM && sessionStore.M.size > 1);
  const isSingleM = $derived(isInM && sessionStore.M.size <= 1);
  const isMaximized = $derived(sessionStore.maximizedItemId === data.id);
  /** ADR-0047 — path-based workspace file, legacy asset, or inline-stored. */
  const hasWorkspacePath = $derived((data.path ?? '').length > 0);
  const hasLegacyAsset = $derived((data.asset_id ?? '').length > 0);
  const isInline = $derived(!hasWorkspacePath && !hasLegacyAsset);
  const workspaceRoot = $derived(sessionStore.effectiveWorkspaceRoot);
  const resolvedWorkspacePath = $derived(
    data.path !== undefined ? resolveWorkspacePath(workspaceRoot, data.path) : null,
  );
  const remoteDocumentSrc = $derived(
    resolvedWorkspacePath !== null
      ? fsFileUrl(resolvedWorkspacePath)
      : hasLegacyAsset
        ? `/api/assets/${data.asset_id}`
        : '',
  );
  const documentCopyPath = $derived(hasWorkspacePath ? resolvedWorkspacePath : null);
  let assetPreviewText = $state<string | null>(null);
  let assetPreviewLoading = $state(false);
  let assetPreviewError = $state<string | null>(null);

  const displayFileName = $derived(
    data.file_name ?? (data.path !== undefined ? basename(data.path) : 'document'),
  );

  /** size_bytes 의 사람용 표기 (KB). */
  const sizeLabel = $derived.by((): string => {
    const bytes = data.size_bytes ?? 0;
    if (bytes < 1024) return `${bytes} B`;
    return `${(bytes / 1024).toFixed(1)} KB`;
  });
  const fileTypeLabel = $derived.by((): string => {
    if (isInline) return 'markdown';
    return fileTypeLabelForPath(displayFileName, data.mime);
  });
  const sourceLang = $derived(sourceLangForDocument(displayFileName, fileTypeLabel, isInline));

  /** ADR-0018 D10 amend ③/④/⑤/⑥ + ADR-0037 — markdown/html viewer + 2-mode
   *  toggle. helper + viewMode store 가 외부화 — DocumentNode (normal) 와
   *  MaximizedItemModal (maximize) 가 *같은 store* 의 같은 itemId 를 구독해
   *  rendering + 전이 + persist 동기화. normal↔maximize 전환 + unmount/remount
   *  도 reset 없음. */
  const viewMode = $derived(documentViewModeStore.get(data.id));
  const canToggleView = $derived(isToggleableFileType(fileTypeLabel));
  const nextViewModeLabel = $derived(getNextViewModeLabel(viewMode, fileTypeLabel));
  const renderedHtmlSrcdoc = $derived(buildRenderedHtmlSrcdoc(data.content ?? ''));
  const renderedHtmlSrcdocAsset = $derived(buildRenderedHtmlSrcdoc(assetPreviewText ?? ''));
  // Inline content (data.content) 의 mime 추정은 markdown 으로 기본 — fileTypeLabel
  // 이 'markdown' 이면 marked parse, 'html' 이면 sanitize only.
  function renderContent(raw: string): string {
    if (fileTypeLabel === 'html') return renderHtml(raw);
    return renderMarkdown(raw);
  }
  const inlineHtml = $derived(renderContent(data.content ?? ''));
  const assetHtml = $derived(renderContent(assetPreviewText ?? ''));
  const isEmpty = $derived(isInline && (data.content ?? '').trim().length === 0);
  const fileStem = $derived.by((): string => {
    const base = displayFileName.trim().split('/').pop() ?? displayFileName.trim();
    const dot = base.lastIndexOf('.');
    if (dot <= 0) return base;
    return base.slice(0, dot);
  });
  const documentTitle = $derived((data.label ?? '').trim() || fileStem || 'Untitled');
  const canPreviewAssetText = $derived.by(() => {
    if (isInline) return false;
    const mime = (data.mime ?? '').toLowerCase();
    return (
      mime.startsWith('text/') ||
      mime === 'application/json' ||
      previewMetaForPath(displayFileName).kind === 'text' ||
      ['markdown', 'html'].includes(fileTypeLabel)
    );
  });
  /** ADR-0018 D10 amend ⑦ — PDF asset 은 browser-native PDF viewer 로 iframe
   *  렌더. ADR-0037 의 sandbox-격리 HTML rendered 와 다른 mental model:
   *  PDF plugin 은 same-origin context 가 필요 (sandbox=allow-scripts 만 주면
   *  browser internal PDF viewer 미작동). single-user trust + same-origin
   *  endpoint 라 안전. text fetch 안 함 (binary). */
  const isPdfAsset = $derived(
    !isInline
    && fileTypeLabel === 'pdf'
    && remoteDocumentSrc.length > 0,
  );
  const pdfAssetSrc = $derived(isPdfAsset ? remoteDocumentSrc : '');

  // svelte-flow 가 selection 변경 시 data prop 의 reactive proxy 를 새 ref 로
  // 갱신 → effect 의 dependency 가 invalidate → fetch 재시작 → "Loading
  // preview…" blink. 회피: id 를 stable derived 로 wrap — *값* 이 같으면
  // svelte 의 derived 가 subscriber notify skip → effect re-fire 안 함.
  const assetFetchId = $derived.by((): string => {
    if (isInline || !canPreviewAssetText) return '';
    return remoteDocumentSrc;
  });
  const assetFetchAccept = $derived(data.mime ?? 'text/plain');

  $effect(() => {
    const src = assetFetchId;
    if (src.length === 0) {
      assetPreviewText = null;
      assetPreviewLoading = false;
      assetPreviewError = null;
      return;
    }

    let cancelled = false;
    assetPreviewText = null;
    assetPreviewError = null;
    assetPreviewLoading = true;

  async function loadPreview(): Promise<void> {
      try {
        const res = await fetch(src, {
          method: 'GET',
          credentials: 'include',
          headers: { Accept: assetFetchAccept },
        });
        if (res.status === 401) throw new UnauthorizedError();
        if (!res.ok) throw new Error(`GET document source returned ${res.status}`);
        const text = await res.text();
        if (!cancelled) assetPreviewText = text;
      } catch (err) {
        if (err instanceof UnauthorizedError) {
          window.location.href = '/auth';
          return;
        }
        if (!cancelled) assetPreviewError = err instanceof Error ? err.message : String(err);
      } finally {
        if (!cancelled) assetPreviewLoading = false;
      }
    }

    void loadPreview();
    return () => {
      cancelled = true;
    };
  });

  // ─ Inline edit (ADR-0018 D4 amend ② / plan-0011 FE Slice-A2) ─
  //   - filename: double-click on .doc-head .filename → InlineEditField.
  //   - content: double-click on .doc-body → InlineEditTextarea. cap 64 KB
  //     (BE DOCUMENT_INLINE_MAX_BYTES 정합).
  //   - commit 시 applyMutation 통과 → history capture (Cmd+Z 자연 정합).
  //   - inline-stored 모드에서만 편집 진입 — asset-based 는 read-only.
  let nameEditing = $state(false);
  let labelEditing = $state(false);
  let contentEditing = $state(false);

  function onLabelDblClick(e: MouseEvent): void {
    if (isLocked) return;
    e.stopPropagation();
    labelEditing = true;
  }

  async function onCopyPathClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    const path = documentCopyPath;
    if (path === null) return;
    const result = await copyTextToSystemClipboard(path);
    toastStore.show({
      message: result.ok ? 'Copied file path.' : (result.reason ?? 'Copy failed.'),
      tone: result.ok ? 'success' : 'error',
    });
  }

  function onNameDblClick(e: MouseEvent): void {
    if (isLocked || !isInline) return;
    e.stopPropagation();
    nameEditing = true;
  }

  function onBodyDblClick(e: MouseEvent): void {
    if (isLocked || !isInline) return;
    e.stopPropagation();
    contentEditing = true;
  }

  async function commitName(next: string): Promise<void> {
    const trimmed = next.trim();
    if (trimmed === displayFileName || trimmed.length === 0) {
      nameEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const oldStem = fileStem;
    const nextBase = trimmed.split('/').pop() ?? trimmed;
    const nextDot = nextBase.lastIndexOf('.');
    const nextStem = nextDot > 0 ? nextBase.slice(0, nextDot) : nextBase;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({
                ...it,
                file_name: trimmed,
                label:
                  (it.label ?? '').trim().length === 0 || (it.label ?? '') === oldStem
                    ? nextStem
                    : it.label,
              } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document filename edit aborted — session reconnect failed.',
        failMessage: 'Document rename failed',
      },
    );
    if (result.ok) nameEditing = false;
  }

  async function commitLabel(next: string): Promise<void> {
    const trimmed = next.trim();
    if (trimmed === (data.label ?? '')) {
      labelEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, label: trimmed } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document title edit aborted — session reconnect failed.',
        failMessage: 'Document title edit failed',
      },
    );
    if (result.ok) labelEditing = false;
  }

  async function commitContent(next: string): Promise<void> {
    if (next === (data.content ?? '')) {
      contentEditing = false;
      return;
    }
    if (sessionStore.active === null) return;
    const byteLength = new TextEncoder().encode(next).length;
    if (byteLength > DOCUMENT_INLINE_MAX_BYTES) {
      toastStore.show({
        message: `Document content too long (${byteLength} / ${DOCUMENT_INLINE_MAX_BYTES} bytes).`,
        tone: 'error',
      });
      return;
    }
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, content: next, size_bytes: byteLength } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Document edit aborted — session reconnect failed.',
        failMessage: 'Document commit failed',
      },
    );
    if (result.ok) contentEditing = false;
  }

  type ResizeParams = { x: number; y: number; width: number; height: number };
  const DOC_MIN_H = 35;
  const DOC_RESTORE_W = 360;
  const DOC_RESTORE_H = 220;
  const DOC_RESIZE_MIN_W = 220;
  const DOC_RESIZE_MIN_H = 160;

  function applyLiveResize(next: ResizeParams): void {
    updateNode(data.id, (node) => ({
      position: { ...node.position, x: next.x, y: next.y },
      width: Math.max(DOC_RESIZE_MIN_W, next.width),
      height: Math.max(DOC_RESIZE_MIN_H, next.height),
    }));
  }

  function onResize(event: unknown, params: ResizeParams): void {
    scheduleLiveAspectResize(
      event,
      params,
      data,
      data.w / data.h,
      DOC_RESIZE_MIN_W,
      DOC_RESIZE_MIN_H,
      applyLiveResize,
    );
  }

  async function onResizeEnd(event: unknown, params: ResizeParams): Promise<void> {
    const constrained = constrainResizeAspectIfShift(
      event,
      params,
      data,
      data.w / data.h,
      DOC_RESIZE_MIN_W,
      DOC_RESIZE_MIN_H,
    );
    await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'document'
            ? ({
                ...it,
                x: constrained.x,
                y: constrained.y,
                w: Math.max(DOC_RESIZE_MIN_W, constrained.width),
                h: Math.max(DOC_RESIZE_MIN_H, constrained.height),
              } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Resize aborted — session reconnect failed.',
        failMessage: 'Resize failed',
      },
    );
  }

  async function onMinimizeClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    if (sessionStore.active === null) return;
    const cur = sessionStore.items.get(data.id);
    if (cur === undefined) return;
    const next = cur.minimized !== true;
    let nextW = cur.w;
    let nextH = cur.h;
    if (next) {
      sessionStore.backupItemGeom(data.id, { x: cur.x, y: cur.y, w: cur.w, h: cur.h });
      nextH = DOC_MIN_H;
    } else {
      const backup = sessionStore.getRestoredGeom(data.id);
      nextW = backup?.w ?? DOC_RESTORE_W;
      nextH = backup?.h ?? DOC_RESTORE_H;
      sessionStore.clearRestoredGeom(data.id);
    }
    await sessionStore.applyMutation(
      (cur2) => ({
        ...cur2,
        items: cur2.items.map((it) =>
          it.id === data.id && it.type === 'document'
            ? ({ ...it, minimized: next, w: nextW, h: nextH } as DocumentItem)
            : it,
        ),
      }),
      {
        abortMessage: 'Minimize aborted — session reconnect failed.',
        failMessage: 'Minimize failed',
      },
    );
  }

  function onMaximizeClick(e: MouseEvent): void {
    e.stopPropagation();
    e.preventDefault();
    sessionStore.toggleMaximize(data.id);
  }

  async function onCloseClick(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    if (isLocked) return;
    await sessionStore.applyDeletion([data.id], { killTerminal: false });
  }

  function initialDocumentDir(): string {
    if (resolvedWorkspacePath === null) return workspaceRoot;
    const slash = resolvedWorkspacePath.lastIndexOf('/');
    return slash <= 0 ? workspaceRoot : resolvedWorkspacePath.slice(0, slash);
  }

  function onLoadFileClick(e: MouseEvent): void {
    e.stopPropagation();
    if (isLocked) return;
    if (workspaceRoot.length === 0) {
      toastStore.show({
        message: 'Workspace root is not available yet.',
        tone: 'error',
      });
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
            it.id === data.id && it.type === 'document'
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

  function onRootClick(e: MouseEvent): void {
    if (isEmpty) void onLoadFileClick(e);
  }

  function onBodyWheel(e: WheelEvent): void {
    e.stopPropagation();
  }

  function sourceLangForDocument(fileName: string, label: string, inline: boolean): string {
    if (inline) return label === 'html' ? 'html' : 'markdown';
    const mapped = shikiLangForPath(fileName);
    if (mapped !== 'text') return mapped;
    if (label === 'html') return 'html';
    if (label === 'markdown') return 'markdown';
    if (label === 'json') return 'json';
    return mapped;
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

  /**
   * ADR-0018 D10 amend ⑧ 보강 (2026-05-22) — drag 중 iframe pointer-events
   * 차단의 *reactive bypass*. 기존 `class:drag-isolated={dragging}` 는 Svelte
   * 5 reactive 흐름 (dragging=true → effect → DOM attr → repaint) 의 frame
   * 갭에서 빠른 mousemove 가 iframe 위 도달하면 capture 회귀.
   *
   * **Trigger 두 path** (둘 다 같은 `isolateIframes` 호출):
   * 1. **자체 panel 내부에서 pointerdown** (`onRootPointerDownCapture`):
   *    self drag (header drag / resize handle 등) 시작 시 즉시 차단.
   * 2. **다른 source 의 pointerdown** (window-level capture-phase listener):
   *    다른 component (NoteNode / PanelNode / ShapeNode 등) 의 drag 시작
   *    시에도 자체 PDF iframe 차단. 다른 panel drag 중 mouse 가 본 PDF
   *    위 통과 시 PDF plugin 이 mouse capture → 그 다른 panel 의 drag 가
   *    멈춰버림 의 회귀 (#drag-cross-component-capture) 차단.
   *
   * **Early return** (사용자 interact 의도 보호):
   * - `e.target` 이 iframe 자체 → 사용자가 그 iframe 안 클릭 / scroll / etc.
   * - self panel 안 pointerdown (window listener 한정) → `onRootPointerDownCapture`
   *   가 이미 처리, 중복 회피.
   */
  function isolateLocalIframes(root: HTMLElement): void {
    const iframes = root.querySelectorAll<HTMLIFrameElement>(
      'iframe.doc-html-frame, iframe.doc-pdf',
    );
    if (iframes.length === 0) return;
    iframes.forEach((f) => {
      f.style.pointerEvents = 'none';
    });
    function restore(): void {
      iframes.forEach((f) => {
        f.style.pointerEvents = '';
      });
      window.removeEventListener('pointerup', restore, true);
      window.removeEventListener('pointercancel', restore, true);
    }
    window.addEventListener('pointerup', restore, { capture: true, once: true });
    window.addEventListener('pointercancel', restore, { capture: true, once: true });
  }

  let rootEl = $state<HTMLDivElement | null>(null);

  function onRootPointerDownCapture(e: PointerEvent): void {
    if (e.target instanceof HTMLIFrameElement) return;
    isolateLocalIframes(e.currentTarget as HTMLElement);
  }

  /** 다른 component 의 drag 가 시작될 때도 자체 iframe 차단 — window
   *  capture-phase listener. self panel 안 pointerdown 은 root.contains 으로
   *  early return (자체 onpointerdowncapture 가 처리, 중복 회피). */
  $effect(() => {
    if (rootEl === null) return;
    const root = rootEl;
    function onWindowPointerDown(e: PointerEvent): void {
      if (e.target instanceof Node && root.contains(e.target)) return;
      if (e.target instanceof HTMLIFrameElement) return;
      isolateLocalIframes(root);
    }
    window.addEventListener('pointerdown', onWindowPointerDown, { capture: true });
    return () => {
      window.removeEventListener('pointerdown', onWindowPointerDown, true);
    };
  });
</script>

{#if isVisible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={rootEl}
    class="document-node shape-document"
    class:m-single={isSingleM}
    class:m-multi={isMultiM}
    class:locked={isLocked}
    class:is-empty={isEmpty}
    class:is-min={data.minimized === true}
    style="width: 100%; height: 100%;"
    role="article"
    aria-label={`Document ${documentTitle}`}
    onclick={data.minimized !== true && isEmpty ? onRootClick : undefined}
    onpointerdowncapture={onRootPointerDownCapture}
  >
    <NodeResizer
      nodeId={data.id}
      isVisible={isInM && !isLocked && data.minimized !== true}
      minWidth={220}
      minHeight={160}
      color="var(--color-accent)"
      handleClass="panel-resize-handle"
      lineClass="panel-resize-line"
      {onResize}
      {onResizeEnd}
    />
    <!-- 1. Doc-head: file svg + title + actions. Filename lives in footer. -->
    <header class="doc-head">
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
        <path d="M3 1.5h4.5L9.5 3.5V10.5H3V1.5z" />
        <path d="M7.5 1.5V3.5h2" />
      </svg>
      {#if labelEditing}
        <InlineEditField
          value={documentTitle}
          editing={true}
          plain={true}
          placeholder={fileStem || 'Untitled'}
          class="doc-title-edit"
          onCommit={(next: string) => void commitLabel(next)}
          onCancel={() => (labelEditing = false)}
        />
      {:else}
        <span
          class="doc-title"
          title={`${documentTitle} — double-click to rename`}
          ondblclick={onLabelDblClick}
          role="presentation"
        >{documentTitle}</span>
      {/if}
      {#if !isLocked || documentCopyPath !== null}
        <div class="doc-actions">
          {#if !isLocked && canToggleView}
            <!-- ADR-0037 amend — markdown/html use one 2-mode rendered↔source
                 transition. The icon hints the next mode. -->
            <button
              type="button"
              class="doc-btn nodrag"
              class:is-active={viewMode !== 'rendered'}
              title={nextViewModeLabel}
              aria-label={nextViewModeLabel}
              onclick={(e: MouseEvent) => {
                e.stopPropagation();
                documentViewModeStore.set(data.id, getNextViewMode(viewMode, fileTypeLabel));
              }}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              {#if viewMode === 'source'}
                <!-- book-open (show rendered) — visibility eye 와 겹침 회피. -->
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z"/>
                  <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z"/>
                </svg>
              {:else}
                <!-- </> code (show source) -->
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
                  <polyline points="16 18 22 12 16 6"/>
                  <polyline points="8 6 2 12 8 18"/>
                </svg>
              {/if}
            </button>
          {/if}
          {#if documentCopyPath !== null}
            <button
              type="button"
              class="doc-btn nodrag"
              title="Copy path"
              aria-label="Copy path"
              onclick={(e) => void onCopyPathClick(e)}
              onmousedown={(e: MouseEvent) => e.stopPropagation()}
            >
              <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="5" y="5" width="8" height="9" rx="1.2"/>
                <path d="M3 11V3a1 1 0 0 1 1-1h6"/>
              </svg>
            </button>
          {/if}
          {#if !isLocked}
          <button
            type="button"
            class="doc-btn nodrag"
            title="Change document"
            aria-label="Change document"
            onclick={(e) => void onLoadFileClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
              <path d="M9 17H7A5 5 0 0 1 7 7h2"/>
              <path d="M15 7h2a5 5 0 1 1 0 10h-2"/>
              <line x1="8" x2="16" y1="12" y2="12"/>
            </svg>
          </button>
          <button
            type="button"
            class="doc-btn nodrag"
            class:is-active={data.minimized === true}
            title={data.minimized === true ? 'Restore' : 'Minimize'}
            aria-label={data.minimized === true ? 'Restore' : 'Minimize'}
            onclick={(e) => void onMinimizeClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            {#if data.minimized === true}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 5h6"/><path d="M3 8h6"/>
              </svg>
            {:else}
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
                <path d="M3 8h6"/>
              </svg>
            {/if}
          </button>
          <button
            type="button"
            class="doc-btn nodrag"
            class:is-active={isMaximized}
            title={isMaximized ? 'Restore' : 'Maximize'}
            aria-label={isMaximized ? 'Restore' : 'Maximize'}
            onclick={onMaximizeClick}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" aria-hidden="true">
              <rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/>
            </svg>
          </button>
          <button
            type="button"
            class="doc-btn close nodrag"
            title="Close"
            aria-label="Close"
            onclick={(e) => void onCloseClick(e)}
            onmousedown={(e: MouseEvent) => e.stopPropagation()}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6"/>
            </svg>
          </button>
          {/if}
        </div>
      {/if}
    </header>

    <!-- 2. Doc-body: eyebrow + h2 + p (placeholder 시 empty hint) -->
    <div
      class="doc-body nowheel"
      ondblclick={onBodyDblClick}
      onwheel={onBodyWheel}
      role="presentation"
    >
      {#if contentEditing}
        <InlineEditTextarea
          value={data.content ?? ''}
          editing={true}
          plain={true}
          placeholder={'# Heading\n\nWrite your document — markdown ok.'}
          rows={8}
          class="doc-content-edit"
          onCommit={(next: string) => void commitContent(next)}
          onCancel={() => (contentEditing = false)}
        />
      {:else if isEmpty}
        <div class="empty-hint">
          <span class="empty-idle" aria-hidden="true">
            <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" stroke-linecap="round">
              <path d="M7 3.5h7l3 3v14H7z"/>
              <path d="M14 3.5v3h3"/>
              <path d="M10 11h4M10 14h5M10 17h3"/>
            </svg>
          </span>
        </div>
      {:else if isInline}
        <div class="eyebrow">Inline document</div>
        <!-- ADR-0018 D10 amend ③/④/⑤ + ADR-0037 — markdown/html rendered
             or source. HTML rendered is sandboxed iframe. -->
        {#if viewMode === 'source'}
          <div class="doc-source-view">
            <CodeViewer text={data.content ?? ''} lang={sourceLang} filename={displayFileName} />
          </div>
        {:else if fileTypeLabel === 'html'}
          <!-- svelte-ignore a11y_missing_attribute -->
          <iframe
            class="doc-html-frame"
            class:drag-isolated={dragging}
            sandbox={RENDERED_HTML_IFRAME_SANDBOX}
            title={displayFileName}
            referrerpolicy="no-referrer"
            loading="lazy"
            srcdoc={renderedHtmlSrcdoc}
          ></iframe>
        {:else}
          <div class="doc-md" use:interceptRenderedLinks>{@html inlineHtml}</div>
        {/if}
      {:else if isPdfAsset}
        <!-- ADR-0018 D10 amend ⑦ — PDF iframe via browser-native plugin.
             sandbox 미지정 (PDF plugin 의 same-origin 요구). same-origin
             endpoint 라 trust safe. eyebrow 숨김 — :has(.doc-pdf) CSS.
             drag 중 pointer-events:none — iframe 의 PDF plugin 이 mouse
             event 를 capture 해 부모 drag 가 멈추는 회귀 차단. -->
        <!-- svelte-ignore a11y_missing_attribute -->
        <iframe
          class="doc-pdf"
          class:drag-isolated={dragging}
          src={pdfAssetSrc}
          title={displayFileName}
          referrerpolicy="no-referrer"
          loading="lazy"
        ></iframe>
      {:else}
        <div class="eyebrow">Document file</div>
        {#if assetPreviewLoading}
          <p>Loading preview…</p>
        {:else if assetPreviewText !== null && (assetPreviewText.trim().length > 0)}
          {#if viewMode === 'source'}
            <div class="doc-source-view">
              <CodeViewer text={assetPreviewText} lang={sourceLang} filename={displayFileName} />
            </div>
          {:else if fileTypeLabel === 'html'}
            <!-- svelte-ignore a11y_missing_attribute -->
            <iframe
              class="doc-html-frame"
              class:drag-isolated={dragging}
              sandbox={RENDERED_HTML_IFRAME_SANDBOX}
              title={displayFileName}
              referrerpolicy="no-referrer"
              loading="lazy"
              srcdoc={renderedHtmlSrcdocAsset}
            ></iframe>
          {:else}
            <div class="doc-md" use:interceptRenderedLinks>{@html assetHtml}</div>
          {/if}
        {:else}
          <div class="asset-summary">
            <svg class="asset-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">
              <path d="M7 3.5h7l3 3v14H7z"/>
              <path d="M14 3.5v3h3"/>
              <path d="M10 11h4M10 14h5M10 17h3"/>
            </svg>
            <h2>{displayFileName}</h2>
            <p>{assetPreviewError ?? 'Preview is not available for this document type.'}</p>
          </div>
        {/if}
      {/if}
    </div>

    <!-- 3. Doc-foot: page-dot + count + right meta -->
    <footer class="doc-foot">
      <span class="page-dot on" aria-hidden="true"></span>
      <span>Page 1</span>
      {#if nameEditing}
        <InlineEditField
          value={displayFileName}
          editing={true}
          plain={true}
          placeholder="filename.md"
          class="doc-name-edit"
          onCommit={(next: string) => void commitName(next)}
          onCancel={() => (nameEditing = false)}
        />
      {:else}
        <span
          class="filename"
          title={isInline ? `${displayFileName} — double-click to rename` : displayFileName}
          ondblclick={onNameDblClick}
          role="presentation"
        >{displayFileName}</span>
      {/if}
      {#if (data.size_bytes ?? 0) > 0}
        <span class="doc-size" title={sizeLabel}>{sizeLabel}</span>
      {/if}
      <span class="right">{fileTypeLabel}</span>
    </footer>
  </div>
{/if}

<style>
  /* ref/frontend-design/components-v5 §02 — .shape-document.
   * grid 30 / 1fr / 26 (head/body/foot). */
  .document-node {
    display: grid;
    grid-template-rows: 30px 1fr 26px;
    box-sizing: border-box;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.06);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    overflow: hidden;
  }

  .document-node.m-single,
  .document-node.m-multi {
    outline: none;
  }

  .document-node.locked {
    cursor: default;
  }

  /* head + foot — surface-2 mono 10px / 0.4px */
  .doc-head,
  .doc-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    background: var(--color-surface-2);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    color: var(--color-fg-muted);
    min-width: 0;
  }

  .doc-head {
    padding: 0 4px 0 14px;
    border-bottom: 1px solid var(--color-border);
    gap: 6px;
  }

  .doc-foot {
    border-top: 1px solid var(--color-border);
  }

  .doc-head svg {
    flex-shrink: 0;
    opacity: 0.75;
  }

  .doc-head .doc-title,
  .doc-foot .filename {
    flex: 1 1 auto;
    color: var(--color-fg);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-foot .filename {
    max-width: 42%;
    text-transform: uppercase;
  }

  .doc-head > span:not(.doc-title),
  .doc-foot > span:not(.page-dot) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .doc-foot .right {
    margin-left: 0;
    max-width: 34%;
    flex-shrink: 1;
  }

  .doc-size {
    flex: 0 1 auto;
    margin-left: auto;
    margin-right: auto;
    max-width: 72px;
    text-align: center;
  }

  .doc-actions {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    flex: 0 0 auto;
    margin-left: 2px;
  }

  .doc-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border: none;
    background: transparent;
    border-radius: 4px;
    color: var(--color-fg-muted);
    cursor: pointer;
    padding: 0;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .doc-btn:hover:not(:disabled) {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .doc-btn.is-active {
    background: var(--color-glass-2);
    color: var(--color-fg);
  }

  .doc-btn.close:hover:not(:disabled) {
    background: #e5484d;
    color: #ffffff;
  }

  .doc-btn:focus-visible {
    outline: 1px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .document-node.is-min {
    grid-template-rows: 30px;
    border: 0;
    box-shadow: none;
    background: transparent;
    cursor: default;
  }

  .document-node.is-min .doc-head {
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: 0 1px 10px rgba(0, 0, 0, 0.10);
  }

  .document-node.is-min.m-single .doc-head,
  .document-node.is-min.m-multi .doc-head {
    border-color: var(--color-accent);
    border-width: calc(1.5px / var(--canvas-zoom, 1));
  }

  .document-node.is-min .doc-body,
  .document-node.is-min .doc-foot {
    display: none;
  }

  /* body — generous padding, eyebrow + h2 + p */
  .doc-body {
    padding: 28px 36px 24px;
    overflow: auto;
    min-height: 0;
    scrollbar-width: thin;
  }

  .doc-body .eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    color: var(--color-fg-muted);
    margin-bottom: 18px;
  }

  .doc-body h2 {
    margin: 0 0 12px;
    font-family: var(--font-sans);
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.6px;
    line-height: 1.15;
    color: var(--color-fg);
    overflow-wrap: anywhere;
  }

  .doc-body p {
    margin: 0 0 8px;
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    letter-spacing: -0.1px;
    color: var(--color-fg-muted);
    overflow-wrap: anywhere;
  }

  /* ADR-0037 amend — rendered HTML lives in a sandboxed iframe. The iframe
     fills the document body and keeps standalone document styles away from
     the app DOM. */
  .doc-html-frame {
    display: block;
    flex: 1 1 auto;
    width: 100%;
    height: 100%;
    min-height: 100%;
    border: 0;
    background: #ffffff;
  }
  .doc-body:has(.doc-html-frame) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .doc-body:has(.doc-html-frame) .eyebrow {
    display: none;
  }

  /* ADR-0018 D10 amend ⑦ (2026-05-22) — PDF iframe (browser-native viewer).
     rendered HTML iframe (ADR-0037) 와 달리 height auto-fit 안 함:
     PDF plugin 이 자체 internal scroll + multi-page navigation 제공 →
     host 영역 100% 채우고 PDF viewer 가 scroll 책임. host overflow 는
     hidden (외부 scroll 미발생). */
  .doc-pdf {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    background: #ffffff;
  }
  .doc-body:has(.doc-pdf) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* ADR-0018 D10 amend ⑧ (2026-05-22, 사용자 보고 #drag-iframe-capture) —
     drag 중 iframe pointer-events 차단. iframe (PDF plugin / sandbox 안의
     rendered HTML) 은 자체 browsing context 라 부모의 mouse event 와
     분리. drag 중 mouse 가 iframe 위로 들어가면 iframe 이 event capture →
     부모 (xyflow) 의 drag mousemove/mouseup 미도달 → drag 가 멈춰버림.
     `dragging` prop 이 true 인 동안 iframe 의 pointer-events 만 차단 →
     mouse event 가 iframe 통과해 부모 wrapper 가 catch. drag 종료 후 즉시
     복원. PDF / rendered HTML 양쪽 동일 fix. */
  .doc-pdf.drag-isolated,
  .doc-html-frame.drag-isolated {
    pointer-events: none;
  }

  /* ADR-0037 D7 — source view uses shared CodeViewer. */
  .doc-source-view {
    flex: 1 1 auto;
    min-height: 0;
    height: 100%;
    --code-viewer-font-size: 11.5px;
    --code-viewer-line-height: 1.55;
    --code-viewer-gutter-width: 34px;
    --code-viewer-padding: 10px 0;
  }
  .doc-body:has(.doc-source-view) {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .doc-body:has(.doc-source-view) .eyebrow {
    display: none;
  }

  /* plan 0011 follow-up (2026-05-21) — marked output container styling.
     ADR-0018 D10 의 inline/asset markdown 모두 본 container 안. */
  .doc-md {
    color: var(--color-fg-muted);
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    letter-spacing: -0.1px;
    overflow-wrap: anywhere;
  }
  .doc-md :global(h1),
  .doc-md :global(h2) {
    margin: 0 0 12px;
    font-family: var(--font-sans);
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.6px;
    line-height: 1.15;
    color: var(--color-fg);
  }
  .doc-md :global(h3) {
    margin: 16px 0 8px;
    font-size: 18px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(h4),
  .doc-md :global(h5),
  .doc-md :global(h6) {
    margin: 14px 0 6px;
    font-size: 14px;
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(p) {
    margin: 0 0 10px;
  }
  .doc-md :global(ul),
  .doc-md :global(ol) {
    margin: 0 0 10px;
    padding-left: 22px;
  }
  .doc-md :global(li) {
    margin: 2px 0;
  }
  .doc-md :global(blockquote) {
    margin: 0 0 10px;
    padding-left: 12px;
    border-left: 2px solid var(--color-border-strong);
    color: var(--color-fg-subtle);
  }
  .doc-md :global(code) {
    font-family: var(--font-mono);
    font-size: 11.5px;
    padding: 0 4px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
  }
  .doc-md :global(pre) {
    margin: 0 0 10px;
    padding: 10px 12px;
    background: var(--color-surface-2);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .doc-md :global(pre code) {
    padding: 0;
    background: transparent;
    border-radius: 0;
  }
  .doc-md :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
  }
  .doc-md :global(table) {
    border-collapse: collapse;
    margin: 0 0 10px;
    font-size: 12px;
  }
  .doc-md :global(th),
  .doc-md :global(td) {
    border: 1px solid var(--color-border);
    padding: 4px 8px;
    text-align: left;
  }
  .doc-md :global(th) {
    background: var(--color-surface-2);
    font-weight: 600;
    color: var(--color-fg);
  }
  .doc-md :global(hr) {
    border: 0;
    border-top: 1px solid var(--color-border);
    margin: 14px 0;
  }
  .doc-md :global(img) {
    max-width: 100%;
    height: auto;
  }

  .asset-summary {
    display: grid;
    justify-items: start;
    gap: 8px;
  }

  .asset-summary h2 {
    margin-bottom: 0;
  }

  .asset-icon {
    width: 24px;
    height: 24px;
    color: var(--color-fg-muted);
  }

  .document-node.is-empty {
    cursor: pointer;
  }

  .document-node.is-empty .doc-head .doc-title {
    color: var(--color-fg-muted);
  }

  .document-node.is-empty .doc-body {
    display: grid;
    place-items: center;
    padding: 14px;
  }

  .empty-hint {
    display: grid;
    grid-template-areas: "stack";
    place-items: center;
    width: 100%;
    height: 100%;
  }

  .empty-idle {
    grid-area: stack;
    display: grid;
    place-items: center;
    gap: 7px;
    color: var(--color-fg-muted);
    opacity: 0.7;
    transition: opacity var(--motion-fast) var(--motion-easing);
  }

  .empty-icon {
    width: 24px;
    height: 24px;
  }

  /* Inline edit chrome — doc-content-edit textarea + doc-name-edit input.
   * Doc-body 의 padding 을 그대로 따라가도록 width 100% + font 정합. */
  :global(.doc-content-edit) {
    width: 100%;
    min-height: 120px;
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--color-fg);
  }

  :global(.doc-name-edit),
  :global(.doc-title-edit) {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: var(--weight-medium);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--color-fg);
  }

  :global(.doc-title-edit) {
    text-transform: none;
  }

  /* page-dot indicator — 5px circle */
  .doc-foot .page-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--color-border-strong);
    flex-shrink: 0;
  }

  .doc-foot .page-dot.on {
    background: var(--color-fg);
  }
</style>
