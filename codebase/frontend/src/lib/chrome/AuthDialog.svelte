<script lang="ts">
  /**
   * AuthDialog — 인증 통과 후 첫 modal.
   *
   * 정본:
   * - ADR-0019 D7 (Dialog: 새 session 추가 / 기존 session 연동)
   * - plan-0007 §14 FE-NEW-1 + frontend-handover §6 Stage 2
   *
   * 본 dialog 는 *pure switchboard* — 두 버튼만 노출하고 실제 modal (New /
   * List) 진입은 부모 컴포넌트가 owns 한다. backdrop click 은 항상 비활성
   * (plan-0007 §14.20). Esc/Cancel 닫힘은 `dismissable` prop 으로 제어 —
   * session 이 없을 때 (active === null) 부모가 false 로 전달해 사용자가
   * New / Open 둘 중 하나를 *반드시* 선택해야 진행되도록 한다.
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
     * Esc / Cancel 트리거. dismissable=false 면 호출 경로 자체가 차단되어
     * 호출되지 않는다.
     */
    onClose?: () => void;
    /**
     * 사용자가 modal 을 dismiss 가능한지. false 면 Esc / backdrop / Cancel
     * 버튼 모두 차단 — 사용자는 New / Open 두 액션 중 하나를 *반드시* 선택
     * 해야 진행 가능. session 이 없을 때의 정책 (active === null 경로).
     */
    dismissable?: boolean;
  }

  const {
    open,
    onCreate,
    onSelect,
    onClose,
    dismissable = true,
  }: Props = $props();
</script>

{#snippet cancelFooter()}
  <Button variant="ghost" onclick={onClose}>Cancel</Button>
{/snippet}

<Modal
  {open}
  onclose={dismissable ? onClose : undefined}
  title="Choose a workspace session"
  dismissOnBackdrop={false}
  dismissOnEsc={dismissable}
  footer={dismissable ? cancelFooter : undefined}
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
    /* Modal auto-focus 가 first focusable (New Session) 에 즉시 focus →
       dashed outline 이 사용자 첫 화면에 노출되며 거슬림. outline 제거 +
       border-color 만 강화로 인디케이터 유지. */
    outline: none;
    border-color: var(--color-accent);
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
