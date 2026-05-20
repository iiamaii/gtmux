# ADR-0013: PTY direct ownership, no tmux backend

- 상태: Accepted (2026-05-14, **amend ×4** 2026-05-15)
- 일자: 2026-05-14 (Proposed + Accepted 동일 — POC 검증으로 즉시 채택)
- 결정자: PM + system-architect (사용자 결정 ↔ Claude 페어, POC `experiments/pty-poc/` 게이트 통과 후)
- 근거 보고서: `docs/reports/0020-debug-classification.md` (Sprint 5 demo 안정화 17건 분류 — 7건이 tmux 통합 노이즈), `docs/reports/0022-logic-amendment-decisions.md` (6 L 결정 grilling), `docs/reports/0023-pty-poc-verification-and-decision.md` (POC 게이트 통과 + 결정 정본)
- **Supersedes**: **ADR-0001** (tmux 통합 = control mode 단일 채널) — 본 ADR 채택과 함께 deprecated 처리.
- 관련 ADR: ADR-0002 (전송 WebSocket — envelope 의미 재정의), ADR-0003 (보안 디폴트 — argv 분리 정책 일부 폐기), ADR-0007 (Server : Session : Port 1:1:1 — Session 의미 단순화), ADR-0008 (single-pane + Group — tmux allowlist 절 폐기, Group 보존), ADR-0009 (daemon 격리 — 의미 재정의 또는 deprecate, O1), ADR-0006 (persistence — implementation 시 자유도 ↑), ADR-0011 (Rust backend — portable-pty crate 추가)
- POC 코드 (throwaway 참조): 브랜치 `poc/pty-direct`, commit `c637c39`, 디렉터리 `experiments/pty-poc/`

## 맥락

`docs/sketch.md` §10.1·§11.2.A 가 *tmux control mode* 를 backend 의 1차 시민으로 두고 ADR-0001 이 이를 단정문으로 격상한 이래, 본 프로젝트는 *tmux-backed Web Canvas* framing 으로 진행됐다. 그러나 Sprint 5 demo 안정화 (2026-05-14) 에서 발견된 17건 결함 중 **7건 (~41%) 이 tmux 통합 표면이 만든 노이즈** 였음이 `0020-debug-classification.md` 에서 분류되었고, 그중 6건이 L-class (planning ambiguity) 로 0022 의 grilling 으로 amend 되었다. 그러나 grilling 과정에서 *근본적인 질문* 이 제기되었다 — "*우리가 tmux 의 어떤 가치를 실제로 사용하고 있는가?*"

