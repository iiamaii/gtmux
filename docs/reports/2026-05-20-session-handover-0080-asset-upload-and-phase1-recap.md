# Session Handover — 2026-05-20 — 0080 asset upload (P0) + 0071/0074/0076 land 회고

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-20
> - 생성 session 의 마지막 BE 커밋: `2911c2c` (`feat(be+fe): 0074 Phase 1 — Server boot identity detection (ADR-0020 D15 신규)`)
> - HEAD (handover 작성 시점): `a4d6923` (`Polish canvas component file workflows`)
> - 이번 session 의 주요 주제: 0071 audit cluster 의 BE land (BE-A/B/C/D + RB-A) + 0074 Phase 1 (Server boot identity) 까지 land. 다음 session 의 **최우선 작업은 0080 local asset upload BE endpoint**.

---

## 1. 프로젝트 개요

- **이름**: gtmux — `CLAUDE.md` 의 canonical short name.
- **한 줄 정체성**: tmux/PTY backend + infinite web canvas — single-user web 앱.
- **현재 phase / 단계**: **Stage 7+** — multi-session pivot 완료, 0071 audit cluster land 완료, 다음은 *asset / connector* 영역의 BE 확장 + 0074 Phase 2 의 큰 design 결정.
- **침범 불가능한 invariants**:
  - PTY direct (ADR-0013, control-mode 폐기 — `crates/pty-backend/`). `CLAUDE.md` 의 "tmux control mode" 는 옛 표현, 무시.
  - Webpage : Session = 1:1 + takeover 금지 (ADR-0019 D3/D4).
  - owner_key = `auth_cookie + 0x1f + webpage_id` (ADR-0019 D5.6, 본 session BE-A 가 코드명까지 통일).
  - disk-of-truth ordering (ADR-0006 D13) — broadcast / index 갱신은 disk write 성공 후.
  - attach_index 의 4 mutation hook + boot rebuild + **self-heal at attach** (ADR-0021 D7 amend ③+④).
  - ADR-before-code (CLAUDE.md hard rule).

## 2. 현재 session 요약

이번 session 에서 한 일 (시간 순):

- 0072 BE handover 의 3 task land (BE-A/B/C) — D5.6 owner_key 통일 + `/api/leave` sendBeacon endpoint + boot-time stale lock scan.
- 0072 follow-up BE-D land — `0x86 MOUNT_CASCADE` wire 의 `trigger_session` 동봉 (FE 72278b1 desync trace 짝) + attach_index rebuild_from_disk parse-fail warn log.
- 0075/0076 RB-A land — `AttachReplayEvent` broadcast 신규 (rebind history replay) + 3 integration test.
- 0074 Phase 1 land — `X-Gtmux-Server-Id` header + FE `lib/session/serverId.ts` + page mount mismatch handler.
- ADR amend 짝 동봉 — ADR-0019 D5.6 amend ② / ADR-0021 D6 amend ② / ADR-0021 D3 amend ② / ADR-0021 D8 amend ② / ADR-0020 D15 신규.

### 결정사항

- **handover 인수 시 unstaged diff 통합 정책** — 사용자 명시: "이전 handover 문서와 함께 해결할 수 있거나 혹은 중복되는 작업은 통합해". BE-A 의 unstaged D5.6 wiring + naming refactor + 회귀 10 test fix 를 하나의 logical commit 으로 통합. memory 에 저장: `feedback_handover_integration.md`.
- **mount_cascade race fix design** — server-side `hub.session_for_owner(K)` 필터만으로는 frame 비행 중 owner 가 session switch 하면 부족. wire payload 에 `trigger_session` 동봉 → FE 가 mismatch drop. BE+FE paired commit. 옛 FE 와 새 BE / 옛 BE 와 새 FE 모두 fail-safe (decode null → drop).
- **RB-A 옵션 (a-1) 채택** — 0076 §8 의 session-aware `AttachReplayEvent`. envelope 안 session 동봉 → WS forward 가 `session_pane_set` filter 우회 (ADR-0025 set hot-update timing race 면역). 거절된 대안: 옵션 (b) HTTP endpoint, 옵션 (c) FE force-reconnect, 옛 옵션 (a) plain `(pane_id, bytes)` envelope.
- **0074 Phase 1 = FE detection only** — 사용자 확인. token 을 Webpage 별 쪼개기 거절 (auth domain 책임 오염). BE 변경은 header 하나만 (`X-Gtmux-Server-Id`), 큰 design change (boot capability nonce) 는 Phase 2 로 분리.
- **commit 분리 vs 통합 기준** — anchor 가 worktree unstaged 와 겹치면 통합 (BE-A 패턴), 독립 결함 영역이면 분리 (RB-A 는 self-heal 과 별 commit).

