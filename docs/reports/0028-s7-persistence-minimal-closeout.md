# Closeout — S7-PERSISTENCE-MINIMAL (Sprint 7, 2026-05-15)

본 문서는 `0027-session-resume-handoff.md` §4.1 dispatch prompt 의 6 작업 항목을 모두 충족한 직후 시점의 *task closeout* 이다. 0027 은 Sprint 7 전체 한드오프, 본 문서는 그 안의 한 task 종결 보고.

ADR-0006 (plain JSON + atomic-write-file + sidecar quarantine) 은 *2026-05-14 Accepted* 상태로 이미 잠겨 있었고 본 task (2026-05-15) 는 그 ADR 의 D1~D13 을 *코드 진입* 시킨 작업. 신규 ADR 없음.

---

## 1. 한 줄 상태

- **시작 시점**: Canvas Layout 이 `RwLock<LayoutSnapshot>` 의 *in-memory 보관* 에 머무름 (http-api/lib.rs `AppState`).
- **종료 시점**: Server 재기동 후에도 layout 보존 + ADR-0006 D10 7-state 표 정합 + PUT 의 disk-first invariant + 손상 파일 sidecar quarantine.
- **빌드/테스트**: `cargo test --workspace --tests` **164 PASS / 0 FAIL** (baseline 150 → +14). clippy clean, fmt clean, svelte-check 0/0.

---

## 2. 변경 요약 (+361 / -9 LOC, 5 files)

| 파일 | 변경 |
|---|---|
| `codebase/backend/Cargo.toml` | `atomic-write-file = "0.3"` workspace dep 추가 (ADR-0011 D7 amend 정합) |
| `codebase/backend/crates/http-api/Cargo.toml` | atomic-write-file + tempfile (dev-dep) |
| **`codebase/backend/crates/http-api/src/storage.rs`** (신규, ~330 LOC) | `LayoutStore::{new, path, load, save}` + ADR-0006 D10 7-state 표 정본 구현 + sidecar quarantine + 0600/0700 perm audit + 9 단위 테스트 |
| `codebase/backend/crates/http-api/src/lib.rs` | `mod storage` + `pub use {LayoutStore, StorageError}`, `AppState.store: Option<Arc<LayoutStore>>` 필드 추가, `AppState::with_hub_and_path` 생성자 (boot-time D10 load 동반), `layout_put_handler` 의 D13 5-step (validate → new ETag → atomic disk write → memory swap → LAYOUT_CHANGED broadcast), 4 통합 테스트 |
| `codebase/backend/bin/gtmux-cli/src/main.rs` | `start()` 에서 `state_files::layout_path_for(session)` 호출 → `build_app()` 가 path 를 받아 `AppState::with_hub_and_path` 로 인계 |

---

## 3. ADR-0006 D 정합 매트릭스

| D | 결정 | 구현 위치 |
|---|---|---|
| D1 | plain JSON 단일 파일 | `storage.rs::LayoutStore::save` + `lib.rs::canonical_serialize` |
| D2 | 경로 = `${XDG_STATE_HOME}/gtmux/<session>.layout.json` | `bin/gtmux-cli/src/main.rs::start()` → `state_files::layout_path_for(session)` (state_files 가 이미 ADR-0014 의 XDG 규약 보유) |
| D3 | atomic-write-file crate (5-step: fsync + rename + dir fsync) | `storage.rs::save` — `AwfOpenOptions::new().mode(0o600).preserve_mode(false).open(path)` + `write_all` + `commit()` |
| D4 | schema_version=1 고정, deny_unknown_fields 정신 | `storage.rs::load` 에서 schema_version 미존재/!=1 → quarantine. (현 lib.rs 는 `serde_json::Value` 로 처리 — strict serde struct 는 gtmux-canvas-layout crate 도입 시) |
| D5 | ETag = SHA256-128, 32-hex lowercase | 변경 없음 — `lib.rs::compute_etag` 가 이미 정본 |
| D6 | 검증 통과 *후* 디스크 진입 | `layout_put_handler`: minimal_layout_check 통과 → `LayoutSnapshot::from_body` → `store.save(canonical_bytes)` → memory swap |
| D7 | MVP schema_version=1, 변환 함수 0개 | 그대로 — v2 도입 시 본 문서가 supersession 트리거 |
| D8 | pid 파일 advisory lock 으로 프로세스 단일성 | 변경 없음 — `state_files::write_pidfile` 가 이미 보유 (ADR-0014) |
| D9 | 클라이언트 race = ETag 412 | 변경 없음 — `layout_put_handler` 412 + current ETag echo |
| D10 | 손상 파일 7-state 표 + sidecar quarantine | **`storage.rs::load`** 가 정본 — `<file>.corrupt-<unix_ts>` 로 rename, 빈 layout 부팅, WARN 또는 ERROR 로그. 7 상태 모두 단위 테스트 |
| D11 | 파일 0600, 부모 dir 0700 | `storage.rs::save` 의 `mode(0o600) + preserve_mode(false)` + `ensure_dir_0700` + load-time `audit_perm` (chmod 0600 if drift) |
| D12 | 페이로드 cap 256 KiB | 변경 없음 — `PUT_MAX_BYTES = 256 * 1024` 가 이미 layout_put_handler 에 존재 |
| D13 | PUT 성공 5-step atomic CAS | `layout_put_handler`: write lock 안에서 (a) ETag match → (b) `LayoutSnapshot::from_body` (new ETag) → **(c) `store.save(body_bytes)`** → (d) `*snap = new_snap` → (e) `hub.publish_layout_changed(new_etag)`. disk write 실패 시 **500 + current ETag echo + memory 무변경** (D6 fail-closed 정합) |

