# 리서치 계획: gtmux 사전 자료 수집

본 문서는 `codebase/`에 코드를 작성하기 전, 그리고 첫 ADR을 쓰기 전에 수집해야 할 외부 자료의 범위와 검색 프롬프트를 정의한다. 산출물은 각 주제별로 `docs/reports/NNNN-<주제>.md` (한국어) 로 남긴다.

## 0. 리서치 원칙

- 1차 출처(tmux man page, RFC, 공식 docs, 라이브러리 README/소스) > 2차 출처(블로그) > 3차(요약). 블로그는 1차로 가는 포인터로만 사용.
- 모든 보고서는 인용(URL + 접근일자)을 남긴다.
- 코드 예시는 짧게, 결론·트레이드오프·gtmux 적용 함의를 우선 기록.
- 결론은 `docs/sketch.md`의 아키텍처 불변식(상태 2분리, 컨트롤 모드, 캔버스≠tmux 레이아웃, 보안 기본값)을 깨지 않는지 검증할 것.

## 1. 리서치 트랙 (P0 우선순위와 정렬)

| # | 트랙 | 산출 보고서 | 대응 P0 항목 |
|---|------|-------------|--------------|
| R1 | tmux control mode (`tmux -C`) 프로토콜·이벤트·한계 | `docs/reports/0001-tmux-control-mode.md` | tmux 연결, 세션/윈도우/pane 리스팅 |
| R2 | 브라우저 터미널 렌더링 (xterm.js 및 대안) | `docs/reports/0002-terminal-rendering.md` | pane 터미널 렌더·입력 |
| R3 | 무한 캔버스/패널 UI 라이브러리 | `docs/reports/0003-infinite-canvas.md` | 캔버스 패널 배치 |
| R4 | 실시간 전송 계층 (WebSocket vs SSE, 백프레셔, 멀티플렉싱) | `docs/reports/0004-transport.md` | 스트리밍 출력, 입력 왕복 |
| R5 | 로컬 단일사용자 웹앱 보안 모델 (토큰, origin, CSRF, 유닉스 소켓) | `docs/reports/0005-security-model.md` | §13 위협 모델 |
| R6 | 레이아웃/뷰포트 영속화 패턴 (스키마, 마이그레이션, 저장소 선택) | `docs/reports/0006-layout-persistence.md` | 레이아웃 영속화 |
| R7 | 백엔드 언어/런타임 후보 비교 (Go / Rust / Node) — pty 핸들링·tmux 프로세스 관리 관점 | `docs/reports/0007-backend-runtime.md` | 엔진 부트스트랩 |
| R8 | 프론트엔드 프레임워크/상태관리 후보 (React+Zustand/Jotai, Svelte 등) — 캔버스·터미널 결합 | `docs/reports/0008-frontend-stack.md` | UI 부트스트랩 |

R1–R5는 첫 ADR(컨트롤 모드 통합 전략) 전제 자료. R6–R8은 두 번째 ADR 묶음 전제.

## 2. 각 트랙 핵심 질문

### R1. tmux control mode
- `tmux -C` / `control mode client`의 메시지 포맷, `%begin/%end/%error`, `%output`, `%session-changed`, `%window-add/close`, `%layout-change` 등 알려진 notification 전체 목록.
- 명령 큐잉과 응답 매칭 규칙. 출력 인코딩(이스케이프, 멀티바이트, 바이너리).
- `iTerm2`가 control mode를 어떻게 쓰는지(레퍼런스 구현).
- 한계: 어떤 정보는 control mode로 안 오고 별도 `list-*` 폴링이 필요한가.
- 버전 호환성: tmux 3.x 이상에서 변경된 메시지.

### R2. 터미널 렌더링
- xterm.js v5.x: 성능(많은 pane 동시 렌더), addon (WebGL, fit, search, web-links, attach), 입력 이벤트, 백버퍼 크기.
- 대안: hterm, Anuviewer, custom WebGL.
- pane N개를 동시에 켜둘 때의 메모리/프레임 코스트와 가상화 전략(off-screen pane은 paused).

### R3. 무한 캔버스
- React Flow / tldraw / Konva / Excalidraw 코어 / Pixi.js — 무한 팬·줌, 패널(노드) 내부에 외부 DOM(터미널) 마운트 가능 여부, z-index/잠금/그룹 모델, 시리얼라이즈 형식.
- 캔버스 좌표계와 터미널 리사이즈(`cols×rows`)의 매핑 전략.

### R4. 전송 계층
- WebSocket vs SSE vs WebTransport: 양방향 필요성(입력+출력)에서 WS가 기본. 인증 핸드셰이크, Origin 검증, subprotocol.
- 단일 소켓에서 여러 pane 스트림 멀티플렉싱하는 메시지 스키마(예: 길이 프리픽스, JSON 헤더+바이너리 payload).
- 백프레셔(터미널이 토해내는 양이 많을 때 클라이언트 처리속도와 어떻게 맞추나).

### R5. 보안
- `127.0.0.1` 바인드 vs Unix socket vs Tailscale-only — 트레이드오프.
- WebSocket에서 토큰 전달 방식(쿼리스트링 vs Sec-WebSocket-Protocol vs 쿠키), CSRF, DNS rebinding 방어.
- tmux 명령 화이트리스트와 argv 분리 (`execve` 스타일) — 쉘 보간 금지 패턴.
- xterm.js 출력의 렌더링 안전성 (악성 이스케이프 시퀀스, OSC 8 링크).

### R6. 레이아웃 영속화
- 스키마: `{paneId(tmux %ID), x, y, w, h, z, hidden, minimized, locked, label, note, groupId}`.
- 저장소 후보: SQLite(파일 기반, 마이그레이션 용이) vs 단일 JSON 파일 vs IndexedDB(클라이언트만).
- tmux pane 식별자(`%N`)가 세션 재시작 시 어떻게 바뀌는지 — 안정 키 설계.
- 마이그레이션 전략(스키마 버전 필드).

### R7. 백엔드 런타임
- tmux 프로세스를 자식으로 띄우고 stdio 파이프로 control mode를 말하기 좋은 언어/라이브러리.
- Go (`os/exec`, `creack/pty`), Rust (`tokio::process`, `portable-pty`), Node (`node-pty`).
- 단일 바이너리 배포 용이성, 정적 자산 임베딩(웹 프론트 같이), 메모리 풋프린트.

### R8. 프론트엔드 스택
- 캔버스 라이브러리와 자연스럽게 결합되는 프레임워크 선택.
- 터미널 컴포넌트(외부 DOM)와 가상 DOM 리렌더의 충돌 회피.
- 상태관리 후보(Zustand/Jotai/Valtio/Redux Toolkit) — `tmux state`와 `web state` 두 스토어 분리 원칙 구현 용이성.

## 3. 보고서 템플릿

각 보고서는 다음 섹션을 반드시 포함한다.

```
# 보고서: <주제>

## 요약 (3문장)
## 조사 범위와 질문
## 핵심 발견
## 옵션 비교표
## gtmux에의 함의 (불변식 검증 포함)
## 미해결 질문 / 후속 ADR 필요 항목
## 출처 (URL + 접근일자)
```

## 4. 실행 순서

1. R1, R4, R5 (전송·tmux·보안 — 백엔드 ADR 1의 전제)
2. R2, R3 (프론트엔드 ADR의 전제)
3. R7, R8 (런타임/프레임워크 선택 ADR)
4. R6 (영속화 ADR — 데이터가 흐를 길이 보인 뒤)

각 트랙 보고서가 끝나면 해당 ADR을 작성한다.
