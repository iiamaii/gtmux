# tmux-backed Web Canvas Workspace 프로젝트 문서

## 1. 프로젝트 개요

### 1.1 프로젝트명

**tmux-backed Web Canvas Workspace**

### 1.2 한 줄 정의

`tmux`를 백엔드 실행 엔진으로 사용하고, 웹앱에서 세션·윈도우·pane을 무한 캔버스 기반 GUI로 시각화·제어하는 단일 사용자용 로컬/개인 서버 웹 애플리케이션.

### 1.3 프로젝트 범위

본 프로젝트는 다음 범위로 제한한다.

* **배포 형태**: 로컬 서버 또는 개인 클라우드 서버에서 실행
* **접속 방식**: 서버 활성화 후 브라우저로 접속하여 사용하는 웹앱
* **사용자 범위**: 단일 사용자
* **비범위**:

  * SaaS 멀티테넌시
  * 사용자 관리, 회원가입, 팀 권한 관리
  * 조직 단위 협업 기능
  * 엔터프라이즈용 중앙 인증 체계

이 범위 제한은 제품 복잡도를 낮추고, 우선적으로 핵심 UX 및 tmux 연동 품질에 집중하기 위한 것이다.

---

## 2. 목표와 목적

### 2.1 목표

본 프로젝트의 목표는 터미널 작업 환경을 단순한 텍스트 기반 분할 화면에서 벗어나, **시각적 작업공간으로 재구성된 tmux GUI 워크스페이스**로 확장하는 것이다.

### 2.2 목적

1. **tmux의 강력한 세션 유지/프로세스 관리 기능을 그대로 활용**한다.
2. **웹 기반 GUI 상에서 pane을 자유롭게 배치·가시화**하여 작업 맥락 파악을 쉽게 한다.
3. **session/window/pane 제어를 직관적인 UI 액션으로 변환**하여 사용성을 높인다.
4. **단일 사용자 중심의 빠르고 안정적인 운영 환경**을 제공한다.
5. **캔버스 기반 상호작용**을 통해 다수의 pane을 시각적으로 정리하고 탐색할 수 있도록 한다.

### 2.3 기대 효과

* 복잡한 tmux 작업공간을 더 빠르게 탐색 가능
* 장시간 유지되는 pane과 프로세스를 시각적으로 분류 가능
* 동일 세션 내 여러 pane의 역할과 상태를 한눈에 파악 가능
* CLI 중심 숙련자에게는 tmux의 장점을 유지하면서 GUI 생산성을 추가 제공

---

## 3. 문제 정의

기존 tmux는 매우 강력하지만 다음과 같은 한계가 있다.

1. **시각적 탐색성이 낮다**.

   * pane 수가 많아질수록 현재 구조를 빠르게 파악하기 어렵다.
2. **window 경계가 강하게 작동한다**.

   * session 전체의 pane을 한 시각적 작업공간에서 보기 어렵다.
3. **GUI 수준 조작이 부족하다**.

   * hide, minimize, 자유 배치, 시각적 그룹화 같은 상호작용이 제한적이다.
4. **상태와 맥락 관리가 텍스트 중심**이다.

   * 어떤 pane이 어떤 역할을 하는지 구조적으로 표현하기 어렵다.

본 프로젝트는 이 문제를 **tmux 엔진 + 웹 캔버스 표현 계층**으로 분리하여 해결한다.

---

## 4. 핵심 개념과 설계 원칙

### 4.1 핵심 설계 원칙

1. **tmux는 실행 엔진이다**.

   * 실제 프로세스, 세션, 윈도우, pane 생명주기는 tmux가 관리한다.
2. **웹앱은 시각화 및 조작 엔진이다**.

   * 캔버스, 패널, 표시 상태, 레이아웃, 잠금 상태 등은 웹앱이 관리한다.
3. **tmux layout과 web canvas layout은 분리한다**.

   * tmux의 native 분할 레이아웃과 웹캔버스상의 자유 배치는 동일 개념이 아니다.
   * gtmux는 **single-pane-per-window 컨벤션**(ADR-0008)을 따른다 — gtmux UI에서 생성되는 모든 tmux window는 1개의 pane만 담는다. 따라서 gtmux-created 영역의 tmux Layout은 trivial이며, 의미를 가지는 tmux Layout은 외부 tmux CLI로 생성된 multi-pane window뿐이다.
4. **tmux-native 기능과 web-only 기능을 구분한다**.

   * 닫기, 생성, 선택, 이름 변경은 tmux 명령으로 연결
   * visibility, minimize, lock, z-index, canvas grouping은 웹 상태로 처리
5. **단일 사용자 환경에 최적화한다**.

   * 권한 관리 복잡도를 줄이고, 대신 로컬/개인 서버 보안과 안전한 기본값에 집중한다.

### 4.2 개념 모델

#### 실행 모델

* Session
* Window
* Pane

#### 표현 모델

* Canvas
* Panel
* Panel position / size / z-index
* Visibility
* Locked state
* Minimized / Maximized state
* Pane list grouping
* Selection / focus state

### 4.3 중요 정의

#### tmux 측 (mirror)
* **Pane**: tmux가 관리하는 실제 실행 단위 (PTY + 프로세스 호스트)
* **Window** (*implementation-only, UI 비노출*): tmux 측 Pane 컨테이너. gtmux 컨벤션상 gtmux가 만드는 모든 Window는 정확히 1개의 Pane을 담는다. 외부 CLI로 생성된 multi-pane Window는 보조 상태로 받아들인다.
* **Session**: 한 Server가 1:1로 바인딩하는 tmux session
* **tmux Layout**: 한 Window 내부에서 tmux가 관리하는 native split 구조. Canvas Layout과 절대 분리되며, 둘은 서로 참조하지 않는다.

