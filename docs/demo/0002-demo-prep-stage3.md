# 데모 시연 준비 패키지 — Sprint 7 (S7-PERSISTENCE-MINIMAL 완료 시점)

- 일자: 2026-05-15
- 작성: agent (S7-FE-SHUTDOWN 진입 전 데모)
- 기준 문서: `docs/reports/0028-s7-persistence-minimal-closeout.md`, `0027-session-resume-handoff.md`
- 폐기 조건: S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-DEMO-STAB 완료 후 `0003-demo-prep-sprint7-closeout.md` 로 갱신
- 이전 산출물: `0001-demo-prep.md` (Sprint 0 직전, M1+M2 한정 — **historical**, 본 문서가 supersede)

## 0. 영향 범위 선언

본 패키지는 *시연 흐름과 사전 점검만* 정의하며 코드/ADR/SSoT 에 쓰기 작업을 하지 않는다. S7-FE-SHUTDOWN dispatch 와 병행 가능.

데모 진행 중 발생할 수 있는 side-effect:
- `${XDG_STATE_HOME}/gtmux/demo.{token,pid,layout.json}` 생성·갱신 — 데모 후 `gtmux teardown --session demo --force` 로 정리
- 브라우저 sessionStorage 의 `gtmux_token` 키 — 데모 후 탭 종료 시 자동 소멸

## 1. 현 시점 capability 매트릭스

`docs/sketch.md` §15 의 5 단계 로드맵 대비 현 시점:

| §15 단계 | 시연 가능 여부 | 비고 |
|---|---|---|
| 1) 엔진 연결 | ✅ | PTY direct backend (ADR-0013) — tmux 의존성 폐기 |
| 2) 기본 캔버스 UI | ✅ | xyflow/svelte + xterm.js + drag/resize |
| 3) 영속화·재연결 | ✅ | **S7-PERSISTENCE-MINIMAL 완료** — atomic disk write + sidecar quarantine |
| 4) UX 폴리시 | 🟡 부분 | Session shutdown UI / close-last-pane guard 잔여 (S7-FE-SHUTDOWN/CLOSE-GUARD) |
| 5) 보안 하드닝 | ✅ | ADR-0003 D1~D13 + nested-tmux guard (ADR-0014 D10 amend) 모두 진입 |

본 시연은 **단계 1~3 정식 + 단계 5 부수** 시현 (M3+).

## 2. 동작 / 미동작 매트릭스

### 2.1 동작 (시연 가능)

| 카테고리 | 항목 |
|---|---|
| Lifecycle | `gtmux start` / `stop` / `teardown` / `rotate-token` / `status` |
| Bootstrap | `/auth/bootstrap?token=<TOK>` 1회 cookie 교환 + token in sessionStorage (HttpOnly cookie 병행) |
| HTTP | `GET /healthz`, `GET /api/layout` (ETag), `PUT /api/layout` (If-Match), security headers full |
| WebSocket | `Sec-WebSocket-Protocol: gtmux.v1, bearer.<TOK>` handshake + multiplexed pane-output broadcast |
| Pane lifecycle | 사용자 "New Panel" → backend PTY spawn → child shell → xterm 렌더 + 입력 echo |
| Layout | drag / resize / z-order → PUT → **atomic disk write** → LAYOUT_CHANGED broadcast |
| 영속화 | Server 재기동 후 layout 보존 (디스크 hash 재계산 → 동일 ETag) |
| Multi-tab | MT-3 Live Mirror — 같은 Server 의 모든 WS 연결이 동일 상태 |
| Guards | nested-tmux 차단 (TMUX env 감지 → exit 4), 손상 파일 sidecar quarantine, 0600/0700 perm audit |
| graceful shutdown | Ctrl-C / SIGTERM → 모든 child shell SIGTERM → 200ms grace → SIGKILL fan-out |

### 2.2 미동작 (시연 회피)

| 항목 | 잔여 task | 회피 |
|---|---|---|
| Session shutdown via UI | S7-FE-SHUTDOWN | 외부 `gtmux stop --session demo` / Ctrl-C 사용 |
| Close-last-pane guard | S7-FE-CLOSE-GUARD | 마지막 패널은 close 하지 않음 (현재 close 가능하지만 invariant 침해) |
| Backend → frontend automount cascade | S7-BE-AUTOMOUNT | 현재 모든 spawn 경로가 frontend 발기 (New Panel) — 영향 없음 |
| 외부 CLI client (다른 tool 이 직접 PTY attach) | 비범위 (ADR-0013 D8) | 시연 생략 |
| Playwright 시각 자동화 | 비범위 (사용자 환경) | 수동 브라우저 시연 |

## 3. 사전 준비 체크리스트 (시연 호스트 — 이미 완료된 항목 ✅)

