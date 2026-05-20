# 보고서: Grill 산출물 — sketch.md 수정 항목 및 ADR 발행 대기열

- 일자: 2026-05-13
- 출처: `/grill-with-docs` 세션 (Q1~Q5)
- 상태: **진행 중** — grill 종료 시 일괄 적용
- 관련 산출물:
  - `CONTEXT.md` (이미 inline 갱신됨)
  - `docs/sketch.md` (아래 §2 amendment list 따라 수정 예정)
  - `docs/adr/` (아래 §3 ADR queue 발행 예정)

## 1. 확정된 결정 사항 (Grill 합의)

### D1. Canvas:Session 1:1
- 1 Canvas는 정확히 1 tmux Session에 대응.
- 다른 session을 보려면 별도 Server를 다른 포트로 띄움.

### D2. Server:Session:Port 1:1:1 바인딩
- 한 gtmux **Server** 프로세스 = 1 tmux Session + 1 포트.
- 사용자가 여러 Server를 동시 실행 가능. 활성 Server 목록·오케스트레이션은 본 프로젝트 비범위.
- 부팅 시 바인딩은 immutable. 런타임 변경 불가.
- 부팅 시 session 부재 → 에러 종료 (자동 생성 안 함).
- 외부 kill 시 Server 프로세스 종료 (재바인딩 UI 없음).

### D3. UI 범위 = Pane 제어 + Panel 제어 + Group 관리
- §6.1의 session 6개 기능: 모두 UI 밖.
- §6.2의 window 6개 기능: 모두 UI 밖 (Group이 UI 측에서 담당).
- §6.3의 pane 기능: UI 안 (단, tmux 측 명령은 제한된 allowlist).

### D4. Window 시각화 — W-a (label-only) → Group으로 재설계
- §5.2.A "한 캔버스에 여러 window/pane"의 spirit은 유지.
- tmux Window는 *implementation-only*로 강등 (UI 비노출).
- 사용자 측 묶음은 web-only **Group** (Figma-식 계층) — 신규 도메인 1차 시민.
- §7.1 "window 색상 태그"는 **Group 색상 태그**로 옮기고 P1+ deferral.

### D5. tmux active window/pane = M의 default seed로만 mirror
- (act-1) mirror-only 채택.
- I(Input Target)는 mirror 대상 아님 — 외부 attach 클라이언트가 active window 바꿔도 gtmux의 I는 불변.

### D6. Panel의 두 직교 active mode
- **M (Manipulation Selection)**: 캔버스 web-only 액션 + close(=pane kill) 대상. 다중 선택.
- **I (Input Target)**: 터미널 입력 라우팅. 단일.
- M·I는 서로 결합 안 함.

### D7. Placement principle (좌표 optional + 자동 cascade) — D23에서 재정의
- **신규 Panel의 캔버스 좌표는 optional** — 사용자가 명시 입력하면 그 위치, 미지정이면 자동 cascade 배치.
- 명시 입력 매커니즘: 빈 캔버스 영역 클릭 + "Create Panel here" 컨텍스트 메뉴. 사이드바 "New Panel" 버튼이나 외부 tmux CLI(`split-window` 등)는 미지정으로 처리.
- **자동 cascade**: 시작점 = Canvas 좌표계의 origin (0, 0). 직전 자동 배치 위치 + (40px, 40px) offset 누적. cursor는 Server 메모리만, 영속화 안 함 (재기동 시 origin부터 재시작).
- Panel 자유 overlap 허용 (D23 정합).
- *이전 정책 (사용자 명시 좌표만 허용 + Unplaced Panel 대기 트레이)은 D23에서 폐기됨.*

### D8. Pane 생성 모델 — (P-α) Single-pane-per-window
- gtmux UI "pane 생성" = tmux `new-window -t <session>` (split-window 금지).
- gtmux backend tmux command allowlist에서 **`split-window`·`resize-pane`·`select-layout` 제외**.
- 외부에서 split된 multi-pane Window는 받아들이되, 그 Window의 Panel들은 **canvas resize 잠금** (move·minimize·hide·close는 가능).
- 사용자가 그 Window의 panes를 하나씩 close해 single-pane이 되면 resize 활성화 복귀.

### D9. 외부 CLI 사용성 완화
- 사용자가 Panel 라벨을 설정하면 gtmux는 `rename-window -t @W <label>`로 tmux 측 Window 이름 동기화 (P1 디테일).

### D10. tmux daemon 격리 모델 — (C-A) Dedicated daemon per Server
- 각 gtmux Server는 자신 전용 tmux daemon을 사용. 격리 단위 = 1:1:1:1 (Server : Session : Port : tmux-daemon).
- 소켓 컨벤션: **`tmux -L gtmux-<session>`** (tmux 표준 `-L` 사용, 경로는 `${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<session>`). tmux가 부모 디렉터리(0700)·소켓 perm·SIGUSR1 재생성을 자동 보장.
- 부팅: daemon 부재 시 자동 spawn (`tmux -L gtmux-<session> start-server`). Session 부재는 자동 생성 안 함 → 에러 종료 (D2 정신 유지).
- 종료: gtmux Server kill → daemon은 살려둠 (재기동 시 session·pane persistence 보존).
- 명시 정리: `gtmux teardown --session <name>` CLI = 본 프로젝트 범위. 2단계 수행:
  1. `tmux -L gtmux-<session> kill-server`
  2. `rm -f ${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<session>` (실측 확인: `kill-server`는 소켓 파일을 자동 정리 안 함)
- **실측 footprint** (참고 데이터):
  - 1 daemon baseline (1 session, 1 pane): **3.4 MB RSS**
  - 60 panes (single-pane-per-window 컨벤션 = 60 windows): 4.3 MB → window당 ≈ 15 KB
  - 6 daemons 동시: 22 MB total
  - 50 Server × 5 pane 시나리오 추정: ~175 MB total (백엔드 프로세스 메모리의 약 6%)
- 거절된 대안:
  - (C-B) shared gtmux-전용 daemon: 메모리 ~170 MB 절감하나 트러스트 경계 공유 → 격리 손실. spec §13.3.6 정신 약화
  - (C-C) 사용자 main tmux server attach: gtmux가 사용자 모든 session 가시·잠재 조작 가능 → §13.3.6 위반
  - `-S` 자체 디렉터리: tmux의 dir perm 자동 강제·SIGUSR1·discovery 컨벤션을 잃음

