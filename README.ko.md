# gtmux

> [English](README.md) · **한국어**

> Rust supervisor + Svelte 5 frontend 로 구성된 single-user 웹 캔버스
> 워크스페이스. PTY 기반 shell 들을 spawn 해서 각각을 무한 캔버스 위의
> draggable 패널로 펼치고, 모든 것을 한 프로세스 뒤 per-session token
> 으로 서빙한다.

---

## 디렉토리 구조

```
codebase/
  backend/    Rust workspace (axum 0.8 + tokio + tokio-tungstenite).
              ADR-0011 D10 기준 5 crate + 2 binary:
                crates/{ws-server, http-api, config, auth, pty-backend}
                bin/{gtmux-cli, gen-openapi}
  frontend/   Svelte 5 + Vite 7 + TypeScript app (ADR-0012).
              Codegen 진입점: codegen/run.sh.
  shared/     Backend ↔ frontend 기계 전용 handoff.
              생성된 openapi.yaml 보유. shared/README.md 참조.
  smoke/      통합 smoke 스크립트.
  Makefile    최상위 orchestrator (codegen / build / test / smoke / clean).
```

---

## 요구사항

| | 버전 | 비고 |
|---|---|---|
| Rust toolchain | **1.85** | `backend/rust-toolchain.toml` 로 pin — 첫 cargo 실행 시 rustup 이 자동 설치. |
| Node.js | **≥ 20** | Vite 7 floor. |
| npm | Node 동봉 | pnpm/yarn 도 OK — codegen 스크립트는 `npm run` 사용. |
| OS | macOS / Linux | x86_64 + aarch64 지원 (`rust-toolchain.toml` targets 참조). Windows 미검증. |

`cargo` 와 `npm` 만 `$PATH` 에 있으면 됨. 글로벌 Node 패키지 불필요 —
Vite + svelte-check 등은 `npm install` 로 받음.

---

## Installation (≈ 5분, 1회)

Fresh clone 에서:

```bash
# 1. Clone (본 디렉토리는 repo 의 codebase/ subtree).
git clone https://github.com/iiamaii/gtmux.git
cd gtmux

# 2. OpenAPI 스키마 + 대응 TS 타입 생성.
#    첫 실행 시 필수 — 안 하면 frontend 타입체크 실패.
make codegen

# 3. Frontend 의존성 설치 (Vite / Svelte / xterm …).
cd frontend && npm install && cd ..

# 4. Release 빌드: Rust workspace + 프로덕션 frontend 번들.
#    ./backend/target/release/gtmux + ./frontend/dist/ 생성.
make build
```

**(선택) Binary 를 system-wide 설치** — 아무 디렉토리에서 실행 가능:

```bash
sudo install -m 755 backend/target/release/gtmux /usr/local/bin/gtmux
```

생략 시 빌드 결과물 경로로 직접 실행
(`./backend/target/release/gtmux …`).

---

## Quickstart

원하는 session 이름으로 server 시작 (기본 port 9001, bind 127.0.0.1):

```bash
gtmux start --session demo
```

기동 시 stdout 에 1회용 URL 출력:

```
gtmux demo ready
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/demo.token (0600)
  Backend:      PtyBackend (ADR-0013, supervisor pid=<n>)
```

**Open URL** 을 한 번 열면 server 가 HttpOnly 쿠키를 발급하고 URL 에서
token 을 지움. 이후로는 path-only URL (`http://127.0.0.1:9001/`) 를
북마크 — 쿠키가 세션을 유지.

`Ctrl-C` 로 supervisor 가 graceful shutdown (모든 child shell reap).
재실행 `gtmux start --session demo` 면 같은 워크스페이스로 reattach.

---

## 개발 (hot reload)

두 프로세스 병렬:

```bash
# Terminal 1 — backend (debug 빌드)
cd codebase/backend
cargo run -p gtmux-cli -- start --session dev

# Terminal 2 — frontend Vite dev 서버 (API + WS 를 backend 로 proxy)
cd codebase/frontend
npm run dev
```

Vite 는 `http://localhost:5173/` 에서 서빙하고 `/api/*` + WS upgrade 를
backend (port 9001) 로 proxy. backend 가 출력한 banner URL 로 *한 번*
들어가서 쿠키 받은 뒤, 이후 `http://localhost:5173/` 로 전환. 또는
`GTMUX_FRONTEND_DIST` 환경변수로 backend 가 빌드된 번들을 직접 서빙
하게 설정.

- `npm run check` — `svelte-check` 만 (빌드 없이 타입 오류 확인).
- `cargo test --workspace` — backend 테스트.

---

## CLI 레퍼런스

```
gtmux start    --session <name> [--port N] [--workspace PATH] [--config PATH]
gtmux stop     --session <name> [--force]
gtmux teardown --session <name> [--force] [--keep-state] [--keep-config]
gtmux status   [--session <name>]
gtmux rotate-token   --session <name>
gtmux set-password
gtmux reset-password
```

