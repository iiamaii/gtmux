// HTTP `GET/PUT /api/layout` 클라이언트.
// ETag + 412 rebase 정책은 R8 §F5. 디바운스 300ms (D12).

export async function getLayout(): Promise<void> {
  // 실제 구현은 P0. openapi-fetch 클라이언트 사용 예정.
}
