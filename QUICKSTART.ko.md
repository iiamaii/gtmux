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
(`bind = "127.0.0.1"`, `port = 9001`) 가 이미 Local 모드다. port·로그를
조정하고 싶을 때만 쓰는 템플릿이 `codebase/config.local.sample.toml` 에
준비돼 있다.

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start --name local
```

기동 시 stdout 에 1회 banner 가 출력된다.

```text
gtmux local ready (instance)
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/local.token (0600)
  Workspace(A): /Users/you
  Store(C):     ~/.local/state/gtmux/store/local
  Backend:      PtyBackend (ADR-0013, supervisor pid=<n>)
```

Local 모드의 특징:

| | |
|---|---|
| `bind` | `127.0.0.1` |
| Mode | Local |
| Auth | token bootstrap (선택적 password 추가 — §4 참조) |
| TLS | 불필요 |
| `[cloud]` 블록 | 불필요 |
| 외부 기기 접속 | 불가 |

`Ctrl-C` 하나로 supervisor 가 graceful shutdown (모든 child shell reap).

다른 터미널에서 종료:

```bash
gtmux stop --name local
```

---

## 3) 실행 모드 B — Cloud (`bind = 0.0.0.0`, config 파일)

같은 신뢰 네트워크의 다른 기기 (노트북, 폰, 또 다른 서버) 에서 접속할
때 쓴다.

### 3.1 Config 파일 작성

`codebase/config.cloud.sample.toml` 을 그대로 복사한 뒤 `PUBLIC_HOST`
자리만 본인 서버 IP 또는 도메인으로 교체 (포트를 9001 외로 바꾸면 그
값도 함께 갱신).

```bash
mkdir -p ~/.config/gtmux
mkdir -p ~/.local/state/gtmux && chmod 700 ~/.local/state/gtmux

cp codebase/config.cloud.sample.toml ~/.config/gtmux/prod.config.toml
$EDITOR    ~/.config/gtmux/prod.config.toml
```

핵심 키:

| 키 | 값 | 의미 |
|---|---|---|
| `[server].session` | `"prod"` | Server Instance 이름 — `--name` 인자와 일치해야 함 |
| `[server].port` | `9001` | listen port |
| `[server].bind` | `"0.0.0.0"` | 모든 인터페이스 listen → cloud mode 자동 발동 |
| `[security].cors_origins` | `["https://PUBLIC_HOST"]` | 정확 일치, wildcard 금지 |
| `[security].host_allowlist` | `["PUBLIC_HOST"]` | DNS rebind 방어 |
| `[cloud].rate_limit_auth_failures_per_minute` | `10` | `[cloud]` 필수 키 — 기본값 없음. 분당 인증 실패 허용 횟수 |
| `[cloud].tls_required` | `true` | 기본값. gtmux 가 TLS 직접 종단 (`tls_cert`/`tls_key` 지정). reverse proxy 가 HTTPS 를 종단하는 신뢰 네트워크 검증 경로에서만 `false` |
| `[cloud].trusted_proxy_ips` | `["10.0.0.2/32"]` | `X-Forwarded-For` 를 신뢰할 reverse proxy IP/CIDR. per-client rate limit 용 (proxy 형태). 아래 주석 참조 |
| `[assets].max_size_bytes` | `52428800` | 업로드 asset 1개당 최대 크기 (이 sample 은 50 MiB) |

> **Server Instance vs. session.** **Server Instance** 는 `--name` (TOML
> 에서는 `[server].session` — 키 이름은 그대로지만 `--name` 과 일치해야
> 함) 으로 식별되는 실행 중인 gtmux server 1개다. **session** 은 UI 안에서
> 전환하는, 저장된 workspace/layout 레코드다. 둘은 서로 다른 개념 — 하나의
> Server Instance 가 여러 session 을 담는다.

모든 키의 인라인 설명은 sample 파일에 그대로 들어있다.

> **Trusted proxy / `X-Forwarded-For`.** gtmux 를 reverse proxy 뒤에 둘
> 땐 `[cloud].trusted_proxy_ips` 에 proxy 의 IP/CIDR 를 명시해야 auth
> rate limit 이 실제 client IP 별로 동작한다. gtmux 는 요청의 socket peer
> IP 가 이 목록과 매치될 때**만** `X-Forwarded-For` 를 신뢰한다 (위조 XFF
> 는 무시). 미설정/빈 목록이면 XFF 가 전부 무시되고 proxy 뒤 모든 client
> 가 **하나의** rate-limit 버킷을 공유한다 (한 사람의 로그인 실패가 전원을
> throttle). 이 경우 boot 시 stderr 경고가 뜨며, `trusted_proxy_ips_required
> = false` 로 끌 수 있다. `X-Forwarded-For` 만 본다 (`-Proto`/`-Host` 미사용).
> Local 모드는 무관. 상세: `docs/deploy.md` §3.6.3.

### 3.2 서버 기동

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start \
  --name prod \
  --config ~/.config/gtmux/prod.config.toml
```