---

## 4. 테스트 매트릭스

### 4.1 `storage::tests` (단위, 9건)

| 테스트 | 검증 항목 |
|---|---|
| `load_absent_returns_empty` | D10 row 1 — 파일 부재 시 빈 snapshot |
| `save_then_load_round_trip` | save 직후 load 가 동일 ETag + body, 파일 0600 |
| `load_zero_bytes_quarantines` | D10 row 3 — 0 바이트 → sidecar |
| `load_parse_fail_quarantines` | D10 row 4 — JSON parse 실패 → sidecar |
| `load_unsupported_schema_version_quarantines` | D10 row 5 (변종 A) — schema_version=99 → sidecar |
| `load_missing_schema_version_quarantines` | D10 row 5 (변종 B) — 필드 부재 → sidecar |
| `load_schema_rule_violation_quarantines` | D10 row 6 — `groups: "not array"` → sidecar |
| `load_bad_perm_chmod_then_loads` | D10 row 7 — 0644 file → chmod 0600 + 정상 load (sidecar 없음) |
| `save_creates_parent_dir_0700` | D11 — 부모 dir 자동 생성 + 0700 |
| `save_is_atomic_no_tmp_left_behind` | D3 — commit 후 tmp sidecar 0개 (atomic-write-file 의 unnamed-tmpfile / rename guarantee) |

### 4.2 `tests` (통합, 4건 신규)

| 테스트 | 검증 항목 |
|---|---|
| `layout_put_persists_to_disk` | PUT 204 직후 디스크 file 존재 + 0600 + body 일치 |
| `layout_put_412_leaves_disk_unchanged` | 412 응답 후 디스크 bytes 무변경 (race 검출의 fail-closed) |
| `boot_after_put_reloads_same_etag` | PUT → 새 AppState 구성 (재기동 simulation) → 동일 ETag 응답 (D13 → D10 row 2 정합) |
| `boot_with_corrupt_file_quarantines_and_serves_empty` | router-level smoke — 손상 file 위에서 server 가 정상 부팅 + 빈 layout 응답 + sidecar 존재 |

---

## 5. Carry-forward / Sprint 7 잔여 작업

0027 §4 의 나머지 두 항목:

### 5.1 S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-BE-AUTOMOUNT (2-3일)

`CONTEXT.md` §"Pane lifecycle invariant 의 UI 측 mirror" 정합:

- **S7-FE-SHUTDOWN**: 우상단 헤더 메뉴 + Session shutdown action + confirm modal. backend 의 `BackendCommand::KillSession` variant 추가 필요 (ADR-0013 D10 amend 필요). graceful exit 6.
- **S7-FE-CLOSE-GUARD**: PanelNode close 버튼 비활성화 (살아 있는 child=1 일 때) + tooltip "마지막 Pane 은 close 할 수 없습니다 — Session shutdown 메뉴 사용".
- **S7-BE-AUTOMOUNT**: `pane-spawned` NOTIFY 시 frontend 가 layout PUT cascade 자동 추가. 새 ADR 필요 여부는 grilling 필요 (frontend vs backend 책임 경계).

