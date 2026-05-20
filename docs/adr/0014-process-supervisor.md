# ADR-0014: Process supervisor — gtmux Server 가 직접 PTY + child process owner

- 상태: Accepted (2026-05-14)
- 일자: 2026-05-14 (Proposed + Accepted 동일)
- 결정자: PM + system-architect (사용자 결정 ↔ Claude 페어, POC `experiments/pty-poc/` 게이트 통과 + ADR-0013 채택 후)
- 근거 보고서: `docs/reports/0023-pty-poc-verification-and-decision.md` §5.3 + §8.2 (Stage A 마무리), ADR-0013 §D2·§D7·§D14
- **Supersedes**: **ADR-0009** ("tmux daemon 격리 모델 — Dedicated daemon per Server"). tmux 가 사라지므로 *daemon 분리* 컨셉 자체가 무의미해진다. 본 ADR 이 새 supervisor 모델로 대체.
- 관련 ADR: ADR-0013 (PTY direct, no tmux — backend 결정 본문), ADR-0007 (Server : Session : Port 1:1:1 — 격리 단위), ADR-0011 (Rust backend — `tokio::process` + `portable-pty` 통합), ADR-0006 (persistence storage — process state 보존 불가 명시)

## 맥락

ADR-0013 채택으로 backend 에서 tmux 가 사라지면, **그동안 tmux daemon 이 우리 Server 와 분리된 별 프로세스로 살아남아 제공하던 격리** (ADR-0009 의 1:1:1:1 Server : Session : Port : tmux-daemon 모델) 도 함께 의미를 잃는다. 우리가 직접 PTY 와 child process 의 owner 가 되면 *격리의 주체* 가 Server 프로세스 자신으로 통합된다 — 분리된 daemon 이 없으므로 daemon 권한 격리, 소켓 파일 perm 자동 강제, `kill-server` teardown 같은 ADR-0009 의 모든 결정 본문이 무효화된다.

본 ADR 은 ADR-0009 를 supersede 하며, *우리 측 process supervisor* 의 라이프사이클·격리·teardown·실측 footprint 를 정의한다. ADR-0009 의 *목적* (한 Server 침해 → 영향 범위 그 Server 안 만) 은 그대로 유지되되, 그 *수단* 이 분리된 daemon 에서 *우리 Server 프로세스 자체의 process 관리* 로 변환된다.

본 ADR 은 ADR-0013 D2·D7·D14 와 짝을 이룬다 — ADR-0013 이 *what* (PTY direct ownership) 를 정의하면, 본 ADR 은 *how to supervise* (process spawn/wait/reap/teardown) 를 정의한다.

## 결정 (Decisions)

- **D1.** [ADR-0013 D1·D2 정합] gtmux Server 프로세스가 **모든 PTY pair + child process (shell) 의 단일 owner** 다. 별 daemon 프로세스 없음. 격리 단위 = **1:1:1 (Server : Session : Port)** — ADR-0007 의 모델 그대로, ADR-0009 의 1:1:1:1 *마지막 차원 (tmux-daemon)* 만 폐기.

- **D2.** [ADR-0013 D2 + 0023 §5.3 S7-PTY-BACKEND] **Pane 라이프사이클 = child process 라이프사이클** — 1:1. Pane 생성 = `portable_pty::native_pty_system().openpty()` + `pair.slave.spawn_command(CommandBuilder)`. Pane 종료 = `kill(child, SIGTERM)` → 200ms wait → 미종료 시 `SIGKILL` (정책 D7 참조) + `child.wait()` 로 reap + PTY pair drop.

- **D3.** [ADR-0013 D1 + 0023 §5.3] 부팅 시 자동 spawn 컨셉 (ADR-0009 D3) **폐기**. Server 부팅 시점에 어떤 child process 도 자동 spawn 하지 않는다. *첫 Pane 은 사용자가 [New Panel] 클릭 또는 API 호출* 로 만든다. 단 CONTEXT.md §"tmux invariant 의 UI 측 mirror" amend 의 *최초 진입 1 panel auto-mount* 정책은 그대로 — Server 부팅이 아니라 *첫 WS attach 시* frontend bootstrap 흐름이 자동으로 [New Panel] 1회 발사. (0022 L-17 결정 정합)

