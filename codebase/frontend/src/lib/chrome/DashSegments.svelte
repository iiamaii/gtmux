<script lang="ts">
  // StyleDropdown — FigureStrokeDash 4 variant 의 custom dropdown.
  //
  // 정본: ADR-0018 D4 amend ① (batch-5) — rect/ellipse/line 공통 stroke_dash.
  // 사용자 요구 (2026-05-21 polish #2):
  //   - "style 은 dropdown 으로 표시" — 단일 trigger + popover 패턴
  //   - "text 아닌 symbol 로" — option 의 시각 cue 는 SVG painted line
  //   - InspectorField 와 visual 정합 — h22 + border + k label inside
  //
  // 구현 노트:
  //   - 파일명은 호환 위해 DashSegments.svelte 유지 (consumer import path).
  //   - trigger button 안에 현재 선택된 pattern preview SVG + chevron 표시.
  //   - popover 는 trigger 아래 absolute, 4 option SVG button 세로 stack.
  //   - outside click / Escape / option click 시 close.

  import { onDestroy } from 'svelte';
  import type { FigureStrokeDash } from '$lib/types/canvas';

  interface Props {
    value: FigureStrokeDash;
    disabled?: boolean;
    onpick: (next: FigureStrokeDash) => void;
  }

  const { value, disabled = false, onpick }: Props = $props();

  const PATTERNS: { id: FigureStrokeDash; title: string; dashArray: string; linecap: 'round' | 'butt' }[] = [
    { id: 'solid',    title: 'Solid',    dashArray: 'none',      linecap: 'round' },
    { id: 'dash',     title: 'Dash',     dashArray: '6 3',       linecap: 'butt' },
    { id: 'dot',      title: 'Dot',      dashArray: '1.5 3',     linecap: 'round' },
    { id: 'dash_dot', title: 'Dash-Dot', dashArray: '6 3 1.5 3', linecap: 'round' },
  ];

  let open = $state(false);
  let rootEl: HTMLDivElement | undefined = $state();

  const current = $derived(PATTERNS.find((p) => p.id === value) ?? PATTERNS[0]!);

  function onDocClick(e: MouseEvent): void {
    if (!open) return;
    if (rootEl && rootEl.contains(e.target as Node)) return;
    open = false;
  }
  function onDocKey(e: KeyboardEvent): void {
    if (open && e.key === 'Escape') {
      open = false;
      e.stopPropagation();
    }
  }
  $effect(() => {
    if (open) {
      window.addEventListener('mousedown', onDocClick, { capture: true });
      window.addEventListener('keydown', onDocKey, { capture: true });
      return () => {
        window.removeEventListener('mousedown', onDocClick, { capture: true });
        window.removeEventListener('keydown', onDocKey, { capture: true });
      };
    }
  });
  onDestroy(() => {
    window.removeEventListener('mousedown', onDocClick, { capture: true });
    window.removeEventListener('keydown', onDocKey, { capture: true });
  });

  function pick(next: FigureStrokeDash): void {
    onpick(next);
    open = false;
  }
</script>

<div bind:this={rootEl} class="style-dropdown inspector-input" class:disabled class:open>
  <span class="k" aria-hidden="true">style</span>
  <button
    type="button"
    class="style-trigger"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label="Stroke style — {current.title}"
    {disabled}
    onclick={() => { if (!disabled) open = !open; }}
  >
    <svg class="style-preview" viewBox="0 0 28 12" aria-hidden="true">
      <line
        x1="3"
        y1="6"
        x2="25"
        y2="6"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-dasharray={current.dashArray}
        stroke-linecap={current.linecap}
      />
    </svg>
    <svg class="chevron" viewBox="0 0 10 6" aria-hidden="true">
      <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>
  </button>
  {#if open}
    <div class="style-popover" role="listbox" aria-label="Stroke style options">
      {#each PATTERNS as p (p.id)}
        <button
          type="button"
          class="style-option"
          class:selected={value === p.id}
          role="option"
          aria-selected={value === p.id}
          title={p.title}
          onclick={() => pick(p.id)}
        >
          <svg viewBox="0 0 28 12" aria-hidden="true">
            <line
              x1="3"
              y1="6"
              x2="25"
              y2="6"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-dasharray={p.dashArray}
              stroke-linecap={p.linecap}
            />
          </svg>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  /* Inspector design 규칙 (2026-05-21): height 24px, label inside left, width full. */
  .inspector-input {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    height: 24px;
    padding: 0 0 0 6px;
    box-sizing: border-box;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-fg);
    transition: border-color var(--motion-fast) var(--motion-easing);
  }
  .inspector-input:hover { border-color: var(--color-border-strong); }
  .inspector-input.open { border-color: var(--color-accent); }
  .inspector-input.disabled { opacity: 0.5; pointer-events: none; }

  .k {
    flex: 0 0 56px;
    width: 56px;
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.4px;
    pointer-events: none;
  }

  .style-trigger {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 6px 0 6px;
    border: 0;
    border-left: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-fg);
    cursor: pointer;
  }
  .style-trigger:disabled { cursor: not-allowed; }

  .style-preview {
    flex: 1 1 auto;
    height: 12px;
    min-width: 0;
    color: var(--color-fg);
  }
  .chevron {
    flex: 0 0 10px;
    width: 10px;
    height: 6px;
    color: var(--color-fg-muted);
  }

  .style-popover {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    right: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 2px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.16);
  }
  .style-option {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 2px;
    background: transparent;
    color: var(--color-fg);
    cursor: pointer;
  }
  .style-option:hover { background: var(--color-glass-1); }
  .style-option.selected {
    background: var(--color-accent);
    color: var(--color-accent-fg);
  }
  .style-option svg {
    width: 100%;
    height: 12px;
  }
</style>