### 5.2 S7-DEMO-STAB (3-7일, closeout)

`docs/sketch.md` §15 2단계 demo 를 *새 backend + 영속화 위에서* 첫 회 구동.
- Sprint 5 17건 결함 부류 회귀 (0020 매트릭스)
- PTY backend multiplex pane_output broadcast backpressure 실측 (50 pane × 5 burst)
- portable-pty production 함정 검증
- **새**: layout 영속화의 atomic write race + sidecar quarantine 실 환경 동작 확인

---

## 6. Open questions / risks

### 6.1 Open

- **O1** (0027 carry-forward): `BackendNotify::ServerReady` 의 emit 시점 미정 — 본 task 는 영속화 한정이라 unaffected. S7-FE-SHUTDOWN 시점 결정.
- **O2** (신규): 50-panel scale 에서 PUT-and-flush latency 실측 — ADR-0006 O1. 현 macOS APFS 에서 `F_FULLFSYNC` 추가 인가 여부는 S7-DEMO-STAB 의 R7 benchmark 후 결정.
- **O3** (신규): Server stopped 중 사용자 직접 파일 편집 시 mode != 0600 audit 로그가 부팅 메시지에 노출되는데, R6 §F10 의 "Server stopped 상태에서만 안전 편집" README/banner 안내 추가 여부는 S7-FE-SHUTDOWN 의 UX 정책 결정 시 동반 처리 권장.

### 6.2 Risks (S7-DEMO-STAB 시점에 부각될 수 있는 신규 부류)

| 카테고리 | risk | 완화 |
|---|---|---|
| Persistence latency | 50-panel × frequent PUT (drag, resize, focus 변경) 의 atomic write disk pressure | debounce — 본 task scope 밖 (frontend 의 PUT 발생 빈도 정책). S7-DEMO-STAB 의 R7 측정 |
| Concurrent edit | 사용자가 multi-tab 으로 동시 편집 시 ETag 412 빈도 | 정상 동작 — D9 cascade. 정량 측정 필요 |
| File mode drift | 외부 도구 (백업, 동기화 도구) 가 mode 를 0644 로 변경 시 부팅마다 WARN | 의도된 동작 (D10 row 7) — 무해 |

---

## 7. Memory / Docs 정합

- **메모리 변경 없음** — `project_gtmux.md` 의 "PTY direct backend" 줄거리 유지, 영속화는 ADR-0006 + 본 closeout 으로 자명. derived from code/docs 영역.
- **ADR 변경 없음** — ADR-0006 은 이미 Accepted 2026-05-14, D1~D13 은 *문언 그대로* 코드 진입.
- **CONTEXT.md 변경 없음** — durable vs ephemeral 분리 (§"Transport split") 정신 유지.
- **본 closeout 의 위치**: `docs/reports/0028-s7-persistence-minimal-closeout.md` — 0027 핸드오프의 자식 (single-task closeout).

---

## 8. 다음 세션 픽업 안내

새 세션이 본 closeout 을 진입점으로 받았을 때:

| 사용자 메시지 | 행동 |
|---|---|
| "S7-FE-SHUTDOWN 진행" | §5.1 — backend `BackendCommand::KillSession` ADR-0013 D10 amend 결정 (grilling 필요) → frontend 헤더 메뉴 + confirm modal |
| "S7-FE-CLOSE-GUARD 진행" | §5.1 — PanelNode 의 close 버튼 비활성화 로직, `muxStore.panes.filter(!dead).length === 1` |
| "S7-BE-AUTOMOUNT 진행" | §5.1 — frontend 의 `pane-spawned` 수신 시 layout PUT cascade 자동화. ADR 결정 필요 |
| "S7-DEMO-STAB 진행" | §5.2 — sketch §15 2단계 demo + Sprint 5 17건 회귀 + 영속화 race/quarantine 실측 |
| "현 상태 점검" | `cargo test --workspace --tests` + clippy + fmt + svelte-check 재실행 |
| "영속화가 어떻게 동작해?" | 본 §3 + §4 + ADR-0006 D10 표 |

---

## 변경 이력

- 2026-05-15: 초안 — S7-PERSISTENCE-MINIMAL 완료 직후, S7-FE-SHUTDOWN / FE-CLOSE-GUARD / BE-AUTOMOUNT / DEMO-STAB 진입 전 시점.
