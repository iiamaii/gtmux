// LayoutRefetchGate — ADR-0053 D7 의 순수 결정 로직.
//
// 0x80 LAYOUT_CHANGED 는 *외부발* layout 변경 (CLI `/layout/ops` 등) 의 유일한
// FE 통지 채널이다. 본 모듈은 "언제 refetch 를 실제로 수행하는가" 의 결정만
// 담당하는 순수 상태 기계 — side effect (fetch / timer / store) 는
// `layoutRefetch.svelte.ts` 가 배선한다. 순수 유지 이유: vitest node 환경
// 단위 테스트 (프로젝트 정책 — pure helper 만 단위 테스트 강제).
//
// 판단 규칙:
// 1. etag 일치 → no-op. 자기 발신 echo (FE 의 PUT 이 유발한 broadcast) 는
//    HTTP 응답 경로에서 이미 etag 를 기록했으므로 여기서 걸러진다.
// 2. hold (node drag / panel resize 진행) 중이면 pending 으로 defer —
//    "드래그 중 items.clear() 가 절대 일어나지 않는다" 불변식의 담보.
// 3. flush 시점 (`takeRunnable`) 에 etag 를 재비교 — defer 되는 사이 자기
//    발신 PUT 응답이 도착해 이미 최신이 된 경우 refetch 를 생략한다.

/**
 * 0x80 payload 의 16B etag → lowercase hex 32자.
 *
 * BE 정합: `http-api/src/sessions.rs::sha256_128` 이 같은 16B 를 `{b:02x}`
 * (lowercase) 로 hex 화해 HTTP `ETag` header 로 내보낸다 — 본 변환 결과와
 * 문자열 동등 비교 가능.
 */
export function etagBytesToHex(bytes: Uint8Array): string {
  let hex = '';
  for (const b of bytes) hex += b.toString(16).padStart(2, '0');
  return hex;
}

export class LayoutRefetchGate {
  #holds = 0;
  #pendingEtagHex: string | null = null;

  /** 진행 중인 interaction hold 수 (drag / resize 중첩 대비 counter). */
  get holds(): number {
    return this.#holds;
  }

  /** Defer 된 refetch 의 대상 etag (없으면 null). */
  get pendingEtagHex(): string | null {
    return this.#pendingEtagHex;
  }

  /**
   * 0x80 수신. `knownEtagHex` = FE 가 마지막으로 확인한 layout etag
   * (`layoutEtag.hex`, 미확인 시 null).
   *
   * @returns true → caller 가 flush 를 schedule 해야 함. false → 이미 최신
   *          (자기 발신 echo) — no-op.
   */
  receive(etagHex: string, knownEtagHex: string | null): boolean {
    if (knownEtagHex !== null && etagHex === knownEtagHex) return false;
    // 최신 통지가 이전 pending 을 대체 — refetch 는 어차피 전체 GET 이라
    // 마지막 etag 만 의미 있다.
    this.#pendingEtagHex = etagHex;
    return true;
  }

  /** Interaction (drag/resize) 시작 — refetch 를 defer 상태로 전환. */
  hold(): void {
    this.#holds += 1;
  }

  /**
   * Interaction 종료.
   *
   * @returns true → hold 가 모두 풀렸고 defer 된 pending 이 있음 — caller 가
   *          flush 를 schedule 해야 함.
   */
  release(): boolean {
    if (this.#holds > 0) this.#holds -= 1;
    return this.#holds === 0 && this.#pendingEtagHex !== null;
  }

  /**
   * Flush 시점 판정.
   *
   * - pending 없음 → null.
   * - hold 중 → null, pending 유지 (release 가 재-schedule).
   * - pending == knownEtagHex → 그 사이 자기 발신 PUT 응답이 도착해 이미
   *   최신 — pending 소거 후 null.
   * - 그 외 → pending 소거 후 etag 반환 = caller 가 refetch 수행.
   */
  takeRunnable(knownEtagHex: string | null): string | null {
    if (this.#pendingEtagHex === null) return null;
    if (this.#holds > 0) return null;
    if (knownEtagHex !== null && this.#pendingEtagHex === knownEtagHex) {
      this.#pendingEtagHex = null;
      return null;
    }
    const etag = this.#pendingEtagHex;
    this.#pendingEtagHex = null;
    return etag;
  }
}
