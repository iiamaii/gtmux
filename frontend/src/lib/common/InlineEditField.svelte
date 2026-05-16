<script lang="ts">
  /**
   * InlineEditField — single-line inline edit (G23 공용 컴포넌트).
   *
   * 정본:
   * - plan-0007 §14.20.1 (Inline edit G23)
   * - frontend-handover §3.7
   *
   * 동작:
   * - 표시 mode (default): 텍스트 span, 더블 클릭 → edit mode 진입
   * - edit mode: input focus + select all + 키 처리
   *   - Enter        → commit (onCommit 호출)
   *   - Esc          → cancel (값 미반영, escRouter priority 1)
   *   - blur         → commit (Figma 컨벤션)
   *   - Empty commit → cancel 효과 (단, allowEmpty=true 면 commit)
   * - validation 실패 → 빨간 hint + 키 비활성 (P1+에서 시각화 보강)
   */

  import { onMount, tick } from 'svelte';
  import { escRouter } from './escRouter.svelte';

  interface Props {
    value: string;
    /** Inline edit 활성 (외부 trigger — true 일 때 input 마운트 + focus). */
    editing: boolean;
    onCommit: (next: string) => void;
    /** Esc / blur-with-empty 등 cancel 경로 — 부모는 editing=false 로 닫음. */
    onCancel?: () => void;
    /** 표시 mode 의 default class — 부모 styling. */
    class?: string;
    placeholder?: string;
    /** 빈 string commit 허용. default false (cancel 효과). */
    allowEmpty?: boolean;
    /** Real-time validator. null = OK, string = error message. */
    validate?: (s: string) => string | null;
  }

  const {
    value,
    editing,
    onCommit,
    onCancel,
    class: extraClass = '',
    placeholder = '',
    allowEmpty = false,
    validate,
  }: Props = $props();

  // $state init 시 prop 의 *최초 값* 만 capture — 실 동기화는 $effect 가 담당.
  let draft = $state('');
  let inputEl: HTMLInputElement | undefined = $state();
  let escUnregister: (() => void) | null = null;

  let validationError = $derived(validate ? validate(draft) : null);

  $effect(() => {
    if (editing) {
      draft = value;
      void tick().then(() => {
        inputEl?.focus();
        inputEl?.select();
      });
      escUnregister = escRouter.register({
        priority: 1,
        handler: () => {
          cancel();
          return true;
        },
      });
    } else {
      if (escUnregister !== null) {
        escUnregister();
        escUnregister = null;
      }
    }
    return () => {
      if (escUnregister !== null) {
        escUnregister();
        escUnregister = null;
      }
    };
  });

  function commit(): void {
    if (validationError !== null) return;
    const trimmed = draft.trim();
    if (trimmed.length === 0 && !allowEmpty) {
      cancel();
      return;
    }
    onCommit(trimmed);
  }

  function cancel(): void {
    draft = value;
    onCancel?.();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      commit();
    }
    // Esc 는 escRouter 가 처리.
  }

  function onblur(): void {
    if (editing) commit();
  }
</script>

{#if editing}
  <input
    bind:this={inputEl}
    bind:value={draft}
    {placeholder}
    class="inline-edit-input {extraClass}"
    class:has-error={validationError !== null}
    aria-invalid={validationError !== null}
    {onkeydown}
    {onblur}
  />
  {#if validationError !== null}
    <span class="inline-edit-error" role="alert">{validationError}</span>
  {/if}
{:else}
  <span class={extraClass}>{value}</span>
{/if}

<style>
  .inline-edit-input {
    height: 22px;
    padding: 0 6px;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    font-family: inherit;
    font-size: inherit;
    line-height: 1.2;
    box-sizing: border-box;
    width: 100%;
  }

  .inline-edit-input:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 0;
    border-color: var(--color-accent);
  }

  .inline-edit-input.has-error {
    border-color: var(--color-danger);
  }

  .inline-edit-error {
    display: block;
    margin-top: 2px;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }
</style>