| 명령 | 효과 |
|---|---|
| `start` | Supervisor 부팅 + token 발급 + HTTP/WS listener 오픈 + banner 출력. `Ctrl-C` / `SIGTERM` 까지 foreground 점유. |
| `stop` | Pidfile 의 프로세스에 `SIGTERM`, 5 초 대기, `--force` 시 `SIGKILL` 로 escalate. Workspace + token 보존. |
| `teardown` | 5-step cleanup (socket / token / layout / pidfile / config). `--keep-state` / `--keep-config` 로 부분 보존. |
| `status` | `$XDG_STATE_HOME/gtmux/` 에 알려진 session 들 + daemon liveness. |
| `rotate-token` | Cloud 모드 전용 — local 모드는 매 `start` 마다 재발급. |
| `set-password` / `reset-password` | Argon2id PHC hash (ADR-0020 password 모드 auth). |

각 subcommand 의 정확한 flag 는 `gtmux <subcommand> --help`.

---

## 설정

우선순위: **CLI flag → `GTMUX_*` env → TOML → 빌트인 디폴트**.

### 빌트인 디폴트

```toml
[server]
session = "<session>"      # --session 으로 지정
port    = 9001
bind    = "127.0.0.1"      # loopback ⇒ Local 모드, 그 외 ⇒ Cloud 모드

[runtime]
ring_buffer_size_kb     = 128
layout_debounce_ms      = 300
panel_state_debounce_ms = 300
log_level               = "info"      # trace|debug|info|warn|error|off
log_format              = "auto"      # auto (tty→text, pipe→json) | text | json

[security]
cors_origins   = []        # 비어있으면 startup 에서 bind 호스트로 합성
host_allowlist = []
```

### 파일 위치 (XDG)

| 용도 | 경로 |
|---|---|
| Per-session config | `$XDG_CONFIG_HOME/gtmux/<session>.config.toml` |
| Token | `$XDG_STATE_HOME/gtmux/<session>.token` (mode 0600) |
| Pidfile | `$XDG_STATE_HOME/gtmux/<session>.pid` |
| Password hash | `$XDG_STATE_HOME/gtmux/password.argon2` |
| Workspace | `$XDG_DATA_HOME/gtmux/workspace/` (또는 `--workspace PATH`) |

`$XDG_CONFIG_HOME` 기본 `~/.config`, `$XDG_STATE_HOME` 기본
`~/.local/state`, `$XDG_DATA_HOME` 기본 `~/.local/share`.

### 환경변수

TOML 키는 `GTMUX_<SECTION>__<KEY>` 로 매핑 (이중 underscore 가 section
구분자):

```bash
export GTMUX_SERVER__PORT=9100
export GTMUX_RUNTIME__LOG_LEVEL=debug
export GTMUX_FRONTEND_DIST=/path/to/built/frontend/dist
```

---

## Make 타겟

```
make help       타겟 목록.
make codegen    Rust utoipa → shared/openapi.yaml → TS 타입.
make build      cargo build --workspace → vite build.
make test       cargo test --workspace → svelte-check.
make smoke      통합 smoke (C4 land 전까지는 placeholder).
make clean      target/, node_modules/, dist/, codegen 출력 모두 제거.
```

Fresh clone 에서는 `make codegen` 을 `make build` 전에 실행해야
`frontend/src/lib/types/api.d.ts` 가 존재.

---

## Codegen 경로

단방향 (ADR-0011 D5 + ADR-0012 D7):

```
Rust struct + utoipa derive
  → cargo run -p gen-openapi
  → shared/openapi.yaml            (커밋됨)
  → openapi-typescript
  → frontend/src/lib/types/api.d.ts  (커밋됨)
```

양 끝단 모두 커밋. CI 의 `codegen-verify` job
(`.github/workflows/ci.yml`) 이 소스 변경 후 재생성을 잊은 PR 을 reject.

---

## Troubleshooting

| 증상 | 원인 | 해결 |
|---|---|---|
| `cannot find type Group / Panel in api.d.ts` | Codegen 전에 frontend build | `make codegen` 후 재빌드 |
| `Address already in use (os error 48)` | 같은 port 의 다른 `gtmux` | `gtmux status` 로 확인 / `gtmux stop --session <name>` |
| `pidfile exists but process is gone` | 이전 실행 crash | `gtmux teardown --session <name> --force --keep-state` |
| 북마크 후 브라우저에서 `Forbidden` | 쿠키 만료 / 삭제 | 최근 `gtmux start` 의 banner URL 다시 열기 |
| `make codegen` 이 `openapi-typescript` 에서 실패 | `npm install` 누락 | `cd frontend && npm install` |

---

## 프로젝트 상태

활발 개발 — multi-session pivot (plan-0007) 의 Stage 5+ 진행, ADR-0019
아래 Session attach-recovery + delete-UI 레이어 land 중. 전체 그림은
`../docs/` 참조:

- `../docs/sketch.md` — 설계 spec (한국어), 범위의 source of truth
- `../docs/adr/` — accepted 아키텍처 결정
- `../docs/plans/` — 구현 계획 (가장 높은 번호 = 현재 활성)
- `../docs/reports/` — 조사 + session handover

---

## 라이선스

**MIT OR Apache-2.0** 듀얼 라이선스 (Cargo workspace metadata 와 정합).
하위 사용처에 맞는 것을 선택.