### D11. Group 데이터 모델 — (G-hybrid)
- Group의 spatial frame을 1차 상태로 저장하지 않는다. 자체 상태 = `{label, color|null, visibility, locked, order}` + 트리 구조 (`parent_id`).
- "Group 이동"은 *액션*: 사용자가 드래그하면 자손 Panel들의 bounding box → 드래그 delta → 모든 자손 좌표에 동일 delta 적용. D7(좌표 사용자 명시 입력)과 충돌 없음 — 드래그가 명시 입력.
- "Group 리사이즈"는 MVP 미지원 (P1+).
- **생성·해체 UX**: Panel/Group 다중 선택(M) → `Group` 액션. Group 단일 선택 → `Ungroup` 액션. 빈 Group은 별도 생성 경로 없음 — 자식 제거로 발생한 빈 Group은 사용자 명시 액션으로만 제거 (auto-prune 안 함).
- Drag-reparent: 사이드바 layer panel 내 드래그만 (MVP). 캔버스 hover-reparent는 P1+.
- 상태 전파: **visibility = AND** (effective visible = self AND 모든 ancestor; ancestor hidden → 자손 hidden 강제), **lock = OR** (effective locked = self OR 모든 ancestor; ancestor locked → 자손도 locked, cascade-down). label/color는 가장 가까운 ancestor 값 inherit. *(2차 coherence G7·G8 정정: 초안의 visibility/lock 통합 AND 표현은 lock semantics와 어긋남 — 분리해 진술.)*
- Group → M 확장: 사이드바 클릭 시 자손 Panel 재귀 등록. 캔버스 Panel 클릭 시 단일 Panel만.
- Group close (destructive): 자손 모든 Pane을 `kill-pane`. §7.6 confirm modal 필수.
- 영속화 스키마(제안): `groups: [{id, parent_id|null, label, color|null, visibility, locked, order}]` + `panels: [{id, parent_id|null, x, y, w, h, z, visibility, locked, label, note, ...}]`.
- 거절된 대안:
  - (G-pure-spatial): frame 1차 상태 → 자식 추가/제거/리사이즈마다 갱신 로직 + 자식 좌표 표현 결정 → MVP 과스코프
  - (G-logical-only): "묶음" 의미가 약함, Figma 멘탈모델 손실

### D12. Canvas Layout 영속화 전송 — (T-mixed) HTTP + WS notify
- **Durable 상태(Canvas Layout = Group 트리 + Panel 좌표/상태)는 HTTP 엔드포인트**가 담당. WebSocket은 ephemeral 신호(live pane output, M/I/viewport/focus, LAYOUT_CHANGED notify)만 담당.
- HTTP API:
  - `GET /api/layout` — 현재 layout + ETag 반환
  - `PUT /api/layout` (+ `If-Match: <etag>`) — 전체 교체. 충돌 시 412.
  - MVP는 PUT 전체 (PATCH 델타 미지원). 페이로드는 < 50KB 수준이라 충분.
- WS `0x80 LAYOUT_CHANGED` notify: D14 표 참조. ETag만 페이로드로 싣고, MT-3로 originator 추적 불요.
- 클라이언트측 디바운스 = 300ms 기본, **설정 가능** (사용자 환경에 따라 조정 가능하게 노출).
- 거절된 대안:
  - (T-WS) durable·ephemeral 모두 WS — reconnect 중 write 손실 위험, 백프레셔 큐 경쟁, optimistic concurrency 직접 구현 필요
  - (T-pure-HTTP) WS 없음 — 멀티 탭 live sync 폴링 필요, pane output 푸시 불가
- 후속: ADR-0002·ADR-0006 본 결정 반영.

### D13. Multi-connection 정책 — (MT-3) Live Mirror, Client 구분 없음
- 한 Server에 동시 연결된 WebSocket들은 **identity로 구분하지 않는다** (단일 사용자 정책). `client_id` 등 connection-level 식별자 폐기.
- **모든 ephemeral UI 상태(M, I, Viewport, Focus mode)는 Server가 단일 진실로 보관**하고 모든 연결에 broadcast. 갱신은 양방향 동일 envelope.
- 멀티 탭/창/디바이스는 *같은 사용자의 거울 뷰*. 멀티 모니터에서 캔버스의 다른 영역을 동시에 보는 시나리오(viewport 분리)는 본 모델에서 **명시적으로 미지원**. 사용자 결정 — 일관성·단순성 우선.
  - 멀티 모니터 분할 사용은 §7.2의 mini-map(P1+) 또는 §7.3의 focus mode 등 단일 viewport 안의 보조 시각화로 우회한다.
- 거절된 대안:
  - (MT-1) Single-tab 강제 — 새로고침마다 kick UX 발생, §9.2 정신 위반
  - (MT-2) durable shared / ephemeral per-conn — 단순하지만 단일 사용자 모델과 어색 (한 사람에 N개의 M이 동시 존재?)
  - MT-3 + viewport per-conn 변형 — 비대칭이지만 멀티 모니터 분할 사용 가능. 사용자가 일관성 우선으로 표준 MT-3 선택

### D14. WS web-domain envelope 슬롯 (0x80–0x8F)
MVP 정의:

| 코드 | 이름 | 페이로드 | 방향 |
|---|---|---|---|
| 0x80 | LAYOUT_CHANGED | `etag(16B)` | 서버 → 모든 연결 (HTTP layout 갱신 알림) |
| 0x81 | M_CHANGED | `varint count + varint panel_ids[]` | 양방향, 갱신 → 모두 broadcast |
| 0x82 | I_CHANGED | `varint pane_id (0=null)` | 양방향, 갱신 → 모두 broadcast |
| 0x83 | VIEWPORT_CHANGED | `int32 x, int32 y, float32 zoom` | 양방향, 갱신 → 모두 broadcast |
| 0x84 | FOCUS_MODE_CHANGED | `1B enabled, varint target_panel_id` | 양방향, 갱신 → 모두 broadcast |
| 0x85–0x8F | reserved | — | 미래 기능용 |

