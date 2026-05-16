// Terminal pool types — `GET /api/terminals` (BE Phase 4-B / BE-NEW-10).
//
// 정본:
// - BE: `codebase/backend/crates/http-api/src/terminals.rs:104` (TerminalInfo)
// - ADR-0021 D7 (Terminal pool — server-wide alive Terminal set)
// - plan-0007 §14 FE-NEW-3 (Terminal list UI)
//
// 어휘:
// - Terminal = backend PTY child (UUID id, schema v2 의 terminal item.id 와 동일,
//   ADR-0018 D2)
// - Pool = server 안 alive Terminal 의 집합 — session 과 무관 (multi-session
//   mirror 의 source)

/** Wire shape — BE 가 `Json(Vec<TerminalInfo>)` 로 bare array 응답. */
export interface TerminalInfo {
  /** Schema v2 UUID. terminal item.id 와 1:1 (ADR-0018 D2). */
  id: string;
  /** Pool 에 alive (`true`). dangling/dead 상태는 BE Phase 4-D (ADR-0021 D10) 후. */
  alive: boolean;
  /** 사용자 라벨 — 기본 빈 string. PATCH 으로 set (BE 후속). */
  label: string;
  /** Unix epoch seconds — re-spawn 무관 stable (BE terminals.rs D-comment). */
  created_at: number;
  /** Workspace 전체 session-file 의 terminal-item reference 수. */
  attach_count: number;
  /** `id` 를 reference 하는 session 이름들 (lex 정렬). */
  attached_sessions: string[];
}

/** Convenience — list endpoint 의 정규화 wrapper. */
export interface TerminalListResponse {
  terminals: TerminalInfo[];
}