- **D4.** [ADR-0007 D3 정합 + supersede ADR-0009 D4] **Session 부재 검증 폐기** — tmux Session 검사를 안 함 (tmux 가 없으므로). 대신 Server 가 부팅 시 *(session_name, port)* 를 CLI 인자로 받아 *자체 검증* — `${XDG_STATE_HOME}/gtmux/<session>.lock` 의 *세션 lock 파일* 으로 중복 spawn 방지 + port bind 결과로 port 충돌 검증. 부재 시 동작 = *새 logical session 시작* (자동 생성 OK — tmux 의 부재 = 에러 정책이 사라짐, *우리* 가 session 의미를 정의하므로). Session 어휘는 ADR-0007 amend 따라 *logical 식별자* 로 의미 단순화.

- **D5.** [supersede ADR-0009 D5] **Server 종료 시 모든 child process 가 함께 종료** — daemon 분리가 없으므로 *daemon outlives Server* 모델은 폐기. 우리가 process owner 이므로 SIGTERM 받으면 graceful teardown 절차 (D7) 따라 자손 child 들도 정리. 사용자 *재기동 후 동일 상태로 복귀* 는 **layout snapshot 영속** (ADR-0006) 으로 흡수 — process state 가 아니라 *Pane 좌표·메타데이터* 만 복원. 새 child process 는 *재기동 후 사용자 명시 액션* 으로 spawn.
    - **정당화**: tmux 의 daemon persistence 도 *control-mode 재attach* 한정으로, *child shell 의 working directory / 환경변수 / 진행 중 작업* 은 어차피 보존 안 됨 (process state 가 아니라 PTY 연결 재수립). 즉 ADR-0009 D5 의 "Pane persistence 보존" 의 실질 가치 ≈ tmux 도 *재attach 후 새 shell* 모델. 우리도 동일 수준 (layout 만) 보존.

- **D6.** [ADR-0013 D7 정합] **Child 의 zombie reap** = pane 당 별 std::thread 가 `child.wait()` blocking. portable-pty 의 `Child::wait` 가 내부 `waitpid` 로 reap. POC Gate #4 검증으로 zombie 0건 보장.

- **D7.** **명시적 정리 명령** = `gtmux teardown --session <name>` (D20 서브명령). **4단계 절차** (ADR-0009 D6 의 5단계에서 tmux 관련 2단계 제거 + lock 정리 1단계 추가):
  1. gtmux Server 프로세스 종료 — `${XDG_RUNTIME_DIR}/gtmux/<session>.pid` 가 살아 있으면 SIGTERM → 200ms wait → 미종료 시 SIGKILL. graceful (WS close + layout flush, D20).
  2. (구) `tmux kill-server` 단계 **폐기** — Server 가 종료되면 모든 child process 가 자손으로서 함께 SIGHUP 수신 + portable-pty 의 child wait thread 가 reap. *자동 정리*.
  3. (구) tmux 소켓 파일 정리 단계 **폐기** — tmux 소켓 자체가 없음. 대신 `${XDG_STATE_HOME}/gtmux/<session>.lock` 명시 삭제.
  4. `rm -f ${XDG_STATE_HOME}/gtmux/<session>.token` + `${XDG_STATE_HOME}/gtmux/<session>.layout.json` + `${XDG_RUNTIME_DIR}/gtmux/<session>.pid` — state 파일 정리.
  5. `rm -f ${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` — config 파일 정리 (port↔session 영속 mapping).

  플래그:
  - `--force` — 사용자 prompt skip.
  - `--keep-config` — 단계 5 skip (재기동 의도 시).

  부분 실패 시 **exit 7** + 잔여 파일 경로 stderr 출력.

