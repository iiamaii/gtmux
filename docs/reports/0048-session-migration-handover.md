# 0048 — Session migration handover (cold-pickup brief)

- 작성일: 2026-05-16
- 작성자: BE agent (next-2 + Stage 6 cleanup + ADR-0026 draft ship 직후, context 77% 시점)
- 종류: **cold-pickup brief** — 다음 세션이 *현 진행 + 즉시 진입 우선순위 + 핵심 컨텍스트* 를 한 문서로 진입 가능하게 만드는 합본
- HEAD: `7e52410` docs: BE next-session handoff 0047 + ADR-0019/0024 amend
- 워크스페이스 baseline: **365 PASS / 0 FAIL**, smoke 02_stage5.sh **12/12 PASS**

---

## 0. 다음 세션 즉시 진입 — 우선순위 ⚠️ MUST READ

다음 두 항목이 **현재 시점의 BE 진입 P0**. cold-pickup 시 본 §부터 처리.

### 🔴 P0-1 — BE `attach_handler` same-cookie idempotent (0046)

**정본**: `docs/reports/0046-be-attach-handler-idempotent.md` — full work package. **0047 §2 의 cold-pickup brief 가 모든 결정 + RED-GREEN-amend 순서**.

**한 줄 요약**: `attach_handler` (`crates/http-api/src/sessions.rs:330`) 의 같은-cookie 같은-session 재attach 가 코멘트 약속과 달리 **409 CONFLICT** 반환. cookie ownership 분기 추가 필요.

**UX critical 영향**:
1. **FE 새로고침 race**: WS close → release_lock 비동기. SPA 의 reattach POST 가 그보다 빠르면 → 409 → `ReconnectModal "in_use"` → 사용자 [Retry] 클릭 필요
2. **plan-0008 Phase 2 silentReattach 의 *모든* 호출 fail**:
   - WS dispatcher 의 `reconnecting → open` 전이 → silentReattach → 409
   - visibility change + heartbeat.isIdle → silentReattach → 409
   - `sessionStore.lastSilentReattachResult = { kind: 'in_use' }` → **mutation guard 가 모든 mutation 진입점 차단**
   - "Session is in use by another webpage" toast (같은 webpage 인데도)
   - **Phase 2 의 silent transparent recovery 의도 완전히 깨짐**

**fix scope** (sessions.rs):

```rust
// line 396 직전에 cookie ownership 분기 추가
{
    let by_cookie = state.session_locks_by_cookie.lock().await;
    if by_cookie.get(&cookie).map(|s| s == &name).unwrap_or(false) {
        drop(by_cookie);
        return reuse_existing_attach_response(&state, wm, &name).await;
    }
}
```

`reuse_existing_attach_response` 헬퍼 추가 — 기존 lock 유지 + layout classification 만 새로 계산 + 200 응답 (`{ attached, name, server_id, matched, unmatched }`).

**진행 순서** (0047 §2.5):
1. RED — 신규 idempotent test 추가 (`attach_idempotent_for_same_cookie_same_session`), 실패 확인
2. GREEN — line 396 직전 cookie 분기 + reuse helper
3. amend — 기존 `attach_409_when_already_held_same_server` → `attach_409_when_held_by_different_cookie` rename + 2 cookie 환경
4. ADR-0019 D3 amend (코멘트가 약속한 동작 land 표기) — *이미 amend ② 로 commit 됨* (`7e52410`)
5. smoke (curl 2회 attach + cargo test --workspace)

**검증 smoke**:
```bash
TOKEN="<magic-link token>"
curl -s -c /tmp/cookies.txt -L "http://127.0.0.1:9999/auth/bootstrap?token=$TOKEN"
curl -s -b /tmp/cookies.txt -X POST -H "Content-Type: application/json" \
  -d '{"ws_conn_id":"t1"}' http://127.0.0.1:9999/api/sessions/<name>/attach \
  -w "\n[%{http_code}]\n"
# 기대 (0046 ship 전): 첫 호출 200, 둘째 호출 409
# 기대 (0046 ship 후): 둘 다 200
```

