# 데모 시연 준비 패키지 (Sprint 0 직전 스냅샷)

- 일자: 2026-05-14
- 작성: PM
- 기준 문서: `docs/reports/0015-progress-status.md`
- 상태: **사이드카 산출물** — `codebase/`, `docs/adr/`, `docs/ssot/`, `docs/plans/`의 진행과 직교. dispatch 0002 Sprint 0/1/2/3에 영향 없음.
- 위치: `docs/demo/` (보고서/ADR/플랜 시퀀스 번호와 분리하기 위해 별도 디렉터리)
- 폐기 조건: §15 1단계 정식 통과 후 본 문서는 `0002-demo-prep-stage1.md`로 갱신 또는 폐기 가능

## 0. 영향 범위 선언 (개발 무간섭 보장)

본 패키지는 다음 산출물에 **쓰기 작업을 수행하지 않는다**:

- `codebase/` 일체 (Rust crates, Svelte 소스, Makefile, smoke 스크립트)
- `docs/adr/`, `docs/ssot/`, `docs/plans/`, `docs/reports/` 기존 파일
- `CLAUDE.md`, `CONTEXT.md`, `docs/sketch.md`

데모 도중에도 (a) 데모 호스트는 read-only 화면 공유만, (b) Sprint 0 dispatch는 별도 세션에서 병렬로 진행 가능. 0015 §2의 3-agent 병렬 호출이 차단되지 않는다.

## 1. 청중 옵션

| 옵션 | 강조점 | 시간 | 모드 |
|---|---|---|---|
| A. 내부 (PM / 엔지니어) | 디자인 디시플린, 다음 스프린트 의존 그래프, 리스크 핫스팟 | 25~30분 | M1+M2 |
| B. 외부 (포트폴리오 / 오픈소스) | "왜 design-first인가", 단일 사용자라도 보안 양보하지 않는 이유 | 15~20분 | M1 축약 |
| C. 본인 리허설 | Sprint 3 완료 후 §15 1단계 라이브 데모 사전 점검 | 10~15분 | M3 |

기본값: **A**. B/C는 §3 스크립트의 sub-section을 발췌해 사용.

## 2. 시연 모드 매트릭스

| 모드 | 가능 시점 | 보여줄 수 있는 산출물 | 명시적 한계 |
|---|---|---|---|
| **M1 Design Walkthrough** | 즉시 | sketch §, ADR 9건, SSoT 3건, R1~R8, 0015 진행 매트릭스, 의존 그래프 | 동작 기능 0건. tmux 미연결 |
| **M2 Skeleton & Build** | 즉시 | `make help/build/codegen` PASS, 40-파일 인벤토리, smoke 2 PASS / 6 GATE | 모든 crate 본문 `todo!()` |
| **M3 Stage-1 Live** | Sprint 0~3 완료 후 (예상 ≥1주) | tmux control mode 연결, 1 패널 라이브 터미널, 패널 1개 layout 영속화 | UX 폴리시·다중 세션·재연결 banner 미완성 |

오늘 발표 가능한 시연 = **M1+M2 조합**. M3는 §4의 사전 점검 항목이 모두 충족되기 전까지 *시연 금지*.

## 3. M1+M2 시연 스크립트 (오늘, 25~30분)

### 3.1 사전 준비 체크리스트 (시연 30분 전, 데모 호스트)

- [ ] `cd codebase && make help` — 사용 가능한 target 출력 확인
- [ ] `cd codebase && make build` — backend cargo + frontend vite 모두 PASS
- [ ] `cd codebase && make codegen` — utoipa → openapi.yaml → openapi-typescript 산출물 byte-equal 확인 (ADR-0011 D5, ADR-0012 D7)
- [ ] `cd codebase && make test` — backend `cargo test` + frontend `svelte-check` 통과
- [ ] `bash codebase/smoke/01_engine_connect.sh` — 2 PASS / 6 GATE / 1 MANUAL 출력 확인
- [ ] 브라우저 4탭 사전 오픈: `docs/sketch.md`, `docs/adr/` 인덱스, `docs/ssot/` 인덱스, `docs/reports/0015-progress-status.md`
- [ ] 에디터 분할: 좌(터미널 — make 실행용) / 우(에디터 — sketch / ADR)
- [ ] `.env` / dev secret 화면 노출 없는지 확인 (§7 안티패턴 3)
- [ ] macOS keychain prompt 자동 차단: GitHub push 시도 금지 (0015 §3.3, 사용자 환경 문제)

### 3.2 오프닝 (2분)

