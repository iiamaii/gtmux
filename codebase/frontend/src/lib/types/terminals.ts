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
  /** Workspace 전체 session-file 의 terminal-item reference 수. *File-
   *  reference 기준* — session detach 후에도 layout 의 panel 이 남으면
   *  count. **kill guard / mirror 보호의 source.** */
  attach_count: number;
  /** `id` 를 reference 하는 session 이름들 (lex 정렬). file-ref 기준. */
  attached_sessions: string[];
  /** `attached_sessions` 중 *현재 attach lock 을 보유한* session 만 (0077
   *  follow-up, ADR-0021 D7 amend ④ 시점부터 wire). 사용자 mental model
   *  의 *live attached*. detach 후 즉시 빠진다. UI badge count 는 본 list
   *  의 length 를 표시. kill guard 는 여전히 `attached_sessions` 기준. */
  live_attached_sessions: string[];
}

/** Convenience — list endpoint 의 정규화 wrapper. */
export interface TerminalListResponse {
  terminals: TerminalInfo[];
}
