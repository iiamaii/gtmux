// WorkspaceSwitcher store — modal stack state machine.
//
// 정본:
// - plan-0007 §14 FE-NEW-1 + frontend-handover §6 Stage 2~3
// - ADR-0019 D7 (인증 후 Dialog: 새 / 기존 session)
//
// 본 store 는 AuthDialog → (NewSessionModal | SessionListModal) →
// AttachConfirmModal 의 흐름을 *명시 stage* 로 잡는 단순 state machine.
// 실제 modal mount + BE 호출은 `lib/chrome/WorkspaceSwitcher.svelte` 가
// 담당. 본 store 는 *trigger 와 stage 관리* 만.

import type { AttachConfirmSummary } from '$lib/types/sessions';

export type SwitcherStage =
  | 'closed'
  | 'choice'
  | 'create'
  | 'list'
  | 'attach_confirm';

export type ListCloseTarget = 'choice' | 'closed';

class WorkspaceSwitcherStore {
  stage = $state<SwitcherStage>('closed');
  /** Attach 시도 중인 session 이름 (attach_confirm stage 의 입력). */
  pendingSession = $state<string | null>(null);
  /** Attach confirm summary — BE 가 confirm_required 응답 시 채워짐. */
  pendingSummary = $state<AttachConfirmSummary | null>(null);
  /** SessionListModal cancel target depends on entry point. */
  listCloseTarget = $state<ListCloseTarget>('choice');

  open(): void {
    this.stage = 'choice';
    this.listCloseTarget = 'choice';
  }

  close(): void {
    this.stage = 'closed';
    this.pendingSession = null;
    this.pendingSummary = null;
    this.listCloseTarget = 'choice';
  }

  goCreate(): void {
    this.stage = 'create';
  }

  goList(closeTarget: ListCloseTarget = this.listCloseTarget): void {
    this.pendingSession = null;
    this.pendingSummary = null;
    this.listCloseTarget = closeTarget;
    this.stage = 'list';
  }

  closeList(): void {
    if (this.listCloseTarget === 'closed') {
      this.close();
      return;
    }
    this.open();
  }

  goAttachConfirm(sessionName: string, summary: AttachConfirmSummary): void {
    this.pendingSession = sessionName;
    this.pendingSummary = summary;
    this.stage = 'attach_confirm';
  }
}

export const workspaceSwitcher = new WorkspaceSwitcherStore();