> "gtmux는 tmux를 백엔드 실행 엔진으로, 무한 캔버스 웹앱을 프런트엔드로 두는 단일 사용자 워크스페이스다. 오늘 보여드릴 것은 **현재 P0 구현이 0건**이지만 **9개 ADR + 3개 SSoT + 빌드 가능한 40-파일 스켈레톤이 정렬돼 있는 상태**다. ADR-before-code 규칙을 강제했기 때문이다."

핵심 한 줄: *"디자인 패키지가 코드보다 먼저 끝났다."*

### 3.3 sketch.md 투어 (4분)

`docs/sketch.md`(810행) 발췌:

- **§4 두 상태 도메인** — tmux state(세션/윈도우/페인/액티브 플래그) vs web state(패널 위치/잠금/라벨/노트). 두 스토어 혼동이 본 프로젝트가 회피하도록 설계된 1차 실패 모드.
- **§13 위협 모델** — 단일 사용자라도 (a) 기본 bind `127.0.0.1`, (b) WS 토큰+origin 체크, (c) 모든 사용자 입력 untrusted. "single-user이니까 보안 양보" 금지.
- **§15 5단계 로드맵** — 1) 엔진 연결 → 2) 캔버스 UI → 3) 영속화/재연결 → 4) UX → 5) 보안 하드닝. **현재 위치: 1단계 진입 직전**.

### 3.4 ADR / SSoT 한 줄 투어 (6분)

`docs/adr/` 인덱스에서 9개 Accepted ADR을 1줄씩:

| ADR | 결정 한 줄 |
|---|---|
| 0001 | tmux control mode (`tmux -C`) 단일 채널. 스크린 스크래핑·반복 shell-out 금지 |
| 0002 | 전송 = HTTP(부트스트랩) + WebSocket(스트림). REST `POST /layouts`는 supersede |
| 0003 | 인증 = 1회 `/auth/bootstrap` cookie → 이후 `Authorization: Bearer` 또는 WS subprotocol. `?token=` 쿼리 금지 |
| 0007 | 세션 1 : 윈도우 1 : 페인 1 컨벤션 (single-pane-per-window) |
| 0008 | tmux command allowlist 9 variant. `split-window`·`resize-pane`·`select-layout` enum 부재 (컴파일 강제) |
| 0009 | tmux daemon 세션별 격리 (`-L gtmux-<session>`) |
| 0010 | Group 모델: lock = OR (cascade-down), visibility = AND |
| 0011 | Rust 백엔드 스택 (axum, tokio, utoipa, winnow, figment) |
| 0012 | Svelte 프런트 스택 (SvelteKit, xyflow/svelte v1.5, xterm.js) |

`docs/ssot/` 3건 — 1줄씩:

- `wire-protocol.md` — WS envelope · `VIEWPORT_CHANGED` little-endian · ETag 16B raw 정본
- `security-defaults.md` — 기본 bind, 토큰 회전 정책, 헤더 정책
- `canvas-layout-schema.md` — 패널 geometry · Group · 영속화 스키마

**강조 1줄**: *"여기 적힌 한 줄이 코드의 함수 시그니처까지 결정한다."*

### 3.5 코드 스켈레톤 데모 (5분)

