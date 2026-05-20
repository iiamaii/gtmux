# gtmux 사전 리서치 의뢰서 (임시)

> 본 문서는 외부 리서처에게 그대로 전달하기 위한 단일 핸드오프 문서다. 코드 작성 전, 아키텍처 ADR 작성 전에 필요한 외부 자료를 수집·정리하는 것이 목적이다.

## 0. 프로젝트 한 줄 컨텍스트

**gtmux**: tmux를 백엔드 실행 엔진으로, 무한 캔버스 웹앱을 시각화/조작 계층으로 분리한 단일 사용자 로컬 웹 애플리케이션. 전체 설계 SSoT는 `docs/sketch.md` (KO).

## 1. 절대 깨면 안 되는 전제 (리서치의 결론은 이걸 보존해야 한다)

1. tmux 상태(세션/윈도우/pane)와 웹 상태(패널 위치·잠금·라벨·노트·뷰포트)는 **물리적으로 분리된 두 스토어**.
2. tmux 통합은 **`tmux -C` 컨트롤 모드 단일 채널**. 화면 스크래핑·반복 shell-out 금지.
3. tmux 레이아웃 ≠ 캔버스 레이아웃. 서로 다른 개념.
4. 보안 기본값은 협상 대상이 아님: `127.0.0.1` 또는 unix socket, WS 핸드셰이크 토큰+Origin 검증, 모든 사용자 입력 untrusted, tmux 명령은 argv 분리된 화이트리스트.

## 2. 일반 리서치 원칙

- **1차 출처 우선**: tmux man/CHANGES/소스, 라이브러리 공식 docs·repo·issue, RFC. 블로그는 1차로 가는 포인터로만.
- 모든 인용은 **URL + 접근일자**. 추정·기억에 의존 금지.
- 결론은 **트레이드오프 명시 + 권장안 1개**. "상황에 따라 다름" 금지.
- 산출물 언어: **한국어**. 코드/식별자/표준 용어는 원문 유지.
- 각 트랙 산출물 경로: `docs/reports/000N-<주제>.md`.

## 3. 산출 보고서 공통 템플릿

```
# 보고서: <주제>
## 요약 (3문장)
## 조사 범위와 질문
## 핵심 발견
## 옵션 비교표
## gtmux에의 함의 (§1 전제 검증 포함)
## 미해결 질문 / 후속 ADR 필요 항목
## 출처 (URL + 접근일자)
```

## 4. 조사 트랙 8개

각 트랙은 **조사 대상 / 핵심 쟁점 / 자료 수집 방식 / 산출 경로** 4단으로 정의한다. 실행 순서 권장: **R1 → R4 → R5 → R2 → R3 → R7 → R8 → R6**.

---

### R1. tmux 컨트롤 모드

- **조사 대상**: `tmux -C` 와이어 프로토콜 전체. tmux 3.0~3.5 변경분. iTerm2 참조 구현.
- **핵심 쟁점**:
  - `%begin/%end/%error/%output/%session-changed/%window-add/-close/-renamed/%layout-change/%exit/%client-detached/%pause/%continue/%subscription-changed` 등 모든 notification의 발생 조건·페이로드·엣지케이스.
  - 명령 파이프라이닝 시 응답 매칭 규칙.
  - `%output` 인코딩(UTF-8, 이스케이프, 바이너리, 초장문).
  - 컨트롤 모드가 **푸시하지 않는** 정보 → `list-* -F` 폴링 필요 항목.
  - 실패 모드: 서버 재시작, 소켓 단절, slow client 트로틀링, pause 의미.
- **자료 수집 방식**: tmux 공식 man page, `CHANGES` 파일, `github.com/tmux/tmux` 소스 직접 grep (`control.c`, `notify.c`). iTerm2 소스의 control mode 클라이언트 코드 위치 명시.
- **산출**: `docs/reports/0001-tmux-control-mode.md`

### R2. 브라우저 터미널 렌더링

