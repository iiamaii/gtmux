# Sprint 5 진척 + 데모 시연 가능 상태 — 2026-05-14

본 문서는 `0017-progress-status.md`의 후속이다. Sprint 4까지의 정착 위에 **Sprint 5-A / 5-B (FE-NEW-PANEL + FE-MUX-MIRROR) / 5-B 추가(Sidebar V0)** 가 모두 main 브랜치에 합류했고, 데모 시연 도중 발견된 두 차단성 결함(`--port` 무시 / `localhost` 403)을 수정해 **사용자가 브라우저에서 실제로 사용 가능한 수준의 데모**가 가능해진 직후 상태를 캡처한다.

## TL;DR

- **현 단계**: sketch §15 **2단계 진입의 첫 박스**(create UX) 통과. backend 184 → **190** 단위 PASS / 5 ignored, frontend svelte-check 221 → **224** files 0/0, main bundle 7.21 → **10.94 KB gzip**, smoke 9-step **8 PASS / 1 N/A / 0 FAIL** 유지.
- **추가된 데모-가능 능력**: ① 캔버스 좌상단 **[New Panel]** 버튼으로 tmux window 생성 + 패널 자동 마운트, ② 좌측 **280 px Layers 사이드바**(read-only, group/panel tree + 가시성/잠금/dead 표시), ③ `localhost:9999` / `127.0.0.1:9999` 둘 다 same-origin으로 통과.
- **차단성 갭 0건**. 잔여 운영 갭은 `§ 6 잔여 갭` 표 참고.
- **다음 단계**: sketch §15 **2단계 마무리** — select/close 라운드 트립 보강 + group reparent + (선택) §15 3단계 entry로 ADR-0006 implement.

## 1. 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타
2. `CONTEXT.md` — 도메인 어휘 + 5대 불변식
3. **본 문서 (`0019-progress-status.md`)** — 현 진척 + 데모 절차 + fix 2건
4. `docs/reports/0018-session-handoff.md` — Sprint 5 진입 직전 핸드오프 (§7~§8 task 분해 + §9 누적 함정)
5. `docs/reports/0017-progress-status.md` — Sprint 4 closeout 스냅샷 (Demo §2, G1~G6 정의)
6. `docs/sketch.md` §12·§15 — 우선순위·단계 정의
7. `docs/adr/0001~0012` — 12개 Accepted
8. `codebase/smoke/01_engine_connect.sh` — 9-step 회귀 게이트

## 2. 누적 진행 매트릭스

| Phase | 상태 | 산출 commit |
|---|---|---|
| Sprint 0~4 (P0 백엔드·프론트·정합·SPA) | ✅ | …`b4900ad` (smoke 9/9) |
| Sprint 4 closeout (0017 + 0018) | ✅ | `6c81b26` |
| **Sprint 5-A** (cors default 합성 + `Command::ResizeWindow` 변형) | ✅ | `50bad9c` |
| **Sprint 5-B** (FE NewPanel + ctrl-registry + mux mirror store) | ✅ | `50bad9c` (5-A와 묶음) |
| **Sprint 5-B 추가** (Sidebar V0 read-only) | ✅ | `7a5c873` |
| **Sprint 5 hotfix** (`--port` figment 정합 + cors loopback alias) | ✅ | `da5c221` |
| sketch §15 **2단계 첫 박스 (create)** | ✅ | `da5c221` 시점 |
| sketch §15 **2단계 마무리** (select/close/group reparent) | ⏳ 다음 | — |
| sketch §15 **3단계** (영속화 정착, ADR-0006 implement) | ⏳ 선택 | — |

## 3. 본 세션 산출 — 변경/추가 단위 정리

### 3.1 Sprint 5-A — backend quick wins (`50bad9c`)