라이브 터미널:

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase
tree -L 3 -I 'target|node_modules' | head -60
make help
make build           # 통과 — utoipa codegen + cargo + vite
make codegen         # 통과 — OpenAPI 3.1 → TS d.ts byte-equal
bash smoke/01_engine_connect.sh   # 2 PASS / 6 GATE / 1 MANUAL
```

내레이션:

> "모든 함수 본문은 `todo!()`다. 그러나 (1) 컴파일 통과, (2) OpenAPI ↔ TypeScript 코드젠 통과, (3) smoke 1단계 2 GATE만 PASS. 이게 의미하는 것 — **컨트랙트는 끝났고, 본문만 비어 있다**. Sprint 0에서 3개 leaf crate(`auth`, `config`, `mux-router`) 본문을 동시에 채우면 의존 그래프상 critical path가 시작된다."

### 3.6 0015 진행 매트릭스 + 다음 단계 (4분)

`docs/reports/0015-progress-status.md` 직접 표시:

- **§1 진행 매트릭스** — Spec/Grill/ADR/SSoT/R1~R8/Coherence/Skeleton/Smoke 행별 ✅, **P0 구현만 ⏳**
- **§2 즉시 다음 액션** — Sprint 0 = 3-agent 병렬 dispatch (P0-AUTH-1 / P0-CFG-1 / P0-MUX-1)
- **§6 의존 그래프** — Sprint 0 (parallel) → Sprint 1 (critical path) → Sprint 2 (parallel) → smoke re-run → Sprint 3 (frontend) → §15 1단계 정식 통과
- **§4 5대 불변식 — 현 시점 평가** — 전부 *강화* 또는 *컴파일 강제 가능*. 위협 0건.

### 3.7 마감 + Q&A (4~5분)

마감 한 줄:

> "오늘 보여드린 것은 '미구현 프로젝트'가 아니라 '구현 진입 직전 상태로 정렬된 프로젝트'다. 다음 1주 내 Sprint 0~3 종료 시점에 §15 1단계 라이브 데모(M3)가 가능해진다."

§6의 예상 Q&A로 자연스럽게 전환.

## 4. M3 (Stage-1 Live) 시연 스크립트 — Sprint 3 완료 후 사용

### 4.1 시연 전 강제 조건 (전부 충족 시에만 실행)

- [ ] Sprint 0 PR 3건 merged: `auth`, `config`, `mux-router` 본문 채워짐
- [ ] Sprint 1 PR 2건 merged: `lifecycle::spawn_daemon`, `Cmd::Start` binding
- [ ] Sprint 2 PR 5건 merged: `http-api` router, `bootstrap_handler`, `ws-server`, `lifecycle::teardown`, `Cmd::Teardown`
- [ ] Sprint 3 PR 3건 merged: WS dispatcher, Canvas+1 Panel, Reconnect banner
- [ ] `bash codebase/smoke/01_engine_connect.sh` — **9/9 PASS** (현재 2/9)
- [ ] `cargo test` + `svelte-check` 모두 PASS
- [ ] `code-review-graph` MCP `detect_changes` 결과 risk score < 임계치

위 1건이라도 불충족 시 **M3 시연 금지**, M1+M2로 회귀.

### 4.2 라이브 데모 시나리오 (12~15분)

1. **엔진 부팅** (2분) — `gtmux-cli start --session demo` → daemon spawn → `/auth/bootstrap` 1회 cookie 교환 (브라우저 자동) → WS 핸드셰이크 성공 로그
2. **첫 패널 표시** (3분) — 브라우저 자동 오픈, 캔버스 중앙에 1 패널, xterm.js 라이브 셸 입력/출력 왕복
3. **명령 발행 시연** (3분) — `echo hello && uname -a` 입력, 출력이 WS 스트림으로 즉시 렌더링됨을 확인
4. **레이아웃 영속화** (2분) — 패널을 드래그 이동 → 새로고침 → 위치 유지 (ETag 16B 검증)
5. **재연결 복구** (2분) — 백엔드 일시 정지 → "Reconnecting…" 배너 표시 → 백엔드 재개 → 1초 grace 후 자동 복구
6. **종료** (1분) — `gtmux-cli teardown --session demo` → daemon 5단계 종료

### 4.3 라이브 실패 시 fallback

라이브 실패 = 즉시 종료, **M1+M2로 전환**. 절대 시연 중 디버깅 금지(handoff §안티패턴 1 — Sprint 0 dispatch는 정식 경로로만).

## 5. 런북 (exact commands)

데모 호스트가 복붙해 사용. 모두 read-only(`make build/codegen/test`만 실행).

```bash
# 위치 진입
cd /Users/ws/Desktop/projects/gtmux

# 0. 디렉터리 인벤토리 (목소리로 설명하며)
tree -L 2 -I 'target|node_modules|.git' docs/

# 1. ADR 인덱스
ls -1 docs/adr/

# 2. SSoT 인덱스
ls -1 docs/ssot/

# 3. 보고서 인덱스 (특히 0015 강조)
ls -1 docs/reports/ | tail -6

# 4. 코드 스켈레톤 인벤토리
cd codebase
tree -L 3 -I 'target|node_modules'

# 5. Make 가용 target
make help

# 6. 빌드 (PASS 확인)
make build

# 7. 코드젠 (PASS 확인)
make codegen

# 8. 테스트 (PASS 확인)
make test

# 9. Smoke 1단계 (2 PASS / 6 GATE 출력 확인)
bash smoke/01_engine_connect.sh