- **조사 대상**: xterm.js v5, hterm, 커스텀 WebGL, 2026년 시점 대안.
- **핵심 쟁점**:
  - 렌더 백엔드(DOM/canvas/WebGL) 별 인스턴스당 메모리·프레임 비용. **10~50개 pane 동시 표시** 시 실측.
  - 오프스크린/최소화 pane의 "일시정지" 전략 — 라이브러리가 제공하는가, 빌드해야 하는가.
  - 캔버스 픽셀 크기 → `cols×rows` 매핑과 리사이즈 디바운싱.
  - 신뢰할 수 없는 출력의 안전 렌더링 — OSC 8, OSC 52, title set, DECRQSS 중 차단할 것.
- **자료 수집 방식**: xterm.js repo README/docs/issues/PRs, 공식 벤치마크, 주요 사용자(VS Code, Hyper) 소스에서의 사용 패턴.
- **산출**: `docs/reports/0002-terminal-rendering.md`

### R3. 무한 캔버스

- **조사 대상**: React Flow, tldraw, Konva(+react-konva), Pixi.js(+pixi-viewport), Excalidraw core, 직접 구현(CSS transform/SVG).
- **핵심 쟁점**:
  - **임의의 DOM 서브트리(xterm.js의 `<div>`)를 노드로 호스팅 가능한가** — 이게 컷오프 기준. 캔버스/WebGL로만 렌더해야 하는 라이브러리는 즉시 탈락.
  - 팬·줌·fit-to-view·미니맵·snap-to-grid 제공 범위.
  - z-index·lock·group·다중 선택 기본 제공 여부.
  - 시리얼라이즈 포맷 안정성(영속화 키).
  - 노드 50+ 시 성능, 라이선스, 유지보수 건강도.
- **자료 수집 방식**: 각 라이브러리 공식 docs·repo issues에서 "custom node DOM" 키워드 검색, 데모/예제 검증.
- **산출**: `docs/reports/0003-infinite-canvas.md`

### R4. 실시간 전송 계층

- **조사 대상**: WebSocket / SSE / WebTransport, 그리고 단일 소켓 위 N개 pane 스트림 멀티플렉싱 패턴.
- **핵심 쟁점**:
  - 워크로드(양방향 + 다중 스트림 + 버스트 출력)에서 셋 중 어느 것? WebSocket 기본 가정의 정당화.
  - 핸드셰이크 인증: `Sec-WebSocket-Protocol` vs 쿼리스트링 vs 쿠키, Origin 검증, DNS rebinding 방어.
  - 멀티플렉싱 메시지 스키마: JSON 프레이밍 vs length-prefixed binary, 헤더+페이로드 구조.
  - 백프레셔(`cat /dev/urandom` 시나리오) + tmux pause/continue 정렬.
  - 재접속/재개: 시퀀스 번호, 리플레이 윈도, 멱등성.
- **자료 수집 방식**: RFC 6455/9220, 주요 라이브러리 docs, 유사 도메인(VS Code Remote, ttyd, gotty, sshx) 프로토콜 사례 분석.
- **산출**: `docs/reports/0004-transport.md` (권장 메시지 스키마를 TS 타입 수준으로 제시)

### R5. 보안 모델

- **조사 대상**: 로컬 단일 사용자 웹앱의 위협 모델 전반. `docs/sketch.md` §13과 직접 매핑.
- **핵심 쟁점**:
  - 바인드 타깃: 127.0.0.1 / unix socket / Tailscale-only — 브라우저가 unix socket을 못 무는 제약 정리.
  - 127.0.0.1 서비스에 대한 **DNS rebinding** 공격과 방어(Host 화이트리스트, 토큰 필수화).
  - WS 핸드셰이크 토큰 전달의 2026년 베스트 프랙티스.
  - 토큰 발급 HTTP 표면의 CSRF.
  - XSS: pane 라벨/노트/tmux 이름 렌더링 규칙. `dangerouslySetInnerHTML` 절대 금지 근거.
  - **명령 주입**: tmux 명령 화이트리스트 + argv 분리, 쉘 보간 금지.
  - 터미널 출력 기반 공격(OSC 8/52, title set 등) 비활성 목록.
  - 시크릿 at-rest: 토큰 저장 위치, 파일 권한.