- broadcast 정책: originator 구분 없이 모든 연결에 동일 메시지 송신 (D13 MT-3 + idempotent 적용으로 단순화)
- ADR-0002 SSoT(`wire-protocol.md`)에 위 표 반영 — 0x80–0x8F 영역 *완전 정의*

### D15. 재접속 시 pane 출력 히스토리 — Per-pane server-side ring buffer
- gtmux backend가 pane별 ring buffer를 메모리에 유지. 새 WS attach 시 즉시 replay → 그 이후 live `%output` stream.
- 크기: **per pane 128 KB 기본, 설정 가능**. 50 pane × 128 KB = 6.4 MB Server RAM (가벼움).
- 단위: 바이트 FIFO ring (라인/ANSI 시퀀스 경계 무관).
- Disk persistence 안 함 (보안: STDOUT 내 비밀 정보 가능성, perf: hot-path I/O 비용). Server restart 시 ring buffer 새로 시작 — capture-pane으로 P1+ 회복.
- Deep scrollback(`capture-pane -p -e -J -S -<lines>`)은 사용자 명시 액션의 P1+ 기능. MVP 직접 노출 안 함.
- 외부 CLI로 새로 split된 pane의 ring buffer는 gtmux mirror 시작 시점부터 시작 (그 전 출력은 capture-pane 회복만 가능).
- 거절된 대안:
  - 매 attach마다 capture-pane: N pane = N 명령 직렬화 → tmux 큐 경쟁, 멀티초 재연결 지연
  - ring buffer disk persistence: §13.3.5 저장 데이터 노출 위험 확장 + perf
- ADR 새로 발급 안 함. ADR-0001(tmux 통합)의 입력 제약으로 흡수.

### D16. Panel Streaming State lifecycle
- Panel별 데이터 흐름 활성 상태 = `Streaming` | `Suspended`.
- 전이 트리거 (MVP):
  - visibility=hidden 또는 minimized=true → **Suspended** → `refresh-client -A '%pid:pause'`
  - 그 외 → **Streaming** → `refresh-client -A '%pid:continue'`
  - 디바운스 300ms (빠른 토글에 명령 폭주 방지)
- MT-3 일관성: visibility는 모든 연결 sync → 전이도 글로벌 단일 결정.
- 효과: 비활성 Panel의 WS 대역폭·ring buffer write·xterm.js write 부하 절감. §13.3.7 "비가시 panel 렌더링 최적화"의 데이터-계층 버전.
- 거절된 대안:
  - `off` 사용 (MVP는 `pause` — catch-up 비용 작음). `off`로의 전환은 long-suspend 검증 후 결정
  - Off-viewport 자동 pause: P1+ (intersection 계산 + pan/zoom 떨림 처리 복잡성)
  - Input target(I) 이외 모두 pause: 과공격적, 시각적 모니터링 패턴 위반
- ADR-0001 입력 제약으로 흡수. 구현 시 검증 항목: *장시간 Suspended 시 tmux 측 buffer 누적 행동 + CONTROL_BUFFER_HIGH 강제 disconnect 정책 적용 여부*. 검증 결과에 따라 max-suspend-duration 후 `off` 전환 정책 추가 가능.

### D23. Panel placement·z-index·overlap (D7 재정의 + 신규)

**Placement** (D7 supersede):
- 신규 Panel의 캔버스 좌표는 **optional**. 사용자 명시 입력하면 그 위치, 미지정이면 자동.
- 자동 배치 = cascade: 시작점 origin (0, 0), 직전 자동 배치 위치 + (40, 40)px offset 누적.
- cursor는 Server 메모리만, 영속화 안 함 (재기동 시 origin 재시작).
- *Unplaced Panel 개념 폐기.* 외부 tmux CLI로 생긴 Pane도 즉시 캔버스 노출 (자동 cascade로).

**Z-index**:
- 모든 Panel은 정수 z (Canvas Layout schema 기존 `z` 필드).
- 신규 Panel z = 현재 최대 z + 1.
- M (Manipulation Selection)에 들어오는 Panel은 z = 현재 최대 z + 1 (자동 최상위). M에서 빠진 후에도 z 유지 (Figma·Photoshop 패턴).
- 명시 z 조정 = sketch §6.4 "panel z-index 조정" 액션 (Bring to front / Send to back / Up one / Down one).
- 영속화: z 변화는 Canvas Layout HTTP PUT debounce(300ms, D12) 안에 모임.

**Overlap**: 자유 허용. 사용자가 의도적으로 겹치게 둘 수 있다.

**위치**:
- D7 보고서 본문 재작성 완료
- CONTEXT.md "Placement principle" 절 재작성 + "Z-index 정책" 신규 절 추가
- sketch §4.3에서 Unplaced Panel 정의 제거
- ADR-0008 D4 본문 갱신 (외부 multi-pane Window mirror = 자동 cascade로)
- sketch §6.4 z-index 정책 보강

**거절**: Unplaced 트레이/사이드바 섹션, 자동 배치 없음 정책, 모달 placement, 키보드 전용 placement (모두 D7 옛 정의의 후속이므로 본 결정으로 자연 폐기).

### D22. Config 파일 — 필드 셋 확정

**위치·포맷**: `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml`, TOML, per-Server.

**선행순위**: CLI flag > `GTMUX_<SECTION>__<KEY>` 환경변수 (figment 컨벤션) > config 파일 > 빌트인 디폴트.

**스키마 (MVP)**:
```toml
schema_version = 1

[server]
session = "foo"                # tmux session 이름 (D2)
port    = 9001                 # 영속 식별자 (D21 c6)
bind    = "127.0.0.1"          # bind 주소. local=loopback/unix socket, 그 외=cloud로 자동 추론
                               # (mode 별도 필드 없음 — bind 값으로 결정)

[runtime]
ring_buffer_size_kb        = 128    # D15
layout_debounce_ms         = 300    # D12
panel_state_debounce_ms    = 300    # D16
log_level                  = "info"
log_format                 = "auto" # tty=banner, pipe=json

[security]
cors_origins   = ["http://localhost:9001"]
host_allowlist = ["localhost:9001", "127.0.0.1:9001"]
# csp = "..."  # 미설정 시 ADR-0003 SSoT의 기본 CSP 사용

[cloud]
# bind가 loopback/unix가 아닐 때만 활성.
# tls_cert = "/path/to/cert.pem"
# tls_key  = "/path/to/key.pem"
# rate_limit_auth_failures_per_minute = 2
```

