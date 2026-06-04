<script lang="ts">
  /**
   * Toolbar2 — 56px Figma-style 13-tool toolbar (FE-2 / Stage 5).
   *
   * 정본:
   * - plan-0007 §14.2 FE-2 (tool groups + dividers)
   * - plan-0007 §14.20.3 G22 (one-shot default + Q lock)
   * - ref/frontend-design/SPEC.md §4 (visual spec)
   * - ADR-0018 D4 (도구 ↔ ItemType 1:1 매핑)
   *
   * Layout (centre 정렬 + left/right absolute):
   *   ├─ left absolute: "Page 1 ▾" (page selector placeholder)
   *   ├─ centre groups:
   *   │   [Select | Hand] | [Terminal] |
   *   │   [Rect | Ellipse | Line | FreeDraw | Text] |
   *   │   [Note | Snippets | Document | Image | FilePath]
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
  import { chromeStore } from '$lib/stores/chrome.svelte';
  import { historyStore } from '$lib/stores/historyStore.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { shortcutRegistry } from '$lib/keyboard/shortcutRegistry.svelte';
  import {
    formatShortcutBinding,
    isMacPlatform,
    labelWithShortcut,
    primaryModifierBinding,
  } from '$lib/keyboard/shortcutDisplay';

  interface ToolDef {
    id: ToolId;
    name: string;
    hint?: string;
  }

  /** 13 tools grouped by semantic. Divider 는 그룹 사이. */
  const GROUPS: ToolDef[][] = [
    // 1) Mode (always sticky)
    [
      {
        id: 'select',
        name: 'Select',
        hint: 'V',
      },
      {
        id: 'hand',
        name: 'Hand',
        hint: 'H',
      },
    ],
    // 2) Terminal (the principal gtmux tool)
    [
      {
        id: 'terminal',
        name: 'Terminal',
      },
    ],
    // 3) Figures (vector primitives + text). Text shares this band because
    //    it composes structurally like a primitive (axis-aligned, drag-spawn).
    [
      {
        id: 'rect',
        name: 'Rectangle',
        hint: 'R',
      },
      {
        id: 'ellipse',
        name: 'Ellipse',
        hint: 'O',
      },
      {
        id: 'line',
        name: 'Line',
        hint: 'L',
      },
      {
        id: 'path',
        name: 'Path',
      },
      {
        id: 'free_draw',
        name: 'Free draw',
        hint: 'P',
      },
      {
        id: 'text',
        name: 'Text',
        hint: 'T',
      },
    ],
    // 4) Content (annotations + assets + references). Notes/snippets share
    //    this band with the asset items because they are all content-bearing
    //    canvas items — distinct from the figure primitives above.
    //    Note icon: lucide scroll-text. Snippets icon: lucide square-library.
    //    24-unit viewBox; layer-tree rows use the same shapes scaled to 12-unit.
    [
      // Note icon = lucide scroll-text (24-unit canonical). Layer-tree rows
      // use a 12-unit simplified silhouette (3 text lines in a rounded rect).
      {
        id: 'note',
        name: 'Note',
        hint: 'N',
      },
      {
        id: 'snippets',
        name: 'Snippets',
      },
      {
        id: 'document',
        name: 'Document',
        hint: 'D',
      },
      {
        id: 'image',
        name: 'Image',
        hint: 'I',
      },
      {
        id: 'file_path',
        name: 'File path',
        hint: 'F',
      },
    ],
  ];

  const current = $derived(toolStore.current);
  const locked = $derived(toolStore.locked);
  const shortcutActions = $derived.by(() => shortcutRegistry.listActions());
  // No active session 시 12 도구는 의미 없음 (canvas mutation 무효). 사용자
  // 가 ActiveSessionDropdown 으로 session 을 먼저 연결하도록 유도.
  const noActiveSession = $derived(sessionStore.active === null);

  function toolActionId(id: ToolId): string {
    return id === 'terminal' ? 'canvas.new_terminal' : `tool.${id}`;
  }

  function activeHint(id: ToolId, fallback?: string): string {
    const action = shortcutActions.find((a) => a.actionId === toolActionId(id));
    const binding = action?.activeBindings[0];
    return binding ? formatShortcutBinding(binding) : fallback ?? '';
  }

  const undoHint = $derived(formatShortcutBinding(primaryModifierBinding('z')));
  const redoHint = $derived(
    isMacPlatform()
      ? formatShortcutBinding({ key: 'z', meta: true, shift: true })
      : formatShortcutBinding({ key: 'y', ctrl: true }),
  );

  function returnToLayerTabForCanvasWork(): void {
    if (chromeStore.state.leftPanelTab === 'files') {
      chromeStore.setLeftPanelTab('layers');
    }
  }

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

