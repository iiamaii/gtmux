# gtmux Quickstart — single-user · localhost + 외부 접속

> Repo 의 `codebase/` 만 clone 한 직후 본 문서 한 장만으로 localhost 또는
> 신뢰된 네트워크에서 접속 가능한 gtmux server 를 띄우는 절차.
> 상세 설명·reverse proxy·systemd 는 `../docs/deploy.md` 참조.

## ⚠ 보안 전제

- gtmux **본체는 TLS 종단 미지원** — `bind = 0.0.0.0` 으로 직접 외부에
  열면 토큰·세션 쿠키가 **평문 HTTP** 로 전송됨. 신뢰된 네트워크 (사내
  VPN, Tailscale 등) 안에서만 사용 권장.
- 인터넷에 직접 노출할 거면 본 문서 대신 `../docs/deploy.md` §3 의
  Caddy/nginx + ACME reverse proxy 경로를 따를 것.
- 단일 사용자 전제 유지 — 본인 1명이 본인의 인스턴스에 접속하는 시나리오.
- localhost 전용 실행은 기본값 그대로 `127.0.0.1` 에 bind 되므로 cloud
  보안 설정이 필요 없다.

## 0) 사전 요구사항

| 항목 | 버전 | 설치 |
|---|---|---|
| Rust | 1.85 | `curl https://sh.rustup.rs -sSf \| sh` — 첫 `cargo` 호출 시 `rust-toolchain.toml` 의 1.85 자동 설치 |
| Node.js | ≥ 20 (22 LTS 권장) | `brew install node` / `nvm install --lts` / 호스트 패키지 매니저 |
| OS | macOS / Linux (x86_64·aarch64) | Windows 미검증 |

## 1) 의존성 설치 + 빌드

```bash
cd codebase

# OpenAPI → TypeScript 타입 (frontend 빌드 전제)
make codegen

# frontend 의존성
( cd frontend && npm install --no-audit --no-fund )

# backend release binary + frontend dist
( cd backend && cargo build --workspace --release )
( cd frontend && npm run build )
# 산출물: backend/target/release/gtmux + frontend/dist/
```

## 2) Localhost 실행 (config 없이)

개발 PC 한 대에서만 접속한다면 config 파일을 만들지 않는다. 기본값이
`bind = "127.0.0.1"` 이고, 이 경우 local mode 로 동작한다.

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session local
```

stdout 에 다음 형태의 bootstrap URL 이 1회 출력된다.

```text
Open URL: http://127.0.0.1:9001/auth/bootstrap?token=...
```

브라우저에서 이 URL 을 한 번 열면 HttpOnly cookie 가 발급되고 canvas 로
진입한다. 이후 같은 브라우저에서는 token 없이 다음 주소로 접속한다.

```text
http://127.0.0.1:9001/
```

localhost 경로의 특징:

| 항목 | 값 |
|---|---|
| bind | `127.0.0.1` |
| mode | Local |
| auth | token bootstrap |
| TLS | 불필요 |
| `[cloud]` config | 불필요 |
| 외부 기기 접속 | 불가 |

종료:

```bash
./backend/target/release/gtmux stop --session local
```

## 3) Localhost 개발 실행 (implementation / hot reload)

UI 또는 backend 구현을 진행할 때는 backend 와 frontend dev server 를 따로
띄운다. backend 는 API/WS 를 담당하고, Vite 는 frontend hot reload 를
담당한다.

```bash
# Terminal 1 — backend
cd codebase/backend
cargo run -p gtmux-cli -- start --session dev
```

```bash
# Terminal 2 — frontend
cd codebase/frontend
npm run dev
```

절차:

1. backend stdout 의 bootstrap URL
   `http://127.0.0.1:9001/auth/bootstrap?token=...` 을 한 번 연다.
2. cookie 발급 후 `http://localhost:5173/` 로 이동한다.
3. frontend 변경은 Vite 가 즉시 반영한다.
4. backend 변경은 `cargo run ...` 프로세스를 재시작해서 반영한다.

개발 중 자주 쓰는 검증:

```bash
# frontend 타입/Svelte 검증
( cd codebase/frontend && npm run check )

# frontend production build
( cd codebase/frontend && npm run build )

# backend tests
( cd codebase/backend && cargo test --workspace )
```

## 4) 외부 접속용 Config 파일 작성

저장소에 동봉된 [`config.sample.toml`](./config.sample.toml) 을 복사해서
**`PUBLIC_IP` 자리만 본인 서버 IP/도메인으로 교체**한다 (port 를 9001
이외로 바꾸면 그 값도 함께).

```bash
mkdir -p ~/.config/gtmux
mkdir -p ~/.local/state/gtmux
chmod 700 ~/.local/state/gtmux
cp config.sample.toml ~/.config/gtmux/prod.config.toml

# 1) PUBLIC_IP → 본인 서버의 외부 IP 또는 도메인 (예: 203.0.113.42
#    또는 gtmux.example.com)
# 2) 기본 port 9001 을 안 쓰려면 [server].port + cors_origins +
#    host_allowlist 의 :9001 도 함께 교체
$EDITOR ~/.config/gtmux/prod.config.toml
```

sample 의 핵심 항목:

| 키 | 값 | 비고 |
|---|---|---|
| `[server].bind` | `"0.0.0.0"` | 모든 인터페이스 listen → cloud mode 자동 |
| `[security].cors_origins` | `["http://PUBLIC_IP:9001"]` | 정확 일치, wildcard 금지 |
| `[security].host_allowlist` | `["PUBLIC_IP:9001"]` | DNS rebind 방어 |
| `[auth].mode` | `"token"` | 현재 Quickstart 검증 경로. `gtmux start` 가 1회용 bootstrap URL 발행 |
| `[cloud].tls_required` | `false` | Quickstart 의 평문 HTTP 검증 경로. 이 값이 `false` 여야 HTTP 에서 cookie 가 저장됨 |

전체 옵션 + 주석은 sample 파일 자체에 인라인으로 있다.

## 5) 외부 접속용 서버 실행

```bash
GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start \
  --session prod \
  --config ~/.config/gtmux/prod.config.toml
```

stdout 에 `Open URL: http://127.0.0.1:9001/auth/bootstrap?token=...` 형태의
bootstrap URL 이 1회 출력된다. cloud mode 에서 `127.0.0.1` 은 로컬 표시용이므로,
외부 브라우저에서는 host 부분을 §4 에서 설정한 `PUBLIC_IP:9001` 로 바꿔 연다.

방화벽이 있다면 `9001/tcp` 를 미리 열어둘 것 (`ufw allow 9001/tcp` 등).

## 6) 외부에서 접속

브라우저로 bootstrap URL 을 연다:

```
http://PUBLIC_IP:9001/auth/bootstrap?token=...
```

cookie 발급 후 canvas 로 진입한다. 이후 같은 브라우저에서는 token 없이
`http://PUBLIC_IP:9001/` 로 접속한다.

## 7) Background 실행

`gtmux start` 는 기본적으로 foreground 프로세스다. 터미널을 닫아도 계속
띄워두려면 아래 중 하나를 사용한다.

### 7.1 `nohup` 으로 간단히 실행

별도 process manager 없이 빠르게 background 로 띄우는 방법이다. 로컬과 외부
접속 모두 같은 방식으로 실행할 수 있다.

```bash
cd codebase
mkdir -p ~/.local/state/gtmux

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
nohup ./backend/target/release/gtmux start --session local \
  > ~/.local/state/gtmux/local.log 2>&1 &
```

외부 접속용 config 를 사용할 때:

```bash
cd codebase
mkdir -p ~/.local/state/gtmux

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
nohup ./backend/target/release/gtmux start \
  --session prod \
  --config ~/.config/gtmux/prod.config.toml \
  > ~/.local/state/gtmux/prod.log 2>&1 &
```

로그에서 bootstrap URL 을 확인한다.

```bash
tail -f ~/.local/state/gtmux/local.log
tail -f ~/.local/state/gtmux/prod.log
```

### 7.2 `tmux` 또는 `screen` 안에서 실행

서버 콘솔과 bootstrap URL 을 직접 보존하고 싶다면 terminal multiplexer 안에서
foreground 로 실행한다.

```bash
tmux new -s gtmux-local
cd codebase
GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session local
```

분리: `Ctrl-b`, `d`

재접속:

```bash
tmux attach -t gtmux-local
```

### 7.3 장기 운영은 systemd 권장

서버 재부팅 후 자동 시작, journal 로그, restart 정책이 필요하면 user-level
systemd unit 으로 운영한다. 예시는 `../docs/deploy.md` §3.7 을 따른다.

## 8) 종료 / 운영

```bash
# 정상 종료
./backend/target/release/gtmux stop --session prod
# 강제 종료
./backend/target/release/gtmux stop --session prod --force
# 상태 확인
./backend/target/release/gtmux status --session prod
# 5-step 청소 (token/layout/pidfile/config)
./backend/target/release/gtmux teardown --session prod --force
```

(Optional) PATH 에 설치:

```bash
sudo install -m 755 backend/target/release/gtmux /usr/local/bin/gtmux
# 이후엔 `gtmux start --session prod --config …` 으로 호출 가능
```

Background 로 실행한 경우에도 종료는 동일하게 `gtmux stop --session <name>` 을
사용한다. `nohup` 으로 띄운 shell job 을 직접 kill 하기보다 pidfile 기반
`stop` 명령을 우선 사용한다.

## Troubleshooting

| 증상 | 조치 |
|---|---|
| `bind=... is cloud-mode but [cloud] section is missing` | §4 의 `[cloud]` 블록 누락. 더미 path 라도 명시 |
| `[cloud].tls_cert and tls_key must be set when cloud.tls_required=true` | HTTPS 운영 경로인데 cert/key marker 가 없음. Quickstart 처럼 평문 HTTP 로 검증하려면 `[cloud].tls_required = false` |
| 브라우저에 `Forbidden` | `cors_origins` / `host_allowlist` 가 실제 접속 origin 과 정확히 일치하는지 확인. port 포함, scheme 포함 (`http://`) |
| `/` 가 `{"error":"not_found"}` | `GTMUX_FRONTEND_DIST="$PWD/frontend/dist"` 없이 서버를 띄움. §2, §5, §7 의 start 명령처럼 dist 경로를 지정 |
| `cannot find type Group / Panel in api.d.ts` | `make codegen` 빼먹음. 재실행 후 빌드 |
| `Address already in use` | `gtmux status` → `gtmux stop --session prod` |
| `EUID==0` 거부 | root 실행 차단됨. 일반 유저로 |

상세 가이드: `../docs/deploy.md`.