- [x] `cargo test --workspace --tests` → 164 PASS / 0 FAIL
- [x] `cargo clippy --workspace --all-targets -- -D warnings` → clean
- [x] `cargo fmt --all -- --check` → clean
- [x] `svelte-check` → 0 errors / 0 warnings
- [x] `cargo build -p gtmux-cli --release` → release binary @ `codebase/backend/target/release/gtmux`
- [x] `cd codebase/frontend && npm run build` → SPA bundle @ `codebase/frontend/dist/`
- [x] Stale Server 정리: `gtmux stop --session demo` (이전 세션의 pre-S7 binary 종료)
- [x] **외부 tmux 밖에서 `gtmux start --session demo --port 9999` 실행 중** — banner 의 token URL 확보

데모 직전 점검:
- [ ] 브라우저 캐시 / sessionStorage 정리 (Cmd-Shift-Delete 또는 비공개창 권장)
- [ ] 화면 공유 시 *token URL 노출 후 회전 계획* — 시연 종료 시 `gtmux rotate-token --session demo`
- [ ] 외부 tmux 안에서 실행하지 않을 것 — ADR-0014 D10 가드는 demoable 하지만 데모 호스트가 그 안에 있으면 step A 부터 실패

## 4. 시연 시나리오 (8 step, 약 15~20분)

### A. Cold start + 첫 패널 표시 (3분)

```bash
# 0. 사전 (이미 실행 중) — 화면에 보여줄 명령
unset TMUX
cd /Users/ws/Desktop/projects/gtmux
GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999

# 1. banner 출력 부분 강조
# - Mode: Local
# - Open URL: http://127.0.0.1:9999/auth/bootstrap?token=<TOK>
# - Backend: PtyBackend (ADR-0013, supervisor pid=<PID>)

# 2. 브라우저로 Open URL 열기 → cookie 교환 → 빈 캔버스
# 3. 우상단 "New Panel" 클릭 → 첫 패널 표시
# 4. 패널 안 클릭 → `echo hello && date && uname -a` 입력
```

내레이션: *"tmux 가 사라졌다. backend 가 PTY + child shell 을 직접 supervisor 한다. ADR-0013."*

### B. 다중 패널 + 드래그 + 영속화 (4분)

```bash
# 5. New Panel 2~3회 추가 → cascade 배치 확인 (각 +40px offset)
# 6. 한 패널에 `vim` 실행 → alt-screen 진입 → 다른 패널과 독립 입력
# 7. 패널 드래그로 위치 변경 (≥ 3회)
# 8. 페이지 새로고침 (F5) → 위치 + 활성 PTY 둘 다 살아 있음 확인
# 9. (개발자 시연) 디스크 영속화 직접 보기:
cat ~/.local/state/gtmux/demo.layout.json | jq .
# {
#   "schema_version": 1,
#   "groups": [],
#   "panels": [{ "id": "p...", "x": .., "y": .., "z": .., ... }, ...]
# }
ls -la ~/.local/state/gtmux/demo.layout.json
# -rw------- 1 ws  staff   ... bytes  ...  ~/.local/state/gtmux/demo.layout.json
```

내레이션: *"atomic write — tmp + fsync + rename + dir fsync. 256 KiB cap. ETag-based optimistic concurrency. ADR-0006."*

### C. Server 재기동 → layout 보존 (★ S7-PERSISTENCE-MINIMAL 핵심, 3분)

```bash
# 10. 현재 layout ETag 확인
TOK=$(grep -oE 'token=[A-Za-z0-9_-]+' /tmp/gtmux-banner.log | head -1 | cut -d= -f2)
curl -s -D - -H "Host: 127.0.0.1:9999" -H "Authorization: Bearer $TOK" \
  http://127.0.0.1:9999/api/layout | grep -i etag

# 11. Server 종료
./codebase/backend/target/release/gtmux stop --session demo
# "gtmux stop: server (pid <N>) stopped gracefully."

# 12. 재기동
GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999

# 13. 부팅 로그 확인 — 이번엔 "cold start" 가 아닌 디스크 hash 재계산
# (silent — D10 row 2)

# 14. 새 banner 의 새 token 으로 브라우저 재인증 → 캔버스에 같은 패널 배치 복원
```

내레이션: *"패널의 child process 는 죽었지만 Canvas Layout 은 살아 있다. 두 도메인 분리 (CONTEXT.md §"두 상태 도메인") 의 정밀한 실증."*

### D. MT-3 Live Mirror — multi-tab (2분)

```bash
# 15. 같은 브라우저에서 새 탭 → 같은 URL 열기 (cookie 공유 → 자동 인증)
# 16. 탭 1 에서 패널 드래그 → 탭 2 에 즉시 반영 (LAYOUT_CHANGED broadcast)
# 17. 탭 1 의 한 패널에서 타이핑 → 탭 2 의 같은 패널에 echo (multiplexed pane output)
```

내레이션: *"identity 로 구분하지 않는다. 단일 사용자 = 모든 연결이 거울. CONTEXT.md §MT-3."*

### E. Nested-tmux guard (1분, 보안 demo)

```bash
# 18. 외부 tmux 열기 (선택 — Claude 세션이 이미 외부 tmux 안일 수 있음)
tmux new -d -s nested_demo
tmux send -t nested_demo 'cd /Users/ws/Desktop/projects/gtmux && ./codebase/backend/target/release/gtmux start --session demo2 --port 9998' Enter
sleep 1
tmux capture-pane -t nested_demo -p | tail -5
# "gtmux start: refusing to start inside an existing tmux session ..."
# exit 4
tmux kill-session -t nested_demo
```

