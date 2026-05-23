// EscRouter — global Esc key dispatcher with priority chain.
//
// 정본:
// - plan-0007 §14.20.2 (Esc 라우팅 7 우선순위)
// - frontend-handover §3.6 (Esc 라우팅)
//
// 우선순위 (top → bottom — 먼저 *consume* 한 handler 가 승자):
//   1. inline edit cancel       (InlineEditField/Textarea 가 등록)
//   2. modal close              (Modal primitive 가 등록 — 현재 ui/Modal 이 직접 Esc 처리, 마이그레이션 후속)
//   3. unmaximize               (sessionStore.maximizedItemId !== null)
//   4. tool lock 해제           (toolStore.locked)
//   5. Select 복귀              (toolStore.current !== 'select'/'hand')
//   6. drill/selection clear    (drillRootId !== null || sessionStore.M.size > 0)
//   7. no-op
//
// Usage:
//   onMount(() => escRouter.register({ priority: 1, handler: () => { ... return true; } }));
//
// Handler 가 *handle 했음* 을 통보하려면 `true` 반환 (chain stop).
// `false` 반환 시 다음 priority 로.

import { sessionStore } from '$lib/stores/sessionStore.svelte';
import { toolStore } from '$lib/stores/toolStore.svelte';

export type EscPriority =
  | 1 // inline edit
  | 2 // modal
  | 3 // unmaximize
  | 4 // tool lock
  | 5 // select 복귀
  | 6 // selection clear
  | 7; // no-op

export interface EscHandler {
  priority: EscPriority;
  /** `true` 반환 시 chain stop. */
  handler: (event: KeyboardEvent) => boolean;
}

class EscRouter {
  #handlers = new Set<EscHandler>();
  #attached = false;

  /** Handler 등록. unregister 함수 반환. */
  register(h: EscHandler): () => void {
    this.#handlers.add(h);
    this.#ensureAttached();
    return () => {
      this.#handlers.delete(h);
    };
  }

  #ensureAttached(): void {
    if (this.#attached) return;
    if (typeof window === 'undefined') return;
    this.#attached = true;
    window.addEventListener('keydown', this.#onkeydown);
  }

  #onkeydown = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape') return;
    // Composition (IME) 중에는 Esc 가 IME cancel — 우리가 가로채면 안 됨.
    if (event.isComposing) return;

    // priority ascending order — 1 부터 차례.
    const sorted = Array.from(this.#handlers).sort(
      (a, b) => a.priority - b.priority,
    );
    for (const h of sorted) {
      const consumed = h.handler(event);
      if (consumed) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
    }
    // Default chain (등록 없이도 작동) — priorities 3~6 fallback.
    if (sessionStore.maximizedItemId !== null) {
      sessionStore.unmaximize();
      event.preventDefault();
      return;
    }
    if (toolStore.handleEsc()) {
      event.preventDefault();
      return;
    }
    if (sessionStore.drillRootId !== null) {
      sessionStore.clearDrill();
      sessionStore.clearM();
      event.preventDefault();
      return;
    }
    if (sessionStore.M.size > 0) {
      sessionStore.clearM();
      event.preventDefault();
      return;
    }
  };

  /**
   * Eager attach — explicit handler 등록 없이도 default fallback chain
   * (unmaximize / tool 취소 / selection clear) 이 동작하도록. module load
   * 시 호출 권장.
   */
  attach(): void {
    this.#ensureAttached();
  }

  /** Test/dev only — handler 모두 제거. */
  _reset(): void {
    this.#handlers.clear();
  }
}

export const escRouter = new EscRouter();

// Eager attach — handler register 없어도 fallback chain (unmaximize / tool
// 취소 / selection clear) 이 동작하도록 module load 시 listener 부착. SSR
// 안전 (typeof window 가드).
if (typeof window !== 'undefined') {
  escRouter.attach();
}