| 파일 | 변경 |
|---|---|
| `crates/config/src/lib.rs` | `effective_cors_origins()` 헬퍼 추가. 빈 `cors_origins`이면 `http://<bind>:<port>` 합성 |
| `crates/http-api/src/lib.rs` | `origin_check_middleware` 가 위 헬퍼 사용 |
| `crates/mux-router/src/lib.rs` | `Command::ResizeWindow { window_id, cols, rows }` 정식 변형 |
| `crates/ws-server/src/cmd_router.rs` | `build_pane_resize_request` 가 `ResizeWindow` 직접 발급 (legacy `ListWindows` + keyword override 제거) |
| `crates/lifecycle/src/lib.rs` | `serialise_command` 가 `ResizeWindow → "resize-window -t @<id> -x <cols> -y <rows>"` 직렬화 |

테스트: 184 → 188 PASS (+4).

### 3.2 Sprint 5-B — frontend New-Panel UX + mux mirror (`50bad9c`)

| 파일 | 역할 |
|---|---|
| `lib/stores/mux.svelte.ts` (신규) | windows / panes / session 미러 store. 8개 메서드 (addWindow / renameWindow / closeWindow / setSession / setLayout / setPaneMode / killPane / addPane) |
| `lib/ws/ctrl-registry.ts` (신규) | CTRL request/response 상관기. UUID-v4 id + per-request timeout |
| `lib/canvas/NewPanelButton.svelte` (신규) | 툴바 버튼. 클릭 → `encodeCtrl(new-window)` → pane_id 캡처 (ctrl-registry 또는 mux.panes 추가의 `Promise.race`) → PUT `/api/layout` (If-Match + 412 자동 1회 rebase) |
| `lib/canvas/Canvas.svelte` | 좌상단 툴바 overlay (pointer-events 분리) |
| `lib/http/layout.ts` | `putLayoutAppendPanel()` 헬퍼 |
| `lib/ws/decode.ts` + `lib/types/envelope.ts` | `encodeCtrl` / `decodeCtrl` / `CtrlDecoded` 추가 |
| `lib/ws/dispatcher.svelte.ts` | NOTIFY_MIRROR 7 kind → muxStore 라우팅, CTRL response → ctrl-registry, 첫 PANE_OUT → `mux.addPane` |

svelte-check 221 → 224 files 0/0. main 번들 7.21 → 9.71 KB gzip.

### 3.3 Sprint 5-B 추가 — Sidebar V0 (`7a5c873`)

| 파일 | 역할 |
|---|---|
| `lib/sidebar/Sidebar.svelte` | 좌측 280 px 칼럼. groupsStore 트리 + panelsStore leaf. caret 펼침 (component-local SvelteSet, 영속화 P1+). 행 클릭 시 `ephemeralStore.m`을 *단일 선택*으로 갱신(Canvas의 toggle과 의도된 차이). 가시성(👁) / 잠금(🔒) / dead(취소선) 표시. unicode 아이콘 + inline CSS variable |

main 번들 9.71 → 10.94 KB gzip (200 KB cap의 5.5 %).

### 3.4 Sprint 5 hotfix — 데모 차단성 결함 두 건 (`da5c221`)

§ 4 참고. 변경: `config/src/lib.rs` + `bin/gtmux-cli/src/main.rs`.

테스트 188 → **190** PASS (+2: `load_with_port_override_passes_validation_for_empty_port`, `effective_cors_origins_non_loopback_bind_synthesises_single`). 기존 `effective_cors_origins_synthesises_from_bind_and_port` 는 loopback alias 셋 검증으로 명칭·내용 갱신.

## 4. 데모 도중 발견된 두 결함 — root cause + 처리

### 4.1 `gtmux start --port 9999` 가 port=0 sentinel로 죽음

**현상**

```
gtmux start: loading gtmux config: config validation error:
server.port must be in [1024, 65535], got 0
```

**Root cause**

`bin/gtmux-cli/src/main.rs::start` 가 다음 순서로 호출됐다:

```rust
let mut config = load_config(args.config_path.as_deref(), &args.session)?;   // ← validate() 여기서 실패
if let Some(p) = args.port { config.server.port = p; }                        // ← 너무 늦음
```

