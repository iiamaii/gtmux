# gtmux Quickstart — 설치 · 설정 · 인증 · 첫 session

> [English](QUICKSTART.md) · **한국어**
>
> Localhost (또는 개인 cloud host) 에서 gtmux server 를 띄우고 브라우저로
> canvas 에 접속하기까지의 1회용 가이드. 설치, 두 가지 실행 모드
> (Local / Cloud), config 작성, auth handshake, UI 안에서 첫 session
> 만들기까지 한 번에 다룬다.

---

## ⚠ 보안 전제

- gtmux 본체는 **TLS 종단을 직접 처리하지 않는다**. `bind = 0.0.0.0` 으로
  인터넷에 직접 노출하면 토큰·쿠키가 **평문 HTTP** 로 전송된다. 본 문서의
  절차는 신뢰 네트워크 (LAN / VPN / Tailscale) 만 가정한다. 인터넷 직접
  노출이 필요하면 HTTPS reverse proxy 뒤에 둔다.
- 단일 사용자 — 본인 1명이 본인 인스턴스에 접속.
- Localhost 전용 (`bind = "127.0.0.1"`, 기본값) 은 cloud 설정도 TLS 도
  필요 없다.

---

## 0) 사전 요구사항

| 항목 | 버전 | 비고 |
|---|---|---|
| Rust | 1.85 | `backend/rust-toolchain.toml` 로 pin. `curl https://sh.rustup.rs -sSf \| sh` 한 번이면 첫 `cargo` 호출 시 자동 설치 |
| Node.js | ≥ 20 (22 LTS 권장) | Vite 7 floor. `brew install node` / `nvm install --lts` |
| OS | macOS / Linux (x86_64 · aarch64) | Windows 미검증 |

`cargo` 와 `npm` 만 `$PATH` 에 있으면 된다. Vite / Svelte / xterm.js
같은 글로벌 패키지는 `npm install` 로만 받는다.

---

## 1) 설치

### 1.1 Source build (local · cloud 동일)

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

# OpenAPI → TypeScript 타입 (frontend build 전제).
make codegen

# Frontend 의존성.
( cd frontend && npm install --no-audit --no-fund )

# Release build: Rust workspace + 프로덕션 frontend 번들.
( cd backend  && cargo build --workspace --release )
( cd frontend && npm run build )

# 산출물:
#   backend/target/release/gtmux   (binary)
#   frontend/dist/                  (binary 가 서빙할 정적 번들)
```

### 1.2 (선택) Binary 를 system-wide 설치

```bash
sudo install -m 755 backend/target/release/gtmux /usr/local/bin/gtmux
```

생략 시 full path 로 호출 (`./backend/target/release/gtmux …`). 본
문서 이후로는 짧은 형태 `gtmux` 만 사용한다.

> `gtmux` 는 root (`EUID == 0`) 실행을 거부한다. 일반 유저로 실행할 것.

---

## 2) 실행 모드 A — Local (`bind = 127.0.0.1`, config 없음)

이 머신에서만 접속한다면 config 파일을 만들 필요가 없다. 빌트인 디폴트
(`bind = "127.0.0.1"`, `port = 9001`, `mode = token`) 가 이미 Local
모드다.

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start --session local
```

기동 시 stdout 에 1회 banner 가 출력된다.

```text
gtmux local ready
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/local.token (0600)
  Backend:      PtyBackend (supervisor pid=<n>)
```

Local 모드의 특징:

| | |
|---|---|
| `bind` | `127.0.0.1` |
| Mode | Local |
| Auth | token bootstrap |
| TLS | 불필요 |
| `[cloud]` 블록 | 불필요 |
| 외부 기기 접속 | 불가 |

`Ctrl-C` 하나로 supervisor 가 graceful shutdown (모든 child shell reap).

다른 터미널에서 종료:

```bash
gtmux stop --session local
```

---

## 3) 실행 모드 B — Cloud (`bind = 0.0.0.0`, config 파일)

같은 신뢰 네트워크의 다른 기기 (노트북, 폰, 또 다른 서버) 에서 접속할
때 쓴다.

### 3.1 Config 파일 작성

`codebase/config.sample.toml` 을 그대로 복사한 뒤 두 군데의
`PUBLIC_IP` 자리만 본인 서버 IP 또는 도메인으로 교체 (포트를
9001 외로 바꾸면 그 값도 함께 갱신).