#### gtmux 측 (own)
* **Server (gtmux Server)**: 한 tmux Session에 1:1 바인딩되어 단일 포트에서 동작하는 gtmux 웹 앱 프로세스. 여러 Server를 다른 포트로 동시 실행 가능.
* **Canvas**: 한 Server가 호스팅하는 단일 무한 작업 공간. Session의 모든 Pane을 Panel로 노출한다.
* **Panel**: Canvas 위에서 한 Pane을 표현하는 시각 객체. 위치/크기/visibility/minimize/lock/z-index/label/note 등 web-only 상태를 가진다.
* **Canvas Layout**: Canvas 위 Panel·Group 트리·상태 집합. HTTP로 영속화된다.
* **Group**: Panel·하위 Group을 묶는 web-only 계층 단위 (Figma-식 layer). frame은 1차 상태로 저장하지 않으며 (G-hybrid), label·color·visibility·lock·order만 보관. tmux Window를 UI 측에서 대체하는 1차 도메인 개념.
* **Manipulation Selection (M)**: 캔버스에서 사용자가 제어 대상으로 잡은 Panel(들). 다중 선택 가능.
* **Input Target (I)**: 키보드 터미널 입력이 라우팅될 단일 Pane. M과 직교.
* **Panel Streaming State**: Panel별 데이터 흐름 활성 여부 (`Streaming` / `Suspended`). visibility/minimize 변화에 따라 tmux `refresh-client -A '%pid:pause/continue'`로 제어.

---

## 5. 사용자 시나리오

### 5.1 대표 사용자

* tmux를 이미 사용하거나 사용할 의향이 있는 단일 고급 사용자
* 개발자, 연구자, 운영자, CLI 중심 워크플로 사용자

### 5.2 주요 사용 시나리오

#### 시나리오 A: 세션 전체 작업공간 시각화

사용자는 하나의 tmux session 내 여러 window/pane을 웹앱에서 한 캔버스 위에 올려놓고, 관련 pane들을 가까이 배치하여 맥락적으로 작업한다.

#### 시나리오 B: 여러 장기 실행 pane 모니터링

사용자는 서버 로그, 학습 프로세스, editor shell, monitoring shell 등을 각각 별도 panel로 보고, 필요 없는 panel은 minimize 또는 hide 처리한다.

#### 시나리오 C: GUI 조작과 CLI 제어 병행

사용자는 pane을 마우스로 정리하면서도, 특정 작업은 직접 tmux command palette나 terminal 입력을 통해 수행한다.

#### 시나리오 D: 세션 복원

서버 재시작 또는 웹앱 재접속 후, 사용자는 기존 tmux session과 저장된 canvas layout을 다시 불러와 이어서 작업한다.

---

## 6. 핵심 기능

### 6.1 tmux 세션 제어 기능 — gtmux UI 비범위

Session 생성·목록·선택·전환·종료·이름변경·attach 대상 선택은 **모두 gtmux UI 밖**이다. 사용자 OS·tmux CLI·외부 도구가 담당한다 (ADR-0007 1:1:1 모델). Server는 부팅 시 CLI 인자(`--session <name> --port <port>`)로 받은 Session에 immutable 바인딩되며, 런타임 중 변경 불가하다. 활성 Server 목록·오케스트레이션 도구는 별도 프로젝트로 분리한다.

### 6.2 Group 관리 기능 (web-only, tmux Window 대체)

tmux Window는 gtmux UI에 노출되지 않는다 (ADR-0008 single-pane-per-window 컨벤션). 사용자 측 Panel 묶음은 **Group** (web-only 계층 단위)이 담당한다.

* Group 생성 — Panel/Group 다중 선택(M) → `Group` 액션
* Group 해체 — Group 단일 선택 → `Ungroup` 액션 (자식은 grandparent 또는 루트로 reparent)
* Group label/color 설정
* Group visibility 토글 (자손에게 AND 전파)
* Group lock 토글 (자손에게 AND 전파)
* Group 이동 (자손 Panel 좌표에 드래그 delta 일괄 적용)
* Group 삭제 (destructive — 자손 모든 Pane을 `kill-pane`. §7.6 confirm modal 필수)
* Group 트리 사이드바 (Figma-식 layer panel) — drag-reparent UX 포함

빈 Group 자동 prune은 하지 않는다. Group resize는 MVP 미지원 (P1+).

### 6.3 tmux pane 제어 기능

* pane 생성 — gtmux는 항상 `new-window -t <session>`을 발급 (single-pane-per-window 컨벤션, ADR-0008). 신규 Panel 좌표는 사용자 명시 입력 (placement principle).
* pane 닫기 — `kill-pane`. M으로 선택된 Panel(들)에 일괄 적용. Destructive confirm modal 필수.
* pane 선택 (M 대상) — 캔버스 Panel 클릭 또는 사이드바에서 선택. 다중 선택 가능.
* pane 포커스 변경 (I 대상) — Panel 터미널 영역 클릭/타이핑 시 set.
* pane 이름 또는 title 기반 식별 표시 — Panel header에 pane identifier + 사용자 label
* pane 현재 command/path 표시 — tmux subscription(`refresh-client -B`)으로 동기화
* pane 입력 전달 — `send-keys -t %<pane>`
* pane 출력 수신 및 렌더링 — `%output` 디코딩 → per-pane ring buffer (128KB, 설정 가능, D15) → WS binary frame → xterm.js `write(Uint8Array)`
* pane resize — single-pane-per-window 컨벤션 하에서 window resize = pane resize. **gtmux backend는 `resize-pane`/`split-window`/`select-layout`을 발급하지 않는다** (allowlist 제외, ADR-0008).