- **D8.** [supersede ADR-0009 D2] **소켓 컨벤션 폐기** (tmux 가 없으므로). 대신 *프로세스 식별자 디렉터리* = `${XDG_RUNTIME_DIR}/gtmux/<session>.pid` (pid 파일) + `${XDG_STATE_HOME}/gtmux/<session>.lock` (단일 인스턴스 lock). 외부 도구의 *활성 Server enumerate* 는 `${XDG_RUNTIME_DIR}/gtmux/*.pid` 패턴으로. ADR-0007 D5 의 *비범위* 정신 그대로 (외부 launcher 도구 분리).

- **D9.** **Shell 종료 시 정책** (CONTEXT.md §"tmux invariant 의 UI 측 mirror" amend 정합):
    - Panel 의 close 비활성 정책 (마지막 child process 1개일 때) 으로 *마지막 child exit 시나리오 자체 차단*.
    - 그래도 (외부 kill 등으로) 마지막 child 가 exit 하면 → Server 는 *자동 재spawn 하지 않음* (ADR-0013 D2 정합) → frontend 에 `pane-died` NOTIFY_MIRROR + 새 panel auto-mount 도 자동 발생하지 않음. 사용자가 명시 New Panel 액션을 다시 호출하거나, 또는 Session shutdown 액션으로 Server 자체 종료.