**Mode 추론 규칙** (사용자 결정):
- `bind` ∈ {`127.0.0.1`, `::1`, `unix:/path/...`} → local 모드 (D17 매시작 토큰 재발급)
- 그 외 (`0.0.0.0`, 외부 IP, etc) → cloud 모드 (영속 토큰 + 명시 회전 + TLS 권장)

**Auto-decide (technical brief)**:
- Crate = `figment` (또는 `config`) — R7 검증 항목
- Validation = serde + unknown field 거부 (오타 방지)
- `gtmux start` 첫 호출 시 스키마 자동 작성 (사용자 인자만 채움, 나머지 디폴트)
- `gtmux config show --session <name>` (P1+)
- 공유 디폴트 `${XDG_CONFIG_HOME}/gtmux/_defaults.toml` (P1+)

### D21. First-run · disconnect · Server lifecycle UX

**(c1) `gtmux start` 콘솔 출력**: tty이면 banner (session·port·url·token·log path·cold start time 다중 라인), pipe이면 자동 JSON (event:ready), `--log-format json` / `--quiet`로 강제 가능.

**(c2) 브라우저 WS 끊김 표시**: Hybrid grace 1s. 끊김 후 1s 이내 재연결 시 UI noise 없음, 1s 초과 시 상단 배너 "Reconnecting (attempt N)". 재연결 성공 시 fade-out.

**(c3) 자동 재연결**: 클라이언트측 exponential backoff 0.5→1→2→4→8→16→cap 30s, **indefinite retry**. 재연결 성공 시 full state re-sync (HTTP GET layout + WS attach + ring buffer replay). 10회 연속 실패 시 배너 메시지 "Server stopped — run `gtmux start --port <N>`" 갱신.

**(c4) Pane zombie (`pane_dead = 1`)**: Panel header에 "exited"/💀 badge, 터미널 출력 그대로 보존. 사용자 explicit close 액션이 있어야 `kill-pane`. 마지막 출력(에러 메시지·exit code 등) 확인 가능성 우선.

**(c5) Server lifecycle ⊥ tmux daemon lifecycle**: gtmux Server가 정상 종료 포함 어떤 이유로든 죽어도 tmux daemon은 background에서 계속 실행 (D10 D5 재확인·강화). Pane 상태(running·zombie·내용·tmux Layout)는 tmux 측에서만 영속. gtmux는 *재기동 시 mirror만*. zombie badge도 tmux `pane_dead = 1`을 mirror한 결과, gtmux 측 디스크 저장 없음.

**(c6) Port-based 재attach (신규, D20 보강)**: port는 *사용자가 기억하는 영속 식별자*. `gtmux start` 인자 처리:
- `--session <name> --port <N>` 둘 다 명시 — 첫 사용 시 `<name>.config.toml`에 port 저장, 재호출 시 일관성 검증
- `--session <name>` 만 — config 읽어 port 결정, 미존재면 에러 + port 명시 안내
- **`--port <N>` 만 — `${XDG_CONFIG_HOME}/gtmux/*.config.toml` 스캔하여 그 port를 가진 config 찾음 (1개 매칭 = 그 session 사용, 0개 = 에러, >1개 = ambiguous 에러)**
- 둘 다 없음 — 에러 + 알려진 session 목록 제안

→ 사용자가 URL을 북마크하면 (`http://localhost:9001/`) `gtmux start --port 9001` 한 줄로 재기동 가능. 1:1:1 모델과 정합.

**(c7) 일관성 충돌**: port↔session mapping 불일치 또는 같은 port가 다른 session에 클레임된 상태에서 새로 시도 → exit 4 + 사용자 조치 안내. `--force-rebind`는 P1+ 옵션.

**(c8) teardown의 config 처리**: D20 teardown 4단계에 *config 파일도 정리* 추가 (5단계). `--keep-config` 플래그로 유지 옵션 (재기동 의도 있을 때).

**Auto-decide (technical brief)**:
- WS close codes: RFC 6455 + custom 4001 (token revoked, rotate 후), 4002 (session killed externally)
- 재연결 토큰: 기본 재사용, 4001 시 사용자에 "Token rotated — visit new URL" 안내
- 외부 session kill 시 Server stderr 로그 1 line + exit 6
- tmux daemon crash 시 Server exit 6, 재시도 없음 (P1+)
- 알림 채널: in-app toast/banner only. 브라우저 system notification 사용 안 함.

**거절**: 별도 registry 파일 (config의 port 필드가 단일 진실), port 자동 할당 영속화 (D20 거절 재확인).

**위치**:
- ADR-0001 (tmux 통합) Open에 외부 session kill 감지 절차 명시
- ADR-0009 D6 teardown 5단계로 확장 (config 포함)
- ADR-0011 (Rust backend) `clap` CLI에 port-기반 lookup 로직 추가
- sketch §7.4 (자동 재연결) + §11.2.D (상태 저장) + §10.1 (lifecycle manager) port = 영속 식별자 명시
- R8 보고서에 클라이언트측 reconnect + grace period + zombie panel UI 패턴 항목 추가

### D20. CLI 설계 (`gtmux` 명령)

**Subcommand 집합 (MVP)**:
- `gtmux start --session <name> --port <port>` — Server 기동, daemon 자동 spawn, session 부재 시 에러 종료, 부팅 콘솔에 token URL 출력
- `gtmux stop --session <name>` — Server 프로세스만 종료, daemon은 유지
- `gtmux teardown --session <name> [--force]` — daemon kill + socket + token + state 파일 정리, 기본 prompt
- `gtmux rotate-token --session <name>` — 새 token 발급·파일 갱신, 활성 WS·HTTP 즉시 끊김, 새 URL stdout
- `gtmux status --session <name> [--json]` — 단일 Server 상태 조회 (외부 *목록 도구* 진입점)
- `gtmux --version` / `gtmux [<sub>] --help`