### 6.4 캔버스 기반 GUI 기능

* 무한 캔버스 기반 작업공간 (Server당 1개 Canvas, 1개 Session 대응)
* panel drag & drop (M 대상에 적용)
* panel resize (M 대상에 적용 — single-pane Window는 자유, multi-pane window mirror는 resize lock)
* panel z-index 조정 (Bring to front / Send to back / Up one / Down one). 선택(M)된 Panel은 자동으로 z 최상위로 점프 후 유지 (D23).
* Panel은 자유롭게 겹칠 수 있다 (overlap 허용, D23).
* panel header 표시

  * Group label (속한 Group이 있을 때)
  * pane identifier
  * pane title 또는 사용자 지정 label
  * 활성 표시 (M 포함 여부, I 여부)
  * Streaming/Suspended 상태 배지
* panel minimize / maximize (web-only, Panel Streaming State 전이 트리거)
* panel show / hide (= visibility=hidden, web-only)
* panel lock (web-only)
* panel close (= `kill-pane`, tmux-native, destructive confirm)
* 다중 panel 선택 (= M 다중 등록)
* canvas pan / zoom — **모든 WS 연결 sync** (MT-3 D13: 동일 사용자의 거울 뷰)

### 6.5 Pane List / 탐색 기능 (Group 트리 사이드바)

사이드바는 **Figma-식 layer panel**로 구현된다. tmux Window 기반 그룹화는 노출하지 않는다.

* Group 트리 (계층 expand/collapse)
* 트리 노드 = Panel 또는 Group
* 활성 표시 (M 포함, I 대상)
* hidden/minimized/locked/Suspended 상태 배지
* 클릭 시 해당 panel을 M에 등록 + viewport pan (옵션)
* 사이드바 내 drag-reparent (Group 트리 재구조화, MVP)
* pane 검색 (P1+)
* pane 필터링 (P1+)

  * current command
  * Group
  * visibility/streaming 상태

### 6.6 명령 전달 기능

* UI 버튼을 통한 tmux 제어
* command palette를 통한 tmux command 실행
* quick actions

  * new pane
  * new window
  * rename
  * kill
  * focus
  * zoom 관련 액션
* 선택 pane 기준 명령 실행

---

## 7. UI/UX 관점에서 필요한 추가 기능

아래 기능들은 초기 논의 범위를 넘어서는 보완 요소지만, 실제 사용성과 생산성을 고려할 때 포함 가치가 높다.

### 7.1 정보 가독성 및 식별성 강화

* **사용자 지정 panel label**

  * tmux name 외에 사용자가 panel 별 별칭 지정 가능
* **상태 배지 표시**

  * active, hidden, minimized, locked, disconnected 등
* **Group 색상 태그 또는 카테고리 표시** (P1+)

  * 시각적 식별성 향상. Group label과 함께 자손 Panel header에도 색 띠로 표시.
* **pane 역할 메모 또는 short note**

  * 예: train, logs, server, scratch

### 7.2 캔버스 편집 편의 기능

* snap to grid
* align/distribute 기능
* panel 자동 정렬
* fit to view
* selected panel만 보기
* mini-map
* undo / redo
* reset layout
* save layout as preset

### 7.3 탐색성과 집중도 향상 기능

* panel highlight on hover/list selection
* focus mode

  * 특정 panel만 크게 보고 나머지는 dim 처리
* quick jump

  * pane 이름 또는 command로 빠른 이동
* 최근 활성 pane 기록
* 즐겨찾기 pane 고정

### 7.4 운영 편의 기능

* **자동 재연결** (D21 c2·c3) — WS 끊김 시 1s grace + exponential backoff (cap 30s) indefinite retry. 10회 연속 실패 시 "Server stopped" 배너로 사용자 행동 유도.
* **Port 기반 재기동** (D21 c6) — 사용자가 URL을 북마크 (`http://localhost:9001/`) 한 뒤, `gtmux start --port 9001` 한 줄로 같은 session 재attach. port가 영속 식별자.
* **Pane zombie 보존** (D21 c4) — 프로세스 종료된 pane은 자동 제거 없이 badge로 표시. 사용자가 직접 close.
* canvas layout 자동 저장 (D12 HTTP PUT + 300ms debounce, 설정 가능)
* 최근 session 재열기 — port 영속 인덱스로 자연 충족
* 시작 시 마지막 workspace 복원 — Server 부팅 인자(`--port`)가 결정 (D2)
* pane 생성 템플릿 (P1+)

  * 예: bash, zsh, python, monitoring

### 7.5 터미널 사용성 향상

* 폰트 크기 조절
* line wrapping 설정
* terminal theme 설정
* copy/paste 개선
* terminal scrollback 길이 설정
* terminal fit 재계산 버튼
* pane detach 표시 또는 reconnect 안내

### 7.6 접근성 및 기본 UX 품질

* 키보드 단축키
* 고대비 모드 고려
* 충분한 hit target
* 시각 상태 변화의 명확한 피드백
* destructive action 확인 모달

  * pane/window/session 종료 시

---

## 8. 기능 분류: tmux-native vs web-only