### 변경된 파일 (본 session 의 commit 만)

| Commit | 파일 | 변경 요약 |
|---|---|---|
| `8814b06` | `crates/http-api/src/lib.rs` / `sessions.rs`, `crates/ws-server/src/hub.rs` / `lib.rs`, `bin/gtmux-cli/src/main.rs`, `docs/adr/0019-...md` | D5.6 owner_key wiring + 8 anchor naming refactor (`*_for_cookie` → `*_for_owner`, `SessionChangeEvent.cookie` → `owner_key` 등) + 3 신규 D5.6 integration test + 10 회귀 test 갱신 + ADR D5.6 amend ② |
| `111378c` | `crates/http-api/src/sessions.rs::leave_handler` (신규), `crates/http-api/src/lib.rs` (router + 4 test), `docs/adr/0021-...md` D6 amend ② | `POST /api/leave?webpage_id=<id>` sendBeacon endpoint + 4 integration test (happy / idempotent / 401 / different-webpage-isolation) |
| `df90859` | `crates/http-api/src/session_lock.rs::scan_and_cleanup_stale_locks` (신규) + `crates/http-api/src/lib.rs::with_workspace` hook | boot-time stale `.lock` scan + 2 unit test (unlinks_stale / preserves_held) |
| `abc5931` | `crates/ws-server/src/payload.rs::encode_mount_cascade` (signature 확장), `crates/ws-server/src/lib.rs` (call site + tests), `frontend/src/lib/ws/decode.ts` (MountCascadePayload + decode), `frontend/src/lib/ws/dispatcher.svelte.ts::handleMountCascade` (race guard), `crates/http-api/src/attach_index.rs::rebuild_from_disk` (3 분기 warn log), ADR-0021 D3 amend ② | mount_cascade wire 에 `trigger_session` 동봉 + FE `triggerSession !== sessionStore.active?.name` drop guard + attach_index parse-fail warn |
| `6de30bb` | `crates/ws-server/src/hub.rs::AttachReplayEvent` (신규 struct + cap 16 + publish/subscribe), `crates/ws-server/src/lib.rs::handle_socket` select! arm, `crates/http-api/src/sessions.rs::put_layout_handler` (apply_diff 직후 emit), `crates/http-api/src/lib.rs` 의 F-3 test, ADR-0021 D8 amend ② | rebind history replay broadcast + 3 신규 integration test (F-1 forwarding / F-2 owner-scoped / F-3 drag idempotency) |
| `2911c2c` | `crates/http-api/src/sessions.rs::list_handler` (header), `crates/http-api/src/lib.rs` (test), `frontend/src/lib/session/serverId.ts` (신규), `frontend/src/lib/http/sessions.ts` (hook 2), `frontend/src/routes/+page.svelte` (handler register + onDestroy detach + raw fetch hook), ADR-0020 D15 신규, `docs/reports/0074-...md` §10 | 0074 Phase 1 — `X-Gtmux-Server-Id` header + FE detection + mismatch handler (sessionStore.clear + reconnectGate.cancel + workspaceSwitcher.open + warning toast) |

**Test baseline** (시간 순):
- 본 session 시작: 416 PASS / 0 FAIL
- BE-A 후: 419 PASS (+3 D5.6 test)
- BE-B 후: 423 PASS (+4 leave test)
- BE-C 후: 425 PASS (+2 stale scan test)
- BE-D 후: 425 PASS (기존 test 갱신만)
- RB-A 후: 428 PASS (+3 attach_replay test)
- Phase 1 후: 429 PASS (+1 server_id header test)