{#snippet toolIcon(id: ToolId)}
  {#if id === 'select'}
    <path d="M3 2 L3 17 L7 13 L9.5 18.5 L11.8 17.4 L9.4 12.2 L15 12 Z" fill="currentColor" stroke="none"/>
  {:else if id === 'hand'}
    <path d="M8 13V5.5a1.5 1.5 0 0 1 3 0V12"/>
    <path d="M11 12V4.5a1.5 1.5 0 0 1 3 0V12"/>
    <path d="M14 11.5V5.5a1.5 1.5 0 0 1 3 0V14"/>
    <path d="M17 8.5a1.5 1.5 0 0 1 3 0V16a5 5 0 0 1-5 5h-3a5 5 0 0 1-4.5-2.5L4 13a1.5 1.5 0 0 1 2.5-1.5L8 14"/>
  {:else if id === 'terminal'}
    <rect x="3" y="4" width="18" height="16" rx="2"/>
    <path d="M7 9l3 3-3 3"/>
    <path d="M13 15h4"/>
  {:else if id === 'rect'}
    <rect x="4" y="5" width="16" height="14" rx="1.5"/>
  {:else if id === 'ellipse'}
    <ellipse cx="12" cy="12" rx="8.5" ry="7"/>
  {:else if id === 'line'}
    <line x1="4.5" y1="19" x2="19.5" y2="5"/>
  {:else if id === 'path'}
    <path d="M4 18h6V7h8"/>
    <path d="m15 4 3 3-3 3"/>
  {:else if id === 'free_draw'}
    <path d="M4 17c2-4 4-2 6-5s2-7 5-7 5 4 5 6"/>
  {:else if id === 'text'}
    <path d="M5 5h14M12 5v14M9 19h6" stroke-width="2"/>
  {:else if id === 'note'}
    <path d="M15 12h-5"/>
    <path d="M15 8h-5"/>
    <path d="M19 17V5a2 2 0 0 0-2-2H4"/>
    <path d="M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3"/>
  {:else if id === 'snippets'}
    <rect x="3" y="3" width="18" height="18" rx="2"/>
    <path d="M7 7v10"/>
    <path d="M11 7v10"/>
    <path d="m15 7 2 10"/>
  {:else if id === 'document'}
    <path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/>
    <path d="M14 3v4h4"/>
    <path d="M8 13h8M8 17h5"/>
  {:else if id === 'image'}
    <rect x="3" y="4" width="18" height="16" rx="2"/>
    <circle cx="9" cy="10" r="1.5"/>
    <path d="M3 17l5-4 4 3 5-5 4 4"/>
  {:else if id === 'file_path'}
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
  {/if}
{/snippet}

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
          {@const hint = activeHint(tool.id, tool.hint)}
          <button
            type="button"
            class="tool"
            class:active={current === tool.id}
            class:locked={current === tool.id && locked}
            title={noActiveSession
              ? 'Connect a session to use canvas tools'
              : labelWithShortcut(tool.name, hint)}
            aria-label={tool.name}
            aria-pressed={current === tool.id}
            disabled={noActiveSession}
            data-tool-id={tool.id}
            onclick={(e) => {
              returnToLayerTabForCanvasWork();
              toolStore.set(tool.id);
              // 클릭 후 button focus retention 차단 — ESC 로 tool 취소 시 옛 button 의
              // :focus-visible outline 잔류 회피. Tab navigation focus 는 그대로.
              (e.currentTarget as HTMLButtonElement).blur();
            }}
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
              {@render toolIcon(tool.id)}
            </svg>
            <span class="tooltip">{labelWithShortcut(tool.name, hint)}</span>
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
        title={labelWithShortcut('Undo', undoHint)}
        aria-label="Undo"
        disabled={!historyStore.canUndo}
        onclick={(e) => {
          void sessionStore.undo();
          (e.currentTarget as HTMLButtonElement).blur();
        }}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 7v6h6"/>
          <path d="M21 17a9 9 0 0 0-15-6.7L3 13"/>
        </svg>
        <span class="tooltip">{labelWithShortcut('Undo', undoHint)}</span>
      </button>
      <button
        type="button"
        class="tool"
        title={labelWithShortcut('Redo', redoHint)}
        aria-label="Redo"
        disabled={!historyStore.canRedo}
        onclick={(e) => {
          void sessionStore.redo();
          (e.currentTarget as HTMLButtonElement).blur();
        }}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M21 7v6h-6"/>
          <path d="M3 17a9 9 0 0 1 15-6.7L21 13"/>
        </svg>
        <span class="tooltip">{labelWithShortcut('Redo', redoHint)}</span>
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