### 8.1 tmux-native 기능

다음 기능은 tmux 명령과 직접 연결한다.

* session 생성/종료/선택/이름변경
* window 생성/종료/선택/이름변경
* pane 생성/종료/선택
* 현재 pane 정보 조회
* pane command/path 조회
* 입력 전달
* layout 관련 기본 제어

### 8.2 web-only 기능

다음 기능은 웹앱의 자체 상태로 관리한다.

* canvas 위치 및 크기
* visibility hidden
* minimized / maximized
* lock
* z-index
* panel grouping
* 사용자 정의 label / note
* focus mode
* saved layout preset

이 분리는 설계 충돌을 줄이고 유지보수를 용이하게 한다.

---

## 9. 시스템 요구사항

### 9.1 기능 요구사항

#### 필수 기능 요구사항

1. ~~사용자는 웹앱에서 tmux session 목록을 확인할 수 있어야 한다.~~ → §6.1로 인해 UI 비범위. Server는 한 Session에 1:1 바인딩.
2. ~~사용자는 새로운 session을 생성할 수 있어야 한다.~~ → UI 비범위. tmux CLI/외부 도구가 담당.
3. 사용자는 **Pane을 생성/종료/선택**하고, 사용자 측 묶음 단위로 **Group을 생성/해체/관리**할 수 있어야 한다.
4. 사용자는 바인딩된 Session 내 Pane들을 캔버스 Panel 형태로 볼 수 있어야 한다.
5. 사용자는 Panel을 드래그/리사이즈할 수 있어야 한다 (Group 단위 드래그 일괄 이동 포함).
6. 사용자는 Panel의 visible/minimized/locked 상태를 조작할 수 있어야 한다 (Group으로 일괄 적용 가능).
7. 사용자는 Group 트리 사이드바에서 Panel을 탐색하고 선택할 수 있어야 한다.
8. 사용자는 Panel terminal에 직접 입력하고 출력을 볼 수 있어야 한다.
9. 사용자는 UI를 통해 allowlist 범위 안의 tmux 명령을 실행할 수 있어야 한다.
10. 사용자는 웹앱 재접속 시 이전 Canvas Layout(HTTP)을 복원하고, 각 Pane의 최근 출력 히스토리(per-pane ring buffer)를 즉시 볼 수 있어야 한다.

#### 선택 기능 요구사항

1. 사용자는 panel label과 메모를 설정할 수 있다.
2. 사용자는 layout preset을 저장 및 불러올 수 있다.
3. 사용자는 단축키로 주요 동작을 수행할 수 있다.
4. 사용자는 pane 검색 및 필터를 사용할 수 있다.
5. 사용자는 focus mode 또는 highlight 기능을 사용할 수 있다.

### 9.2 비기능 요구사항

#### 성능 (D19 정량 기준)

* **동시 panel**: MVP 50개 / stretch 100개. 그 이상은 P1+ 측정 후 결정.
* **Cold start (`gtmux start` → 첫 paint)**: < 500ms (stretch < 300ms).
* **Warm reconnect (브라우저 새 탭, daemon·서버 살아 있음)**: < 300ms (stretch < 200ms).
* **Per-pane output latency** (process stdout → 픽셀): p50 < 30ms, p99 < 100ms (stretch p50 < 15ms / p99 < 50ms).
* **Panel drag commit → 모든 연결 sync 완료**: < 500ms (stretch < 300ms).
* **Server backend memory baseline**: < 30 MB (stretch < 20 MB). Per-Server total (gtmux + tmux daemon + buffers) < 50 MB.
* **Frontend tab memory**: < 100 MB (stretch < 60 MB).
* **HTTP `PUT /api/layout` 페이로드 상한**: 256 KB (SSoT 강제).
* **WS write lag 한계** (gtmux → 브라우저, tmux 대비): < 5s (stretch < 1s). 초과 시 D16의 Panel Streaming State Suspend 자동 전환.
* **동시 WS 연결 cap (MT-3)**: MVP 없음 / 권장 ≤ 10.
* pane 출력이 많은 경우 backpressure (D16의 pause-after + Panel Streaming State)로 브라우저·gtmux·tmux 사이 흐름 제어.

#### 안정성

* tmux session이 웹 UI 재시작과 무관하게 유지되어야 한다.
* 웹앱이 새로고침되어도 재연결 가능한 구조여야 한다.

#### 사용성

* 직관적인 패널 조작이 가능해야 한다.
* 텍스트 기반 tmux를 모르는 사용자라도 기본적인 기능은 UI만으로 사용할 수 있어야 한다.

#### 유지보수성

* tmux state와 canvas state가 명확히 분리되어야 한다.
* command routing과 UI state management는 모듈화되어야 한다.

---

## 10. 제안 아키텍처

### 10.1 백엔드 구성 (**2026-05-14 amend — ADR-0013 채택**)