미커밋 변경:

- 미커밋: **있음 (본 session 무관)**
  - `D ref/frontend-design/components-v5` (worktree 에서 dir 삭제, ref/ 영역)
  - `?? ref/frontend-design/components-v5.html` / `components-v6.html` (untracked, ref/ 영역)
  - 본 BE session 의 work 와 무관. parallel worker 가 ref 디자인 자료 정리 중.

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
| ---- | ---- | --------------- |
| 프로젝트 instructions | `CLAUDE.md` | 컨벤션·invariants. *주의*: §"Architectural invariants" 5 의 "control mode" 는 옛 표현 — PTY 직접 (ADR-0013) 이 진실. |
| 스펙 | `docs/sketch.md` (한글) | scope · MVP · 우선순위 |
| **최우선 작업** | **`docs/reports/0080-be-handover-local-asset-upload.md`** | **§4.1 의 다음 step 의 정본 — 본 handover 의 §4.1 도 같이 읽기** |
| 본 session 의 직전 audit cluster | `docs/reports/0071-...md` / `0072-...md` / `0073-...md` / `0075-...md` / `0076-...md` / `0077-...md` | BE land 의 결정 출처 모음 |
| 0074 Phase 2 의존 | `docs/reports/0074-webpage-auth-epoch-and-stale-tab.md` §10 (본 session land) + §4.2 / §7.2 | Phase 2 = BE boot capability — 큰 design change |
| Connector ADR | `docs/adr/0036-canvas-component-connector.md` + `docs/adr/0018-canvas-item-data-model.md` D12 amend (`b81d8a6`) | 0078/0079 handover 의 결정 출처 |
| Connector BE/FE handover | `docs/reports/0078-be-handover-connector.md` / `0079-fe-handover-connector.md` | 0080 land 후 다음 batch |
| 활성 plan | `docs/plans/0011-component-design-batch-caption-document.md` | FE-side, 본 BE session 과 일부 정합 (캡션/문서 노드) |
| 직전 handover | `docs/reports/2026-05-18-session-handover-state-machines-sot-and-adr-amends.md` | 본 session 이전의 SSoT 정리 맥락 |

## 4. 진행중인 작업

본 session 의 BE land 사이클 (0071 audit cluster) 은 *모두 closed*. 새 session 의 진입 대상은 *최우선 0080* + 이후 0078 connector (BE 측).

### 4.1 **[P0]** 0080 — local file asset upload (BE endpoint)

- **상태**: BE handover 문서 완비 (`0080-be-handover-local-asset-upload.md`). FE 측 contract (`localFilePicker.ts` / `assets.ts` / `ImageNode` / `DocumentNode` / `FilePathNode`) 는 *이미 BE 응답 기대 형태로* 구현 — 본 endpoint 가 land 되어야 image/document 도구의 ship 가능.
- **관련 문서**: `docs/reports/0080-be-handover-local-asset-upload.md` (§1 FE 계약 / §2 BE API spec / §3 검증 규칙 / §5 acceptance checklist 15 항목)
- **관련 파일·코드** (BE 신규):
  - `crates/http-api/src/assets.rs` (신규 module 권장) — multipart parse + sha256 + content-addressed store
  - `crates/http-api/src/lib.rs` — `.route("/api/assets", post(assets::upload_handler))` + `.route("/api/assets/{id}", get(assets::serve_handler))` 등록, `AppState` 에 asset store path 추가
  - `crates/http-api/src/workspace.rs` — `assets_dir()` helper 추가 (XDG state 영역 의 `assets/`)