- **자료 수집 방식**: OWASP, MDN(WebSocket/Origin), Chromium의 private network access 문서, 유사 로컬 도구(Jupyter, Docker Desktop)의 토큰 패턴.
- **산출**: `docs/reports/0005-security-model.md`

### R6. 레이아웃 영속화

- **조사 대상**: 웹 상태(패널 geometry, 플래그, 라벨/노트, 그룹, 뷰포트, 저장된 레이아웃) 저장 전략. tmux 상태는 저장하지 않음.
- **핵심 쟁점**:
  - **안정 식별자**: tmux pane id(`%N`)는 서버 재시작 시 불안정 → (session_name, window_index, pane_index, command, start_time) 등의 지문 전략 비교.
  - 저장소 선택: SQLite / 단일 JSON + atomic rename / 임베디드 KV(BoltDB·sled) / IndexedDB. 단일 사용자 로컬에서의 트레이드오프.
  - 스키마 버전·마이그레이션.
  - 동시 쓰기(여러 브라우저 탭) — 금지 vs 락.
  - 백업/익스포트 포맷.
- **자료 수집 방식**: 각 저장소 공식 docs, SQLite 베스트 프랙티스, 유사 로컬 앱(Obsidian, Zed) 데이터 모델 사례.
- **산출**: `docs/reports/0006-layout-persistence.md` (권장 스키마를 SQL 또는 JSON Schema로)

### R7. 백엔드 런타임

- **조사 대상**: Go / Rust / Node.js (대안 있으면 추가).
- **핵심 쟁점**:
  - 자식 프로세스 + pty 라이브러리 성숙도: `os/exec`+`creack/pty`, `tokio::process`+`portable-pty`, `node-pty`.
  - WebSocket 서버 라이브러리: `nhooyr/websocket`/`gorilla`, `axum`+`tokio-tungstenite`, `ws`/`uWebSockets.js`.
  - 프론트엔드 정적 자산을 바이너리에 임베딩하는 방법.
  - 크로스 컴파일(macOS/Linux. Windows는 MVP 제외 — tmux가 unix 전용).
  - 유휴 메모리 풋프린트.
- **자료 수집 방식**: 각 라이브러리 repo + 벤치마크, 유사 도구(`ttyd`, `gotty`, `wetty`, `sshx`)의 스택 선택 사례.
- **산출**: `docs/reports/0007-backend-runtime.md` (랭크 + 결정적 기준)

### R8. 프론트엔드 스택

- **조사 대상**: React 19 / Svelte 5 / SolidJS / Vue 3, 상태관리 후보(Zustand/Jotai/Valtio/Redux Toolkit/XState), 빌드 도구.
- **핵심 쟁점**:
  - **명령형 라이브러리(xterm.js) + R3 캔버스 라이브러리** 와의 통합 난이도.
  - **두 스토어 분리(§1.1)** 를 가장 깔끔히 강제할 수 있는 상태관리 도구.
  - 구독 효율성: "해당 pane만 리렌더" 보장 능력.
  - 빌드 도구는 Vite 가정, 더 나은 게 있으면 근거 제시.
- **자료 수집 방식**: 각 도구 공식 docs, 캔버스+터미널 결합 사례(VS Code, code-server, sshx, Theia).
- **산출**: `docs/reports/0008-frontend-stack.md`

---

## 5. 인계 체크리스트

리서처가 작업을 시작하기 전에 확인할 것:

- [ ] `docs/sketch.md` §4, §8, §10, §11, §12, §13, §15 1독.
- [ ] `docs/plans/0001-research-plan.md` 의 트랙 매핑 확인.
- [ ] 본 문서 §1(절대 전제)와 §3(보고서 템플릿) 숙지.
- [ ] 트랙 실행 순서 합의 (기본 권장: R1 → R4 → R5 → R2 → R3 → R7 → R8 → R6).
- [ ] 각 트랙 종료마다 보고서 PR/커밋 단위로 인계.