**기본 동작**: 항상 foreground (tmux·Jupyter·code-server 패턴 일치). 백그라운드화는 사용자 책임(`nohup`/`systemd`/`launchctl` 등). `--daemon` 플래그 없음.

**디렉터리 레이아웃**:
- `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml` — 사용자 편집 가능, per-Server
- `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token` — 0600, 사용자 직접 편집 비권장 (D17)
- `${XDG_STATE_HOME}/gtmux/<session>.layout.json` — Canvas Layout 영속화 (ADR-0006 storage 결정에 따라 위치/포맷 변경 가능)
- `${XDG_RUNTIME_DIR:-/tmp}/gtmux/<session>.pid` — gtmux Server PID
- `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/gtmux-<session>` — tmux daemon socket (D10)

**Config 포맷**: TOML (주석 + 자료형 지원, Rust 생태 표준). 선행순위: CLI flag > `GTMUX_*` 환경변수 > config 파일 > 빌트인 디폴트.

**Exit code 규약**:
- 0 성공 / 1 일반 오류 / 2 사용법 오류 (clap 표준) / 3 Session 부재 / 4 포트 사용 중 또는 중복 Server / 5 권한 거부 / 6 tmux daemon 통신 실패 / 7 teardown 부분 실패

**구현 디테일 (auto)**: `tracing`+`tracing-subscriber` 로깅 (ANSI on tty, JSON in pipe / `--log-format json` 강제), SIGTERM/SIGINT → graceful shutdown (WS close + layout flush), `--config <path>`로 명시 경로 지정 가능, stdin 사용 안 함.

**거절**: `--daemon` 자체 fork (운영 복잡 증가), 자동 포트 할당 기본 (재기동 시 포트 변경 → 1:1:1 UX 와그러짐, stretch로만), 단일 binary embed 정적 자원 (MVP는 별도 dir 배포, stretch).

**위치**: ADR-0009·ADR-0011 운영 절차로 흡수. sketch §10.1 lifecycle manager 절에 본 CLI 명세 인용.

### D19. 성능 예산 (R7 benchmark DoD + ADR-0001 백프레셔 정책 입력)

| 차원 | MVP 목표 | Stretch |
|---|---|---|
| Cold start (`gtmux start` → 첫 paint) | < 500ms | < 300ms |
| Warm reconnect (새 탭, daemon·서버 살아 있음) | < 300ms | < 200ms |
| Per-pane output latency (process stdout → 픽셀, p50) | < 30ms | < 15ms |
| Per-pane output latency p99 | < 100ms | < 50ms |
| Panel drag commit → 모든 연결 sync 완료 | < 500ms | < 300ms |
| Concurrent panel per Server | 50 | 100 |
| Server backend memory baseline | < 30 MB | < 20 MB |
| Per-Server total (gtmux + tmux daemon + buffers) | < 50 MB | < 35 MB |
| Frontend tab memory | < 100 MB | < 60 MB |
| HTTP `PUT /api/layout` 페이로드 상한 | 256 KB (SSoT enforce) | — |
| gtmux→브라우저 WS write lag (tmux 대비) | < 5s | < 1s |
| 동시 WS 연결 cap | MVP 없음 / 권장 ≤ 10 | — |

측정 환경 (R7/R8 benchmark 동봉):
- macOS Apple Silicon 16GB / Linux x86_64 16GB
- tmux 3.4+
- Chromium 최신 안정 + Firefox 최신 안정
- 워크로드: 50 pane 동시 (5 고출력 + 45 idle)

거절:
- Cold start < 100ms — tmux 부팅 자체가 50ms대, 비현실
- Pane p99 < 10ms — burst·GC-free 환경에서도 측정 데이터 없는 상태에서 과약속
- 100+ panel MVP — 브라우저 측 xterm.js × 100 측정 안 됨, 50을 MVP·100을 stretch

후속:
- ADR-0001 (tmux 통합) 작성 시 `pause-after=<sec>` 임계값을 본 예산 p99에 맞춰 결정
- ADR-0011 Open O5 (50-pane benchmark target) closed
- R7 benchmark DoD = 위 MVP 컬럼 측정 시나리오 산출
- sketch §9.2 정량화 amend

### D18. Tech stack — Rust (backend) + Svelte 5 (frontend)
- **백엔드**: **Rust + tokio + axum + tokio-tungstenite + tower-http**. 단일 정적 바이너리 배포. 메모리 baseline ≈ 10–30 MB/Server.
- **프론트엔드**: **Svelte 5 (signals) + Vite + TypeScript + xterm.js**. MT-3 라이브 갱신에 fine-grained reactivity 최적합.
- **결정 맥락**: 사용자가 *AI agent로 구현 진행* + *성능 우선* 명시. 따라서 dev velocity 페널티(LLM의 Rust 코드 생성 반복 비용·컴파일 대기)는 *예측 가능하고 한정적인 wall-clock 비용*으로 수용. 도메인 제약(binary throughput, predictable latency, 다수 Server 동시 실행 메모리, 단일 바이너리 배포) 모두 Rust가 최상.
- **AI agent 친화 분석**:
  - Rust용 LLM 코드 품질: axum/tokio/tungstenite는 광범위 학습 데이터 보유, 1.5–2x iteration 비용으로 *해결 가능 범위*
  - 정적 타입 + ownership semantics → AI agent 리팩터 안전성 ↑
  - AI-generated 코드는 hand-optimized 대비 1.5–3x 비효율 — Rust headroom이 흡수
- **거절된 대안**:
  - **Bun (TS) + Svelte**: 충분히 작동하지만 메모리 footprint 2–3x. AI-generated 비효율 흡수 여유 ↓. 50 Server 시 1.5–2.5 GB 차이.
  - **Node.js**: Bun이 dominant.
  - **Go**: Bun과 비등하지만 백·프론트 언어 분리 비용 (TS 공유 불가). Rust 대비 메모리·throughput 모두 약간 약함.
  - **Python (FastAPI)**: binary throughput·distribution 모두 약함.
  - **React/Vue (프론트엔드)**: VDOM diff 비용. 50+ Panel + MT-3 live state에서 memoization 부담 → fine-grained reactivity가 우월.
