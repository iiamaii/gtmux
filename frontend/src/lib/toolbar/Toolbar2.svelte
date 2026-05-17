<script lang="ts">
  /**
   * Toolbar2 — 56px Figma-style 12-tool toolbar (FE-2 / Stage 5).
   *
   * 정본:
   * - plan-0007 §14.2 FE-2 (12 도구 + dividers)
   * - plan-0007 §14.20.3 G22 (one-shot default + Q lock)
   * - ref/frontend-design/SPEC.md §4 (visual spec)
   * - ADR-0018 D4 (도구 ↔ ItemType 1:1 매핑)
   *
   * Layout (centre 정렬 + left/right absolute):
   *   ├─ left absolute: "Page 1 ▾" (page selector placeholder)
   *   ├─ centre groups:
   *   │   [Select | Hand] | [Terminal] | [Rect | Ellipse | Line | FreeDraw]
   *   │   | [Text | Note] | [Image | Document | FilePath]
   *   └─ right absolute: Comment | More (low-priority)
   *
   * Behaviour:
   *   - Click → toolStore.set(id) → one-shot mode (Stage 5 의 creation gesture
   *     완료 시 toolStore.consume() 호출하면 자동 Select 복귀, G22).
   *   - Q key → toggleLock (sticky lock indicator: ring around active tool).
   *   - Esc → toolStore.handleEsc() (lock 해제 → Select 복귀 chain).
   *   - Tooltip: tool name + 단축키 (마우스 hover, 6px 아래에 표시).
   *
   * Icons: 18×18 inline SVG (stroke 1.5). lucide-svelte 의 strict-build 충돌
   * (handover §7) 회피 — 직접 SVG.
   */

  import { onMount } from 'svelte';
  import { toolStore, type ToolId } from '$lib/stores/toolStore.svelte';
  import ActiveSessionDropdown from '$lib/chrome/ActiveSessionDropdown.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import { historyStore } from '$lib/stores/historyStore.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';

  interface ToolDef {
    id: ToolId;
    name: string;
    hint?: string;
    /** inline SVG path (viewBox 24 24, stroke="currentColor" 가 적용됨). */
    path: string;
    /** filled icon (text 'T' 등) — true 면 fill 사용. */
    fill?: boolean;
  }

  /** 12 tools grouped by semantic. Divider 는 그룹 사이. */
  const GROUPS: ToolDef[][] = [
    // 1) Mode (always sticky)
    [
      {
        id: 'select',
        name: 'Select',
        hint: 'V',
        path: '<path d="M3 2 L3 17 L7 13 L9.5 18.5 L11.8 17.4 L9.4 12.2 L15 12 Z" fill="currentColor" stroke="none"/>',
        fill: true,
      },
      {
        id: 'hand',
        name: 'Hand',
        hint: 'H',
        path: '<path d="M8 13V5.5a1.5 1.5 0 0 1 3 0V12"/><path d="M11 12V4.5a1.5 1.5 0 0 1 3 0V12"/><path d="M14 11.5V5.5a1.5 1.5 0 0 1 3 0V14"/><path d="M17 8.5a1.5 1.5 0 0 1 3 0V16a5 5 0 0 1-5 5h-3a5 5 0 0 1-4.5-2.5L4 13a1.5 1.5 0 0 1 2.5-1.5L8 14"/>',
      },
    ],
    // 2) Terminal (the principal gtmux tool)
    [
      {
        id: 'terminal',
        name: 'Terminal',
        hint: 'T',
        path: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3"/><path d="M13 15h4"/>',
      },
    ],
    // 3) Shapes (vector primitives)
    [
      {
        id: 'rect',
        name: 'Rectangle',
        hint: 'R',
        path: '<rect x="4" y="5" width="16" height="14" rx="1.5"/>',
      },
      {
        id: 'ellipse',
        name: 'Ellipse',
        hint: 'O',
        path: '<ellipse cx="12" cy="12" rx="8.5" ry="7"/>',
      },
      {
        id: 'line',
        name: 'Line',
        hint: 'L',
        path: '<line x1="4.5" y1="19" x2="19.5" y2="5"/>',
      },
      {
        id: 'free_draw',
        name: 'Free draw',
        hint: 'P',
        path: '<path d="M4 17c2-4 4-2 6-5s2-7 5-7 5 4 5 6"/>',
      },
    ],
    // 4) Text
    [
      {
        id: 'text',
        name: 'Text',
        hint: 'T',
        path: '<path d="M5 5h14M12 5v14M9 19h6" stroke-width="2"/>',
      },
      {
        id: 'note',
        name: 'Note',
        hint: 'N',
        path: '<path d="M5 4h10l4 4v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"/><path d="M15 4v4h4"/><path d="M7 13h8M7 17h5"/>',
      },
    ],
    // 5) Assets / refs
    [
      {
        id: 'image',
        name: 'Image',
        hint: 'I',
        path: '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.5"/><path d="M3 17l5-4 4 3 5-5 4 4"/>',
      },
      {
        id: 'document',
        name: 'Document',
        hint: 'D',
        path: '<path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/><path d="M14 3v4h4"/><path d="M8 13h8M8 17h5"/>',
      },
      {
        id: 'file_path',
        name: 'File path',
        hint: 'F',
        path: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
      },
    ],
  ];

  const current = $derived(toolStore.current);
  const locked = $derived(toolStore.locked);

  function onkeydown(e: KeyboardEvent): void {
    // Q toggles lock (only if a non-mode tool is active).
    if (e.key === 'q' || e.key === 'Q') {
      // Ignore when modifier or focus is in editable element
      const t = e.target as HTMLElement | null;
      const isEditable =
        t &&
        (t.tagName === 'INPUT' ||
          t.tagName === 'TEXTAREA' ||
          t.isContentEditable);
      if (isEditable) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      toolStore.toggleLock();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onkeydown);
    return () => window.removeEventListener('keydown', onkeydown);
  });
