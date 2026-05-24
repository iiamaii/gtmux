// ToolStore — Toolbar 의 활성 도구 + lock 상태.
//
// 정본:
// - plan-0007 §14.2 FE-2 (Toolbar2 의 12 도구)
// - plan-0007 §14.20.3 G22 (one-shot default + Q lock + Esc 해제)
// - frontend-handover §3.8 (Toolbar = one-shot default + Q lock)
//
// Behaviour:
//   - 모든 도구는 *one-shot default* — 클릭 → tool 으로 mode 전환 → 한 번 사용
//     (creation gesture 완료) 후 자동 Select 복귀.
//   - Q 단축키 또는 long-press 로 lock sticky → 같은 도구 반복 사용 가능.
//   - Esc → lock 해제 (locked) / Select 복귀 (mode) / 그 다음은 escRouter.
//   - Select / Hand 는 mode 라서 항상 sticky (one-shot 적용 X) — 사용 중에는
//     lock 무관, mode 자체가 "지속".
//
// 도구 12개 (Stage 5 / ADR-0018 D4 의 type 과 1:1):
//   select / hand / terminal / text / note / rect / ellipse / line /
//   free_draw / image / document / file_path

export type ToolId =
  | 'select'
  | 'hand'
  | 'terminal'
  | 'text'
  | 'note'
  | 'rect'
  | 'ellipse'
  | 'line'
  | 'free_draw'
  | 'image'
  | 'document'
  | 'file_path'
  | 'snippets';

/** Select / Hand 는 *mode* — one-shot 적용 X. */
const STICKY_MODES: ReadonlySet<ToolId> = new Set(['select', 'hand']);

class ToolStore {
  /** 현 활성 도구. */
  current = $state<ToolId>('select');

  /**
   * Lock state — true 면 도구 사용 후 자동 복귀 X (Q lock).
   * Select / Hand 에서는 무의미 (이미 sticky mode).
   */
  locked = $state(false);

  /** 도구 선택. */
  set(id: ToolId): void {
    this.current = id;
    // 모드 전환 시 lock 초기화 — 새 도구가 처음부터 locked 인 것은 의도 아닙.
    if (STICKY_MODES.has(id)) {
      this.locked = false;
    }
  }

  /**
   * 도구 사용 완료 통지 — one-shot 의 *자동 복귀* trigger.
   * Locked 또는 sticky mode 면 no-op.
   * Stage 5 의 creation gesture 종료 시 (panel mount / shape commit / text
   * commit 등) caller 가 호출.
   *
   * **Async creation 의 timing** (2026-05-22): file picker / uploadAsset 등
   * long-running async 의 creation 분기는 await *완료 후* consume 한다 —
   * tool 의 활성 상태가 사용자에게 *시각 단서* 로 유지되어야 한다는 UX
   * 정합 (toolbar 의 활성 아이콘 = "지금 입력 중") 때문. picker 중복 open
   * 의 회귀는 `lib/files/localFilePicker.ts` 의 reentrant guard 가 책임.
   */
  consume(): void {
    if (this.locked) return;
    if (STICKY_MODES.has(this.current)) return;
    this.current = 'select';
  }

  /** Q lock toggle. Select / Hand 에서는 no-op. */
  toggleLock(): void {
    if (STICKY_MODES.has(this.current)) return;
    this.locked = !this.locked;
  }

  /**
   * Esc 처리 — escRouter (§14.20.2) 의 4단계 (tool lock 해제) + 5단계 (Select
   * 복귀). 본 메서드는 *둘 다* 처리하고 *실행 여부* 를 반환 — escRouter 가
   * priority 체인 결정에 사용.
   */
  handleEsc(): boolean {
    if (this.locked) {
      this.locked = false;
      return true;
    }
    if (!STICKY_MODES.has(this.current)) {
      this.current = 'select';
      return true;
    }
    return false;
  }
}

export const toolStore = new ToolStore();