### 🔴 P0-2 — Plan-0009 `/auth` page FE-bundle pivot (BE side)

**정본**: `docs/plans/0009-auth-page-fe-pivot.md` — full plan. **ADR-0020 D13 (이미 amend land)**.

**한 줄 요약**: BE 의 `/auth` server-rendered `auth_page_handler` 제거 → SPA fallback (index.html) 이 자연 catch → FE main.ts `pickPage` 가 `/auth` 도 `AuthPage` 로 라우팅. FE AuthPage (`routes/auth/+page.svelte`) 는 **production-ready** (login fetch / `?t=` magic-link / rate-limit countdown / theme apply 모두 구현).

**BE blocking** — BE auth.rs / lib.rs 변경 land 전까지 FE pickPage 분기만 해도 `/auth` 접근 시 BE-rendered HTML 우선 → FE bundle 미동작.

**BE 작업 (Slice-D-A1, plan-0009 §2)**:

| 파일 | 변경 |
|---|---|
| `crates/http-api/src/auth.rs` `auth_page_handler` (~line 408) | **함수 제거** (의존 helper 동반 제거 — askama / minijinja / format! macro 정리) |
| `crates/http-api/src/lib.rs` (~line 631) | `.route("/auth", get(auth::auth_page_handler))` line 제거 |
| `crates/http-api/src/lib.rs` `is_auth_path` (~line 679) | `path == "/auth"` 매칭 **유지** (D13 결정 — cookie 없이 도달 허용) |
| SPA fallback 검증 (`lib.rs:541` 끝부분) | `Router::nest_service` / `fallback_service` / `route("/*path", ...)` 중 `/auth` catch 정상 동작 확인. 누락 시 `.fallback_service(ServeDir::new("dist").fallback(ServeFile::new("dist/index.html")))` 추가 |

**테스트 영향**:
- `auth_page_handler` unit test → SPA fallback test 로 변경:
  ```rust
  let res = app.oneshot(Request::get("/auth").body(Body::empty()).unwrap()).await?;
  assert_eq!(res.status(), 200);
  let body = body_to_string(res).await;
  assert!(body.contains(r#"<div id="app">"#));
  ```
- `auth_login_handler` / `auth_logout_handler` / `bootstrap_handler` 영향 없음

**검증 순서** (plan-0009 §2.5):
1. `cargo test -p gtmux-http-api` — handler 제거에 따른 test 1~2 update
2. `cargo build --release` workspace PASS
3. 수동 — `curl -i http://localhost:9527/auth` →
   - status 200
   - Content-Type: text/html
   - body 가 FE bundle index.html (`<script type="module" src="/assets/index-...js">`)

**FE 측 작업** (BE land 후, plan-0009 §3): `main.ts::pickPage` 에 `/auth` 분기 추가 + `routes/auth/+page.svelte` 의 stale "demo only" 주석 정리. 본문 로직 변경 X.

---

## 1. 본 세션 (2026-05-16) 안 ship 누적

| commit | 영역 | 내용 |
|---|---|---|
| `7e52410` | docs | **BE next-session handoff 0047 + ADR-0019/0024 amend** (0045 P0 후속) |
| `c84cae4` | docs | **ADR-0020 D13 + plan-0009** — /auth page FE-bundle pivot |
| `da7663b` | FE | 묶음 E — 0045 refresh reconnect loop P0 후속 |
| `53f11cf` | docs | **ADR-0026** server identity (workspace-derived) + `--session` retirement (Proposed) |
| `682b584` | FE | UI/UX batch 3 — multi-drag commit, selection persist, lasso, layout |
| `21ea4ea` | BE | **legacy `/api/layout` v1 + LayoutStore retire** (Stage 6 cleanup) |
| `752f7c1` | FE | UI/UX batch — inspector text-align, panel ghost, subbar 폐기 외 |
| `92a507b` | BE | **next-2 session-scoped PANE_OUT filter + ADR-0025 Accepted** |
| `6b5fb2e` | FE | ToolbarSubbar — Excalidraw-style floating panel |
| `51f3a86` | BE | **Slice D-5 graceful shutdown + ADR-0014 D12 + WS 0x89** |
| `e583853` | BE | **Slice D-4** sessions import (G28) |
| `e61c4ac` | BE | **Slice D-3** password rotation + logout-all + ADR-0020 D12 |
| `032b83a` | BE | **Slice D-2** file_path open + ADR-0023 amend |
| `349ea2c` | BE | **Slice D-1** settings API + ADR-0025 next-2 draft + Stage 5 smoke |