* **Process supervisor** (`crates/pty-backend`, ADR-0014) — portable-pty 0.9 위에서 PTY pair + child process (shell) 의 단일 owner. Pane = 1 PTY + 1 child process 의 1:1:1 (Pane:PTY:process). spawn / wait / reap / resize / kill 의 단일 책임.
* **WebSocket server** — ephemeral 신호 (live pane output, M/I/viewport/focus/LAYOUT_CHANGED notify) 만 담당. tokio::broadcast 로 multi-attach mirror trivial 달성 (ADR-0013 D11).
* **HTTP API server** — durable 영속화 (`GET/PUT /api/layout` + ETag, D12). Origin/Host/CSRF 미들웨어 통합.
* **Wire router** (구 `mux-router`, 의미 단순화) — WS envelope 의 0x01 CTRL payload 를 우리 API command schema enum (`new-pane` / `kill-pane` / `resize-pane` / `set-cwd` / `set-env` 등) 으로 dispatch. Rust enum exhaustive match 가 compile-time allowlist 역할 (ADR-0013 D12).
* **PTY I/O loop** — 각 Pane 마다 std::thread × 2 (master fd reader + writer) + tokio child-watcher thread. reader 가 `Bytes` chunk 를 `tokio::broadcast::Sender<Bytes>` 로 fan-out, writer 가 `tokio::mpsc::UnboundedReceiver<Vec<u8>>` 를 drain. per-pane ring buffer (128 KiB 기본, ADR-0001 D7 정신 계승) 가 재attach replay 용.
* **WS notify dispatcher** — ADR-0002 D2 의 0x01–0x0F (PTY-domain, 2026-05-14 amend) + 0x80–0x8F (web-domain) envelope 발송.
* **Lifecycle manager** — gtmux Server 기동 시 lock 파일 확인 + bind port, `gtmux teardown` 시 모든 child SIGTERM → 자손 reap + state/lock 파일 정리 (ADR-0014 D7 의 4단계). CLI 인터페이스 (`start`/`stop`/`teardown`/`rotate-token`/`status`) 는 D20 정의 따름. 디렉터리 컨벤션: `${XDG_CONFIG_HOME}/gtmux/` (config), `${XDG_STATE_HOME}/gtmux/` (token·layout·lock), `${XDG_RUNTIME_DIR}/gtmux/` (pid). (구) `${TMUX_TMPDIR}/tmux-${uid}/gtmux-<session>` 소켓 컨벤션 폐기 — tmux 가 없음.
* **Auth manager** — 256-bit CSPRNG 토큰 발급/검증/회전 (D17). 매 서버 시작 시 재발급(local) / 영속+명시 회전(cloud).
* **Local config manager** — 사용자 환경 옵션 (ring buffer 크기, 디바운스 시간 등).

> (구) "tmux control mode client" / "tmux command router" / "session·pane state collector (%output/%pane-*/%window-*)" 컴포넌트 모델은 ADR-0001 deprecation 과 함께 폐기. 위 *Process supervisor* + *Wire router* + *PTY I/O loop* 가 그 책임을 흡수.

### 10.2 프런트엔드 구성

* infinite canvas renderer
* panel manager
* terminal renderer
* pane list sidebar
* toolbar / command palette
* state store
* layout manager

### 10.3 상태 분리 구조 (**2026-05-14 amend**)

#### PTY-domain state (gtmux Server 가 직접 관리, 구 "tmux state")

* logical Session (사용자 부여 식별자)
* Panes (= PTY pair + child process, 1:1:1)
* Pane 메타데이터 (cwd, env, exit status)
* live byte stream (PTY master fd)

#### web state

* panel geometry
* visibility
* minimize/maximize
* lock
* z-index
* notes/labels
* selected elements
* viewport state

---

## 11. MVP 명세

### 11.1 MVP 목표 (**2026-05-14 amend**)

MVP의 목적은 다음을 검증하는 것이다.

1. PTY direct backend (process supervisor, ADR-0014) 위에서 웹 캔버스 UI 가 안정적으로 동작하는가
2. logical Session 단위의 Pane 제어 (생성/종료/select) 가 UI 로 충분히 유의미하게 추상화되는가
3. 단일 사용자 환경에서 장시간 사용 가능한 수준의 실용성이 있는가

### 11.2 MVP 포함 범위 (**2026-05-14 amend**)

#### A. Pane backend (구 "tmux 연동")

* portable-pty 기반 PTY pair 생성 + child process (shell) spawn (ADR-0013 D1·D2)
* Pane 목록 조회 — Server 메모리 의 PTY supervisor 가 진실
* Pane 생성 — 우리 API `new-pane` 호출 (ADR-0013 D10, single-pane-per-process 컨벤션 ADR-0008 amend)
* Pane 종료 — 우리 API `kill-pane` 호출 → SIGTERM → 200ms 후 SIGKILL → wait reap (ADR-0014 D2·D6)
* Pane terminal 출력 표시 (per-pane ring buffer + WS 0x02 PANE_OUT binary frame)
* Pane 입력 전달 — WS 0x03 PANE_IN raw bytes 를 master fd writer 에 그대로 write
* Panel Streaming State 제어 — visibility=hidden 전이 시 dispatcher 가 broadcast subscribe drop (ADR-0002 §D7 amend; 구 `refresh-client -A pause` 컨셉 폐기)
* Resize — WS 0x04 PANE_RESIZE → `MasterPty::resize()` → TIOCSWINSZ → SIGWINCH

#### B. 캔버스 UI

* 무한 캔버스
* panel 생성 및 렌더링
* drag
* resize
* zoom / pan
* panel header에 window/pane 식별자 표시
* minimize / hide / close

#### C. 탐색 및 제어

* pane list sidebar (Figma-식 layer panel)
* Group 계층 트리 정렬 (tmux Window 별 grouping은 §6.5에 따라 폐기됨)
* pane 검색 (P1+)
* active pane 표시 (M / I 분리, D6)
* command palette 또는 기본 quick action 버튼

#### D. 상태 저장 (**2026-05-14 amend**)

