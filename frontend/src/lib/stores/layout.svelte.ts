// LayoutStore — CanvasLayout (HTTP GET/PUT /api/layout 동기화 대상) + ETag.
// R8 §F5 디바운스 commit 패턴.
//
// `etag` 는 *hex 직렬화된 16-byte raw ETag* — canvas-layout-schema.md §2 의 정규화
// 규칙에 따라 HTTP 구간은 hex, WS 구간은 raw 16B. WS dispatcher 는 raw → hex 변환
// 후 본 store 에 보관해 `If-Match` 헤더에 그대로 쓸 수 있게 한다.

class LayoutStore {
  etag = $state<string | null>(null);
  schemaVersion = $state<number>(1);

  /** Dispatcher (`0x80 LAYOUT_CHANGED` 수신 시) 가 호출. */
  setEtag(hex: string): void {
    this.etag = hex;
  }
}

export const layoutStore = new LayoutStore();
