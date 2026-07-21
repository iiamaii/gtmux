// layoutEtag — FE 가 마지막으로 확인한 active-session layout ETag (hex).
//
// ADR-0053 D7: 0x80 LAYOUT_CHANGED 의 16B etag 와 비교해 "자기 발신 echo"
// (FE 의 PUT/DELETE 가 유발한 broadcast) 를 걸러내는 기준값. HTTP 응답에서
// ETag 를 받는 *모든* 경로 (`getLayout`/`putLayout`/`deleteItem`/
// `reloadActiveLayout`/`attemptReattach`) 가 note 한다.
//
// 별도 모듈인 이유: `http/sessions.ts` ↔ `sessionStore.svelte.ts` 양쪽에서
// 기록해야 하는데 http 모듈이 sessionStore 를 import 하면 순환 — 의존 없는
// leaf 모듈로 분리. UI 반응성 불요 (비교 전용) → 순수 TS, $state 아님.

class LayoutEtagStore {
  #hex: string | null = null;

  /** 마지막으로 확인한 etag (lowercase hex). 미확인 시 null. */
  get hex(): string | null {
    return this.#hex;
  }

  /** ETag 확인 시 기록. 빈 문자열 / undefined 는 무시 (기존 값 유지). */
  note(hex: string | null | undefined): void {
    if (typeof hex === 'string' && hex.length > 0) {
      this.#hex = hex.toLowerCase();
    }
  }

  /** Session detach / clear 시 초기화. */
  clear(): void {
    this.#hex = null;
  }
}

export const layoutEtag = new LayoutEtagStore();