</script>

<nav class="toolbar" aria-label="Canvas tools">
  <div class="left">
    <!-- ADR-0019 D9 + UX 결정: active session 버튼은 *SessionListModal 직접
         진입*. session *생성* 진입점은 SessionMenu 의 "Switch workspace session…"
         (workspaceSwitcher.open() → AuthDialog 의 [New session]) 만. -->
    <ActiveSessionDropdown onSwitch={() => workspaceSwitcher.goList('closed')} />
  </div>

  <div class="center">
    {#each GROUPS as group, gi (gi)}
      {#if gi > 0}
        <div class="divider" aria-hidden="true"></div>
      {/if}
      <div class="group">
        {#each group as tool (tool.id)}
          <button
            type="button"
            class="tool"
            class:active={current === tool.id}
            class:locked={current === tool.id && locked}
            title={tool.name + (tool.hint ? ` — ${tool.hint}` : '')}
            aria-label={tool.name}
            aria-pressed={current === tool.id}
            data-tool-id={tool.id}
            onclick={() => toolStore.set(tool.id)}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              {@html tool.path}
            </svg>
            <span class="tooltip">{tool.name}{tool.hint ? ` · ${tool.hint}` : ''}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>

  <div class="right">
    {#if locked}
      <span class="lock-indicator" title="Tool locked (Q to release)">Q</span>
    {/if}
    <!-- ADR-0028 D8 — Undo / Redo. canUndo / canRedo 가 historyStore 의
         derived. Active session 없거나 stack 빈 경우 disabled. -->
    <div class="group history-group" aria-label="History">
      <button
        type="button"
        class="tool"
        title="Undo (⌘Z)"
        aria-label="Undo"
        disabled={!historyStore.canUndo}
        onclick={() => void sessionStore.undo()}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 7v6h6"/>
          <path d="M21 17a9 9 0 0 0-15-6.7L3 13"/>
        </svg>
      </button>
      <button
        type="button"
        class="tool"
        title="Redo (⇧⌘Z)"
        aria-label="Redo"
        disabled={!historyStore.canRedo}
        onclick={() => void sessionStore.redo()}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M21 7v6h-6"/>
          <path d="M3 17a9 9 0 0 1 15-6.7L21 13"/>
        </svg>
      </button>
    </div>
  </div>
</nav>

<style>
  .toolbar {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    height: var(--layout-toolbar-h);
    padding: 0 var(--space-12);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    z-index: var(--z-toolbar);
    user-select: none;
    flex: 0 0 auto;
  }

  .left {
    position: absolute;
    left: var(--space-12);
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
  }

  .right {
    position: absolute;
    right: var(--space-12);
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
    min-width: 1px;
  }

  .center {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .group {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .divider {
    width: 1px;
    height: 22px;
    background: var(--color-border);
    margin: 0 var(--space-6);
  }

  .tool {
    position: relative;
    width: 36px;
    height: 36px;
    border-radius: var(--radius-md);
    color: var(--color-fg-muted);
    background: transparent;
    display: grid;
    place-items: center;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing),
      box-shadow var(--motion-fast) var(--motion-easing);
    cursor: pointer;
  }

  .tool:hover:not(:disabled) {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .tool:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* History group — toolbar 우측 끝, divider 시각 분리. */
  .history-group {
    margin-left: var(--space-6);
    padding-left: var(--space-6);
    border-left: 1px solid var(--color-border);
  }

  .tool:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .tool.active {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }

  .tool.active:hover {
    background: color-mix(in srgb, var(--color-accent) 90%, white);
  }

  /* Q-lock visual ring around active tool */
  .tool.locked {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 35%, transparent);
  }

  .tooltip {
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%) translateY(-2px);
    padding: 4px 8px;
    background: var(--color-fg);
    color: var(--color-bg);
    font-size: var(--text-base);
    font-family: var(--font-sans);
    letter-spacing: 0;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      transform var(--motion-fast) var(--motion-easing);
    z-index: calc(var(--z-toolbar) + 1);
  }

  .tool:hover .tooltip {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
    transition-delay: 200ms;
  }

  .lock-indicator {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    color: var(--color-accent);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border: 1px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
  }
</style>