정직한 진단 결과 (0023 §1):
- **사용 중**: 표준 PTY 핸들링 + battle-tested signal/race 처리 (둘 다 *tmux-specific 아님*)
- **미사용**: persistence (ADR-0006 미구현), 외부 attach (sketch 미홍보), tmux Layout (불변식 #3 으로 명시 무시)
- **비용**: control-mode parser + argv quoting + `#` quirk + `-d` detach + mutex split + two-domain envelope (불변식 #1 의 강제 비용) + ADR-0001/0008/0009 의 ~3000 LOC

이 진단을 토대로 본 세션 (2026-05-14 동일 일자) 에 `experiments/pty-poc/` POC 가 구축되었다 — portable-pty 0.9 + tokio broadcast + axum WS 의 199-LOC Rust binary + 75-LOC HTML. **Day 1 게이트 (signal / resize / exit + zombie reap) + Day 2 게이트 (alt-screen / burst + 안정성) 모두 통과**. 보너스로 *multi-tab mirror* 가 `tokio::broadcast` 의 자연 동작만으로 trivial 하게 달성 — tmux 의 marquee 다중-attach mirror 기능이 ~20 LOC 로 재현됨.

본 ADR 은 이 POC 결과를 단정문으로 격상하고, ADR-0001 을 supersede 한다.

## 결정 (Decisions)

- **D1.** [POC §1.1 + 0023 §2.1] gtmux backend 의 *terminal 실행 단위* 는 **portable-pty crate (0.9+) 가 추상화한 PTY pair (master + slave)** 다. 각 **Pane** (CONTEXT.md 어휘) = 1 PTY pair + 1 child process (`$SHELL` 또는 사용자 설정 명령). tmux 또는 다른 multiplexer 를 backend 로 두지 않는다. *우리가 직접 OS PTY 를 owner*.

- **D2.** [POC §1.1] Pane 라이프사이클 = child process 라이프사이클. spawn 은 `portable_pty::native_pty_system().openpty()` + `pair.slave.spawn_command(CommandBuilder)` 의 단일 경로. shell 종료 (`exit` / Ctrl-D / 외부 kill) = Pane 종료 = WS 측 `pane-died` NOTIFY_MIRROR broadcast 후 Pane resource (master fd, broadcast channel, mpsc queue, scheduled tasks) 해제. **자동 재시도 / 자동 재spawn 없음** (ADR-0001 D12 의 "자동 재시도 안 함" 정신 그대로 계승).

- **D3.** [POC §1.1] **PTY 출력 fan-out** = pane 당 `tokio::sync::broadcast::Sender<Bytes>` 1개. cap = 512 (default), 운영 단계에서 측정 후 조정. master fd 의 blocking read 는 *std::thread* 위에서 수행 (portable-pty 의 reader 는 `Box<dyn Read + Send>` blocking), 각 chunk 를 `Bytes::copy_from_slice` 후 broadcast. broadcast cap overflow 시 portable-pty 측 read 가 자연 stall → kernel PTY buffer 가 차오르면 shell 측에서 자연 backpressure (line discipline ` IXON`/`IXOFF` 또는 epoll wait). pause-after 같은 명시 backpressure 명령 컨셉 폐기 — *직접 fd 제어로 충분*.

    **Layered late-mount 책임 분리 (2026-05-14 amend)**: 두 buffer 가 *서로 다른 race* 를 다룬다 — 같은 데이터를 두 번 캐싱하는 게 아님.
    - **Backend per-pane ring** (128 KiB) — WS attach 가 *Pane spawn 직후 ~ms* 안에 일어나지 않을 때의 갭. broadcast::Receiver subscribe 이전에 흘러간 PTY master bytes 를 다음 attach 가 catch-up 할 수 있게 보존. lifetime = Pane 생존 동안.
    - **Frontend dispatcher buffer** (256 KiB, 0022 L-12) — XtermHost 가 *registerPaneOut* 등록 *이전* 에 dispatcher 에 도달한 PANE_OUT 을 보존. lifetime = first registerPaneOut OR pane close (frontend Panel 생애주기 기준).

    두 buffer 의 *주체* 가 다르므로 (backend = Pane, frontend = Panel) 의미 중첩이 아니라 *서로 다른 race window* 의 격리.

- **D4.** [POC §1.1] **PTY 입력 fan-in** = pane 당 `tokio::sync::mpsc::UnboundedSender<Vec<u8>>` 1개. writer thread (std::thread) 가 mpsc 를 blocking_recv 후 master writer 에 `write_all` + `flush`. 다수 WS 클라이언트의 입력이 mpsc 로 합류 — *clientId 구분 없음* (MT-3 정합, ADR-0002 D3).

- **D5.** [POC §1.1] **리사이즈** = WS `0x04 PANE_RESIZE` 또는 동등 envelope 수신 → `MasterPty::resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })` 호출. portable-pty 가 내부적으로 `TIOCSWINSZ` ioctl 발급 → 커널이 child process 에 SIGWINCH 전송. vim / top / less 등 TUI 가 자연 reflow.

- **D6.** [POC §1.1] **신호 처리** = PTY line discipline 이 자동. Ctrl-C → SIGINT, Ctrl-D → EOF, Ctrl-Z → SIGTSTP, Ctrl-\ → SIGQUIT 모두 child 에 자연 전달. 우리는 *바이트만 PTY 에 write* 하면 됨. 추가 명시 신호 인터페이스 없음. 단 *child 의 비정상 종료* (segfault, OOM kill 등) 는 D2 의 wait() 경로로 흡수.

- **D7.** [POC §1.1] **Child exit + zombie reap** = pane 당 별 std::thread 가 `child.wait()` 를 blocking 호출. exit 시 status broadcast (`0x07 NOTIFY_MIRROR { kind: "pane-died", code, signal? }`) 후 thread 종료. portable-pty 의 `Child::wait` 가 내부 `waitpid` 로 reap. zombie 0건 보장 (POC Gate #4 검증).

- **D8.** [0023 §2.3 R4 + 7.1] **외부 attach (tmux a 류) 비범위**. gtmux Server 의 *유일한 접근 경로* = HTTP + WS. CLI client 도입은 P1+ 사용자 검증 후 별 ADR 로 결정. 본 ADR 시점 영구 비범위.

- **D9.** [0023 §2.3 R4 + 7.1] **Persistence** = ADR-0006 (Canvas Layout 영속) 의 구현 시 자유도가 ↑. 우리가 PTY 와 child process 를 직접 보유하므로 *process state 보존* 은 어차피 불가능 (tmux 도 동일 수준이었음 — Server 재기동 후 새 shell 로 재attach). Layout snapshot + Group 트리 + Panel 메타데이터 만 sqlite 또는 JSON file 로 영속.

- **D10.** [0023 §4 + ADR-0002 amend 계획] WS envelope (ADR-0002 D2~D4) 구조 자체는 보존하되 *의미 재정의*:
    - 0x01–0x0F slot 의 "tmux-domain" 라벨이 **"PTY-domain / process-supervisor-domain"** 으로 변환.
    - `0x01 CTRL` payload = *우리 API command schema* enum. **2026-05-14 amend** — MVP 범위 = `new-pane`, `kill-pane`, `resize-pane` 3종 (YAGNI). 구 amend draft 의 `set-cwd`/`set-env` 는 *실제 사용 시점* 까지 재추가 보류. tmux argv 배열 폐기. allowlist 컨셉이 *Rust enum exhaustive match* 로 compile-time 강제 (ADR-0011 D4 정합, ADR-0008 D2 allowlist 표는 폐기).
    - `0x02 PANE_OUT` payload = PTY master fd 의 raw bytes 그대로 (tmux 의 `%output` 8진수 decode 라운드트립 폐기 — 우리가 raw bytes 를 직접 받음).
    - `0x03 PANE_IN` payload = raw bytes. 우리가 PTY master writer 에 그대로 write.
    - `0x04 PANE_RESIZE` 그대로.
    - `0x05/0x06 PANE_PAUSE/RESUME` — visibility 기반 Panel Streaming State (CONTEXT.md, ADR-0001 D8 의 *visibility=hidden* 트리거) 는 유지하되 backend 구현이 *우리 broadcast 채널의 subscribe drop* 으로 변환. tmux 의 `refresh-client -A pause` 컨셉 폐기.
    - `0x07 NOTIFY_MIRROR` payload = 우리 정의 enum (`pane-died`, `pane-spawned`, `layout-changed`, `server-ready` 등). tmux 의 `%window-add`/`%session-changed`/`%pane-mode-changed` 같은 14종 native 알림과 1:1 매핑이 사라짐 → 우리가 *필요한 종류만* 정의. **2026-05-14 amend** — `daemon-started` (구) → `server-ready` (ADR-0014 D1 정합, daemon 어휘 영구 폐기).

- **D11.** [POC §1.4 보너스] **Multi-attach mirror** = pane 당 `broadcast::Receiver` subscribe 한 *각 WS 클라이언트* 에게 PANE_OUT 자동 fan-out. tmux 의 multi-attach mirror 패턴 (한 사용자 + 여러 client 가 같은 session view) 이 *자동 달성* — 추가 코드 없이. ADR-0002 D3 의 MT-3 정책 정합 강화.

- **D12.** [0023 §4 ADR-0008 부분 amend] **Command allowlist 컨셉 폐기**. tmux 명령 어휘 (new-window, kill-pane, send-keys, refresh-client, ...) 자체가 사라짐. 대신 *우리 API command schema enum* 이 compile-time allowlist 역할 — Rust 의 exhaustive `match` + `#[non_exhaustive]` 부재로 *enum 추가가 곧 명시 API 확장*. 명령 주입 표면 영구 0.

- **D13.** [ADR-0011 D8 정합] **argv 안전 quoting** 컨셉 (0022 의 L-7 → ADR-0001 D13 amend 본문) **폐기 / moot**. 우리가 shell 또는 tmux 의 stdin 토큰화에 의존하지 않으므로 `#` 라인-주석 quirk + whitespace/quote escape 표면 자체 소멸. `lifecycle::serialise_command` 도 폐기 (lifecycle crate 자체 폐기).

- **D14.** [Sprint 7 계획, 0023 §5.3] **crate 구조 변경**:
    - 신규: `crates/pty-backend` — PTY pair owner, broadcast/mpsc fan-out/in, child supervisor.
    - 폐기: `crates/lifecycle` (TmuxDaemon, control-mode parser, %output decoder, command-number matcher).
    - 폐기 또는 의미 단순화: `crates/mux-router` (allowlist + argv serialiser) → 필요 시 `crates/wire-router` 로 rename + WS envelope routing 만 담당.
    - 그대로: `crates/auth`, `crates/config`, `crates/http-api`, `crates/ws-server` (CTRL 라우터만 재배선), `bin/gtmux-cli`.

## 거절된 대안 (Rejected)

- **R1. 현 tmux 유지** — 본 세션 결함 17건 중 7건이 tmux 통합 노이즈였고, 같은 부류가 누적될 추세 (ADR-0001 의 O1·O2·O3 carry 항목 + ADR-0009 의 O1·O2 + 본 세션 0022 의 6 amend) 가 명백. *유지 비용 > 전환 비용*. 0022 amend 6건은 결함 부류를 *containing* 하는 작업이었으나 *제거* 가 더 깔끔. 0023 §2.3 R1.

- **R2. Strangler fig (trait `TerminalBackend` 추상화 + 두 구현 영구 유지)** — 코드 표면 ↑, 추상화 누설 (tmux 의 어휘가 trait 에 새는 함정), 결국 두 구현 모두 디버깅. 한 번에 swap 이 더 깔끔 + Sprint 7 시점에서 *학습 비용 회수* 가능. 0023 §2.3 R2.

- **R3. 다른 PTY crate 채택** — `nix` raw / `pty-process` (tokio-aware) / 직접 `forkpty(3)` 호출. portable-pty 가 wezterm·alacritty 가 production 사용 + 크로스플랫폼 (macOS / Linux / Windows ConPTY) + tokio 와 std::thread 양면 통합 가능. POC 가 portable-pty 로 게이트 통과 → 변경 사유 없음.

- **R4. Persistence with process state (CRIU 등)** — Linux CRIU (Checkpoint/Restore In Userspace) 또는 fork-resurrection 기법으로 *Server 재기동 후 child process 상태 복원*. (a) macOS 미지원, (b) Linux 에서도 권한 / namespace 의존성 복잡, (c) shell + tmux 도 이 기능 제공 안 함 → 사용자 기대치도 그 수준. 비범위. 0023 §7.1.

- **R5. Day 3 persistence prototype 추가 후 결정** — POC scope 확장. Day 1·2 게이트 이미 통과 + persistence 가 MVP 미사용이라 검증 가치 작음. Sprint 7 안에서 본격 측정 가능. 0023 §2.3 R3.

## 결과 (Consequences)

- 긍정:
    - **결함 부류 영구 소거** — control-mode parser / `#` quirk / `-d` detach / mutex split / argv injection / allowlist 우회 — Sprint 5 demo 안정화의 7건 부류가 다시 발생할 *표면이 코드에 존재하지 않음*. 
    - **multi-tab mirror trivial** — `tokio::broadcast` 의 자연 동작만으로 tmux 의 marquee 기능 (다중 attach mirror) 달성. ADR-0002 D3 의 MT-3 정책이 *zero-cost*. 
    - **외부 의존 제거** — tmux 3.2+ 시스템 의존성 사라짐. 배포 단순화 (`cargo install gtmux` 만으로 끝).
    - **코드 net 감소** — lifecycle 1820 + mux-router 890 LOC = ~2700 LOC 폐기, 새 pty-backend ~1500 LOC. Net **약 -1000 LOC** + 도메인 지식 부담 (control-mode protocol, tmux 어휘 14종 알림, allowlist 표 9 명령) 영구 제거.
    - **불변식 #1 단순화** — *tmux 상태 vs 웹 상태 분리* 의 *역사적 동기* (tmux 가 만든 외부 진실 mirror 보호) 가 사라짐 → 단일 진실 모델. envelope 의 두-도메인 분리 (0x01-0x0F vs 0x80-0x8F) 는 구조로 유지하되 *negative space 강제* 컨셉이 weaker.
    - **Sprint 6 의 L-7 amend (ADR-0001 D13) 가 자동 무효화** — 0022 의 amend 6건 중 1건 작업 폐기, 5건은 그대로 살아남음.
    - **persistence (ADR-0006) 의 자유도 ↑** — tmux 의 persistence 모델 (재attach with control-mode) 흉내가 아니라 우리가 *layout snapshot only* 로 자유 설계.
- 부정/비용:
    - **~3000 LOC + ADR 1건 (0001) + Sprint 5 grilling 작업의 일부 (L-7) 가 throw**. 0023 §3 의 maintain/discard 매트릭스로 잔존 가치는 보존.
    - **PTY 함정 직접 부담** — portable-pty 가 추상화하지 못하는 edge case ($TERM 변종, OSC 시퀀스, 일부 alt-screen quirk) 가 발생할 때 *우리가* 디버깅. tmux 30년 검증 코드 의 wisdom 손실. 단 wezterm/alacritty 가 portable-pty 위에서 production 운영 중 → wisdom 의 일부 흡수.
    - **외부 attach 같은 power-user 기능 영구 상실** — sketch 가 약속 안 한 기능이라 마찰 ↓ 이나 *tmux 사용자 mind-share* 의 일부 포기. CLI client 신설은 P1+ 옵션 (D8 비범위 명시).
    - **프로젝트 framing 변경** — "gtmux" 의 *graphical-tmux* 의미가 흐려짐. README + sketch.md framing 재정의 필요. 명칭 자체 rename 은 운영 비용 ↑ → "originally tmux-backed, self-hosted PTY supervisor since 2026-05-14" 로 historical note 유지가 현실적 (0023 §7.2 O3).
    - **Sprint 7 가 일정 ↑** — 2-3 주 추가 work (S7-PTY-BACKEND + S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE + S7-DEMO-STAB). 단 회수 = 향후 Sprint 안정성.
- 후속 작업:
    - **ADR-0008** amend — tmux command allowlist 절 폐기. single-pane-per-window 컨벤션은 *single-pane-per-process* 로 의미 단순화. Group 부분 그대로. 0023 §8.2.
    - **ADR-0009** — deprecate 또는 전면 amend (process supervisor 의미로 재해석). 0023 §7.2 O1 다음 세션 결정.
    - **ADR-0002** amend — envelope 의 두-도메인 라벨 재정의 + D7 backpressure 의 pause-after 컨셉 폐기.
    - **ADR-0003** amend — D7 (argv 분리 정책) + D8 (식별자 정규식 tmux 측) 일부 폐기. CORS / token / CSP 절 그대로.
    - **ADR-0007** amend — Session 의미 단순화 (tmux session 대응 사라짐). 0023 §7.2 O5.
    - **CONTEXT.md** amend — Pane 정의에서 "tmux 가 관리하는" 표현 제거, Window 어휘 폐기, tmux Layout 어휘 폐기, "tmux-측 mirror" 섹션 전체 재정의.
    - **sketch.md** §10.1 / §11.2.A / §13.3.6 / §14 rewrite — tmux backend 전제 부분 전면 재작성.
    - **Sprint 7** 의 본격 코드 swap 실행 — 0023 §5.3 의 S7 task 표 따라.

## 불변식 검증

| # | 불변식 | 검증 (본 ADR 하) |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **재정의: PTY 상태 / 웹 상태 분리** — 본질 동일. D10 envelope 의 0x01-0x0F (PTY-domain) / 0x80-0x8F (web-domain) 구획이 *바이트 수준 분리* 그대로 유지. *우리가* 진실 owner 인 PTY-domain 데이터 (raw bytes, process state) 는 web state (panel geometry, group, visibility) 와 *물리적으로 다른 자원* (broadcast channel vs HTTP layout PUT). 강제력은 ADR-0011 enum exhaustive match. ADR-0001 시점 대비 약간 *weaker* (외부 진실 vs 우리 진실) 이지만 코드 분기 구조는 동일. |
| 2 | tmux-native vs web-only 분기 | **재정의: PTY-native vs web-only 분기** — D10 의 envelope 구획 + D12 의 API command schema enum 이 분기 dispatch. PTY-native 액션 (spawn, kill, resize) 은 enum variant 가 정의된 것만 — 분기 *밖* 의 호출 경로 없음. PASS. |
| 3 | tmux Layout ≠ Canvas Layout | **부분 obsolete + 단순화**: tmux Layout 개념 자체가 사라짐. 본 ADR 하 *PTY Layout* 같은 컨셉도 없음 — single-pane-per-process 컨벤션 (ADR-0008 amend) 하 trivial. Canvas Layout 만 남음 → 비교 대상 자체가 없으므로 *trivially PASS* + 코드 표면 ↓. |
| 4 | 보안 기본값 | **PASS (강함)** — (a) tmux command allowlist 의 *문자열 표* 가 사라지고 Rust enum 의 compile-time exhaustive match 로 강제 (더 강함), (b) argv quoting / 셸 escape / `#` quirk 부류 결함 표면 영구 소거, (c) ADR-0003 D5 (WS subprotocol token) / D6 (Authorization Bearer + Sec-Fetch-Site, 0022 amend 의 cookie 폐기 정합) / D11 (CSP) 그대로 — 모두 backend 무관. PTY 자체는 OS-수준 격리 (kernel 가 line discipline + 권한 관리). |
| 5 | control mode 사용 | **OBSOLETE / 재정의** — 원안 "tmux control mode 단일 채널" 은 본 ADR 시점 *영구 폐기*. 대체 정신: *"shell 호출 → 결과 파싱" 같은 fork-exec 폴링 경로를 두지 않는다. PTY master fd 의 단일 byte 스트림이 진실 채널"*. ADR-0001 의 원 거절 대안 R1 (스크린 스크레이핑) 과 R2 (셸아웃 폴링) 의 *거절 정신* 은 그대로 유지 — 우리도 그것들을 채택 안 함. |

## 미해결 항목 (Open)

- **O1. ADR-0009 처리 방식** — deprecate 후 신 ADR-0014 "process supervisor" 신설 vs 본 ADR-0009 의 §맥락/§결정을 *우리 측 supervisor* 로 재해석하는 amend. 다음 세션 (Stage A 마무리) 에서 결정. 0023 §7.2 O1.

- **O2. Sprint 7 안 multi-pane 스케일 측정** — 50 pane × 5 burst 시나리오 (ADR-0001 D9 / Grill D19 의 원 게이트) 의 p99 latency + memory baseline. POC 는 1 pane scope 였으므로 본격 측정 필요. 결과로 D3 의 broadcast cap (512) + master fd read buffer size (8192) 가 적정한지 보정.

- **O3. shell exit 시 UX 정책** — 현 POC 는 child exit 시 `\e[33m[poc] child exited\e[0m` 메시지 한 줄. Sprint 7 본격 구현에서는 0022 의 L-17 prevention 모델 (close 비활성 + Session shutdown UI 액션) 과 정합 — *exit 자체가 발생하지 않게* 막는 것이 우선이고 그래도 발생하면 dead pane placeholder 로 처리.

- **O4. $TERM 변종 호환성 매트릭스** — 본 POC 는 `TERM=xterm-256color` 고정. 일부 사용자 환경 (예: vt220, screen-256color, linux) 또는 일부 TUI (mosh-like, ncurses 변종) 에서 escape sequence 차이 검증 필요. 다음 세션 또는 Sprint 7 데모 안정화 시점.

- **O5. 외부 CLI client 도입 시점** — D8 의 비범위 결정은 *MVP 시점* 한정. 사용자 검증으로 power-user 가 외부 attach 를 강하게 원하면 P1+ ADR 로 도입. 우리 측 CLI 가 WS endpoint 에 attach 하는 표준 인터페이스 — tmux a -t 같은 사용 경험 제공 가능.

- **O6. portable-pty Windows 지원** — portable-pty 가 ConPTY 를 지원하나, 본 ADR 시점 sketch 는 Windows native 를 우선 시나리오로 두지 않음. WSL 사용자는 Linux backend 그대로. P1+ 결정.

- **O7. persistence 구체 구현** — ADR-0006 의 sqlite vs JSON file vs hybrid. 본 ADR 의 D9 가 자유도만 ↑ 하고 결정 안 함. Sprint 8 이후 별 ADR.

- **O8. xterm.js 키맵 갭** — 0023 §1.5 의 Shift / Option 모디파이어. tmux / PTY 무관, frontend 측 별 개선 — `SECURE_XTERM_OPTIONS` (ADR-0004) 에 `macOptionIsMeta: true` 등 추가. Sprint 7 backlog.

## 변경 이력

- 2026-05-14: Accepted — POC `experiments/pty-poc/` (commit `c637c39`) 의 Day 1·2 게이트 통과 직후, 0022 grilling + 0023 결정 정본 위에서 채택. ADR-0001 supersede.