**누적 통계** (본 세션 안):
- Workspace tests: 329 baseline → 388 PASS (Stage D + next-2 후) → **365 PASS** (legacy `/api/layout` 23 test 의도적 삭제 후)
- Smoke `02_stage5.sh`: **12 gates**, release-binary E2E
- 새 ADR: 0025 (Accepted), 0026 (Proposed)
- ADR amends: 0014 D12, 0017/0018/0021 (FE), 0019 (amend ②), 0020 D11+D12+D13, 0023 ①, 0024 (amend), 0025 ②, 0006 amend ×2 (layout v1 retire)
- 신규 모듈: `settings.rs`, `file_open/` (5 files), `shutdown.rs`, `session_pane_set.rs`
- 삭제: `storage.rs` (legacy LayoutStore)
- WS frame 할당: `0x80~0x89` (10 frames), `0x8A~` unassigned

---

## 2. 프로젝트 핵심 컨텍스트

### 2.1 어휘 핵심 (CONTEXT.md SoT)

- **Server : Workspace = 1:1** — Workspace 가 Server identity 의 canonical source
- **Workspace : Session = 1:N** — Session 은 workspace 안 named layout snapshot record
- **Webpage : Session = 1:1 single-attach** — Webpage = WS 연결 = session 의 편집 채널
- **Terminal pool : Session = N:N** — Terminal 은 server-pool, multi-session mirror (입력 공유)
- **Terminal vs Panel**: Terminal = backend PTY pair + child process. Panel = `type:"terminal"` Canvas Item (시각 객체)
- **Canvas Layout**: 한 Session 의 모든 Item 의 직렬화 — Session file record (`<workspace>/<session-name>.json`) 의 본체

### 2.2 Architectural invariants (CLAUDE.md §"Architectural invariants")

1. **Two state domains** — tmux state vs web state, 절대 섞지 않음
2. **tmux-native vs web-only feature split** — 어느 측이 owner 인지 미리 결정
3. **tmux layout ≠ canvas layout** — 별 개념
4. **Security defaults are not optional** — 127.0.0.1 default, WS auth token + Origin check, untrusted input escape
5. **tmux integration uses control mode** — `tmux -C`, screen-scraping 금지

### 2.3 Project rules (CLAUDE.md)

- **ADR-before-code is a hard rule** — 비-trivial 결정은 `docs/adr/` 에 ADR 선행
- **ADR ↔ plan/handover coherence is a hard rule** — ADR amend 시 모든 linked plan/handover/report 동시 갱신. silent contradiction 차단. (본 세션 안 도입 — CLAUDE.md amend)
- **Language**: 코드/식별자/커밋 = 영어, docs (ADR/plan/report/CONTEXT) = 한국어
- **NEVER commit changes unless the user explicitly asks** — user 명시 요청 시만 commit
- **MCP code-review-graph 우선** — Grep/Glob/Read 전에 graph 도구 사용

### 2.4 BE Slice D + next-2 + Stage 6 cleanup ship 누적 (간략)