* Canvas Layout 저장 — HTTP `PUT /api/layout` + ETag (durable). 클라이언트측 디바운스 300ms 기본 (설정 가능).
* 재접속 시 layout 복원 — HTTP `GET /api/layout` 한 번 호출로 즉시 복원.
* "마지막 session" 복원은 Server **부팅 인자** 가 결정 (D2). `--port` 만 명시해도 config 파일의 port↔session 매핑으로 자동 lookup (D21 c6).
* Pane 출력 히스토리는 per-pane ring buffer 즉시 replay. Deep scrollback 은 P1+.
* **Pane 상태 (running 중 child process state) 는 Server 재기동 시 보존되지 않는다** — ADR-0014 D5 amend. (구) tmux daemon 의 외형적 persistence (재attach with control-mode) 모델 폐기. 재기동 후 layout (panel 좌표·label) 만 복원되고, 사용자가 명시 New Panel 액션으로 새 child shell spawn. 이 trade-off 는 ADR-0013 §결과 + ADR-0014 §결과 정본.

### 11.3 MVP 제외 범위

* 다중 사용자 협업
* 계정 시스템
* 복잡한 권한 모델
* 공유 링크 기능
* 모바일 최적화 완성도
* 고급 preset 시스템
* 다중 workspace template
* 완전한 undo/redo 히스토리
* 고급 시각 그룹 편집 기능 (Group resize, Group spatial frame, drag-reparent via 캔버스 hover 등은 P1+)
* 활성 Server 목록 / 멀티-Server 오케스트레이션 도구 (별도 프로젝트)
* Viewport per-connection 분리 (MT-3 의도적 제약 — 멀티 모니터 분할 시나리오 미지원)
* Canvas Layout PATCH 델타 전송 (MVP는 PUT 전체만)
* Deep scrollback UI (`capture-pane` 사용자 명시 호출)
* OS 인증 위임 (PAM/SSH) — cloud 모드 옵션, P1+

### 11.4 MVP 성공 기준

1. 사용자가 하나의 session에서 여러 pane을 웹 panel로 열람 가능
2. 사용자가 pane을 캔버스에서 정리하고 관리 가능
3. 사용자가 pane 생성/종료/선택을 UI로 수행 가능
4. 웹앱 재접속 후 session과 layout을 복원 가능
5. 일반적인 터미널 작업이 끊김 없이 수행 가능

---

## 12. 상세 기능 우선순위

### 12.1 P0 (반드시 필요)

* tmux 연결
* session/window/pane 조회
* pane terminal 렌더링
* pane 입력 전달
* canvas panel 배치
* pane list
* create/close/select
* layout persistence

### 12.2 P1 (강하게 권장)

* pane 검색
* custom label
* focus/highlight
* auto reconnect
* fit to view
* keyboard shortcut
* confirm modal for destructive action

### 12.3 P2 (후속 단계)

* preset layout
* mini-map
* undo/redo
* panel note
* grid/snapping
* advanced filtering
* usage telemetry for local debug

---

## 13. 보안 및 취약점 고려사항

본 프로젝트는 SaaS가 아니며 단일 사용자 환경이지만, **보안을 가볍게 보면 안 된다**. 특히 서버가 활성화된 상태로 웹앱을 띄우는 구조이므로, 로컬 또는 개인 서버 환경에서도 공격면이 존재한다.

### 13.1 위협 모델

본 프로젝트는 다음 상황을 우선 고려한다.

* 로컬 머신에서 브라우저로 접속
* 개인 클라우드 서버에 올린 뒤 본인이 접속
* 동일 네트워크 또는 외부 노출 환경에서 잘못된 설정으로 접근 가능해지는 상황

### 13.2 핵심 보안 원칙

1. **기본값은 로컬 바인딩 우선**

   * 기본 listen address는 `127.0.0.1` 또는 unix socket 우선
2. **외부 노출은 명시적 설정일 때만 허용**
3. **브라우저 세션과 tmux 제어 채널은 임의 접근이 불가능해야 함**
4. **쉘 명령 실행 경로를 최소화하고 명확히 구분**
5. **웹 UI 입력값은 모두 불신 입력으로 간주**

### 13.3 주요 보안 취약점 및 대응 방향

#### 1. 무단 접근

위험:

* 서버가 0.0.0.0 등으로 열려 있을 경우 외부 접근 가능
* 개인 서버에서 인증 없이 노출되면 tmux 전체 제어권이 노출됨

대응:

* 기본값은 localhost only
* 외부 노출 시 반드시 reverse proxy + TLS + 강한 인증 사용
* 최소한의 단일 사용자 access token 또는 session secret 적용
* origin / host 검증
* CSRF 방어

#### 2. WebSocket 하이재킹 및 세션 탈취

위험:

* 웹소켓으로 terminal 입출력을 주고받기 때문에 세션 탈취 시 조작 가능

대응 (ADR-0003, D17 확정):

* WebSocket handshake 시 인증 토큰 확인 — **`Sec-WebSocket-Protocol` 서브프로토콜로 전달** (쿼리스트링 금지, OWASP 권고)
* HTTP는 **`Authorization: Bearer <token>`** + 보조로 `SameSite=Strict` HttpOnly secure cookie
* 토큰 = **256-bit CSPRNG**, base64url, 상수시간 비교, `${XDG_STATE_HOME}/gtmux/<session>.token` (0600). (D17·ADR-0003 D4 정정 — CONFIG는 사용자 편집 가능, STATE는 머신 발급 자료)
* Origin 헤더 화이트리스트 (CSWSH 방어)
* Host 헤더 화이트리스트 (DNS rebinding 방어)
* 회전: 로컬 = 매 서버 시작 시 재발급. Cloud = 영속 + 명시 회전 명령 (`gtmux rotate-token`).
* idle timeout 및 수동 disconnect 기능

