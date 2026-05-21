# gtmux Deploy Guide — Local + Cloud

> 본 문서는 *현 시점 main* (`be79482b`) 에서 gtmux 를 **로컬 머신**과
> **클라우드 호스트** 양쪽으로 배포·실행하는 절차를 정본화한다. 코드
> 사실은 `codebase/`, 보안·설정 결정은 ADR-0003·ADR-0007·ADR-0019·
> ADR-0020 + `docs/ssot/security-defaults.md` 가 정본이며, 본 가이드는
> 그 결정을 *operator 시점* 으로 묶어 재구성한다.
>
> 동일 내용을 영어로 요약한 quickstart 는 `codebase/README.md` 에 있다.
> 본 문서는 KO + cloud 운영 디테일을 포함한다.

---

## 0. TL;DR

```bash
# 1) 의존성 (1회).
brew install rustup node               # macOS 예시. Linux 는 rustup.rs + apt/dnf.
rustup default 1.85                    # backend/rust-toolchain.toml 와 정합.

# 2) build.
cd codebase
make codegen                           # OpenAPI + TS 타입 생성 (frontend 빌드 전제)
cd frontend && npm install && cd ..
make build                             # cargo build --workspace + vite build

# 3) local 실행.
./backend/target/release/gtmux start --session demo
# stdout 의 "Open URL" 한 번 클릭 → cookie 발급 후 path 만 북마크.
```

Cloud 는 §3 참조 — `bind` 를 loopback 밖으로 두면 자동 cloud 정책 (rate
limit, HSTS) 이 발동되고, gtmux 자체는 TLS 종단을 하지 않으므로
**Caddy/nginx + ACME** reverse proxy 가 필수다.

---

## 1. 사전 요구사항

