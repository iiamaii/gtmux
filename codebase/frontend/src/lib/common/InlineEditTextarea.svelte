<script lang="ts">
  /**
   * InlineEditTextarea — multi-line inline edit (G23 공용 컴포넌트).
   *
   * 정본:
   * - plan-0007 §14.20.1 (Inline edit G23)
   *
   * 키 처리:
   *   - Cmd/Ctrl + Enter → commit
   *   - Enter (plain)    → newline (default)
   *   - Esc              → cancel (escRouter priority 1)
   *   - blur             → commit
   *   - Empty commit     → allowEmpty 정책에 따름 (default true — note body 등)
   */

  import { tick } from 'svelte';
  import { escRouter } from './escRouter.svelte';

  interface Props {
    value: string;
    editing: boolean;
    onCommit: (next: string) => void;
    onCancel?: () => void;
    class?: string;
    placeholder?: string;
    /** 빈 string commit 허용. default true (multi-line 의 의도 — note body 등). */
    allowEmpty?: boolean;
    rows?: number;
    /** Canvas text inline edit 처럼 별도 input chrome 없이 원 위치에서 편집할 때 사용. */
    plain?: boolean;
    /** focus 시 전체 선택 여부. Canvas text 는 기존 위치 편집감을 위해 false 사용. */
    selectOnFocus?: boolean;
    textAlign?: 'left' | 'center' | 'right';
  }

  const {
    value,
    editing,
    onCommit,
    onCancel,
    class: extraClass = '',
    placeholder = '',
    allowEmpty = true,
    rows = 4,
    plain = false,
    selectOnFocus = true,
    textAlign = 'left',
  }: Props = $props();

  // $state init 시 prop 의 *최초 값* 만 capture — 실 동기화는 $effect 가 담당.
  let draft = $state('');
  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let escUnregister: (() => void) | null = null;

  /**
   * Auto-grow — plain 모드(.text-node 등 chrome-less inline edit)에서 textarea
   * 의 height 가 display span 과 같이 콘텐츠 fit 이도록 매 input 마다 갱신.
   * flex `align-items` 가 적용되는 컨테이너에서 display↔edit 박스 크기 차이로
   * 텍스트 시작 위치가 어긋나는 것을 방지.
   */
  function syncAutoHeight(): void {
    if (!plain || textareaEl === undefined) return;
    textareaEl.style.height = 'auto';
    textareaEl.style.height = `${textareaEl.scrollHeight}px`;
  }

  $effect(() => {
    if (editing) {
      draft = value;
      void tick().then(() => {
        textareaEl?.focus();
        if (selectOnFocus) textareaEl?.select();
        syncAutoHeight();
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

  // Draft 변할 때마다 height 재계산.
  $effect(() => {
    void draft;
    if (editing) syncAutoHeight();
  });

  function commit(): void {
    if (draft.length === 0 && !allowEmpty) {
      cancel();
      return;
    }
    onCommit(draft);
  }

  function cancel(): void {
    draft = value;
    onCancel?.();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !e.isComposing) {
      e.preventDefault();
      commit();
    }
    // Enter plain → newline (textarea default). Esc → escRouter.
  }

  function onblur(): void {
    if (editing) commit();
  }
</script>

{#if editing}
  <textarea
    bind:this={textareaEl}
    bind:value={draft}
    {placeholder}
    {rows}
    class="inline-edit-textarea {extraClass}"
    class:plain
    style:text-align={textAlign}
    {onkeydown}
    {onblur}
  ></textarea>
{:else}
  <span class="inline-edit-display {extraClass}">{value}</span>
{/if}

<style>
  .inline-edit-textarea {
    width: 100%;
    padding: 6px 8px;
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    font-family: inherit;
    font-size: inherit;
    line-height: var(--leading-normal);
    box-sizing: border-box;
    resize: vertical;
    min-height: 60px;
  }

  .inline-edit-textarea:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 0;
    border-color: var(--color-accent);
  }

  .inline-edit-textarea.plain {
    min-height: 0;
    padding: 0;
    background: transparent;
    border: 0;
    border-radius: 0;
    resize: none;
    outline: none;
  }

  .inline-edit-textarea.plain:focus-visible {
    outline: none;
    border-color: transparent;
  }

  .inline-edit-display {
    white-space: pre-wrap;
  }
</style>
