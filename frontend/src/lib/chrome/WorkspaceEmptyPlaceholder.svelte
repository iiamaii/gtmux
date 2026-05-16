<script lang="ts">
  /**
   * WorkspaceEmptyPlaceholder — canvas 위 centered placeholder. `active === null`
   * & workspaceSwitcher 가 closed 일 때만 mount. 사용자가 *첫 진입* 또는
   * modal cancel 후 canvas 가 빈 dot grid 만 보이는 상태에서 *인지 단서* 제공.
   *
   * 정본:
   * - 사용자 요구 — "사실상 session 을 추가하거나 선택할 수 밖에 없도록"
   * - ADR-0019 D7 (인증 후 Dialog: 새 / 기존 session)
   * - AuthDialog 의 *non-modal* 변형 (modal 이 닫혀도 canvas 위 항상 표시)
   *
   * 동작:
   * - 두 button: "New session" (workspaceSwitcher.goCreate) / "Open existing"
   *   (workspaceSwitcher.goList('closed')). 둘 다 modal 재 open.
   * - Cancel 버튼 없음 — 사용자가 두 액션 중 하나 *반드시* 선택해야 진행.
   */

  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';

  function onNew(): void {
    workspaceSwitcher.goCreate();
  }

  function onOpen(): void {
    workspaceSwitcher.goList('closed');
  }
</script>

<div class="placeholder" role="region" aria-label="No workspace session">
  <div class="card">
    <div class="mark" aria-hidden="true">
      <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round">
        <rect x="4" y="6" width="24" height="20" rx="3"/>
        <path d="M4 11h24"/>
        <circle cx="7.5" cy="8.5" r="0.5" fill="currentColor"/>
        <circle cx="10" cy="8.5" r="0.5" fill="currentColor"/>
        <circle cx="12.5" cy="8.5" r="0.5" fill="currentColor"/>
      </svg>
    </div>
    <h2 class="heading">Choose a workspace session</h2>
    <p class="deck">
      Sessions hold your canvas — terminals, notes, layout, and viewport. Start
      with a fresh canvas or pick from saved workspaces.
    </p>
    <div class="actions">
      <button type="button" class="btn primary" onclick={onNew}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="12" y1="5" x2="12" y2="19"/>
          <line x1="5" y1="12" x2="19" y2="12"/>
        </svg>
        New session
      </button>
      <button type="button" class="btn" onclick={onOpen}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 7h6l2 2h10v11H3z"/>
        </svg>
        Open existing
      </button>
    </div>
  </div>
</div>

<style>
  .placeholder {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    pointer-events: none;
    z-index: 5;
  }

  .card {
    pointer-events: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-10);
    max-width: 420px;
    padding: 32px 40px 28px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    text-align: center;
  }

  .mark {
    width: 56px;
    height: 56px;
    border-radius: var(--radius-md);
    display: grid;
    place-items: center;
    background: var(--color-surface-2);
    color: var(--color-fg-muted);
    margin-bottom: var(--space-4);
  }

  .heading {
    margin: 0;
    font-size: 18px;
    font-weight: 540;
    letter-spacing: -0.2px;
    color: var(--color-fg);
  }

  .deck {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--color-fg-muted);
    letter-spacing: -0.05px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-8);
    margin-top: var(--space-8);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-6);
    height: 32px;
    padding: 0 14px;
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
    color: var(--color-fg);
    border: 1px solid var(--color-border);
    font-size: 13px;
    font-weight: 480;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing);
  }

  .btn:hover {
    background: var(--color-glass-1);
    border-color: var(--color-border-strong);
  }

  .btn:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .btn.primary {
    background: var(--color-fg);
    color: var(--color-bg);
    border-color: var(--color-fg);
  }

  .btn.primary:hover {
    opacity: 0.92;
  }
</style>
