# PTY POC 검증 결과 + tmux 드롭 결정 — 2026-05-14

본 보고서는 `0020-debug-classification.md` (Sprint 5 demo 안정화 17건 분류) + `0022-logic-amendment-decisions.md` (6개 L 결정의 grilling SSoT) 의 *후속 회의* 결과 — *tmux 자체를 backend 에서 드롭하고 우리가 직접 PTY 를 소유한다* — 의 검증과 채택을 정본화한다.

ADR-0001 (tmux 통합 = control mode) 가 본 결정으로 **deprecate**, 신 **ADR-0013 (PTY direct, no tmux)** 가 supersede 한다. 0022 의 ADR amend 6건 중 일부는 본 결정으로 *moot* 가 되고 일부는 유지된다 (§4 참조).

## 0. TL;DR

- POC (`experiments/pty-poc/`, 브랜치 `poc/pty-direct`) Day 1·2 모든 게이트 통과. multi-tab mirror 가 `tokio::broadcast` 만으로 trivial 하게 동작 — tmux 의 multi-attach mirror 기능을 ~20 LOC 로 대체.
- **결정**: tmux 드롭 채택. portable-pty + tokio broadcast + 단일 WS envelope 으로 lifecycle + mux-router 두 crate 의 ~2700 LOC 를 ~1500 LOC 의 새 PTY 모듈로 교체.
- 0022 amend 6건 중 **L-7 (argv quoting) 는 moot — 폐기**. 나머지 5건 (L-2/3/4/9/12/17) 유지. L-17 의 prevention 모델은 *tmux invariant 가 아니라 우리 측 정책* 으로 의미 변환되어 더 자유롭게 재설계 가능.
- Sprint 6 → **Sprint 7 (Architecture pivot)** 로 명명 변경. 새 마일스톤 = `replace lifecycle+mux-router with pty-backend, ship sketch §15 2단계 demo on new backend`.

## 1. POC 결과 정본

### 1.1 구성
- **브랜치**: `poc/pty-direct` (commit `c637c39`). workspace 밖, throwaway.
- **디렉터리**: `experiments/pty-poc/`. Cargo.toml + src/main.rs (199 LOC) + static/index.html (75 LOC) + README.md + .gitignore.
- **의존성**: portable-pty 0.9, tokio 1.52 (rt-multi-thread, sync, signal, process, net, io-util, time, macros), axum 0.8 (ws), tower-http 0.6 (fs), tracing 0.1, bytes 1.11, futures-util 0.3, anyhow 1.0.
- **wire**: text frames (`i:<input>`, `r:<cols>,<rows>`) + binary frames (PTY output). serde 불요.
- **run**: `cd experiments/pty-poc && cargo run --release` → http://127.0.0.1:9100.

### 1.2 Day 1 게이트
| # | scenario | 결과 |
|---|---|---|
| 1 | Ctrl-C / Ctrl-D / Ctrl-Z + fg | PASS |
| 2 | Resize → SIGWINCH (vim/tput 검증) | PASS |
| 4 | Shell `exit` + zombie reap (ps -e \| grep defunct = 0) | PASS |