`config::load()` 는 figment chain(`defaults → TOML → env → CLI session`) merge 후 `validate()` 를 돌린다. TOML이 없거나 `[server].port` 가 비어 있으면 빌트인 sentinel `port = 0` 이 살아남는다. CLI `--port` 는 figment 안으로 들어가지 않아 validate가 sentinel을 만나 죽는다.

**처리**

```rust
pub fn load(path: Option<&Path>, session: &str) -> Result<Config, ConfigError> {
    load_with_overrides(path, session, None)
}

pub fn load_with_overrides(
    path: Option<&Path>,
    session: &str,
    port_override: Option<u16>,
) -> Result<Config, ConfigError> {
    // ... 기존 chain ...
    if let Some(p) = port_override {
        figment = figment.merge(Serialized::default("server.port", p));
    }
    let cfg: Config = figment.extract()?;
    validate(&cfg)?;          // ← override 후에 돌아 sentinel을 만나지 않음
    Ok(cfg)
}
```

CLI는 `load_with_overrides(.., args.port)` 사용. `load`는 기존 14개 호출자 호환을 위한 thin wrapper로 유지(테스트 코드 변경 0건). `config`도 `let mut`이 `let`으로 좁혀짐.

**검증**

- 단위 테스트 `load_with_port_override_passes_validation_for_empty_port` — port_override `None` → Validation 에러 / `Some(9999)` → 통과.
- 실측: `./codebase/backend/target/debug/gtmux start --session demo --port 9999` 즉시 booting + 배너 출력.

### 4.2 `localhost:9999` 접속 시 403 Forbidden / 빈 화면

**현상**

서버 배너는 `http://127.0.0.1:9999/?token=…` 를 출력하지만, 사용자가 `localhost:9999` 로 접속하면 브라우저 콘솔에 `Failed to load resource: 403 (Forbidden)` 만 보이고 화면은 비어 있음.

**Root cause**

Sprint 5-A의 `effective_cors_origins()` 는 빈 `cors_origins` 일 때 `vec![format!("http://{}:{}", bind, port)]` 단일 entry만 합성했다. bind = `127.0.0.1` 일 때 사용자가 보내는 `Origin: http://localhost:9999` 는 *문자열 mismatch* 라 ADR-0003 D3의 "정확 일치" 정책에 걸려 `origin_check_middleware` 가 403을 반환. 브라우저는 cross-origin이 아닌 same-host fetch에서 본 403을 받고 SPA 부트 단계에서 죽는다.

브라우저 운영 관행상 `127.0.0.1` / `localhost` / `[::1]` 셋은 same-origin으로 인지되지만, 본 프로젝트의 화이트리스트는 그렇지 않았다.

**처리**

```rust
pub fn effective_cors_origins(&self) -> Vec<String> {
    if !self.security.cors_origins.is_empty() {
        return self.security.cors_origins.clone();
    }
    let port = self.server.port;
    let bind = self.server.bind.to_ascii_lowercase();
    if bind == "127.0.0.1" || bind == "::1" || bind == "localhost" {
        return vec![
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
            format!("http://[::1]:{port}"),
        ];
    }
    vec![format!("http://{}:{}", self.server.bind, port)]
}
```

- loopback bind 한정으로만 alias 3개 출력. cloud 모드(non-loopback)는 single entry 유지 → 사용자가 `cors_origins` 를 명시해야 한다는 ADR-0003 D3 의도가 그대로 유지된다.
- 대소문자 무시(`to_ascii_lowercase`) — `bind = "LocalHost"` 같은 입력도 alias 셋이 적용된다.

**검증** (live curl)

| Origin | 응답 |
|---|---|
| `http://127.0.0.1:9999` | **200** |
| `http://localhost:9999` | **200** (이전엔 403) |
| `http://evil.test` | **403** (allowlist 차단 유지) |

## 5. 데모 시연 가능 범위 + 절차 (실측)

### 5.1 데모-가능 능력 매트릭스