- **D10.** **외부 tmux nested 사고 차단 — 2-layer 방어** (ADR-0009 O1 → 2026-05-14 amend). tmux 가 사라지므로 ADR-0009 의 *nested attach* 시나리오 자체 무효 — 다만 *외부 tmux 안에서 gtmux Server 가 기동되는* 사고 패턴은 여전히 가능. 두 layer 로 격리:

    **1차 방어 (gtmux-cli startup guard, Stage B)** — Server 부팅 시 `TMUX` env 검출되면 *exit 4* + stderr diagnostic: `"gtmux refuses to run inside an existing tmux session (TMUX env detected). unset TMUX or exit the outer tmux first."`. 0022 L-17 (prevention > recovery) + ADR-0007 D3 (exit 4 = lock conflict 부류 diagnostic) 정합. *fast-fail* — 사용자가 사고를 인지하고 즉시 시정.

    **2차 방어 (PtyBackend spawn 시 env scrub, Stage A 완료)** — `CommandBuilder::env_clear` 후 `TMUX`, `TMUX_PANE`, `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `TERM_SESSION_ID` 5종 제거 후 상속. 1차 방어가 우회된 *비정상 경로* (예: 외부 tmux 가 TMUX env 만 unset 한 채 잔여 메타 env 만 흘리는 변형 환경, 또는 future iTerm/Kitty 변종) 에서도 *TERM_PROGRAM 계열 escape sequence 변종* 으로 인한 화면 깨짐 차단. `TMUX_TMPDIR` 같은 tmux 3.2+ 특수 변수는 1차 방어로 충분 (TMUX 가 set 됐는데 TMUX_TMPDIR 만 set 된 상황은 비현실적).

## 거절된 대안 (Rejected)

- **R1. 별 daemon 프로세스를 우리도 spawn (process supervisor 분리)** — ADR-0009 의 daemon 격리 정신을 그대로 유지하되 tmux 대신 *우리 process supervisor binary* 를 별로 띄움. 장점: Server 재기동 시 child process 들이 daemon 안에서 살아남음. 단점: (a) IPC 채널 (socket / pipe) 새로 설계 — 우리가 *작은 우리 control-mode* 를 만들게 됨 (역설), (b) child process 의 *실제 작업 상태* (working dir, env, 진행 중 명령) 는 어차피 보존 못 함 — ADR-0009 D5 의 *value* 가 실은 매우 작음, (c) POC 가 1-process 모델로 이미 검증됨. 거절.

- **R2. ADR-0009 본문을 amend 로 의미 재해석** — daemon 격리 문구를 process supervisor 어휘로 in-place 교체. D1~D6 모두 tmux-specific 어휘라 amend 가 사실상 rewrite 분량. supersession chain 이 불분명해지고 historical 추적 어려움. 거절.

- **R3. ADR-0007 D3 의 "Session 부재 시 에러" 정책 유지** — tmux Session 검사가 사라져도 *우리 측* 으로 session 존재 검사 (state 디렉터리에 session 파일 존재 여부) 를 유지. 단 우리가 session 의미를 정의하면 *부재 = 새 session 시작* 이 더 자연. 사용자 오타로 빈 session 양산 위험 우려는 *layout 파일이 자동 생성되지 않고 사용자 첫 액션 시점 으로 미뤄짐* 으로 완화. ADR-0009 D3 의 자동 spawn 폐기 (D3) + Session 의 자동 생성 OK (D4) 가 정합.

- **R4. portable-pty 대신 `nix::pty` raw 또는 `pty-process` 사용** — ADR-0013 R3 와 동일. portable-pty 가 production 사용 + 크로스플랫폼 + POC 게이트 통과 → 변경 사유 없음.

- **R5. CRIU 같은 process state checkpoint/restore 채택** — ADR-0013 R4 와 동일. macOS 미지원 + Linux 권한 복잡 + shell/tmux 도 미제공 → 비범위.

## 결과 (Consequences)

- 긍정:
  - **단일 프로세스 모델** — daemon 분리가 없어 IPC / 소켓 파일 / 권한 관리 부담 영구 제거. 디버깅 표면 ↓.
  - **격리 정신 보존** — 1:1:1 (Server : Session : Port) 모델로 한 Server 침해 = 그 Server 의 child process 들만 영향. sibling Server / user main 환경 무영향 (ADR-0007 D5 의 다중 Server 동시 실행 정합).
  - **teardown 단순화** — ADR-0009 D6 의 5단계 → 4단계, 그중 1단계는 *자동* (자손 process SIGHUP). 사용자 체감 동일 + 코드 ↓.
  - **외부 의존 영구 제거** — tmux 3.2+ 시스템 의존성 사라짐 (ADR-0013 정합).
  - **불변식 #4 보안 기본값 강화** — daemon 권한 격리 (tmux 가 자동 0700 디렉터리 보장) 가 사라지지만, *우리 Server 프로세스* 가 spawn 하는 child 는 *우리 권한* 으로만 동작 → 같은 사용자 권한이지만 별 process 트리. 침해 표면이 *우리 Server 프로세스 자체* 로 통합되어 attack surface ↓.

- 부정/비용:
  - **Server 재기동 후 child process 복원 불가** — ADR-0009 D5 의 *외형적* persistence (tmux 재attach) 가 사라짐. 실질 손실은 사용자 체감으로 *재기동 후 새 shell* — 그러나 tmux 도 동일 수준. layout snapshot 으로 *Pane 좌표 + label* 만 복원, child process 는 사용자 New Panel 액션으로 다시 spawn.
  - **사용자 mind-share 측면** — `tmux 가 backend 다` 라는 framing 손실. *우리가 직접 process owner* 라는 새 framing 학습 필요. README + sketch.md framing 갱신으로 흡수.
  - **외부 다른 도구 (예: 다른 IDE / TUI 가 tmux session 에 attach) 와의 통합 불가** — sketch 가 약속 안 한 시나리오, 영구 비범위.

- 후속 작업:
  - **ADR-0009 → Deprecated** (header 만 갱신, 본문 보존).
  - **ADR-0007** amend — D1·D4 의 *tmux Session* 어휘를 *logical Session (사용자 부여 식별자)* 로 정정.
  - **ADR-0006** amend (Sprint 8+) — 본 ADR D5 의 *layout snapshot only* 정책을 implementation 시 input.
  - **ADR-0011** amend — Rust backend stack 표에 `portable-pty 0.9` 추가, `tmux` 시스템 의존 제거.
  - **CONTEXT.md** amend — Pane 정의에서 "tmux 가 관리" 표현 제거, "우리 Server 가 직접 관리하는 PTY + child process" 로 정정. Window 어휘 폐기.
  - **sketch.md** §10.1 (백엔드 구성) — *tmux control-mode client* 를 *process supervisor (portable-pty)* 로 교체. §13.3.6 (tmux socket) — 본 ADR 의 lock 파일 + pid 파일 모델로 교체.
  - **Sprint 7** 의 `S7-PTY-BACKEND` task — 본 ADR 의 D2/D6/D7/D8 를 input 으로 구현.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **재정의: PTY-state / web-state 분리** — 본 ADR 하 *PTY-state* (raw bytes from master fd, child process pid + status) 는 우리 Server 메모리 안에 broadcast channel + child handle 로 보유, *web-state* (panel geometry, group tree, visibility) 는 layout.json + sqlite (ADR-0006) 으로 별도 영속. 두 도메인 자원이 *물리적으로 다른 store* 에 격리. PASS. |
| 2 | tmux-native vs web-only 분기 | **재정의: PTY-native vs web-only** — 본 ADR D2·D6·D9 의 process lifecycle 액션 (spawn, kill, reap, child exit 처리) 은 PTY-native. visibility / lock / group / label 등 panel UI 액션은 web-only. 두 카테고리 dispatch 가 ADR-0013 D10 의 envelope 구획 + Rust enum exhaustive match 로 강제. PASS. |
| 3 | tmux Layout ≠ Canvas Layout | **부분 obsolete** — tmux Layout 컨셉 자체가 ADR-0013 + 본 ADR 채택으로 영구 소거. 본 ADR 하 *single-pane-per-process* 컨벤션은 trivial 유지. 비교 대상 자체가 없으므로 *trivially PASS*. |
| 4 | 보안 기본값 | **PASS** — (a) D7 teardown 5단계 (이제 4단계) 가 sensitive 자원 (token, layout, lock, pid) 모두 정리, (b) D8 의 lock 파일이 단일 인스턴스 강제 + 외부 도구 enumerate 인터페이스, (c) D10 의 `TMUX` env 제거가 nested attach 우발 차단, (d) child process 는 부모 Server 권한 그대로 (= 사용자 권한) — 권한 escalation 없음. ADR-0003 의 12-item 체크리스트는 본 ADR 영향 안 받음. |
| 5 | control mode 사용 | **OBSOLETE / 재정의** — tmux control mode 사용 자체가 ADR-0013 으로 폐기. ADR-0001 의 "스크린 스크레이핑·셸아웃 폴링 금지" 정신은 본 ADR 의 *PTY master fd 단일 byte stream* 로 자연 계승. shell-out 폴링 경로 없음 — child process state 는 PTY master fd 와 `child.wait()` 의 두 채널로만 관찰. PASS. |

## 미해결 항목 (Open)

- **O1. SIGTERM → SIGKILL 의 grace period** — D7 의 200ms 잠정값. 일부 shell (특히 ZSH plugin 로딩 중) 이 200ms 안에 graceful exit 못 할 가능성. Sprint 7 구현 단계에서 실측 후 조정 (500ms 또는 1s 후보). 임계값을 너무 길게 잡으면 teardown 체감 latency ↑.

- **O2. Process re-parent 처리** — 사용자 child shell 이 `disown` 또는 `nohup` 으로 *Server 의 process tree 에서 detach* 한 경우 우리 SIGHUP 가 도달 안 함. 정책: 그런 process 는 *우리 관리 대상 밖*, OS 가 관리. Sprint 7 에서 검증 필요.

- **O3. `${XDG_RUNTIME_DIR}` 부재 시 fallback** — 일부 환경 (특히 macOS sandboxed 또는 일부 Linux distro) 에서 `$XDG_RUNTIME_DIR` 미설정. 후보: `/tmp/gtmux-$UID/` 사용 + 0700 perm 직접 강제. ADR-0011 D6 (config crate) 와 통합 결정.

- **O4. 다중 pane × N 스케일 측정** — 50 pane × 5 burst 시나리오 (ADR-0013 O2) 가 본 ADR 의 *단일 Server 가 50 child process 를 관리* 시 OS 자원 (file descriptor, process slot, memory baseline) 영향. Sprint 7 의 demo 안정화 단계 측정.

- **O5. Persistence-only 사용자 시나리오** — Server 재기동 시 layout 만 복원되고 child process 가 사라지는 UX 가 사용자에게 명확히 전달되는지. (예: 재기동 후 panel 들이 "stale" 표시로 떠 있고 사용자가 클릭해서 새 child spawn 하는 흐름.) → **2026-05-15 amend** 로 ADR-0006 D14 `panels[] strip on boot` 도입 + 본 ADR D11 `boot orphan cleanup` 도입으로 해소.

## 2026-05-15 Amend ×1 — D11 신규: 자손 process 식별 marker + boot-time orphan cleanup

D5 의 graceful shutdown (drop PtyBackend → 모든 child SIGTERM fan-out) 은 *정상 종료* 시점에만 동작. 다음 시나리오는 *비정상 종료* 로 child shells 가 *orphan* (PID 1 입양) 으로 살아남는다:
- Server process 가 SIGKILL 받음 (예: `kill -9`, OOM killer, kernel panic)
- Server process panic + Drop 미실행
- Server process 가 SIGHUP / SIGSEGV 등으로 즉시 종료

이 경우 *다음 Server boot* 가 시작 시 시스템에는 *gtmux 가 spawn 한 child shells* 가 흩어져 있을 수 있다. 자손 식별 + cleanup 메커니즘이 본 amend.

### D11. 자손 marker (env injection) + boot scanner

**spawn 측 (marker 주입)**:
- 모든 child shell 의 env 에 `GTMUX_SESSION=<session>` + `GTMUX_SERVER_PID=<server pid>` 주입 (`PtyBackend::with_session(Some(session))` 생성자)
- `cmd.env_clear` 이후 + `spec.env` user override 직전 단계 (`pty-backend/src/lib.rs::spawn_inner`)
- `noisy env scrub` (D10 1차 방어) 외에는 정상 env inherit — `GTMUX_*` 는 우리만 사용하므로 noise 와 무관

**boot 측 (orphan scanner)**:
- `gtmux-cli start()` 의 step 4a — config 로드 후, PtyBackend 생성 *직전*
- `process_audit::reap_orphans(&session)` 호출
- `sysinfo::System::refresh_processes_specifics` + `Process::environ()` 로 모든 live process 의 env 검사
- `GTMUX_SESSION == <our session>` + `GTMUX_SERVER_PID != current_pid` → orphan
- SIGTERM 발사 + 250ms grace + 잔존 시 SIGKILL escalate
- 결과: `OrphanAuditReport { candidates, signalled, force_killed, warnings }`

**왜 sysinfo crate**:
- Linux: `/proc/<pid>/environ` 읽기. macOS: `proc_pidinfo` + `KERN_PROCARGS2`. cross-platform 추상화.
- 0.32.1 (rust 1.74 호환) 채택. workspace dep `sysinfo = { version = "0.32", default-features = false, features = ["system"] }`.

**거절 (R11)**:
- **R11-A. PID file 의 자손 list** — spawn 시 `<session>.children.json` 에 PID 기록, boot 시 그 PID 들 cleanup. *PID 재활용 race* 문제 (오랜 시간 후 boot 시 우연히 다른 process 가 그 PID 점유). spawn time 같이 기록 시 OS-specific 처리. 거절.
- **R11-B. cgroup / job object** — Linux/macOS 별 OS 특화 + 권한 필요 + 단순 데모 도구에 과잉. 거절.
- **R11-C. ps shell-out** — `ps -E -ax` (macOS) / `/proc/*/environ` (Linux) 직접 처리. cross-platform 추상화 직접 작성. sysinfo 가 이미 처리. 거절.

**정합**:
- ADR-0006 D14 (panels[] strip on boot) 와 함께 *2-layer 정합* — D11 은 *process 수준*, D14 는 *layout 수준*. 둘 다 통과 후에야 fresh PtyBackend 가 안전하게 spawn 시작.
- ADR-0007 D2 (Server : Session 1:1) — `GTMUX_SESSION` 가 1:1 식별자라 다른 session 의 process 는 영향 없음.
- 본 ADR D5 의 graceful shutdown 은 *정상 경로* 그대로 유지 — D11 은 *crash recovery* 측면.

## 2026-05-16 Amend ×2 — D12 신규: HTTP-initiated graceful shutdown + WS `0x89 SERVER_SHUTDOWN` 할당

D7 의 graceful teardown 은 *CLI 진입* (`gtmux teardown --session <name>`) 만 가정. Stage 7 Tier 3 (`docs/sketch.md` §11.2.A 의 G27) 의 FE 측 `ServerShutdownConfirmModal` 은 *browser 진입* — *POST /api/shutdown* 으로 같은 teardown 시퀀스를 트리거해야 함. 본 amend 는 그 endpoint + WS notify frame 의 할당.

### D12. `POST /api/shutdown` (HTTP-initiated) + `0x89 SERVER_SHUTDOWN` 프레임

#### D12.1 Endpoint

```
POST /api/shutdown
  Auth: 기존 `/api/*` middleware (bearer 또는 cookie)
  Body: 무시 (no required fields)
  Res 202 (accepted) + JSON { "shutdown": "scheduled", "expected_exit_code": 6 }
```

**202 의의** = HTTP layer 가 *accepted, async* 라고 명시. 실 teardown 은 detached background task 가 수행 — 본 응답이 client 에 flush 된 후 진행. 동기 응답 (200) 으로 *teardown 완료 후 응답* 패턴은 *connection close 가 응답 도착보다 먼저* 일 가능성 (HTTP 의 일반 race) 때문에 거부.

#### D12.2 6-step teardown 시퀀스 (background task)

```
t0:  HTTP handler return 202 (즉시)
t0+~50ms:
  1. WS broadcast `0x89 SERVER_SHUTDOWN` to every connected webpage
     (server-wide — 다른 session attach 한 webpage 도 notify)
t0+~200ms:
  2. WS connections all close (`CloseFrame::NORMAL_CLOSURE = 1000`)
  3. Session locks 명시 release (LockGuard::release per holder map)
  4. Session record sync flush 보장 — PUT /layout 이 *항상 atomic*
     이라 이 단계는 무동작 (invariant 확인만)
  5. PtyBackend Drop → 모든 child SIGHUP (D5 의 자연 fan-out)
  6. `std::process::exit(6)`
```

각 단계 사이의 short delay (~50-200ms) 가 *FE 의 0x89 수신 + render switch* 시간을 보장 — 응답 race 회피.

#### D12.3 WS frame `0x89 SERVER_SHUTDOWN` 할당

- Type byte: **0x89** (다음 미할당 슬롯, handover §2.3 의 frame 표 정합)
- Inner payload: `varint(0) + UTF-8 JSON { "reason": "user_initiated", "expected_exit_code": 6 }`
  - 선두 varint(0) = web-domain frame 의 *no-paneId* convention (0x85/0x86/0x87/0x88 와 동일)
  - `reason` 필드 = enum: `"user_initiated"` (MVP) / `"oom"` / `"upgrade"` (P1+). FE 는 *알 수 없는 reason* 도 정상 처리 (forward-compat)
  - `expected_exit_code` = `POST /api/shutdown` 응답과 mirror — FE 의 toast 가 양 source 에서 동일 값
- Routing: **server-wide broadcast** (모든 webpage). cookie-driven session filter 적용 X — shutdown 은 모든 session 영향.
- Cap: hub.broadcast cap 32 (low-freq, 매 server lifetime 0~1 회)

#### D12.4 FE 측 처리 (영향, 0044 §3.10 와 정합)

- `lib/ws/decode.ts`: `decodeServerShutdown` 신규 — JSON 파싱 + reason / expected_exit_code 추출
- `lib/ws/dispatcher.svelte.ts`: `handleServerShutdown` 신규 — `ReconnectBanner` 의 *재연결 시도 없음* 분기 진입 + toast "Server stopped (intentionally)" + close code 1000 normal 의 정합 분기 (1000 + 0x89 frame 도착 = intentional, 1000 only without 0x89 = ambiguous)
- `lib/chrome/ServerShutdownConfirmModal.svelte`: 신규 — confirm dialog (활성 session 수 + terminal 수 + "all data flushed") → POST /api/shutdown → 202 응답 후 toast + ReconnectBanner 의 *intentional shutdown* state.

#### D12.5 D7 (CLI teardown) 와의 관계

| 측면 | D7 (CLI) | D12 (HTTP) |
|---|---|---|
| Trigger | `gtmux teardown` 명령 | `POST /api/shutdown` (browser) |
| Auth | OS user (file system 접근) | cookie 또는 bearer |
| WS notify | 없음 (CLI 시점에 서버 외부) | `0x89 SERVER_SHUTDOWN` precede |
| Step 의 자손 cleanup | 4-step (D7) | 6-step (D12.2) |
| 사용자 시나리오 | 개발자 CLI / script | FE 의 명시 Shutdown 버튼 |

둘 다 같은 *underlying teardown primitives* (PtyBackend Drop, LockGuard release, file unlink) 를 호출 — 코드 path 의 큰 부분은 공유. 진입 / notify 만 다름.

#### D12.6 거절된 대안 (R12)

- **R12-A. 동기 응답 (200) + teardown 완료 후 응답**: HTTP connection 의 close 가 응답 도착보다 먼저 가능 — race + UX 불명. 거절.
- **R12-B. WS frame 없이 close 만 (1000)**: FE 가 *어느 close 가 intentional 인지* 알 수 없음 → unnecessary reconnect 시도. 거절 — WS frame 의 명시 의도가 FE UX 의 차별화 가치.
- **R12-C. 0x80 LAYOUT_CHANGED 의 amend 로 처리** (예: 특별 ETag): 의미 overload + decode 분기 복잡. 거절 — 신규 type byte 가 자연.
- **R12-D. SIGTERM via /api/shutdown response handler** (handler 안에서 직접 exit): 응답 flush 보장 안 됨 + tokio runtime 의 task cleanup race. 거절 — detached background task.

#### D12.7 보안 / 가시성

- POST /api/shutdown 은 `/api/*` middleware 그대로 (bearer 또는 cookie) — CLI 와 같은 trust level. CSRF: cookie SameSite=Strict + Origin check (ADR-0020 D2, ADR-0003 D3) 가 third-party origin 차단.
- audit log: 본 amend 는 별 audit 추가 X — 정상 exit (6) 이 server.log 의 마지막 라인으로 충분 가시성.
- abuse: shutdown 은 *destructive* 이나 *single-user gtmux* 이므로 user-vs-user concern 없음. multi-user mode (P3+) 진입 시 별 rate-limit + admin role 필요.

### 정합 갱신

- ADR-0014 D7 (CLI teardown, 4-step) 는 그대로 — D12 는 *별 진입* (HTTP) 의 6-step. 두 path 가 underlying primitives 공유.
- ADR-0002 WS frame 표 갱신 — `0x89 SERVER_SHUTDOWN` 추가, `0x8A~` 미할당.
- handover `0041` §2.3 의 frame 표 갱신 필요 (별 amend).
- `0044` §3.10 의 wire shape 와 정합 (본 D12.1 + D12.3 의 wire 이 그 본 § 의 진실).