```bash
mkdir -p ~/.config/gtmux
mkdir -p ~/.local/state/gtmux && chmod 700 ~/.local/state/gtmux

cp codebase/config.sample.toml ~/.config/gtmux/prod.config.toml
$EDITOR    ~/.config/gtmux/prod.config.toml
```

핵심 키:

| 키 | 값 | 의미 |
|---|---|---|
| `[server].session` | `"prod"` | `--session` 인자와 일치 |
| `[server].port` | `9001` | listen port |
| `[server].bind` | `"0.0.0.0"` | 모든 인터페이스 listen → cloud mode 자동 발동 |
| `[security].cors_origins` | `["http://PUBLIC_IP:9001"]` | 정확 일치, wildcard 금지 |
| `[security].host_allowlist` | `["PUBLIC_IP:9001"]` | DNS rebind 방어 |
| `[auth].mode` | `"token"` | `gtmux start` 가 매번 새 bootstrap URL 발행 (기본 검증 경로) |
| `[cloud].tls_required` | `false` | 신뢰 네트워크 평문 HTTP 검증 경로. HTTPS reverse proxy 운영 시 `true` |
| `[assets].max_size_bytes` | `104857600` | 업로드 asset 1개당 최대 크기 (이 sample 은 100 MiB) |

모든 키의 인라인 설명은 sample 파일에 그대로 들어있다.

### 3.2 서버 기동

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start \
  --session prod \
  --config ~/.config/gtmux/prod.config.toml
```

방화벽이 있다면 `9001/tcp` 를 미리 연다 (`sudo ufw allow 9001/tcp`
등). Banner 의 `Open URL` 은 여전히 `http://127.0.0.1:9001/...` 형식이라
외부 브라우저에서는 host 부분을 `PUBLIC_IP` 로 교체해서 연다:

```text
http://PUBLIC_IP:9001/auth/bootstrap?token=<hex>
```

---

## 4) Auth — 쿠키 발급

두 모드 모두 흐름은 같다.

1. Banner 의 **bootstrap URL** 을 브라우저로 한 번 연다.
2. 서버가 토큰을 검증하고 `HttpOnly` + `SameSite=Strict` session
   쿠키 (7일 rolling renewal) 를 발급하면서 URL 에서 토큰을 지운다.
3. 이후로는 path-only URL 을 북마크한다 — Local 은
   `http://127.0.0.1:9001/`, Cloud 는 `http://PUBLIC_IP:9001/`. 쿠키가
   만료되거나 logout 하기 전까지 그대로 로그인 상태.

### Password 모드로 전환 (선택)

기본값인 token 모드는 `gtmux start` 마다 새 bootstrap URL 을 발행한다.
대신 안정적인 비밀번호 로그인을 쓰고 싶다면:

```bash
gtmux set-password           # 2회 입력. Argon2id hash 저장
                              # → ~/.local/state/gtmux/password.argon2 (0600)
```

Config 의 `[auth].mode = "password"` 로 바꾼 뒤 재시작. 로그인 페이지가
토큰 자동 소비 대신 비밀번호 입력 폼으로 바뀐다 (5회 / 5분 throttle).

비밀번호 분실 시 회전:

```bash
gtmux reset-password
```

토큰 유출 시 회전 (cloud 모드 한정 — local 모드는 매 start 마다 자동
재발급):

```bash
gtmux rotate-token --session prod
```

---

## 5) UI 안에서 첫 session 만들기

§4 의 cookie 발급이 끝나면 브라우저가 canvas 페이지에 도달하면서 **Auth
dialog** 가 나타나 session 을 선택 또는 생성하라고 묻는다.

1. **[New session]** 클릭.
2. session 이름 입력 (영문·숫자·`-`·`_`).
3. 서버가 `${XDG_STATE_HOME:-~/.local/state}/gtmux/<name>.json` 에
   workspace 파일을 만들고, 빈 canvas 가 열린다.
4. 이후 toolbar 좌측 상단의 **Active session dropdown** 으로 같은
   서버 안의 session 들을 전환한다. Titlebar 의 kebab (`Session menu`)
   안에는 [New session] / [Session list] / [Import session] / [Export
   session] / [Rotate token] / [Settings] / [Shutdown] / [Logout] 가
   있다.