#### 3. 명령 주입 및 입력 검증 부재

위험:

* pane 이름, session 이름, 명령 palette 입력 등에서 shell injection 또는 tmux command injection 발생 가능

대응:

* 쉘을 거치지 않고 명령 인자를 분리 전달
* 허용된 tmux 명령만 라우팅하는 allowlist 중심 설계
* 사용자 입력을 문자열 조합으로 직접 shell 실행하지 않음
* 식별자 검증 및 escaping

#### 4. XSS

위험:

* pane label, note, session 이름, pane title 표시 영역에 스크립트 삽입 가능

대응:

* 모든 사용자 입력 HTML escape
* dangerouslySetInnerHTML 금지
* markdown 허용 시 sanitize 엄격 적용
* CSP 적용

#### 5. 저장 데이터 노출

위험:

* canvas layout, label, note, 최근 session 정보가 파일 또는 로컬 DB에 저장되며 민감 정보 노출 가능

대응:

* 저장 위치 명확화
* 파일 권한 최소화
* 민감 정보는 필요 이상 저장하지 않음
* export/import 기능 시 경로 및 파일 검증

#### 6. PTY + 프로세스 owner 권한 (**2026-05-14 amend — 구 "tmux socket 노출"**)

> ADR-0013 채택으로 tmux 가 사라지면서 *tmux socket 접근 권한 = 제어권* 부류 위험이 자연 소거됨. 대체 위험: 우리 Server 프로세스가 직접 PTY + child shell 의 owner 이므로 *Server 프로세스 자체* 가 침해되면 child shell 모두 영향.

위험:

* gtmux Server 프로세스가 직접 PTY 와 child shell 을 spawn 함 — Server 가 침해되면 사용자 권한의 모든 shell 동작이 노출
* lock 파일 (`${XDG_STATE_HOME}/gtmux/<session>.lock`) 의 권한 부적절 시 동일 사용자의 다른 session 에 중첩 spawn 우려
* PTY master fd 의 byte stream 이 메모리 / 로그 / dump 에 노출될 위험 (예: password prompt 응답)

대응:

* gtmux Server 프로세스 권한 최소화 (사용자 권한, root 거부 — D7 R(rej)6)
* 루트 권한 실행 금지 (`--allow-root` 명시 플래그 없으면 exit 5)
* 상태 디렉터리 권한 0700 자동 강제 + lock 파일 0600 강제 (ADR-0014 D7·D8)
* PTY master fd byte stream 의 디스크 영속 금지 (ADR-0001 §"R6 ring buffer disk persistence" 정신 계승 — 메모리 전용 ring buffer)
* `TMUX` / `TMUX_PANE` / `TERM_PROGRAM` 등 noisy env 명시 제거 (ADR-0014 D10) — nested attach 우발 차단

#### 7. 서비스 거부 또는 브라우저 성능 저하

위험:

* 출력량이 많은 pane으로 인해 websocket/buffer/browser가 과부하될 수 있음

대응:

* output throttling
* buffer limit
* pause/resume 처리
* 비가시 panel은 렌더링 최적화
* scrollback 제한 옵션

#### 8. 개인 서버 운영 상 설정 실수

위험:

* TLS 없이 외부 공개
* 프록시 설정 오류
* 인증 없이 포트 노출

대응:

* 문서화된 안전한 배포 가이드 제공
* 개발 모드와 운영 모드 설정 분리
* unsafe config 경고 표시

### 13.4 단일 사용자 환경에서 권장하는 최소 보안 요구사항

#### 로컬 서버 실행 시

* localhost bind
* 랜덤 세션 토큰
* 브라우저 자동 실행 옵션은 선택형
* CSRF/origin 검증
* 로컬 파일 권한 최소화

#### 개인 클라우드 서버 실행 시

* HTTPS 필수
* reverse proxy 사용
* 외부 접근용 단일 강한 비밀 토큰 또는 기본 인증
* 방화벽으로 허용 IP 제한 권장
* fail2ban 등은 선택 사항이나 권장 가능

### 13.5 보안상 의도적으로 제외하거나 단순화하는 부분

단일 사용자 범위이므로 다음은 초기에는 단순화 가능하다.

* 복잡한 RBAC
* 다중 사용자 권한 분리
* 공유 세션 초대
* 세밀한 감사 로그

단, 아래는 단순화 대상이 아니다.

* 인증 없는 외부 노출 방지
* XSS/CSRF 방어
* 명령 주입 방지
* socket 및 저장 파일 권한 관리

---

## 14. 개발 시 고려해야 하는 기술적 난점 (**2026-05-14 amend**)