- **Slice D-1 Settings API**: `GET/PATCH /api/settings` — 4 section (build / server / behavior / auth), `BehaviorSettings.auto_kill_terminal_on_panel_close` toggle. ADR-0020 D11.
- **Slice D-2 file_path open**: `/api/file-path/*` 5 endpoint — allowlist (ext+prefix tuple, ADR-0023 D2) + check + open + NDJSON audit log. ADR-0023 amend ① (0044 wire 정합).
- **Slice D-3 Auth**: `POST /api/settings/password` + `/logout-all` — Argon2id rehash + caller cookie re-issue + revoke_others. ADR-0020 D12.
- **Slice D-4 Sessions Import (G28)**: `POST /api/sessions/import` — schema v2 validation + 409 conflict + cache seed.
- **Slice D-5 Graceful shutdown (Tier 3)**: `POST /api/shutdown` + WS `0x89 SERVER_SHUTDOWN` + 6-step background task + exit code 6. ADR-0014 D12.
- **next-2 session-scoped PANE_OUT filter**: per-WS owned `HashSet<u64>` + per-WS filter on `pane_output` arm. ADR-0025 Accepted (Proposed → Accepted amend ②). `SessionPaneSetProvider` trait + Hub broadcast `session_change_events`.
- **Legacy `/api/layout` v1 retire**: `LayoutStore`, `LayoutSnapshot`, `AppState.layout/.store`, `with_hub_and_path`, 2 v1 handlers + 4 helper, 23 v1 unit test 모두 제거. `with_hub_and_workspace` 만 잔존.
- **ADR-0026 server identity (Proposed)**: `--session` flag retirement plan. `machine_id = sha256(canonicalize(workspace_path))[..6]` (12 hex) + `${XDG_STATE_HOME}/gtmux/instances/{id}/` directory layout. Phase 1 + Phase 2 단계 진행.

---

## 3. Cold-pickup reading order

| # | 파일 | 목적 |
|---|---|---|
| 1 | 본 문서 §0 + §1 + §2 | 한 줄 + 본 세션 ship + 프로젝트 컨텍스트 핵심 |
| 2 | `docs/reports/0046-be-attach-handler-idempotent.md` | **P0-1** full work package |
| 3 | `docs/plans/0009-auth-page-fe-pivot.md` | **P0-2** full plan |
| 4 | `docs/reports/0047-be-next-session-handover.md` | 직전 BE handover — 0046 P0 정본 + §4 work matrix |
| 5 | `CLAUDE.md` | 프로젝트 룰 (ADR-before-code, coherence rule, language convention) |
| 6 | `CONTEXT.md` | 어휘 SoT — Server/Workspace/Session/Webpage/Terminal/Panel 정의 |
| 7 | `docs/adr/0019-session-and-workspace-model.md` D3 (amend ②) | single-attach invariant — 0046 fix 의 spec 짝 |
| 8 | `docs/adr/0020-auth-lifecycle.md` D13 | `/auth` page FE-bundle pivot 결정 — P0-2 의 정본 |
| 9 | `docs/adr/0025-session-scoped-pane-output-filter.md` (Accepted) | next-2 의 ADR — 본 세션 ship |
| 10 | `docs/adr/0026-server-identity-and-session-flag-retirement.md` (Proposed) | `--session` flag retirement 의 design lock |
| 11 | `docs/reports/0044-be-slice-d-work-package.md` §3 / §8 | Slice D 전체 ship 기록 + endpoint wire 진실 |
| 12 | `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` | FE 측 분석 — 0046 의 motivation context |
| 13 | `docs/reports/0043-fe-integrated-session-handover.md` §1.14 | FE 측 묶음 E (0045 P0 FE 후속 land) |

---

## 4. 빌드 / 검증 / 실행

### 4.1 표준 빌드 / 테스트

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace --color=never
cargo build --release --bin gtmux --color=never

# 테스트 (현 baseline: 365 PASS / 0 FAIL)
cargo test --workspace --no-fail-fast --color=never 2>&1 | grep "test result:"