| 기능 | 동작 | 비고 |
|---|---|---|
| 서버 기동 / 배너 URL / 토큰 query 자동 인증 | ✅ | env 우회 불필요 |
| SPA 정적 서빙 (`GTMUX_FRONTEND_DIST`) | ✅ | tower-http ServeDir + fallback |
| same-origin SPA fetch | ✅ | loopback alias 셋 자동 합성 (`localhost` / `127.0.0.1` / `[::1]`) |
| WS handshake (`gtmux.v1, bearer.<token>`) | ✅ | 12 frame round-trip byte-equal |
| **[New Panel]** 버튼 → tmux window 생성 → 패널 마운트 → xterm 양방향 | ✅ | CTRL+NOTIFY_MIRROR `Promise.race`로 pane_id 캡처 |
| 패널 드래그/리사이즈 → PUT `/api/layout` (If-Match + 412 자동 rebase) | ✅ | in-memory (서버 재시작 시 휘발) |
| 좌측 **Layers** 사이드바 (group/panel tree, 가시성/잠금/dead 표시) | ✅ | read-only |
| 사이드바 행 클릭 → 캔버스 선택 동기화 (`ephemeralStore.m`) | ✅ | 단일 선택 (Canvas는 toggle — 의도된 차이) |
| 외부 `tmux attach` 변경 → mux 미러 store 반영 | ✅ | sidebar는 panels/groups 기반이므로 mux store 직접 시각화는 차후 |
| `gtmux teardown --force` 5단계 정리 | ✅ | tmux daemon kill + socket/token/pid 제거 |

### 5.2 단계별 절차 (5 step)

```bash
cd /Users/ws/Desktop/projects/gtmux

# 1) frontend SPA 빌드 → dist/
(cd codebase/frontend && npm run build)

# 2) backend 빌드
(cd codebase/backend && cargo build --bin gtmux)

# 3) 서버 기동 (SPA 경로 환경변수로 주입)
GTMUX_FRONTEND_DIST=$PWD/codebase/frontend/dist \
  ./codebase/backend/target/debug/gtmux start --session demo --port 9999
# → 배너 예시:
#   gtmux demo ready
#     Mode:         Local
#     Bind:         127.0.0.1:9999
#     Open URL:     http://127.0.0.1:9999/auth/bootstrap?token=<...>
#     Token path:   ~/.local/state/gtmux/demo.token (0600)
```

브라우저에서 배너 URL을 그대로 열거나 `http://localhost:9999/?token=<...>` 로도 접속 가능.

```
# 4) 브라우저 흐름 (manual)
#  ① 좌측 280 px LAYERS 사이드바 + 본문 캔버스 표시. 초기엔 "No panels yet."
#  ② 캔버스 좌상단 [New Panel] 클릭
#      → backend로 CTRL new-window 송신
#      → 새 tmux window 생성 + NOTIFY_MIRROR "window-add"
#      → pane_id 캡처 → PUT /api/layout (If-Match)
#      → LAYOUT_CHANGED 수신 → 패널 마운트 → xterm 즉시 인터랙티브
#      → 사이드바에 행 1개 추가
#  ③ 외부 터미널: tmux -L gtmux-demo -S /tmp/gtmux-501/demo.sock attach -t demo
#      Ctrl-b c 로 새 window 생성 → 브라우저 muxStore.windows 에 mirror 갱신
#  ④ 패널 드래그 / 리사이즈 → PUT /api/layout 자동 (Network 탭 확인)
#  ⑤ 새로고침 → 1세션 내에는 layout 복원 (서버 재시작 시는 휘발)

# 5) 정리 (server foreground 종료 → daemon은 teardown으로)
#    foreground server: Ctrl-C 또는 kill <pid>
./codebase/backend/target/debug/gtmux teardown --session demo --force
```

### 5.3 회귀 검증 한 줄

```bash
SMOKE_GATE_RUNTIME=0 ./codebase/smoke/01_engine_connect.sh
```

기대치: `8 PASS / 1 N/A (step 8 visual) / 0 GATE / 0 FAIL`. 포트 점유로 step 3 실패하면 leftover daemon 정리:

```bash
lsof -nP -iTCP:9999 -sTCP:LISTEN
kill <PID>
./codebase/backend/target/debug/gtmux teardown --session smoke --force
```

## 6. 잔여 갭 (sketch §15 2단계 마무리·3단계 진입)

| 코드 | 항목 | 차단성 | 처리 시점 |
|---|---|---|---|
| **G3** | Canvas Layout 디스크 영속(ADR-0006 implement) — 현재 in-memory `RwLock<LayoutSnapshot>`, 서버 재시작 시 layout 휘발 | 1세션 내 데모 무관 | sketch §15 3단계 entry, S5-C 또는 별도 sprint |
| **G5-b** | NOTIFY_MIRROR mux store는 자료를 모으되 사이드바는 아직 panels/groups만 시각화. 외부 변경(예: 새 window add)이 사이드바 표시로 자동 승격되려면 muxStore↔panelsStore 합성 또는 mux 전용 패널 row 필요 | 비차단 (관찰자가 콘솔로 확인 가능) | S6-FE-MUX-VIS |
| **G6** | TLS / cloud 모드 helper | sketch §15 4단계 진입 시 | 별도 sprint |
| **L-1** | `gtmux teardown` 은 tmux daemon만 죽이고 foreground gtmux server는 그대로 둠 (ADR-0009 D5 의도). `gtmux stop` 도 pidfile 의존 — foreground server는 Ctrl-C / `kill <pid>` 가 정본 | 운영 시 문서로 안내 | 본 문서 §5.2 단계 5 |
| **L-2** | `Cmd::Stop` 이 pidfile 의존 → `teardown` 후 stop 시도하면 pidfile이 이미 제거돼 `not found` 보고 | 운영 시 문서로 안내 | 본 문서 §5.2 + 0018 §9 |
| **CTRL-ACK** | backend는 NOTIFY_MIRROR `window-add` 만 송신하고 CTRL success ack에 `result.pane_id` 가 미작성 — frontend가 `Promise.race(ctrl-registry, mux.panes 추가)` fallback으로 동작. ack가 정식 wire되면 fallback 자동 deprecated | 비차단 | S6-BE-CTRL-ACK |
| **SIDEBAR-TOGGLE** | 사이드바의 가시성(👁) / 잠금(🔒) 아이콘은 표시만, click 토글 없음 | P1+ |  |
| **SELECTION-UNIFY** | 사이드바 = 단일 선택, Canvas = toggle 선택 — 의도된 차이지만 P1+에서 통일 검토 | P1+ |  |

## 7. 게이트 요약

| 항목 | 결과 |
|---|---|
| `cargo test --workspace --tests` | 184 → **190** passed / 5 ignored / 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `npm run check` (svelte-check) | 221 → **224** files / **0** errors / **0** warnings |
| `npm run build` main bundle gzip | 7.21 → **10.94 KB** (200 KB cap의 5.5 %) |
| `smoke 01_engine_connect.sh` | 8 PASS / 1 N/A / 0 GATE / 0 FAIL |
| 실측 curl (Origin alias) | 127.0.0.1 → 200, localhost → 200, evil.test → 403 |

## 8. 안티패턴 / 함정 — Sprint 5 누적

0018 §9 누적 위에 본 세션이 추가한 학습:

- **figment chain의 검증 순서는 *마지막 layer 합류 이후* 다.** CLI `--port` 같은 후속 override를 *figment provider* 가 아닌 *post-load mutation* 으로 적용하면, `validate()` 가 sentinel을 만나서 죽는다. CLI 옵션을 figment에 합류시키는 것이 정본 — `load_with_overrides` 패턴 그대로 다른 CLI 옵션(`--bind` 추가 시)에도 적용.
- **`effective_cors_origins` 의 single-entry 합성은 브라우저 운영 관행과 불일치.** 사용자는 배너 URL의 host 문자열을 그대로 쓰지 않는다(`localhost` 자동 입력, 북마크의 IPv6 별칭 등). loopback bind에 한해 alias 셋을 출력하는 게 안전 디폴트. 명시 화이트리스트 사용자는 영향 없음.
- **`gtmux teardown` 은 server 종료 책임이 없다.** 사용자가 foreground server (`gtmux start &`) 를 띄운 상태로 teardown 하면 tmux daemon은 죽지만 server process는 9999 점유 상태로 남는다. 운영 문서에 명시 + 다음 데모는 server 종료 → teardown 순서 권장.
- **테스트 모드별 prerequisite — `bind = 0.0.0.0` 등 cloud-mode TOML 만으로는 load가 실패한다** (`ModeMismatch`). cloud mode 동작을 단위 테스트로 검증하려면 `[cloud]` 섹션을 함께 채워야 한다.

## 9. 다음 단계 task 분해 (sketch §15 2단계 마무리)

본 보고서 발행 시점 기준 0018 §7 의 잔여 항목 + 본 세션 회수 누적:

### S6-A. 백엔드 ACK & resize

| Task | 작업 | DoD |
|---|---|---|
| **S6-BE-CTRL-ACK** | 0x01 CTRL response 정식 wire. `new-window` 응답에 `result.pane_id`(`#{pane_id}` 캡처값) 동봉. ws-server / cmd_router 갱신 | frontend `ctrl-registry` fallback 자동 deprecated, smoke step 7 후속 step 추가 |
| **S6-BE-CLOSE** | `Command::KillWindow { window_id }` allowlist 정식화 + lifecycle::serialise_command. close 동작이 FE의 close 액션으로 호출 | 단위 테스트 2개 (KillWindow_serialisation + cmd_router 라우팅) |

### S6-B. 프론트 select / close / group reparent

| Task | 작업 | DoD |
|---|---|---|
| **S6-FE-SELECT** | tmux `select-window` 발사 — 사이드바 클릭 또는 캔버스 dblclick. NOTIFY_MIRROR `session-changed` mirror 확인 | manual probe |
| **S6-FE-CLOSE** | 패널 컨텍스트 메뉴 / 사이드바 우클릭 → close. confirm 다이얼로그 (sketch §13 destructive-action confirm prereq) | svelte-check / bundle |
| **S6-FE-GROUP-REPARENT** | 사이드바에서 panel drag → group drop. ADR-0010 G-hybrid drag-delta 액션 (PUT `/api/layout` `panels[].parent_id`) | bundle, manual probe |
| **S6-FE-MUX-VIS** | muxStore.windows / panes를 사이드바에 추가 표시 (panels 미등록 window 도 *Available* 섹션으로 노출) | bundle |

### S6-C. (선택) 영속화 정착

`P0-LAYOUT-STORAGE-1` — 0018 §8 Agent #4 prompt 그대로. ADR-0006 implement. sketch §15 3단계 entry.

### S6-D. CI / 정합

| Task | 작업 |
|---|---|
| **S6-CI** | GitHub Actions smoke 9-step gate (현재 build + codegen만 자동) |
| **S6-DOC** | 본 보고서의 §6 G3 / G5-b / L-1 / L-2 항목을 처리 후 polish 또는 삭제 |
| **S6-WIRE** | `Arc<Mutex<TmuxDaemon>>` → tokio::io::split 검토 (성능 측정 후 결정) |

## 10. Commit history (본 세션 추가분)

```
da5c221 fix(config): --port CLI override + cors loopback alias for demo flow
7a5c873 Sprint 5-B sidebar v0: read-only layer panel
50bad9c Sprint 5-A+B: cors default, ResizeWindow variant, New-Panel UX, mux mirror
6c81b26 docs: Sprint 0~4 closeout — progress report 0017 + handoff 0018
```

## 11. 변경 이력

- 2026-05-14: 초안 (Sprint 5-A/B/B-sidebar + demo hotfix `da5c221` 직후, sketch §15 2단계 첫 박스 통과 + 마무리 진입 직전)