- **R7/R8 보고서 scope 축소**: 후보 비교는 본 D18로 종결. R7/R8은 *specific crate set 검증 + benchmark + scaffolding 코드* 단계로 정의.
- **ADR 발행 큐**: ADR-0011 (backend stack), ADR-0012 (frontend stack). 본 grill 종료 시 Proposed 초안. R7/R8 결과로 Accepted 승격.
- **R5 권장 그대로 채택**: 256-bit CSPRNG 토큰, `${XDG_CONFIG_HOME}/gtmux/<session>.token` (0600), 상수시간 비교, WS는 `Sec-WebSocket-Protocol` 서브프로토콜, HTTP는 `Authorization: Bearer` (+ secure cookie 보조), Origin·Host 화이트리스트 강제, 로깅 redaction.
- **(a) 토큰 회전 정책 결정**: MVP 로컬 = **매 서버 시작 시 재발급 (Jupyter 방식)**. Cloud 모드 = **영속 + 명시 회전 명령** (`gtmux rotate-token --session <name>`). 사용자 UX 완화: 부팅 콘솔에 `?token=...` URL 출력 → 사용자 즐겨찾기는 path만, 토큰은 cookie 1회 발급으로 transport.
- **(b) OS 인증 위임 (PAM/SSH) — MVP 미적용**. 단일 사용자 전제(§1.3)에서 gtmux 자체 토큰으로 충분. P1+ cloud 옵션 재방문 여지 남김.
- **Password 별도 메커니즘 안 둠**: 사용자 발언 "password/token 기능을 바탕으로"를 *인증 필요성 강조*의 추상 표현으로 해석. R5 분석상 단일 사용자 환경에서 토큰 only가 충분. 사용자 의도가 *진짜 이중 메커니즘*이었다면 후속 grill에서 재진입.
- R5 §1 미해결 #1(회전 정책), #4(OS 인증 통합) 두 항목 본 결정으로 closed.
- ADR 새로 발급 안 함. ADR-0003(보안 디폴트)의 입력 제약으로 흡수.

## 2. sketch.md 수정 항목 (Pending — grill 종료 후 일괄 적용)

| 절 | 현재 본문 요지 | 수정 방향 |
|---|---|---|
| **§4.1.3** | "tmux layout과 web canvas layout은 분리한다" | 보강: gtmux-created Window는 single-pane → tmux Layout이 trivial. 의미 있는 tmux Layout은 외부 split된 multi-pane Window뿐. |
| **§4.3 중요 정의** | Pane/Panel/Canvas Layout/tmux Layout 4종 | 추가: **Server (gtmux Server)**, **Group**, **Manipulation Selection (M)**, **Input Target (I)**, **Panel Streaming State**. Window의 정의에 "*implementation-only, UI 비노출*" 명기. (Unplaced Panel 개념은 D23에서 폐기되어 미포함.) |
| **§6.1 tmux 세션 제어 기능** | 6개 기능 (생성/조회/선택/종료/이름변경/attach) | **전면 삭제 또는 비범위로 재서술**. "Session 제어는 gtmux UI 밖, 사용자 OS·tmux CLI·외부 도구가 담당" 한 문단으로 축약. |
| **§6.2 tmux window 제어 기능** | 6개 기능 (생성/조회/선택/이름변경/종료/그룹조회) | **삭제하고 새 절 §6.x "Group 관리 기능"으로 대체**. tmux Window는 implementation-only로 강등 — 사용자 노출 없음. |
| **§6.3 tmux pane 제어 기능** | 9개 (생성/닫기/선택/포커스/식별표시/command·path/입력/출력/resize) | 보강: "pane 생성" = tmux `new-window` (gtmux 컨벤션). "resize" = single-pane window의 window-size 변경. `split-window`·`select-layout`은 발급하지 않음. |
| **§6.4 캔버스 기반 GUI 기능** | panel drag/resize/z-index, header (window name 포함), minimize/maximize/hide/lock/close, 다중 선택, pan/zoom | "panel header window name" → **Group label** 또는 panel label. "close" 정의 명확화: close = pane kill (tmux-native). hide = visibility off (web-only). 다중 선택 = M. |
| **§6.5 Pane List / 탐색 기능** | "window별 그룹 정렬" 등 | **Group 계층 트리(사이드바 layer panel, Figma-식)**로 재서술. "window별 그룹" 표현 제거. |
| **§7.1 정보 가독성** | "window 색상 태그 또는 카테고리 표시" | **Group 색상 태그**로 교체. 우선순위 P1+ 유지. |
| **§10.1 백엔드 구성** | tmux control mode client / websocket server / tmux command router / state collector / canvas layout persistence store / local config manager | 보강: tmux control mode client는 **dedicated daemon (-L gtmux-<session>)에 attach**하는 단일 클라이언트. tmux command router의 발급 가능 명령 집합은 single-pane-per-window 컨벤션에 맞게 축소된 allowlist (split-window·resize-pane·select-layout 제외). 새 항목 추가: **lifecycle manager** (daemon spawn/teardown, socket 정리), **HTTP API server** (Canvas Layout `GET/PUT /api/layout` + ETag), **WS notify dispatcher** (`0x80 LAYOUT_CHANGED`). |
| **§11.2.A (MVP tmux 연동)** | session 목록/생성/선택, window 목록/조회, pane CRUD | **session·window 관련 항목 삭제**. Pane CRUD + 출력/입력만 남김. |
| **§11.2.D (MVP 상태 저장)** | canvas layout 저장 / 재접속 시 layout 복원 / 마지막 session 복원 | 명확화: durable 영속화는 **HTTP `GET/PUT /api/layout` + ETag**. 재접속 시 클라이언트가 GET으로 복원. WS는 `LAYOUT_CHANGED` notify만. "마지막 session 복원"은 D2에 따라 *Server 부팅 인자*가 결정 (앱 안에서 동작 아님). |
| **§11.3 MVP 제외** | 기존 6항목 (다중사용자, 계정, 권한, 공유, 모바일, preset 일부) | **추가**: "여러 Server 오케스트레이션/인덱싱", "Group spatial frame · resize"(P1+), "Canvas Layout PATCH 델타 전송"(MVP는 PUT 전체만), "Viewport per-connection 분리"(MT-3 의도적 제약 — 멀티 모니터 분할 시나리오 미지원). |
| **§6.4 캔버스 기반 GUI 기능** (추가 보강) | "canvas pan / zoom" P0 | 명시: pan/zoom은 **모든 WS 연결에 sync**된다 (MT-3 D13). 두 탭/창에서 동일 영역을 봄. |
| **§13.3.2 WebSocket 인증** | "secure cookie 또는 one-time session token" | 명확화 (D17): 256-bit CSPRNG 토큰, WS는 `Sec-WebSocket-Protocol` 서브프로토콜, HTTP는 `Authorization: Bearer` (보조로 secure cookie), Origin/Host 화이트리스트 필수, 매 서버 시작 시 재발급(local) / 영속+명시 회전(cloud). |
| **§14 기술적 난점** | 5개 | 보강: "single-pane-per-window 컨벤션 하에서 tmux Window 개수가 panel 개수와 동일 — 대량 panel 시 bootstrap event 수가 panel 수에 비례", "외부 multi-pane Window mirror 시 size lock UX 처리". |
| **§15 개발 우선순위** | 5단계 | 추가: "1단계 시작 전 ADR-0007/0008/0009/0010 발행 필수, ADR-0002·0006은 D12·D14의 입력 제약 반영하여 작성". |