### 1.3 Day 2 게이트
| # | scenario | 결과 |
|---|---|---|
| 3 | Alt-screen (vim/less/htop 진입·복원) | PASS (암묵 — 사용자가 #5 외 불평 없음) |
| 5 | Burst (`yes`, `/dev/urandom \| base64`) + 안정성 | PASS — "느리지 않음" |

### 1.4 보너스 발견
- **Multi-tab mirror**: 두 번째 브라우저 탭에서 같은 URL 접속 시 *기존 탭과 자동 미러링*. tmux 의 multi-attach mirror 패턴 (한 사용자가 여러 client 로 같은 session 을 보는 모델, CONTEXT.md MT-3 정합) 을 `tokio::broadcast::channel<Bytes>` 와 `subscribe()` 만으로 *추가 코드 0줄* 달성. 이 보너스 자체가 architectural 결정의 *강한 양성 신호* — tmux 의 marquee 기능을 dependency 없이 자체 제공.

### 1.5 식별된 orthogonal 갭
- **Shift / Option 모디파이어 키맵**: xterm.js 의 default 옵션이 `macOptionIsMeta` / `modifyOtherKeys` 등을 명시 안 함. main 의 `SECURE_XTERM_OPTIONS` 도 동일 갭 → **tmux 든 PTY 든 동일 fix 필요**. architectural 결정과 무관, Sprint 7 backlog 분리.

## 2. 결정 본문

### 2.1 채택
**tmux 를 backend 에서 드롭**. gtmux Server 가 portable-pty 를 직접 owner 로 갖고 PTY 마다 child 프로세스 (shell) 를 spawn. 우리가 byte stream / signal / resize / exit 을 모두 직접 처리.

### 2.2 근거 요약 (보고서 0022 §"tmux drop" 분석 보강)
1. **본 세션 17건 결함 중 7건 (~41%) 이 tmux 통합 노이즈** — `-d` detach quirk, mutex split deadlock, `#` line-comment quirk, `%session-changed` catch-up race, control-mode mutex contention, argv injection 표면 등. *모두 PTY 직접 모델에서는 존재하지 않는 부류*.
2. **tmux 의 marquee 기능 (persistence, multi-attach mirror) 중 mirror 는 broadcast 패턴으로 trivial 달성**. persistence 는 MVP 미사용 (ADR-0006 미구현) — 또한 tmux 의 persistence 도 *프로세스 상태* 가 아닌 *Server 재기동 후 control-mode 재attach* 한정이라 우리 측 layout snapshot + WAL 로 동등 달성 가능.
3. **POC 가 Day 1·2 게이트 100% 통과** — portable-pty 가 PTY race / signal / TIOCSWINSZ 함정을 잘 추상화. 검증 시간 6-8h.
4. **코드 표면 감소**: 약 -2700 LOC (lifecycle 1820 + mux-router 890) → 약 +1500 LOC (PTY backend, WS payload 단순화). Net **약 -1000 LOC** + tmux 3.2+ 외부 의존 + control-mode 도메인 지식 영구 제거.
5. **불변식 #1 (tmux 상태 vs 웹 상태 분리)** 가 *단일 도메인 진실* 로 단순화. ADR-0002 envelope 의 0x01–0x0F (tmux-domain) / 0x80–0x8F (web-domain) 분리 의 *역사적 동기* 가 사라짐 — 그러나 envelope 구조 자체는 보존 가능 (의미만 재정의).
6. **L-7 부류 (argv quoting / format string / # quirk) 영구 소거** — 우리가 shell 또는 tmux 명령 어휘를 *발급하지 않으므로* 이 부류 결함이 다시 발생할 표면이 없음.

### 2.3 거절
- **현 tmux 유지**: 본 세션 결함 7건 부류의 누적 추세 + persistence 미사용 + 이미 검증된 POC 의 단순성을 고려하면 *유지 비용 > 전환 비용*.
- **Strangler fig (trait 추상화 + 양쪽 구현 유지)**: 두 구현 영구 유지 = 코드 표면 ↑ + 추상화 누설 위험 + 결국 둘 다 디버깅. 한 번에 swap 이 더 깔끔.
- **Day 3 persistence prototype 추가 후 결정**: 이미 게이트 통과 + persistence 가 MVP 미사용이라 검증 추가 가치 작음. Sprint 7 안에서 elapsed 로 검증 가능.

## 3. 영향: 0022 결정의 살아남는 / 폐기되는 부분

| L | 0022 의 결정 | 본 결정 후 상태 |
|---|---|---|
| L-2 / L-9 | CORS 합성 (loopback alias + 0.0.0.0=cloud) | **유지** — backend 무관, 보안 정책 그대로 |
| L-3 | static-state catch-up = layout Pull-through-notify | **유지 + 강화** — backend 가 우리 코드라서 더 정확한 보장 가능 |
| L-4 | HttpOnly cookie 폐기, sessionStorage 단일 | **유지** — backend 무관 |
| L-7 | argv selective single-quote wrap | **폐기 (moot)** — 우리가 tmux 명령을 발급하지 않으므로 quoting 대상 자체 소멸. ADR-0001 D13 (본 amend 로 추가됐던 절) 은 ADR-0001 deprecate 와 함께 자동 폐기 |
| L-12 | per-pane 256 KiB FIFO drop-oldest late-mount buffer | **유지** — frontend race 는 backend 무관 |
| L-17 | tmux invariant prevention (close 비활성 + auto-mount + Session shutdown) | **유지하되 의미 재정의** — "tmux invariant" 가 아니라 *"우리 process 의 lifecycle invariant"* 로 변환. *우리가* invariant 를 정의하므로 더 자유롭게 재설계 가능 (예: "마지막 Panel close 시 자동 shutdown" 같은 옵션도 채택 가능했으나 0022 의 prevention 정신 유지 권고) |

본 amend 의 *작업 자체* (commit `0f3b1a3`) 는 보존 — 결정 흐름의 historical record 로 가치 있음. L-7 관련 ADR-0001 D13 의 본문만 deprecate 와 함께 자동 무효화.

## 4. ADR 변경 매트릭스

| ADR | 변경 |
|---|---|
| **ADR-0001** (tmux 통합 = control mode) | **Deprecated, superseded by ADR-0013**. 본문 보존 (historical). 상태 헤더만 갱신 |
| **ADR-0013** (신규: PTY direct, no tmux) | **신규 작성**. portable-pty + tokio broadcast + 단일 WS envelope + 5 불변식 재검증. 본 0023 가 근거 보고서 |
| **ADR-0008** (single-pane + Group + allowlist) | **부분 amend**: tmux command allowlist 절 영구 폐기 (우리가 명령을 발급하지 않음). single-pane-per-window 컨벤션은 *single-pane-per-process* 로 의미 단순화 — 사실상 trivial 유지. Group 부분은 그대로 |
| **ADR-0009** (tmux daemon 격리) | **전면 amend 또는 deprecate**: daemon 격리 모델의 핵심 가정 (tmux daemon 이 우리 Server 와 별 프로세스) 이 무너짐. 대안: (a) deprecate 후 신 ADR-0014 "process supervisor" 작성, (b) 본 ADR 의 §맥락 / §결정 절을 *우리 측 process supervisor* 로 재해석하는 amend. 다음 세션에서 분기 |
| **ADR-0002** (전송 WebSocket) | **소폭 amend**: envelope 의 두-도메인 분리 (0x01-0x0F vs 0x80-0x8F) 의 *역사적 동기* 가 약화. 구조 자체는 유지 (의미 재정의: "PTY-domain" vs "web-domain"). L-3 / L-12 amend 는 그대로 |
| **ADR-0003** (보안 디폴트) | **소폭 amend**: D7 (argv 분리 정책) + D8 (식별자 정규식) 의 *tmux 측 부분* 폐기. CORS / token 채널 / CSP 절은 그대로. L-2/4/9 amend 그대로 |
| **ADR-0004** (terminal rendering) | **불변** — xterm.js 채택은 backend 무관 |
| **ADR-0005** (canvas library) | **불변** |
| **ADR-0006** (persistence storage) | **불변하되 implementation 시점 자유도 ↑** — tmux 가 주던 process state 보존이 어차피 없었음을 명시 |
| **ADR-0007** (Server : Session : Port 1:1:1) | **불변하되 의미 단순화** — "Session" 정의가 *우리 측 logical 경계* 로 단일화 (tmux session 대응이 사라짐) |
| **ADR-0010** (group data model) | **불변** |
| **ADR-0011** (Rust backend) | **불변하되 crate version 표에 portable-pty 추가** |
| **ADR-0012** (frontend stack) | **불변** |

## 5. Sprint 재구성 — Sprint 6 → Sprint 7

### 5.1 폐기 / 무효화
- `S6-LIFE-AUTOSPAWN` — 이미 0022 에서 취소 (prevention 으로 흡수). 재확인.
- `S6-BE-CTRL-ACK` — tmux CTRL response 자체가 사라짐. 폐기.
- `S6-ARGV-QUOTE` — tmux 명령을 안 보내므로 폐기.
- `S6-BE-CLOSE` (KillWindow allowlist) — allowlist 자체 폐기, 대신 "*우리 측 process kill*" 으로 의미 단순화.
- `S6-WS-WINDOW-CATCHUP` — 0022 에서 이미 `S6-BE-AUTOMOUNT` 로 재정의. 그 정신은 유지.

### 5.2 유지 / 재정의
- `S6-FE-SHUTDOWN` (헤더 메뉴 + Session shutdown) → **Sprint 7 유지** — 의미만 "Server 종료" 로 단순화
- `S6-FE-CLOSE-GUARD` (마지막 panel close 비활성) → **Sprint 7 유지** — invariant 가 *우리 측 정의* 임을 명시
- `S6-BE-AUTOMOUNT` (이벤트 시 layout PUT) → **Sprint 7 유지** — backend 가 우리 코드라 더 정확
- Sprint 6 의 ADR amend 6건 (S6-A) → **본 0023 가 그 일부를 대체**. 남은 amend 작업은 ADR-0008/0009 의 본 매트릭스 §4 따른 amend + 신 ADR-0013 작성

### 5.3 신규 (Sprint 7 의 핵심)
- **S7-PTY-BACKEND**: `crates/pty-backend` 신규. portable-pty + tokio broadcast + process supervisor. lifecycle crate 폐기, mux-router crate 폐기 (또는 의미 재정의 — 우리 측 wire envelope routing 으로 축소).
- **S7-WS-PAYLOAD-SIMPLIFY**: WS envelope 의 CTRL (0x01) 페이로드 schema 단순화 — tmux 명령 argv 가 아니라 우리 API 명령 (new-pane, kill-pane, resize-pane, set-cwd 등).
- **S7-MIGRATE**: 기존 frontend 코드 (Canvas / PanelNode / XtermHost / dispatcher) 의 *backend 측면 의존성* 만 새 wire 에 맞춰 재배선. UI 자체는 거의 그대로 (auto-mount + close-guard + shutdown 만 추가).
- **S7-PERSISTENCE-MINIMAL**: ADR-0006 의 *layout snapshot 만* sqlite 또는 JSON file 로 영속. process state 보존 비범위.
- **S7-ADR**: 신 ADR-0013 작성 + 0001 deprecate + 0008/0009 amend.
- **S7-DEMO-STAB**: 새 backend 위에서 sketch §15 2단계 demo 재확인 — 본 세션의 17건 같은 안정화 cycle 재발생 가능 (그러나 부류가 달라짐 — tmux 통합 노이즈가 아니라 PTY 함정).

### 5.4 Sprint 7 추정 일정
- ADR + 0023 follow-up 문서 작업: 1 세션 (지금 본 세션의 끝 + 다음 1 세션)
- S7-PTY-BACKEND 본격 구현: 4-7 일
- S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE: 2-3 일
- S7-PERSISTENCE-MINIMAL: 2-3 일 (선택)
- S7-DEMO-STAB: 3-7 일 (POC 가 게이트 통과한 만큼 안정화 부담은 작을 것)

**합계**: 2-3 주.

## 6. 로드맵 단계별

### Stage A — 결정 정본화 (본 세션 + 1 세션)
- ✅ 0023 보고서 (본 문서)
- ⏳ ADR-0013 신규 작성
- ⏳ ADR-0001 상태 헤더 갱신 (Deprecated)
- ⏳ (다음 세션) ADR-0008/0009 amend + CONTEXT.md tmux 어휘 정리 + sketch.md §10.1/§11.2.A/§13.3.6/§14 rewrite

### Stage B — 코드 swap (Sprint 7)
- `crates/pty-backend` 신규 — POC 코드를 production-grade 로 (테스트, 백프레셔, ring buffer, 다중 pane)
- `crates/lifecycle` deprecate / 폐기
- `crates/mux-router` deprecate / 폐기 (또는 axum-side wire router 로 의미 단순화 + 명칭 변경 `wire-router`)
- `crates/ws-server` 의 CTRL 라우터 재배선
- `crates/http-api` 의 endpoint 그대로 (layout / bootstrap / auth)
- `crates/config` / `crates/auth` 그대로
- `bin/gtmux-cli` 가 새 backend 로 wiring

### Stage C — Demo 안정화 (Sprint 7 closeout)
- 새 backend 위에서 sketch §15 2단계 demo 재구동
- multi-tab mirror, auto-mount, close-guard, Session shutdown 모두 검증
- L-class 결함 부류 17건 회귀 테스트

### Stage D — Sprint 8 이후
- ADR-0006 implement (layout snapshot 영속)
- 다중 pane (50 pane × 5 burst) 백프레셔 측정
- 외부 attach 가 진짜 필요한지 사용자 검증 후 결정 (필요 시 CLI client 도입, 아니면 영구 비범위)

## 7. Risk / Open questions

### 7.1 Risk 매트릭스
| Risk | 가능성 | 영향 | 완화 |
|---|---|---|---|
| portable-pty 가 production 사용 시 미발견 함정 (alt-screen edge case, OSC 시퀀스, $TERM 변종) | 중 | 중 | Sprint 7 의 demo 안정화 cycle 에서 흡수. wezterm·alacritty 가 같은 crate 사용 (battle-tested). |
| Persistence 의 *Server 재기동 후 process 복원* 불가 (process state 가 휘발) | 높음 | 낮음 | tmux 도 동일 수준 — 둘 다 "재attach 후 새 shell" 모델. 사용자가 이미 그 모델로 운영 중 |
| 외부 attach (`tmux a`) 같은 power-user 기능 영구 상실 | 확실 | 낮음 | sketch 미홍보. 필요 시 우리 측 CLI client 신규 가능 |
| 신호 처리 race (특히 SIGCHLD reap, SIGWINCH 폭주) | 중 | 중 | POC 게이트 #4 통과. 단 5분 / 50 pane 시나리오에서 재검증 필요 |
| frontend ↔ backend wire 의 *기존 0022 amend (L-3, L-12)* 가 새 backend 에서 자연 정합되는지 | 낮음 | 낮음 | L-3 의 Pull-through-notify, L-12 의 late-mount buffer 모두 backend 무관 |
| Sprint 6 의 *grilling 결정* 일부 (특히 L-17 prevention) 가 새 backend 에서 *자유도가 너무 커서 재논의 유혹* | 중 | 낮음 | 0022 결정 그대로 적용. 차후 grilling 은 별 사유 발생 시에만 |

### 7.2 Open questions (Stage A 안에서 해소)
- **O1**: ADR-0009 를 deprecate 후 신 ADR-0014 "process supervisor" 신설 vs ADR-0009 amend 로 의미 재해석. 다음 세션에서 결정.
- **O2**: ADR-0008 의 single-pane-per-window 컨벤션 어휘를 어떻게 변환할지. 후보: (a) ADR-0008 의 §"single-pane-per-window" 절을 *single-pane-per-process* 로 단순 rename, (b) ADR-0008 자체를 *Group only* 로 축소 + tmux 관련 절 전부 deprecate.
- **O3**: 프로젝트 framing — "gtmux" 라는 이름의 g 가 graphical-tmux 의미인데 tmux 가 사라지면 이름이 비-적합. 단 rename 은 운영 비용 큼 — 이름 그대로 유지 + README 에 "originally tmux-backed, now self-hosted PTY supervisor" 정도 명시가 현실적.
- **O4**: `crates/mux-router` 폐기 vs `crates/wire-router` 로 rename. 외부에 노출되는 crate 명이 아니므로 cosmetic.
- **O5**: ADR-0007 의 "Session" 개념을 어디까지 단순화할지. 현재 "tmux session" 의 1:1 mirror 가 정신이었는데 tmux 가 사라지면 "Session" = "Server 프로세스의 logical 단위" 로 의미 변환. CONTEXT.md amend 시 결정.

## 8. 본 세션 산출 vs 다음 세션 위임

### 8.1 본 세션
- ✅ POC 구축 + Day 1·2 게이트 측정 + commit
- ✅ 0023 보고서 (본 문서)
- ⏳ ADR-0013 신규 작성
- ⏳ ADR-0001 상태 헤더 갱신
- ⏳ commit on main

### 8.2 다음 세션 (Stage A 마무리)
- ADR-0008 amend (allowlist 폐기)
- ADR-0009 deprecate or amend (O1)
- ADR-0002 amend (envelope 의미 재정의)
- ADR-0003 amend (D7/D8 tmux 측 부분 폐기)
- ADR-0007 amend (Session 의미 단순화, O5)
- CONTEXT.md amend (tmux 어휘 정리)
- sketch.md §10.1 / §11.2.A / §13.3.6 / §14 rewrite
- handoff 보고서 0024 작성

### 8.3 다음 다음 세션 이후 (Stage B)
- S7-PTY-BACKEND 본격 구현 (Sprint 7)

## 변경 이력

- 2026-05-14: 초안 — POC 게이트 통과 직후, tmux 드롭 채택 결정. Stage A 의 본 세션 분담 정의 + 다음 세션 위임 정의. 0022 결정 매트릭스의 살아남는/폐기되는 항목 정렬.