| 항목 | 버전 | 비고 |
|---|---|---|
| Rust toolchain | **1.85** | `codebase/backend/rust-toolchain.toml` 가 pin. rustup 이 첫 `cargo` 호출 시 자동 설치. |
| Node.js | **≥ 20** | Vite 7 의 floor (`23.x` 까지 동작 확인, 22 LTS 권장). |
| npm | Node 동봉 | codegen 스크립트가 `npm run` 사용. pnpm/yarn 으로 대체 가능하나 미검증. |
| OS | macOS / Linux | `rust-toolchain.toml` targets: aarch64/x86_64 darwin·linux-gnu. Windows 미검증. |
| tmux | **불필요** | ADR-0013 채택 이후 PTY 직접 spawn — tmux 바이너리 의존 없음. |
| 외부 reverse proxy | cloud 한정 | Caddy 또는 nginx + ACME (Let's Encrypt 등). MVP 의 gtmux 는 자체 TLS 종단 미지원. |

`cargo` 와 `npm` 만 `$PATH` 에 있으면 된다. 글로벌 Node 패키지는 필요
없다.

---

## 2. 로컬 (single host) 실행

### 2.1 1회성 setup

Repo root 기준 `codebase/` 하위로 이동해 codegen → frontend install →
build 순서로 진행한다. **codegen 을 frontend build 보다 먼저 돌려야**
`frontend/src/lib/types/api.d.ts` 가 생성되며, 빠지면 `svelte-check` 가
실패한다.

```bash
cd codebase

# A. OpenAPI 스키마 + TypeScript 타입 생성 (Rust utoipa → openapi.yaml → api.d.ts).
make codegen

# B. Frontend 의존성 (Vite·Svelte·xterm·…).
cd frontend && npm install --no-audit --no-fund && cd ..

# C. Workspace 전체 release 빌드.
make build
# 산출물: backend/target/release/gtmux (≈ 3.9 MB) + frontend/dist/.
```

`make` 가 없는 환경은 raw 명령으로 대체할 수 있다:

```bash
cd backend && cargo build --workspace --release && cd ..
cd frontend && npm run build && cd ..
```

> **fresh clone 의 함정** — codegen 을 빼면 `cannot find type Group /
> Panel in api.d.ts` 로 frontend 빌드가 깨진다. README §Troubleshooting
> 도 동일 증상을 다룬다.

### 2.2 실행 (foreground supervisor)

```bash
./backend/target/release/gtmux start --session demo
```

`Ctrl-C` / `SIGTERM` 으로 종료. 동일 `--session` 이름으로 다시 띄우면
저장된 layout 으로 재진입한다.

#### 첫 진입 흐름

```text
gtmux demo ready
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/demo.token (0600)
  Backend:      PtyBackend (ADR-0013, supervisor pid=<n>)
```

1. **Open URL** 을 한 번 연다 → BE 가 token 검증 후 `gtmux_auth` cookie
   (HttpOnly · SameSite=Strict) 발급 → `/auth?t=<...>` 로 redirect →
   AuthPage 가 자동 login.
2. 이후엔 **path 만 북마크** (`http://127.0.0.1:9001/`). 토큰은 URL 에서
   사라지고 cookie 가 lifecycle (default 7d rolling) 을 잡는다.
3. Cookie 가 만료/소거되면 마지막 `gtmux start` 콘솔의 banner URL 을 다시
   열거나 (`token` mode) 또는 `/auth` 에서 password 입력 (`password`
   mode) 로 재진입.

### 2.3 개발 모드 (hot reload)

`vite dev` 가 `/api/*` + WS 를 backend 로 proxy 한다.

```bash
# 터미널 1 — BE (debug build, ADR-0013 supervisor)
cd codebase/backend
cargo run -p gtmux-cli -- start --session dev

# 터미널 2 — FE (Vite 5173)
cd codebase/frontend
npm run dev
```

BE 콘솔의 banner URL (token 포함) 을 한 번 연 뒤, 같은 origin 의
`http://localhost:5173/` 로 전환한다 (cookie 가 동일 host 면 통과). 또는
`GTMUX_FRONTEND_DIST=$PWD/dist npm run build && cargo run …` 으로 BE 가
바로 SPA 를 서빙하게 할 수도 있다.

검증 명령:

```bash
cd codebase/backend  && cargo test --workspace
cd codebase/frontend && npm run check      # svelte-check (타입만 — 빌드 안 함)
```

### 2.4 LAN 외부에서도 접근하려면 *명시적으로* cloud 로 전환

`bind` 가 loopback 인 동안엔 `127.0.0.1` 외부에서 접근 불가다. 같은 LAN
의 다른 호스트나 외부에서 열고 싶으면 §3 cloud 절차로 전환해야 한다
(ADR-0003 D22 — `bind` 가 loopback 밖이면 자동 cloud 정책 활성).

### 2.5 종료 / 청소

```bash
# 정상 종료
gtmux stop --session demo

# 정상 종료 안 되는 경우 escalation (SIGTERM → 5s 대기 → SIGKILL)
gtmux stop --session demo --force

# 5-step 청소 (socket / token / layout / pidfile / config)
gtmux teardown --session demo --force
#   --keep-state   토큰·layout 보존 (재진입 의도)
#   --keep-config  TOML 보존
```

기동 중 / 종료 상태 확인:

```bash
gtmux status                       # 모든 session 의 daemon 살아있음 여부
gtmux status --session demo        # 단일 행 표 + 바인딩 정보
```

---

## 3. Cloud (외부 노출) 배포

### 3.1 위협 모델 요약 (sketch §13.1, ADR-0003)

Cloud 모드는 **본인 한 명이 인터넷 너머에서 본인의 인스턴스에 접속**하는
시나리오를 가정한다. 멀티 사용자/공유는 범위 밖. 그래도 다음은 자동
강제된다:

- TLS·HSTS 강제, HTTP 부팅 시 stderr 경고 + 명시 opt-in 없으면 거부
- `Authorization: Bearer` + `Origin/Host` allowlist + `Sec-Fetch-Site:
  same-origin` 3축 fail-closed
- 인증 실패 rate limit (Password mode: 5 시도 / 5분 / IP)
- HSTS 등 strict 보안 헤더 자동 부착

### 3.2 권장 토폴로지

```
Internet
   ↓ :443 (TLS)
[Caddy or nginx] --- ACME 자동 갱신
   ↓ :9001 (HTTP loopback or unix socket)
[gtmux start --session prod]   foreground 또는 systemd 하 supervisor
   ↓ PTY
[shell processes]
```

gtmux 본체는 **TLS 종단 미지원** — 항상 reverse proxy 뒤에 둔다.

### 3.3 호스트에서의 sequence

```bash
# 0. SSH 로 호스트 접속, 사용자 본인 계정으로 작업 (root 금지 — ADR-0003 R(rej)6).
ssh you@gtmux.example.com

# 1. rustup + node setup (위 §1 과 동일).
curl https://sh.rustup.rs -sSf | sh
nvm install --lts                       # 또는 호스트 패키지 매니저로 20+

# 2. 코드 가져오기 + 빌드.
git clone https://github.com/iiamaii/gtmux.git ~/gtmux
cd ~/gtmux/codebase
make codegen
( cd frontend && npm install --no-audit --no-fund )
make build

# 3. /usr/local/bin 으로 install.
sudo install -m 755 backend/target/release/gtmux /usr/local/bin/gtmux

# 4. config 디렉터리 + 파일 준비.
mkdir -p ~/.config/gtmux ~/.local/state/gtmux ~/.local/share/gtmux
```

### 3.4 Cloud 모드 config 예시

`~/.config/gtmux/prod.config.toml`:

```toml
schema_version = 1

[server]
session = "prod"
port    = 9001
# bind 를 0.0.0.0 또는 호스트의 사설/공인 IP 로 두면 mode = cloud 자동.
# 권장: 127.0.0.1 으로 두고 reverse proxy 만 외부로 노출.
bind    = "127.0.0.1"

[runtime]
log_level  = "info"
log_format = "json"           # systemd journal 친화

[security]
# Reverse proxy 가 종단할 public host 명시 — cloud 모드 필수.
cors_origins   = ["https://gtmux.example.com"]
host_allowlist = ["gtmux.example.com"]

[auth]
mode                  = "password"       # 원격은 password 권장 (CLI 자동화 불필요 시)
cookie_max_age_days   = 7                # 1–30
rate_limit_per_5min   = 5

# [cloud] 섹션은 reverse proxy 가 TLS 를 맡으므로 비워둔다.
# gtmux 본체가 직접 TLS 종단할 일이 없으므로 tls_cert / tls_key 도 비워둠.
```

> **bind = 127.0.0.1 + cloud cors_origins = 외부 origin** 의 조합이
> 추천이다. `bind` 자체는 loopback (`mode = local`) 이지만 외부 origin
> 을 명시함으로써 reverse proxy 가 종단해 들어오는 https origin 을
> 허용한다. 진짜로 `0.0.0.0` 바인딩이 필요한 컨테이너/Pod 환경이면
> §3.7 참조.

### 3.5 Password 설정 (one-time)

`auth.mode = "password"` 면 부팅 전에 hash 가 디스크에 있어야 한다:

```bash
gtmux set-password           # TTY 에서 2회 입력 → Argon2id PHC hash 저장
# 저장 위치: ~/.local/state/gtmux/password.argon2 (0600)

# 잊은 경우 (file system 접근 가능자만):
gtmux reset-password
```

`auth.mode = "token"` 이면 `gtmux start` 가 매번 새 token 을 발행한다
(local 과 동일). Cloud 에서 token 모드를 쓸 때는 `gtmux rotate-token
--session prod` 로 명시 회전한다.

### 3.6 Reverse proxy 설정

#### 3.6.1 Caddy (권장 — ACME 자동)

`/etc/caddy/Caddyfile`:

```caddyfile
gtmux.example.com {
  encode zstd gzip

  # WebSocket + HTTP 모두 같은 backend 로.
  reverse_proxy 127.0.0.1:9001 {
    header_up Host {host}
    header_up X-Forwarded-For {remote_host}
    header_up X-Forwarded-Proto {scheme}
  }

  # HSTS 는 gtmux 가 자체로도 부착하지만 edge 에서도 강제.
  header Strict-Transport-Security "max-age=31536000; includeSubDomains"
}
```

```bash
sudo systemctl reload caddy
```

#### 3.6.2 nginx + certbot 대안

```nginx
server {
  listen 443 ssl http2;
  server_name gtmux.example.com;

  ssl_certificate     /etc/letsencrypt/live/gtmux.example.com/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/gtmux.example.com/privkey.pem;

  location / {
    proxy_pass http://127.0.0.1:9001;
    proxy_http_version 1.1;
    proxy_set_header Host              $host;
    proxy_set_header Upgrade           $http_upgrade;
    proxy_set_header Connection        "upgrade";
    proxy_set_header X-Forwarded-For   $remote_addr;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_read_timeout 3600s;             # WS 장기 idle 허용
  }
}

server {                                 # HTTP → HTTPS redirect
  listen 80;
  server_name gtmux.example.com;
  return 301 https://$host$request_uri;
}
```

### 3.7 systemd unit (영속 supervisor)

`~/.config/systemd/user/gtmux@.service` (user-level unit — privilege 분리):

```ini
[Unit]
Description=gtmux server (%i)
After=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/gtmux start --session %i --config %h/.config/gtmux/%i.config.toml
Restart=on-failure
RestartSec=3
# Frontend bundle 을 BE 가 서빙하도록 — proxy 가 별도 정적 서버를 안 두는 경우.
Environment=GTMUX_FRONTEND_DIST=%h/gtmux/codebase/frontend/dist
# log 는 journal 로 → log_format = "json" 권장.
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
```

활성화:

```bash
loginctl enable-linger $USER             # 사용자 세션이 닫혀도 unit 살아있음
systemctl --user daemon-reload
systemctl --user enable --now gtmux@prod
journalctl --user -u gtmux@prod -f
```

> **루트로 띄우지 말 것.** ADR-0003 R(rej)6 — EUID==0 에서는 명시
> `--allow-root` 플래그가 없으면 거부된다.

### 3.8 컨테이너 / Pod 시나리오 (대안)

`0.0.0.0` 바인딩이 필수면 container 내부에서 `bind = "0.0.0.0"` 으로
두고, container 앞단 (load balancer / ingress) 이 TLS 종단을 한다.
이때:

```toml
[server]
bind = "0.0.0.0"
port = 9001

[security]
cors_origins   = ["https://gtmux.example.com"]
host_allowlist = ["gtmux.example.com"]
```

`bind` 가 loopback 밖이라 `Mode = cloud` 자동 추론 → rate limit + HSTS
헤더 자동 부착. Ingress 가 `X-Forwarded-*` 를 신뢰 가능한 IP 에서만
세팅하도록 ingress 측에 trusted-proxy 화이트리스트 설정 필수 (sketch §13
+ SSoT §1.11).

### 3.9 토큰 회전 / logout-all (operator 액션)

```bash
# Token mode — 현재 서버의 token 무효화 + 새 URL 발행 (stdout banner 재출력).
gtmux rotate-token --session prod

# Web 측 — 모든 cookie session 무효화 (SettingsOverlay 의 [Logout all]).
# 또는 BE 재기동 (현 MVP 는 cookie session table in-memory — 재기동 시 모두 무효).
```

분실 디바이스 / 의심 토큰 노출 시엔 위 두 명령으로 즉시 차단할 수 있다.

---

## 4. Config 참고표

CLI flag > env (`GTMUX_*__*`) > TOML > built-in default.

| 키 | 기본 | env | CLI | 영향 |
|---|---|---|---|---|
| `server.session` | (필수) | — | `--session` | 세션 식별자. 1:1:1 (Server : tmux daemon : port). |
| `server.port` | `9001` | `GTMUX_SERVER__PORT` | `--port` | HTTP/WS listen port. |
| `server.bind` | `"127.0.0.1"` | `GTMUX_SERVER__BIND` | — | loopback / `unix:/…` → local mode, 그 외 → cloud. |
| `runtime.log_level` | `"info"` | `GTMUX_RUNTIME__LOG_LEVEL` | — | tracing 레벨. |
| `runtime.log_format` | `"auto"` | `GTMUX_RUNTIME__LOG_FORMAT` | — | `auto|text|json`. systemd 면 `json`. |
| `security.cors_origins` | `[]` (loopback 시 자동 합성) | — | — | cloud 면 명시 필수. wildcard 금지. |
| `security.host_allowlist` | `[]` (loopback 시 자동 합성) | — | — | DNS rebind 방어. cloud 면 명시 필수. |
| `auth.mode` | `"token"` | — | — | `"token"` 또는 `"password"` (ADR-0020 D1). |
| `auth.cookie_max_age_days` | `7` | — | — | 1–30. Rolling renewal. |
| `auth.rate_limit_per_5min` | `5` | — | — | password mode 실패 시도 한도. |
| `frontend_dist` | `None` | `GTMUX_FRONTEND_DIST` | — | BE 가 직접 SPA 정적 서빙할 디렉터리. |
| `workspace_path` | `${XDG_DATA_HOME}/gtmux/workspace/` | — | `--workspace` | layout / state 저장 위치. boot-immutable. |

XDG 경로:

| 용도 | 경로 |
|---|---|
| 세션 config | `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml` |
| 토큰 | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token` (0600) |
| Pidfile | `${XDG_STATE_HOME}/gtmux/<session>.pid` |
| Password hash | `${XDG_STATE_HOME}/gtmux/password.argon2` (0600) |
| Workspace | `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/` |

---

## 5. 트러블슈팅

| 증상 | 원인 | 조치 |
|---|---|---|
| `cannot find type Group / Panel in api.d.ts` | codegen 미수행 | `make codegen` → frontend 빌드 재시도 |
| `Address already in use (os error 48)` | 다른 gtmux 가 같은 포트 점유 | `gtmux status` 로 찾기 → `gtmux stop --session <name>` |
| `pidfile exists but process is gone` | 직전 실행이 비정상 종료 | `gtmux teardown --session <name> --force --keep-state` |
| 브라우저에 `Forbidden` (cloud) | `host_allowlist` 또는 `cors_origins` 누락 | 위 §3.4 표대로 외부 origin / host 명시 |
| 로그인 시 `connect-src` 위반 | cloud CSP 가 `wss://<host>` 를 모름 | `host_allowlist[0]` 또는 향후 `[cloud].public_host` 가 실제 외부 host 와 일치하는지 확인 |
| HTTP 로 cloud 부팅 거부 | TLS 미설치 + `--allow-cloud-without-tls` 미지정 | reverse proxy + ACME 설치, 또는 *위험을 알고* 명시 플래그 사용 |
| `EUID==0` 거부 | root 실행 차단 | 일반 유저로 재실행. 다른 유저의 PTY 와 권한이 섞이는 표면 차단. |
| Cookie 만료 후 접근 불가 | rolling 7d 초과 | banner URL 재오픈 (token mode) 또는 `/auth` 에서 password 재입력 |

`docs/sketch.md` §13 (위협 모델 8 카테고리) 와 `docs/ssot/security-
defaults.md` 가 결정의 정본이며, 본 가이드는 operator 시점의 *재구성*
이다. 보안 정책을 amend 할 일이 생기면 SSoT 와 ADR 측을 먼저 갱신하고
본 문서로 sync 한다 (CLAUDE.md ADR ↔ plan coherence 규칙).