이제 canvas 에 Terminal panel 을 떨어뜨려 본다:

- Toolbar 의 **Terminal** tool 클릭 (단축키 **T**).
- Canvas 임의 위치를 클릭 — 새 PTY 가 spawn 되고 해당 위치에 Terminal
  panel 이 mount 된다.
- Shell 은 `$SHELL` 을 따른다 (macOS 기본 `/bin/zsh`, 대부분의 Linux
  는 `/bin/bash`). WebSocket 끊김에도 살아남고, panel 의 **×** 로 닫을
  때는 confirm modal 거쳐 shell 이 종료된다.

전체 기능 (toolbar 모든 tool, Group, layer tree, clipboard, shortcut)
설명은 [`USAGE.ko.md`](USAGE.ko.md) 에 있다.

---

## 6) Background / 장기 실행

`gtmux start` 는 기본적으로 foreground 프로세스다. 터미널을 닫아도
계속 띄워두는 방법 2가지.

### 6.1 `nohup` (가장 간단)

```bash
cd codebase
mkdir -p ~/.local/state/gtmux

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
nohup gtmux start --session local \
  > ~/.local/state/gtmux/local.log 2>&1 &

tail -f ~/.local/state/gtmux/local.log   # bootstrap URL 확인
```

Cloud 도 동일 — `--config ~/.config/gtmux/prod.config.toml` 만 추가.

### 6.2 systemd (장기 운영 권장)

장기 운영에는 auto-restart 와 journal 로그가 있는 user-level service 를
사용한다.

어떤 방식으로 띄웠든 종료는 항상 다음 명령을 쓴다:

```bash
gtmux stop --session <name>            # SIGTERM, 5초 대기
gtmux stop --session <name> --force    # 이어서 SIGKILL
```

이게 pidfile 을 존중하면서 모든 child shell 까지 reap 한다. Shell job
을 직접 kill 하면 orphan PTY 가 남는다.

---

## 7) Lifecycle 명령 reference

```bash
gtmux status                            # 알려진 session + liveness
gtmux status   --session prod           # 단일 session
gtmux stop     --session prod [--force] # graceful / 강제
gtmux teardown --session prod --force   # 5단계 청소
                                        # (token / layout / pidfile / socket / config)
                                        # 부분 보존: --keep-state / --keep-config
gtmux set-password / reset-password     # password 모드 credential
gtmux rotate-token --session prod       # cloud 모드 token 회전
```

전체 flag 는 `gtmux <subcommand> --help`.

---

## 8) Troubleshooting

| 증상 | 원인 | 조치 |
|---|---|---|
| `bind=... is cloud-mode but [cloud] section is missing` | cloud 모드인데 `[cloud]` 블록 누락 | Config 에 `[cloud]` 추가 (Quickstart 경로는 `tls_required = false`) |
| `[cloud].tls_cert and tls_key must be set when cloud.tls_required=true` | TLS 강제인데 cert marker 누락 | 평문 HTTP 검증이면 `tls_required = false`, 운영이면 cert/key 지정 |
| 브라우저에 `Forbidden` | `cors_origins` / `host_allowlist` 불일치 | scheme + host + port 가 모두 정확 일치해야 한다 |
| `/` 가 `{"error":"not_found"}` | `GTMUX_FRONTEND_DIST` 없이 기동 | env var 지정 또는 번들 설치 |
| `cannot find type Group / Panel in api.d.ts` | `make codegen` 누락 | 재실행 후 rebuild |
| `Address already in use (os error 48)` | port 충돌 | `gtmux status` → `gtmux stop --session <name>` |
| `pidfile exists but process is gone` | 이전 실행 crash | `gtmux teardown --session <name> --force --keep-state` |
| 재방문 시 `Forbidden` | 쿠키 만료 / 삭제 | 가장 최근 `gtmux start` 의 banner URL 다시 열기 |
| `gtmux` 가 기동 거부 | root (`EUID == 0`) 실행 | 일반 유저로 |

## 다음 단계

- [`USAGE.ko.md`](USAGE.ko.md) — main canvas 사용 설명서 (session
  관리, architecture, 모든 toolbar tool, Group 기능).
- [`README.ko.md`](README.ko.md) — project 개요와 문서 안내.