## 3. ADR 발행 대기열 (Grill 종료 시 한꺼번에 발행)

기존 plan 0002의 ADR 번호 흐름 (0001~0006)을 침범하지 않도록 **0007부터 부여**. 단, **D12(영속화 전송)는 새 ADR을 발행하지 않고 기존 ADR-0002(전송)·ADR-0006(영속화)의 입력 제약으로 반영**한다 — plan 0002 task A2·B3 프롬프트에 본 보고서 D12를 인용 추가할 것.

### ADR-0007 (Proposed): Server / Session / Port 1:1:1 바인딩 모델
- **결정 한 줄**: 한 gtmux Server 프로세스는 정확히 한 tmux Session에 부팅 시 immutable로 바인딩되고 단일 포트에서 동작한다. 여러 Server를 다른 포트로 동시 실행 가능.
- **거절된 대안**:
  - R1. 한 Server가 다중 Session UI 다중화 — 모델 정직성 손실, 영속화·인증 키 복잡도 증가.
  - R2. 런타임 중 바인딩 변경 UI — Scope boundary(UI=pane/panel/group)에 위배.
- **결과**:
  - 긍정: URL·포트·인증·영속화 키가 단순 1:1로 떨어짐. UI scope 명확.
  - 부정: 멀티-Session 사용자는 외부 오케스트레이션 도구 또는 수동 다중 실행 필요. — 본 프로젝트 비범위로 명시.
