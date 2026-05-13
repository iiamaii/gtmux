// LayoutStore — CanvasLayout (HTTP GET/PUT /api/layout 동기화 대상) + ETag.
// R8 §F5 디바운스 commit 패턴.

class LayoutStore {
  etag = $state<string | null>(null);
  schemaVersion = $state<number>(1);
}

export const layoutStore = new LayoutStore();
