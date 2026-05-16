<script lang="ts">
  /**
   * AuthDialog — 인증 통과 후 첫 modal.
   *
   * 정본:
   * - ADR-0019 D7 (Dialog: 새 session 추가 / 기존 session 연동)
   * - plan-0007 §14 FE-NEW-1 + frontend-handover §6 Stage 2
   *
   * 본 dialog 는 *pure switchboard* — 두 버튼만 노출하고 실제 modal (New /
   * List) 진입은 부모 컴포넌트가 owns 한다. Esc / outside click → 닫힘 없음
   * (plan-0007 §14.20 의 "modal stack outside click 비활성" — 사용자 의도된
   * 명시 액션만으로 진행).
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';

  interface Props {
    open: boolean;
    /** [새 session 추가] 클릭 — 부모가 NewSessionModal 띄움. */
    onCreate: () => void;
    /** [기존 session 연동] 클릭 — 부모가 SessionListModal 띄움. */
    onSelect: () => void;
    /**
     * Esc 또는 backdrop click. 단 plan-0007 §14.20 정합으로 backdrop click
     * 은 false 처리 — Esc 만 닫음. 본 dialog 가 닫히면 사용자는 *세션 진입
     * 전* 상태로 — 부모가 자동 reopen 가능.
     */
    onClose?: () => void;
  }

  const { open, onCreate, onSelect, onClose }: Props = $props();
</script>

<Modal
  {open}
  onclose={onClose}
  title="Choose a workspace session"
  dismissOnBackdrop={false}
>
  {#snippet body()}
    <p class="lead">
      Sessions hold your canvas — terminals, notes, layout, and viewport. Pick
      an existing session or create a fresh one.
    </p>
    <div class="choice-grid">
      <button type="button" class="choice" onclick={onCreate}>
        <span class="choice-icon" aria-hidden="true">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </span>
        <span class="choice-title">New session</span>
        <span class="choice-sub">Start with an empty canvas.</span>
      </button>

      <button type="button" class="choice" onclick={onSelect}>
        <span class="choice-icon" aria-hidden="true">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="4" width="18" height="16" rx="2" />
            <line x1="3" y1="10" x2="21" y2="10" />
            <line x1="9" y1="14" x2="15" y2="14" />
          </svg>
        </span>
        <span class="choice-title">Open existing</span>
        <span class="choice-sub">Pick from saved workspaces.</span>
      </button>
    </div>
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={onClose}>Cancel</Button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-16);
    font-size: var(--text-md);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
  }

  .choice-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-12);
  }

  .choice {
    display: grid;
    grid-template-rows: auto auto auto;
    align-items: start;
    gap: var(--space-6);
    padding: var(--space-16);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    text-align: left;
    color: var(--color-fg);
    transition:
      background var(--motion-fast) var(--motion-easing),
      border-color var(--motion-fast) var(--motion-easing),
      transform 50ms var(--motion-easing);
    cursor: pointer;
  }

  .choice:hover {
    background: var(--color-glass-1);
    border-color: var(--color-border-strong);
  }

  .choice:active {
    transform: scale(0.99);
  }

  .choice:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .choice-icon {
    width: 32px;
    height: 32px;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-fg);
    display: grid;
    place-items: center;
    box-shadow: var(--shadow-sm);
  }

  .choice-title {
    font-size: var(--text-lg);
    font-weight: var(--weight-medium);
    letter-spacing: -0.1px;
    color: var(--color-fg);
  }

  .choice-sub {
    font-size: var(--text-base);
    color: var(--color-fg-muted);
    line-height: var(--leading-normal);
  }
</style>