- **불변식 검증**: 5개 모두 PASS (특히 #4 보안 디폴트 — 포트별 토큰 격리).
- **근거**: 본 grill D1, D2.

### ADR-0008 (Proposed): Single-pane-per-tmux-window 컨벤션 + Group 기반 UI 계층
- **결정 한 줄**: gtmux UI가 만드는 모든 tmux Window는 정확히 1개의 Pane을 담는다. 사용자 측 Panel 묶음은 tmux Window를 노출하지 않고 web-only 계층 Group으로 표현한다.
- **거절된 대안**:
  - R1. (P-γ) Multi-pane Window를 1차 시민화 — 같은 Window 내 Panel의 size가 tmux Layout에 강제로 묶여 자유 resize UX 불가. R1 보고서 §9 (iTerm2 한계)와 동일 문제.
  - R2. tmux Window를 UI에 노출하면서 flat 그룹화 유지 — flat 구조 표현력 부족 + (P-α)에서 의미 없는 "왜 모든 window가 single-pane?" 의문 유발.
- **결과**:
  - 긍정: backend tmux command allowlist 축소(`split-window`·`resize-pane`·`select-layout` 제외) → 보안 표면 ↓. Panel 자유 resize 완전 보장. Group 계층의 표현력은 tmux Window 1-level grouping보다 강함.
  - 부정: tmux 측 Window 수가 Panel 수와 동일 → bootstrap 이벤트 N개 발생 (메모리/CPU 영향은 무시 가능, R1 보고서 §5·§9 근거). 외부 tmux CLI 사용자는 다수의 single-pane window를 보게 됨 → D9의 rename 동기화로 완화.
  - 후속: ADR-0001(tmux 통합)의 명령 allowlist 표를 본 결정에 맞게 조정. ADR-0010(Group 데이터 모델) 별도 발행 — spatial-frame vs logical-only 결정 포함.
- **불변식 검증**: 5개 모두 PASS (특히 #3 tmux Layout ≠ Canvas Layout — gtmux-created 영역에서 기계적으로 보장됨).
- **근거**: 본 grill D4, D8.

### ADR-0009 (Proposed): tmux daemon 격리 모델 (Dedicated daemon per Server)
- **결정 한 줄**: 각 gtmux Server는 자신 전용 tmux daemon에 attach한다. 소켓은 `tmux -L gtmux-<session>` 컨벤션을 사용하며, daemon은 자동 spawn되고 gtmux Server 종료 시 살아남는다.
- **거절된 대안**:
  - R1. (C-B) 단일 gtmux 공유 daemon — 트러스트 경계 공유로 격리 손실 (§13.3.6 정신 약화).
  - R2. (C-C) 사용자 main tmux server attach — gtmux가 사용자 모든 session 가시·잠재 조작. §13.3.6 위반.
  - R3. `-S` 자체 디렉터리 경로 — tmux의 dir perm 자동 강제·SIGUSR1·표준 discovery 컨벤션 손실.
- **결과**:
  - 긍정: 보안 표면 N으로 분할 (한 Server 침해 = 그 session 1개만 영향). 모델 일관성 (1:1:1:1). gtmux Server kill에도 session·pane persistence 유지.
  - 부정: daemon 메모리 baseline ≈ Server × 3.4 MB (50 Server ≈ 175 MB total — 백엔드 프로세스의 약 6%). 외부 attach 명령 길어짐(`tmux -L gtmux-<sess> a -t <sess>`).
  - 후속: `gtmux teardown` CLI에서 socket 파일 명시적 `rm` 단계 필요 (`kill-server` 단독으로는 socket 잔존, 실측 확인).
- **불변식 검증**: 5개 모두 PASS. 특히 §13 보안 디폴트 — 소켓 perm·dir perm tmux 자동 강제 + 격리 가산.
- **근거**: 본 grill D10, 실측 데이터.

### ADR-0011 (Proposed): Backend stack = Rust + axum + tokio
- **결정 한 줄**: gtmux backend는 Rust로 작성하며, async runtime은 `tokio`, HTTP framework는 `axum`, WebSocket은 `tokio-tungstenite`, middleware/tracing/observability는 `tower-http` + `tracing`을 채택한다. 단일 정적 바이너리로 배포한다.
- **거절된 대안**: Bun(메모리/throughput headroom 부족), Node(Bun이 dominant), Go(언어 분리 비용), Python(throughput·distribution 약함). 자세한 비교는 D18.
- **결과**:
  - 긍정: 메모리 footprint ↓, GC pause 없음 → predictable streaming latency, 단일 바이너리 cross-platform 배포, 정적 타입의 보안 표면 ↓.
  - 부정: AI agent의 Rust iteration 비용 1.5–2x, 컴파일 대기 시간.
  - 후속: R7 보고서가 specific crate set 검증 + 50-pane benchmark + scaffolding 코드 산출. R7 결과로 Accepted 승격.
- **불변식 검증**: 5개 모두 PASS. 특히 #4 보안 디폴트 — 정적 타입 시스템이 allowlist·argv 분리 강제를 컴파일 타임에 검증.
- **근거**: 본 grill D18.

### ADR-0012 (Proposed): Frontend stack = Svelte 5 + Vite + TypeScript
- **결정 한 줄**: gtmux frontend는 Svelte 5 (signals) + Vite 빌드 + TypeScript를 채택한다. 터미널 렌더링은 xterm.js (DOM widget) 사용. 무한 캔버스 라이브러리는 R3 결과에 따라 결정 (DOM-host 호환 후보 중에서).
- **거절된 대안**: React(VDOM diff + 50+ Panel memoization 부담), Vue(중립적 trade-off), Solid.js(생태 niche), Vanilla TS(wheel 재발명). 자세한 비교는 D18.
- **결과**:
  - 긍정: MT-3 live state 갱신 비용 ↓, 번들 사이즈 작음, TypeScript로 백엔드 schema와 wire-protocol 공유 (utoipa/schemars로 Rust → TS 타입 자동 생성).
  - 부정: 생태계가 React보다 작음 — 일부 라이브러리 직접 구현 필요 가능.
  - 후속: R8 보고서가 Svelte 5 signals 사용 패턴 + 캔버스 라이브러리 정합 검증 + scaffolding 코드 산출.
- **불변식 검증**: 5개 모두 PASS. 특히 #3 tmux Layout ≠ Canvas Layout — Svelte signals가 panel 위치/크기 변경을 자손 노드까지 자동 전파 (Canvas Layout reactive update).
- **근거**: 본 grill D18.

### ADR-0010 (Proposed): Group 데이터 모델 (G-hybrid)
- **결정 한 줄**: Group은 frame을 1차 상태로 저장하지 않으며 `{label, color, visibility, locked, order, parent_id}`만 가진다. "Group 이동"은 드래그 delta를 자손 Panel 좌표에 일괄 적용하는 액션이고, 생성·해체는 다중 선택(M) 기반 group/ungroup 액션으로만 한다.
- **거절된 대안**:
  - R1. (G-pure-spatial): frame을 1차 상태로 저장 — 자식 추가·제거·리사이즈마다 갱신 로직 + 자식 좌표 표현(절대 vs 상대) 결정 부담 → MVP 과스코프.
  - R2. (G-logical-only): label·visibility·lock만 묶이고 이동은 무관 — Figma 멘탈모델 손실, "묶음" 의미 약함.
  - R3. "+ New Group" 빈 그룹 생성 어포던스 — 다중 선택 group/ungroup만 두어 빈 Group 발생 경로 자체를 차단(명시적 사용자 의도 없이 빈 Group이 생기는 것 방지).
- **결과**:
  - 긍정: 데이터 모델 단순(frame 저장 0). D7과 충돌 없음. Figma 컨벤션과 정렬. Canvas Layout 영속화 스키마 작아짐.
  - 부정: "Group 리사이즈"는 MVP 미지원 (P1+에서 검토).
  - 후속: Canvas Layout 영속화 스키마 ADR-0006(R6 입력)에서 본 결정 반영.
- **불변식 검증**: 5개 모두 PASS. 특히 #2 (tmux-native vs web-only 분기) — Group은 web-only로 명백.
- **근거**: 본 grill D11.

## 4. 후속 grill 대상 (남은 분기)

본 보고서 시점 미해결:
- *(없음 — 본 grill 주요 분기 모두 해소. Unplaced Panel은 D23에서 개념 자체 폐기로 자연 해소.)*

해소 완료:
- ~~G1 Canvas scope~~ → D1
- ~~G2 Window 시각 역할~~ → D4
- ~~G3 panel close 의미~~ → D6 (close = pane kill)
- ~~G4 멀티 탭 정책~~ → D13 (MT-3 Live Mirror) + D14 (WS 0x80–0x8F 정의)
- ~~G5 재접속 출력 히스토리~~ → D15 (per-pane 128 KB ring buffer)
- ~~G6 tmux 서버 라이프사이클~~ → D10
- ~~G7 영속화 전송~~ → D12 (T-mixed: HTTP + WS notify)
- ~~G8 인증 토큰 형식~~ → D17 (R5 흡수 + 매시작 재발급(local)/영속+회전(cloud), OS 위임 P1+)
- ~~G9 panel resize 매핑~~ → D8 (single-pane-window → window resize = pane resize)
- ~~G10 Group 데이터 모델~~ → D11
- ~~Panel Streaming State 도입 요청~~ → D16

## 5. 변경 이력

- 2026-05-13: 초안 (grill 세션 Q1~Q5 산출물 캡처)