1. PTY master fd 의 byte stream 과 Canvas Panel 상태를 안정적으로 동기화해야 한다. (구 "tmux output 과 canvas 상태 동기화" 의 동일 부담 — backend 만 portable-pty 로 단순화.)
2. Pane 이 많아질수록 브라우저 렌더링 최적화가 중요해진다 — hidden Panel 의 broadcast subscribe drop (ADR-0002 §D7 amend) 가 1차 방어.
3. Terminal resize 와 canvas resize 간 연결이 정밀해야 한다 — Pane = PTY 1개 = window-size = pane-size (single-pane-per-process 컨벤션, ADR-0008 amend 후 trivial).
4. Hidden/minimized 상태의 Panel 은 비렌더링 + Panel Streaming State Suspended 전이 (broadcast subscribe drop) 로 데이터 흐름 자체를 차단한다.
5. Reconnect 시 복원 순서: terminal state (per-pane ring buffer replay) → canvas state (HTTP GET /api/layout). ring buffer 가 비어 있는 *새 child process* 의 경우엔 stream 자체가 0 시점에서 시작.
6. **PTY signal race + zombie reap** — child process 의 SIGCHLD 가 polled 시점 vs SIGTERM teardown 시점 race. portable-pty 의 `Child::wait` 가 standard `waitpid` 로 reap 하므로 함정 작음 (POC Gate #4 검증). 그러나 50 pane × 5 burst 시나리오의 실측은 Sprint 7 측정.
7. **Resize burst 폭주 방지** — 사용자 브라우저 창을 빠르게 드래그하면 SIGWINCH 가 폭주. fit() 디바운스 150ms + dedup 으로 흡수 (R2 F8 정합).
8. **Shell exit 시 graceful 처리** — 사용자 명시 [Close] 클릭 외에 shell 의 자체 `exit` / Ctrl-D 가 child process 종료 → `pane-died` NOTIFY_MIRROR broadcast → frontend 에서 dead placeholder 표시 또는 panel 자동 제거 (CONTEXT.md §"Pane lifecycle invariant" 정합).
9. **(구) "long-suspend tmux buffer 동작 검증" 폐기** — tmux 가 없으므로 무의미. 대체 검증: 우리 측 broadcast cap (512 chunk 기본) 가 long-idle subscriber 의 lag 처리에서 RecvError::Lagged 를 발생시키는 시점 — 그때 client 가 자동 re-subscribe + ring buffer replay 로 catch-up.
10. **PTY edge case 누적** — alt-screen / OSC 시퀀스 / $TERM 변종 / readline 의 모디파이어 키맵 (ADR-0013 O8 의 xterm Shift/Option) — portable-pty 가 흡수하지 못하는 edge 는 Sprint 7 데모 안정화 cycle 에서 발견·수정.

---

## 15. 개발 우선순위 제안

**선행 조건 (1단계 시작 전 필수, 2026-05-14 amend)**: ADR-0002 (전송 + wire-protocol SSoT) / ADR-0003 (보안 디폴트) / ADR-0007 (1:1:1 모델) / ADR-0008 (single-pane + Group, allowlist 절은 ADR-0013 으로 폐기) / ADR-0010 (Group 데이터 모델) / **ADR-0011 (백엔드 stack = Rust + axum)** / **ADR-0012 (프론트엔드 stack = Svelte 5 + Vite)** / **ADR-0013 (PTY direct, no tmux)** / **ADR-0014 (Process supervisor)**. ADR-0001 (tmux 통합 control mode) + ADR-0009 (tmux daemon 격리) 는 *2026-05-14 deprecated* (POC 검증 후 ADR-0013 으로 supersede, ADR-0014 로 supersede). **2단계 선행 조건** (기본 UI 워크스페이스 진입 전 필수): **ADR-0004 (터미널 렌더링 — xterm.js v6 + fit + unicode11 + placeholder-on-zoom, R2 기반, Accepted 2026-05-14)** + **ADR-0005 (캔버스 라이브러리 — `@xyflow/svelte` v1.5.x, R3 기반, Accepted 2026-05-14)**. **3단계 선행 조건** (상태 저장/복원 진입 전 필수): **ADR-0006 (영속화 storage/스키마 — plain JSON file + atomic-write-file + sidecar quarantine, R6 기반, Accepted 2026-05-14)**.

### 1단계: 엔진 연결 검증 (**2026-05-14 amend**)

* PTY direct backend 연결 — gtmux Server 가 portable-pty 로 첫 Pane spawn (`$SHELL`) (ADR-0013·0014)
* Pane 입력/출력 — WS 0x02 PANE_OUT + 0x03 PANE_IN
* Pane 조회 (logical Session·Pane list)
* (구) tmux control mode 연결 항 폐기 — POC 단계에서 검증 완료, Sprint 7 의 `S7-PTY-BACKEND` 가 본격 구현 task

### 2단계: 기본 UI 워크스페이스

* canvas
* panel
* pane list
* quick actions

### 3단계: 상태 저장 및 복원

* layout persistence
* reconnect
* session restore

### 4단계: UX 개선

* 검색
* 라벨
* highlight
* keyboard shortcuts
* fit to view

### 5단계: 안정화 및 보안 강화

* 외부 노출 모드 보안 설정
* throttling
* CSP
* safe deployment guide

---

## 16. 최종 결론

이 프로젝트는 **tmux를 그대로 백엔드 실행 엔진으로 유지하면서, 웹 기반 무한 캔버스 UI 위에서 pane을 panel로 재구성해 다루는 단일 사용자용 도구**로 정의할 수 있다.

기술적으로 충분히 실현 가능하며, 핵심 성공 요인은 다음 네 가지다.

1. tmux state와 web canvas state를 분리할 것
2. tmux-native 기능과 web-only 기능을 구분할 것
3. 단일 사용자 범위에서도 보안 기본값을 안전하게 설계할 것
4. 출력 처리와 재연결, 캔버스 사용성을 제품 수준으로 다듬을 것

이 정의 아래에서는 본 프로젝트는 실험적 아이디어가 아니라, 충분히 구체적이고 구현 가능한 개발 프로젝트이다.