# 10. 0015 진행 매트릭스 직접 표시 (에디터 또는 less)
cd ..
less docs/reports/0015-progress-status.md
```

데모 도중 절대 실행 금지:

- `git push` (0015 §3.3 keychain 차단)
- `cargo run` / `npm run dev` (`todo!()` 패닉)
- `gtmux-cli start` (lifecycle 미구현)
- 어떤 형태든 `tmux -C` 직접 호출

## 6. 예상 Q&A

**Q1. "9개 ADR + 3개 SSoT는 과한 거 아닌가? 그냥 시작해서 리팩토링하면 안 되나?"**
A. `CLAUDE.md`의 핵심 invariant — 두 상태 도메인 분리, 5대 불변식 — 는 구현 후 리팩토링으로 회복 불가능한 카테고리다. 0009/0011 coherence review에서 (이전 분기의) 미해소 갭 3건이 발견됐고, 모두 ADR/SSoT 정정으로 흡수했다. 코드로 흡수했다면 dependency hell.

**Q2. "왜 Rust + Svelte인가?"**
A. ADR-0011/0012 참조. 요약: (a) tmux control mode 파서는 `winnow` 결정성, (b) axum + utoipa = OpenAPI 3.1 단일 소스, (c) Svelte는 무한 캔버스 패널 다수 마운트에 React보다 메모리 fingerprint 적음, (d) xyflow/svelte v1.5는 lock·z-index 모델이 직접 매핑됨.

**Q3. "tmux 대신 직접 PTY 띄우면?"**
A. ADR-0001 §대안 검토. PTY 직접 = (a) 세션 복원 직접 구현, (b) 다중 attach 직접 구현, (c) tmux의 검증된 IPC 포기. control mode는 *공짜로* 얻는다.

**Q4. "Sprint 0 dispatch가 왜 3-agent 병렬인가?"**
A. 0015 §6 의존 그래프 — `auth`, `config`, `mux-router`는 의존 그래프상 leaf. critical path(`lifecycle`)가 둘 모두에 의존하므로 동시 출발이 wall-clock 최단.

**Q5. "P1/P2를 당겨서 작업하면?"**
A. `CLAUDE.md`의 "Don't jump stages" 규칙. §15 1단계 미통과 상태에서 영속화(P2)·미니맵(P2)을 작업하면 wire-protocol·canvas-layout-schema 변경 시 회귀 위험. Sprint 0~3 종료까지 P1/P2 동결.

**Q6. "single-user인데 보안이 이렇게 까다로워야 하나?"**
A. sketch §13 위협 모델 — (a) 로컬 악성 프로세스의 `127.0.0.1` 접근, (b) 브라우저 확장의 origin 스푸핑, (c) HTML 인젝션을 통한 RCE 경로. 기본값을 풀어두는 비용은 (외부 노출 1회 실수 시) 회복 불가.

**Q7. "데모 도중 화면 공유로 토큰이 노출되면?"**
A. `/auth/bootstrap` cookie는 *1회용*. 노출 즉시 `rotate_token`(ADR-0003 D5) 호출. 단 데모 환경의 dev token이라도 화면 캡처 채널 통과 시 회전 권장.

**Q8. "왜 GitHub push가 막혀 있나?"**
A. 0015 §3.3 — macOS keychain `SewingRobot` 캐시 충돌. *사용자 환경 문제*이고 자동 해결 시 다른 키체인 항목을 깨뜨릴 수 있어 시도 금지. SSH URL 전환 또는 수동 keychain 정리로 처리.

## 7. 시연 직전 안티패턴

1. **시연 도중 코드 작성 시도 금지** — Sprint 0 dispatch가 정식 경로 (handoff §dispatch-prompts)
2. **`gtmux-cli start` 라이브 실행 금지** — 본문 `todo!()`, 패닉 발생
3. **.env / `~/.config/gtmux/` / keychain 화면 노출 금지** — 토큰 회전 비용 발생
4. **0015 외 보고서 인라인 표시 금지** — 청중 혼란 (0014 handoff·0013 coherence review는 PM 내부 용도)
5. **데모 후 즉흥 결정 ADR 작성 금지** — Q&A 결과는 plan 0002 §2 B6 큐에 1줄 기록, 정식 ADR은 별도 dispatch
6. **데모 머신에서 `git push` 시도 금지** — 0015 §3.3

## 8. 사후 처리

- 데모 종료 직후: 본 문서 §6 Q&A에서 발견된 사항을 plan 0002 §2 B6에 1줄씩 기록 (정식 ADR 발행은 별도)
- M3 시연 가능 시점 도래 시: `docs/demo/0002-demo-prep-stage1.md` 신규 작성, 본 문서는 *historical reference*로 유지
- 시연 녹화 / 슬라이드 자산은 본 디렉터리 외부(`docs/demo/assets/`)에 보관 — 단 토큰·`.env` 포함 캡처 금지
- 본 문서 자체는 0015처럼 *임시 산출물* — Sprint 0 종료 보고서(`reports/0016+`)가 본 문서를 reference하면 폐기 가능

## 9. 변경 이력

- 2026-05-14: 초안 (Sprint 0 dispatch 직전, M1+M2 즉시 시연 가능 / M3는 시연 강제 조건 충족 후)