# clippy
cargo clippy -p gtmux-http-api -p gtmux-ws-server --no-deps --color=never
```

### 4.2 Smoke (release-binary E2E, 12 gates)

```bash
/Users/ws/Desktop/projects/gtmux/codebase/smoke/02_stage5.sh
# 기대: ALL STAGE 5 GATES PASSED (5-1 ~ 5-12)
# Gate 5-12 는 process exit 6 검증 — 마지막에 server kill
```

### 4.3 0046 진행 시 검증

본 문서 §0 P0-1 의 검증 smoke 참조. cookie file (`/tmp/cookies.txt`) 의 2회 attach 흐름.

### 4.4 0009 진행 시 검증

```bash
# BE D13 land 후
curl -i http://localhost:9999/auth
# 기대: 200 + Content-Type: text/html + body 가 index.html
#       (script type="module" src="/assets/index-..." 포함)
```

---

## 5. 유의사항 / 함정

### 5.1 Pre-commit hook 동작

`.git/hooks/pre-commit` 가 `code-review-graph update` 를 실행 + `PostToolUse` hook 이 동일. 결과: 단일 `git commit` 호출이 *staging 을 unstage* 시키는 경우 발견됨 (본 세션 안 D-1 commit 시도 시 재현). **회피 패턴**: 같은 Bash 호출 안에서 `git add ... && git commit` 묶음 — staging + commit 을 atomic 으로.

### 5.2 `wait $SERVER_PID` + `set -e` 충돌

Smoke gate 5-12 의 process exit 6 검증에서 `wait` 의 non-zero return (6) 이 `set -e` 와 충돌. **해결 패턴**: `EXIT_CODE=0; wait $SERVER_PID 2>/dev/null || EXIT_CODE=$?`.

### 5.3 macOS `/tmp` symlink

macOS 의 `/tmp` 는 `/private/tmp` 의 symlink. `canonicalize` 가 `/private/tmp/...` 로 resolve. allowlist / open path 검증 시 입력 path 와 stored canonical prefix 가 다름 — DELETE 등 compound-key match 시점에 client 가 canonical prefix 를 echo back 해야 정합.

### 5.4 `instance` 격리 — XDG_*_HOME 오염

Smoke 가 `~/.config/gtmux/file-open-allowlist.json` 같은 사용자 dir 을 오염시키지 않도록 `02_stage5.sh` 가 per-run `XDG_CONFIG_HOME` / `XDG_STATE_HOME` 을 tempdir 로 isolate. 새 smoke gate 추가 시 동일 패턴 따름.

### 5.5 ADR ↔ plan coherence

CLAUDE.md 의 coherence 룰 — ADR amend 시 grep 으로 linked plan/handover/report 찾아 동시 갱신. 본 세션 안 첫 사례: ADR-0023 amend ① + 0044 §3.5-§3.8 wire 정합. 후속 사례: ADR-0026 + handover 0041 §5.3.1.

### 5.6 미커밋 BE 3 파일 의 history

본 세션 진입 시 작업트리에 BE 3 미커밋 파일 (ws-server/lib.rs log 강등, http-api/schema.rs TextAlign, http-api/lib.rs attach 회귀 테스트) — 이전 BE 세션 의 leftover. 본 세션 안 `349ea2c` 의 attach 테스트 부분 + `65cd120` 의 schema 부분 으로 ship 완료.

### 5.7 WS frame 할당 추적

`0x80~0x89` 사용 중, `0x8A~` 미할당. 신규 frame 추가 시 handover §2.3 의 frame 표 + 관련 ADR amend 필수 (coherence 룰).

### 5.8 `--session` flag 의 향후

ADR-0026 Phase 1 진입 시점에 `--session` → `--name` rename + deprecation warning. Phase 2 (Stage 7+) 에서 compile-time 제거. 본 세션은 ADR draft only — 코드 진입 X.

### 5.9 `gtmux test --no-fail-fast` 의 `gate1_signal_ctrl_c_interrupts_sleep` flake

`crates/pty-backend/tests/integration_pane.rs` 의 gate1 이 workspace 동시 실행 시 PTY signal race 로 간헐적 fail. 단독 재실행 PASS. 본 세션 안 baseline (365 PASS) 은 가끔 +1 flake 가능 — gate1 isolated run 으로 회복.

---

## 6. BE work 매트릭스 (현 상태)

| 영역 | 상태 | 비고 |
|---|---|---|
| Stage 5-A ~ 5-D (모든 sub-stages) | ✅ ship | handover §2.3 frame 표 참조 |
| Slice D-1 ~ D-5 | ✅ ship | 5 commits — 본 §1 |
| next-2 session-scoped PANE_OUT filter | ✅ ship | `92a507b`, ADR-0025 Accepted |
| Legacy `/api/layout` v1 retire | ✅ ship | `21ea4ea`, ADR-0006 amend ×2 |
| ADR-0026 server identity (Proposed) | ✅ draft | `53f11cf`, Phase 1 코드 진입 대기 |
| **🔴 P0-1 — 0046 attach_handler idempotent** | **❌ pending** | refresh / Phase 2 직타격 |
| **🔴 P0-2 — plan-0009 /auth FE pivot (BE side)** | **❌ pending** | auth_page_handler 제거 + route 제거 + SPA fallback 검증 |
| D6 webpage heartbeat 구현 (ADR-0021 D6) | ✅ ship | ws-server `handle_socket` 의 ping_timer + last_pong tracking, hub `HeartbeatTimings` config + setter (2026-05-16 amend ②), cli `_heartbeat_task` → `refresh_lease_for_cookie` wire, integration test 2개 (timeout + pong). ADR-0021 D6.2 ship 정합 amend 짝. |
| Schema v3 item.order field | ❌ pending | FE Layer V2 의 item 정확 위치 enabler |
| Tier 3 — Template CRUD (G36) | ❌ pending | frontend-handover-v3 Stage 4 amend |
| Tier 3 — Token rotation `/auth/rotate` | ❌ pending | ADR-0020, FE Settings Auth section wire |
| ADR-0026 Phase 1 — `--session` rename | ❌ pending | 11 file 변경 + 1-2 일 |
| WS handshake D10 β/γ | — | FE 의존 / β 는 FE only |
| Rate limiter X-Forwarded-For | — | Cloud mode 진입 시 |
| WS subscriber Lagged reconciliation | — | P2+ 우선순위 낮음 |

---

## 7. 진입 시 첫 명령 후보

권장 순서:
1. **P0-1 진입** = "0046 attach_handler same-cookie idempotent fix" → 본 §0 P0-1 의 RED-GREEN-amend 순서. 1-2 시간 작업
2. **P0-2 진입** = "plan-0009 /auth page FE-bundle pivot BE side" → 본 §0 P0-2 + plan-0009 §2. 1 시간 작업
3. P1 진입 (P0 둘 다 ship 후) = "D6 webpage heartbeat 구현" 또는 "ADR-0026 Phase 1 진입"
4. Tier 3 / 새 외부 요구 / bug report 우선 처리

또는 외부 신호 대기 시점 시 P1 / deferred 항목 진입.

---

## 8. 변경 이력

- 2026-05-16: 초안 — Slice D 전체 ship + next-2 ship + ADR-0026 draft + Stage 6 layout v1 retire + 0046 P0 발주 + plan-0009 발주 시점의 session migration handover. 다음 세션이 cold pickup 으로 진입 가능 + 두 P0 항목 (0046 / 0009) 을 우선순위 명시. 0047 handover 보다 *plan-0009 BE side 강조* + *프로젝트 컨텍스트 합본* 의 superset 위치.
- 2026-05-16: P0 + P1 ship 정합 amend. (a) **P0-1 (0046 attach idempotent)** + **P0-2 (plan-0009 /auth FE-bundle pivot BE side)** ship — commit `e9eb9a6` (workspace 362 PASS, release build PASS). (b) **D6 webpage heartbeat — 사실 이미 ship 상태였음**: §6 의 "❌ stub only" claim 이 stale 로 확인. ws-server `handle_socket` 의 ping_timer + last_pong, hub heartbeat_sink + cli wiring + http-api `refresh_lease_for_cookie` 모두 wire 완료. 본 amend 와 짝으로 (i) `Hub::set_heartbeat_timings` setter 추가 (테스트가 ms 단위 override, production default 변경 0), (ii) integration test 2개 추가 (`heartbeat_timeout_closes_1011_and_emits_disconnect` + `heartbeat_pong_reply_emits_heartbeat_sink`), (iii) ADR-0021 D6.2 ship 정합 amend. workspace 362 → 364 PASS.