방화벽이 있다면 `9001/tcp` 를 미리 연다 (`sudo ufw allow 9001/tcp`
등). Banner 의 `Open URL` 은 여전히 `http://127.0.0.1:9001/...` 형식이라
외부 브라우저에서는 host 부분을 `PUBLIC_HOST` 로 교체해서 연다:

```text
http://PUBLIC_HOST:9001/auth/bootstrap?token=<hex>
```

---

## 4) Auth — 쿠키 발급

두 모드 모두 흐름은 같다.

1. Banner 의 **bootstrap URL** (magic link) 을 브라우저로 한 번 연다.
2. 서버가 토큰을 검증하고 `gtmux_auth` 쿠키 (`HttpOnly` +
   `SameSite=Strict` + `Path=/`, Cloud/HTTPS 에선 `Secure`, 7일 rolling
   renewal) 를 발급한다.
3. 이후로는 path-only URL 을 북마크한다 — Local 은
   `http://127.0.0.1:9001/`, Cloud 는 `http://PUBLIC_HOST:9001/`. 쿠키가
   만료되거나 sign out 하기 전까지 그대로 로그인 상태.

### Password 추가 (선택)

**token** 은 항상 유효하다 — 위 magic link 는 매 `gtmux start` 마다
동작한다. 여기에 **password** 를 **추가로** 설정할 수 있고, 둘 다 동시에
유효하므로 아무거나로 로그인하면 된다. **선택할 auth "mode" 는 없다** —
`[auth].mode` 는 deprecated·무시되며, password 추가에 **config 편집도
재시작도 필요 없다**.

password 설정은 둘 중 하나:

```bash
gtmux set-password           # 2회 입력. Argon2id hash 저장
                              # → ~/.local/state/gtmux/password.argon2 (0600)
                              # 다음 `gtmux start` 부터 활성
```

또는 UI **Settings → Auth** — 실행 중 즉시 활성, 재시작 불요
(password 로그인 폼은 5회 / 5분 throttle).

**Password 제거** (token-only 로 복귀 — 비밀번호 분실 시 복구 경로):

```bash
gtmux reset-password         # hash 파일 삭제 → token-only 로그인
```

또는 UI **Settings → Auth → "Delete password"** (token 또는 현재 password
로 재인증 후 → token-only).

**유출 token 회전.** UI **Settings → Auth → "Rotate token"** 은 서버
token 을 재발급하고 **모든** session 을 sign out 시키며 (활성 탭 전부
연결 해제) 새 로그인 링크를 보여준다. 먼저 현재 credential 재입력을
요구한다. password 는 그대로 유지된다. CLI 등가 (정지된 서버용):

```bash
gtmux rotate-token --name prod   # offline 재발급. local 모드는 어차피
                                 # 매 start 마다 재발급
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
nohup gtmux start --name local \
  > ~/.local/state/gtmux/local.log 2>&1 &

tail -f ~/.local/state/gtmux/local.log   # bootstrap URL 확인
```

Cloud 도 동일 — `--config ~/.config/gtmux/prod.config.toml` 만 추가.

### 6.2 systemd (장기 운영 권장)

장기 운영에는 auto-restart 와 journal 로그가 있는 user-level service 를
사용한다.

어떤 방식으로 띄웠든 종료는 항상 다음 명령을 쓴다:

```bash
gtmux stop --name <name>            # SIGTERM, 5초 대기
gtmux stop --name <name> --force    # 이어서 SIGKILL
```

이게 pidfile 을 존중하면서 모든 child shell 까지 reap 한다. Shell job
을 직접 kill 하면 orphan PTY 가 남는다.

---

## 7) Lifecycle 명령 reference

```bash
gtmux status                            # 알려진 instance + liveness
gtmux status   --name prod              # 단일 Server Instance
gtmux stop     --name prod [--force]    # graceful / 강제
gtmux teardown --name prod --force      # 5단계 청소
                                        # (socket / token / layout / pidfile / config)
                                        # 부분 보존: --keep-state / --keep-config
gtmux set-password / reset-password     # 선택적 password credential 추가 / 제거
gtmux rotate-token --name prod          # 서버 token 재발급 (cloud / offline)
```

(`--session` 은 `--name` 의 deprecated alias 로 여전히 동작하나 deprecation
경고를 출력한다.)

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
| `Address already in use (os error 48)` | port 충돌 | `gtmux status` → `gtmux stop --name <name>` |
| `pidfile exists but process is gone` | 이전 실행 crash | `gtmux teardown --name <name> --force --keep-state` |
| 재방문 시 `Forbidden` | 쿠키 만료 / 삭제 | 가장 최근 `gtmux start` 의 banner URL 다시 열기 |
| `gtmux` 가 기동 거부 | root (`EUID == 0`) 실행 | 일반 유저로 |

## 다음 단계

- [`USAGE.ko.md`](USAGE.ko.md) — main canvas 사용 설명서 (session
  관리, architecture, 모든 toolbar tool, Group 기능).
- [`README.ko.md`](README.ko.md) — project 개요와 문서 안내.