내레이션: *"prevention > recovery (0022 L-17). child shell 의 환경 오염은 사후 복구 불가."*

### F. 손상 layout 복구 (★ ADR-0006 D10, 2분)

```bash
# 19. Server stop
./codebase/backend/target/release/gtmux stop --session demo

# 20. layout 일부러 손상
echo 'broken json {{{' > ~/.local/state/gtmux/demo.layout.json

# 21. 재기동 → WARN + sidecar 격리 + 빈 layout cold start
GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999

# 22. 디스크 상태 확인
ls -la ~/.local/state/gtmux/demo.layout.json*
# demo.layout.json.corrupt-<unix_ts>  ← 격리된 손상본
# (demo.layout.json 은 부재 → 빈 layout cold start)

# 23. 브라우저 새로고침 → 빈 캔버스 정상 응답 (fail-closed)
```

내레이션: *"fail-closed. 손상된 데이터로 부분 기동 0. 사용자가 sidecar 를 직접 검수·복원 가능."*

### G. Token rotation (1분)

```bash
# 24. 토큰 회전 (cloud 모드에서 의미가 큼; local 도 명시 호출 가능)
./codebase/backend/target/release/gtmux rotate-token --session demo
# "gtmux demo token rotated."
# "  New token:    <NEW_TOK>"
# "  Open URL:     http://127.0.0.1:9999/auth/bootstrap?token=<NEW_TOK>"

# 25. 브라우저 탭은 close code 4001 (Policy Violation) — 재인증 필요
```

### H. Graceful teardown (1분, 시연 마감)

```bash
# 26. teardown — 4단계 cleanup
./codebase/backend/target/release/gtmux teardown --session demo --force
# Server: stopped via SIGTERM
# Files: pidfile / token / layout / config → 전부 removed
# exit 0

# 27. 확인
ls ~/.local/state/gtmux/ 2>&1 | grep demo  # 없음
lsof -nP -iTCP:9999 | head -3              # 없음
```

내레이션: *"ADR-0014 D7. graceful exit 6 = Server quit = Session quit (Server : Session : Port 1:1:1)."*

## 5. 시연 직전 안티패턴

| 안티패턴 | 이유 | 대안 |
|---|---|---|
| 외부 tmux 안에서 시연 호스트 실행 | ADR-0014 D10 amend → 모든 step A 부터 차단 | 호스트 셸에서 `unset TMUX` 또는 외부 tmux 종료 후 시연 |
| 화면 공유 시 token URL 캡처 후 회전 안 함 | 데모 후에도 token 유효 → 화면 캡처 채널 통과 시 위험 | 시연 종료 시 `gtmux rotate-token --session demo` 또는 `teardown` |
| `gtmux start` 를 같은 session 으로 두 번 | exit 4 + 친절한 메시지 → 시연 흐름 깨짐 | 다른 session 명 또는 stop 후 재시작 |
| 라이브 코딩 / 코드 수정 시연 | S7-FE-SHUTDOWN dispatch 의 정식 경로 | 시연 후 별도 task 로 진행 |
| `git push` 시도 | (handoff 0027 §8 carry-forward) credential 영역 | 시연 후 별도 sync |
| Playwright / 자동 시각 비교 | 미구현 (사용자 환경) | 수동 브라우저 시연 |

## 6. 시연 후 사후 처리

- [ ] `gtmux teardown --session demo --force` — state 파일 4건 모두 제거
- [ ] 브라우저 sessionStorage `gtmux_token` 확인 (탭 종료 시 자동)
- [ ] 시연 도중 발견한 새 부류 결함 → `docs/reports/0029-...md` 로 별도 보고서
- [ ] 본 문서는 S7-FE-SHUTDOWN 완료 시점에 supersede

## 7. 부록 — 현 시점 서버 상태 (시연 진입점)

| 항목 | 값 |
|---|---|
| 바이너리 | `codebase/backend/target/release/gtmux` (release profile) |
| SPA dist | `codebase/frontend/dist/` (latest `npm run build`) |
| Bind | `127.0.0.1:9999` |
| Session | `demo` |
| Mode | Local (token 매 start 재발급, HttpOnly cookie + sessionStorage 병행) |
| State dir | `${XDG_STATE_HOME:-~/.local/state}/gtmux/` |
| Layout file | `~/.local/state/gtmux/demo.layout.json` (현재: 부재 = cold start 직후) |

확인 명령:
```bash
curl -s http://127.0.0.1:9999/healthz                                          # {"ok":true}
curl -s -H "Authorization: Bearer <TOK>" http://127.0.0.1:9999/api/layout      # 빈 layout + ETag
./codebase/backend/target/release/gtmux status --session demo                  # running 표시
```

## 변경 이력

- 2026-05-15: 초안 — S7-PERSISTENCE-MINIMAL 완료 직후, S7-FE-SHUTDOWN 진입 전 시점의 시연 prep.