- **다음 한 step**: `0080` §2.1 의 `POST /api/assets` shape 그대로 신규 module `assets.rs` 시작. 첫 commit scope:
  1. multipart parse (axum `extract::Multipart`) + `kind: "image"|"document"` 검증
  2. sha256 content hash → `asset_id` (64 char hex)
  3. allowlist MIME (image: png/jpeg/gif/webp/svg; document: text/* + json + pdf) + magic byte sniff (예: `infer` crate 또는 inline 패턴)
  4. size cap 20 MB (image+document 공통) → 413
  5. `<workspace>/.assets/<sha256>` 으로 atomic write (rename pattern)
  6. response `{ asset_id, mime, file_name, size_bytes, original_w?, original_h? }` — image 면 `image` crate 또는 가벼운 PNG/JPEG header parser 로 dimensions 추출
  7. cookie auth middleware 통과 (기존 `/api/*` 와 동일)
  8. `GET /api/assets/{id}` — 64-char hex regex validate → file read → `Content-Type: <stored mime>` + body
  9. integration test 5 종 (§5 의 *integration test* 항목 그대로): roundtrip / oversize / invalid asset_id / unauthorized / idempotent same-bytes
- **선행 조건**: 없음 (BE-only, 기존 owner-attach guard 와 분리된 새 endpoint). FE 측은 이미 mocked / placeholder 상태.

### 4.2 0078/0079 — Component Connector (BE schema validation + FE rendering)

- **상태**: ADR-0036 Accepted (`b81d8a6` commit) + ADR-0018 D12 amend land. BE/FE handover 작성 완료, 코드는 미land.
- **관련 문서**: `docs/reports/0078-be-handover-connector.md` (Self-grilling 9+ Q, anchor, AC, integration test 명세) / `docs/reports/0079-fe-handover-connector.md`
- **관련 파일·코드** (BE):
  - `crates/http-api/src/schema.rs` — `Item::Connector { common, from_id, to_id, style? }` variant 추가 + `validate()` arm (id_index O(N) build + endpoint refer-무결성 + self-loop reject + connector-of-connector reject)
  - `crates/http-api/src/openapi.rs` (있다면) — schema 노출
- **다음 한 step**: 0080 land 후 0078 §B-1/B-2 의 schema variant + validate arm 부터.
- **선행 조건**: 0080 와 독립 진행 가능. 단 commit 분리 — schema 영역 변경은 ADR-0018 의 무거운 영역이라 별 commit.

### 4.3 0074 Phase 2 — BE `webpage_boot_nonce` + write-sensitive guard

- **상태**: Phase 1 (FE detection) land 완료 (commit `2911c2c`). Phase 2 (BE 강제) 는 사용자 결정 + ADR-0020 amend draft 필요.
- **관련 문서**: `docs/reports/0074-webpage-auth-epoch-and-stale-tab.md` §4.2 / §4.3 / §7.2
- **관련 파일·코드** (예상 진입):
  - `crates/http-api/src/lib.rs` — `AppState::webpage_boots: HashMap<WebpageBootNonce, BootCapability>` 추가
  - `crates/http-api/src/auth.rs` 또는 새 `bootstrap.rs` — `GET /api/bootstrap` 신규 (cookie + webpage_id → nonce 발급)
  - write-sensitive endpoint middleware — `attach_handler` / `attach_confirm_handler` / `create_terminal_handler` / `put_layout_handler` / `delete_item_handler` / `detach_handler` / `respawn_handler` / `kill_handler` / WS upgrade 에 nonce 검증
- **다음 한 step**: 사용자 grill — Phase 2 의 *읽기 endpoint 도 가드 할지* (0074 §8 Open Q1) + *WS upgrade nonce 전달 방식* (query vs subprotocol, §8 Open Q2) 결정 후 ADR-0020 amend draft. 본 BE session 에서 직접 진행은 *큰 design change* 라 별 cycle 권장.
- **선행 조건**: 사용자 결정. FE detection (Phase 1) 만으로 *체감 desync* 차단은 이미 완료된 상태라 *시급도 ↓*.

### 4.4 0074 Phase 3 — logout / rotate 시 nonce 제거

- **상태**: Phase 2 의존. Phase 2 의 `webpage_boots` store land 후 작업 시작.
- **관련 문서**: `docs/reports/0074-...md` §7.3
- **다음 한 step**: Phase 2 land 까지 대기.

### 4.5 (verify-only) FE-D — AttachConfirmModal cancel chain 8s warning toast 실 출력

- **상태**: 0073 §E 의 manual E2E verify. 코드 변경 가능성 낮음 — *시연 후* 회귀 여부만 확인.
- **관련 문서**: `docs/reports/0073-fe-handover-from-0071-audit.md` §E (FE-D verify)
- **다음 한 step**: BE+FE 같이 띄워 시나리오 실행 — `WorkspaceSwitcher` 의 `cancelAttachConfirm` chain step 4 (failure fallback) 의 8s warning toast 가 실제로 보이는지 확인.

### 4.6 (verify-only) FE-E — rebind history replay 부재 시연

- **상태**: **0076 land 로 fix 됨** (commit `6de30bb` RB-A). 본 verify task 는 *반대로 진행* — 사용자 시연 후 history 가 정상 표시되는지 확인 (회귀 가드).
- **관련 문서**: `docs/reports/0076-rebind-history-replay-missing.md` + `0073-...md` §F (FE-E verify)
- **다음 한 step**: dev 환경에서 [Attach to this session] 흐름 시연 — α session 에서 `echo HELLO` 실행 → β session 으로 같은 terminal mount → β xterm 에 HELLO 즉시 표시되는지 확인.

## 5. 향후 작업

### 5.1 ref/frontend-design 영역의 worktree 변경 정리 (외부 worker)

- **목표**: `D ref/frontend-design/components-v5` + untracked `components-v5.html` / `components-v6.html` 정리. 본 BE session 무관.
- **관련 문서**: ref/frontend-design/ 하위 자료
- **선행 조건**: parallel FE worker 결정.
- **예상 진입 지점**: `git diff -- ref/frontend-design/` 로 의도 확인 → parallel worker 가 자체 commit.

### 5.2 (P2 cosmetic) 0053 §7 RFC3339 통일

- **목표**: `POST /api/sessions/import` 의 `created_at` (u64 unix_secs) ↔ Export envelope `exported_at` (RFC3339 string) wire 형식 통일.
- **관련 문서**: `docs/reports/0053-be-verification-checklist.md` §7
- **선행 조건**: FE wire 의 타입 변경 영향 — FE 와 짝 commit 필요.

### 5.3 (P2) ADR-0034 D2 directory carve-out

- **목표**: file-stat 의 `kind: "directory"` 응답이 현 시점 403 (allowlist 의 ext 매칭과 모순). 별 처리 정책 결정.
- **관련 문서**: `docs/reports/0060-be-file-stat-work-package.md` §3 amend ② note + `docs/adr/0034-file-stat-endpoint.md` D2
- **선행 조건**: ADR-0023 allowlist 의 (ext, prefix) 모델에 directory probe 의 ext-less mode 추가 또는 별 endpoint 분리 결정.

### 5.4 (P2) BE-5(b) — `session_pane_set` provider 의 `attach_index` 활용

- **목표**: `crates/http-api/src/session_pane_set.rs` 의 layout-read 패턴을 attach_index 의 *session→uuids* 역방향 map 으로 단축.
- **관련 문서**: `docs/reports/0068-be-attach-index-work-package.md` §1.4
- **선행 조건**: 진입 전 *필요성 측정* — `hub.session_for_owner` 호출 빈도 × 평균 layout 크기 가 lock 비용 의미 있게 만드는지 확인.

## 6. 주의사항 / Gotchas

- **handover 인수 시 unstaged diff 통합** (memory `feedback_handover_integration.md`): worktree 의 unstaged 변경이 handover task 의 anchor 와 겹치면 별 commit 으로 분리하지 말고 통합. BE-A 가 그 패턴. 단 parallel worker 의 *다른 file* 은 통합 대상 아님.
- **release binary stale trap (0077 교훈)**: source 의 fix 만으로 사용자 시연 환경이 즉시 갱신되는 게 아님. *반드시* `cargo build --release` 후 binary mtime 확인. 본 session 의 모든 commit 도 release build 검증 포함.
- **owner_key 명명 강제** (ADR-0019 D5.6 amend ②): `*_for_cookie` / `*_by_cookie` / `cookie_value` 같은 이름 절대 추가 금지. 새 코드 작성 시 owner_key / *_for_owner / *_by_owner 패턴 사용. 진짜 cookie 영역만 `auth_cookie` / `cookie_value` 유지 (`CookieValidator::validate`, `extract_cookie_value`).
- **attach_index self-heal 의 위치 load-bearing** (`a276058` + ADR-0021 D7 amend ④): `classify_layout_terminals` + `attach_confirm_handler` 의 200 응답 직전. 이 위치를 옮기면 stale tab race 회복 보장 깨짐. 본 session 의 RB-A (`6de30bb`) 도 self-heal 위치는 그대로 보존.
- **`AttachReplayEvent` 의 race-immune routing** (ADR-0021 D8 amend ②): WS handler 의 새 arm 이 *반드시* envelope.session 매칭만 사용. `session_pane_set` filter 적용하면 ADR-0025 의 set hot-update timing race 가 발생 — 본 session anti-pattern #1.
- **0074 Phase 1 의 mismatch handler 가 onMount 에 등록되어야** — listSessions 호출 *전에* `onServerIdMismatch` 가 wire 되지 않으면 첫 mismatch 발화 시 cleanup 누락. 본 session `+page.svelte` 의 onMount 첫 라인에 등록. 다른 page 가 listSessions 호출하기 전에 동일 패턴 필요.
- **`/api/leave` 와 `DELETE /attach` 의 의미 차이**: 같은 함수 (`release_lock_for_owner`) 호출하지만 wire 는 다름. `/api/leave` = sendBeacon 의 page-unload best-effort, `DELETE /attach` = 명시 user-action reliable channel. document 작성 시 둘을 *별 endpoint* 로 표기.
- **CLAUDE.md 의 "tmux control mode" 옛 표현**: ADR-0013 cutover 이후 PTY 직접. handover/SSoT 의 일부 문서가 옛 표현 유지. 다음 session 이 control mode 코드를 찾지 말 것 — `crates/pty-backend/` 가 진실.
- **parallel worker commit sweep**: 본 session 내내 FE worker 가 동시 marathon-ship. `git commit` 직전 `git status` 로 staged 영역 확인 필수. 본 session 도 state-machines.md / dispatcher.svelte.ts 의 worker 분 unstaged 를 `git restore --staged` 로 격리한 패턴 사용.
- **0080 의 SVG serve 정책 (§7 후속 결정)**: SVG 는 active content (XSS) 표면. `Content-Disposition: attachment` 강제 또는 sanitize 결정 필요. 본 endpoint 설계 시 사용자 grill 권장.

## 7. 새 session 시작 방법

1. 이 handover 문서를 끝까지 읽는다.
2. **`docs/reports/0080-be-handover-local-asset-upload.md` 를 읽는다** — §4.1 의 정본.
3. `CLAUDE.md` (+ `docs/sketch.md`) 를 읽는다. *주의*: "tmux control mode" 옛 표현 무시.
4. §4.1 의 **다음 한 step** 부터 진행:
   - 신규 `crates/http-api/src/assets.rs` 작성 시작
   - 0080 §5 의 acceptance checklist 15 항목 따라 commit scope 분할
5. handover 작성 이후 변경 확인: `git log --oneline a4d6923..HEAD` (본 handover HEAD 이후 신규 commit).
6. **release build 필수** — `cargo build --release --bin gtmux` 후 사용자 시연 환경 갱신 (0077 교훈).

만약 §4 의 0080 진행이 막히면 §4.2 connector → §5 의 P2 항목으로 이동.

### 검증 baseline (본 session 종료 시점, 마지막 BE 커밋 `2911c2c`)

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -3
# 기대: 429 PASS / 0 FAIL

cargo build --release --bin gtmux --color=never
# 기대: PASS

cd /Users/ws/Desktop/projects/gtmux/codebase/frontend
pnpm check
# 기대: 317 files / 0 errors / 0 warnings
pnpm build
# 기대: OK
```

신규 test 누적 (본 BE session): +13
- BE-A: +3 (same-cookie / detach-scoped / list-session-disable)
- BE-B: +4 (leave happy / idempotent / 401 / different-webpage)
- BE-C: +2 (stale unlink / preserves held)
- BE-D: +0 (기존 mount_cascade test 갱신만)
- RB-A: +3 (replay forwarding / owner-scoped / drag idempotency)
- Phase 1: +1 (server_id header)

---

_생성: `session-handover` skill v1_
