# gtmux

> [English](README.md) · **한국어**

**gtmux는 터미널 중심 작업을 위한 단일 사용자 웹 캔버스 워크스페이스입니다.**
로컬 또는 개인 cloud 환경에서 Rust 서버를 실행하고, PTY 기반 shell을
spawn한 뒤, 브라우저의 무한 캔버스 위에 terminal, note, snippet,
document, image, shape, file reference를 자유롭게 배치합니다.

터미널 탭을 여러 개 열어두고 “어느 탭이 무엇이었는지” 기억하는 대신,
관련 terminal panel과 메모, 명령 snippet, 참고 문서, 이미지, 도형을 한
화면 안에서 공간적으로 정리하는 것이 목표입니다.

```
Browser canvas
  ├─ Terminal panels      xterm.js 로 렌더되는 live PTY shell
  ├─ Snippets             자주 쓰는 command/text 를 badge 로 저장하고 copy
  ├─ Notes & documents    markdown, PDF, file reference, image
  ├─ Shapes & text        가벼운 다이어그램과 시각적 구분
  └─ Groups & layers      구조화, visibility, lock, z-order

          HTTP + WebSocket
                │
                ▼
gtmux server: Rust · axum · tokio · portable-pty
```

---

## 왜 필요한가

실제 터미널 작업은 대개 터미널 하나로 끝나지 않습니다.

- 실행 중인 서버, DB shell, log tail, deploy command가 동시에 필요합니다.
- 현재 incident나 실험의 맥락을 메모해야 합니다.
- 자주 쓰는 명령은 정확히 복사해야 하고, 매번 다시 입력하면 실수합니다.
- 파일, 스크린샷, 다이어그램, 참고 문서가 command 옆에 있어야 합니다.
- 관련 작업끼리는 묶고, 다른 작업과는 시각적으로 분리해야 합니다.

gtmux는 이 작업들을 하나의 지속 가능한 workspace로 만듭니다. terminal
panel을 의미 있는 위치에 두고, 주변에 note와 snippet을 붙이고, 관련
항목을 group으로 묶은 뒤, 나중에 같은 layout으로 돌아올 수 있습니다.

---

## 주요 기능

- **브라우저 안의 실제 shell**  
  Terminal panel은 gtmux 서버가 관리하는 PTY에 연결됩니다. 서버가 살아
  있는 동안 browser reload나 WebSocket reconnect에도 live terminal 상태를
  다시 붙일 수 있습니다.

- **무한 캔버스 기반 작업 공간**  
  panel을 drag/resize하고, group, hide, lock, minimize, maximize, z-order
  조정으로 복잡한 작업 화면을 정리합니다.

- **Snippet collection**  
  자주 쓰는 command나 text block을 badge로 저장합니다. badge 클릭으로
  body를 system clipboard에 복사할 수 있어, 반복 입력과 오타를 줄입니다.

- **작업 중 문서화**  
  note, markdown document, PDF, image, file path, shape, free draw, text
  label을 terminal 옆에 붙여 작업 맥락을 함께 유지합니다.

- **Group과 layer tree**  
  visual hierarchy, visibility, lock, z-index를 분리해 관리합니다. workflow
  단위로 panel과 자료를 묶어 정리할 수 있습니다.

- **Reconnect와 복구 흐름**  
  짧은 네트워크 끊김, sleep, browser refresh 상황에서 reconnect banner와
  attach recovery가 작동합니다. terminal output은 ring buffer로 일부 replay
  됩니다.

- **Import / Export**  
  session layout JSON을 export/import할 수 있습니다. live terminal output과
  업로드 asset byte는 export에 포함되지 않습니다.

---

## 편의성 및 기대 효과

gtmux는 shell을 대체하려는 도구가 아니라, shell 작업에 공간과 맥락을
더하는 도구입니다.

- **컨텍스트 전환 감소**: terminal, note, snippet, reference가 한 화면에
  함께 있습니다.
- **명령 입력 실수 감소**: 반복 command를 snippet badge에서 복사합니다.
- **작업 기억 부담 감소**: 위치, label, group, note가 “이 terminal이 무엇을
  하던 중인지” 알려줍니다.
- **긴 작업 재개가 쉬움**: layout이 session 파일에 저장되어, 나중에 다시
  열어도 시각적 구조를 복원할 수 있습니다.
- **설치/운영 단순화**: 하나의 Rust 프로세스가 frontend serving, HTTP API,
  WebSocket, auth, layout persistence, PTY supervisor를 함께 담당합니다.

---

## 사용 기술 스택

### Backend

- **Rust 1.85**
- **axum 0.8**, **tower/tower-http** — HTTP API, static serving,
  middleware, CORS, Host validation
- **tokio 1.52** — async runtime, process, IO, signal, timer
- **tokio-tungstenite** — WebSocket transport
- **portable-pty** — cross-platform PTY 기반 child shell
- **serde / serde_json** — layout 및 API 데이터
- **figment + TOML** — 설정 파일
- **argon2** — password mode credential 저장
- **utoipa + openapi-typescript** — OpenAPI 기반 frontend type 생성

### Frontend

- **Svelte 5**, **TypeScript 5.9**, **Vite 7**
- **@xyflow/svelte** — canvas/node interaction 기반
- **xterm.js 6** + fit / Unicode 11 addon — terminal rendering
- **marked + DOMPurify** — markdown document rendering 및 sanitization
- **lucide-svelte** — UI icon
- Backend OpenAPI 계약에서 생성되는 TypeScript API 타입

---

## 빠른 시작

전체 설치/설정/auth 흐름은 [QUICKSTART.ko.md](QUICKSTART.ko.md)를
참조하세요. 짧은 버전:

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

make codegen
( cd frontend && npm install --no-audit --no-fund && npm run build )
( cd backend  && cargo build --workspace --release )

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session demo
```

서버 stdout에 출력되는
`Open URL: .../auth/bootstrap?token=...` 주소를 브라우저에서 한 번 엽니다.
쿠키가 발급된 뒤에는 `http://127.0.0.1:9001/` 같은 일반 root URL을
사용하면 됩니다.

---

## Local / Cloud 모드

gtmux는 단일 사용자 앱입니다. 권장 운영 범위는 다음과 같습니다.

- **Local mode**: `127.0.0.1`에 bind. 내 컴퓨터에서만 접속. TLS 불필요.
- **Private cloud mode**: LAN / VPN / Tailscale 같은 신뢰 네트워크에서
  명시적인 CORS/Host allowlist와 함께 사용.
- **Public internet 노출**: gtmux를 직접 HTTP로 노출하지 말고 HTTPS reverse
  proxy 뒤에 둡니다. 토큰과 쿠키가 평문으로 나가는 구성을 피해야 합니다.

reverse proxy 뒤에서 운영할 땐 `[cloud].trusted_proxy_ips` 에 proxy 의
IP/CIDR 를 지정해, auth rate limit 이 모두를 한 버킷에 묶지 않고 실제
client IP 별로 동작하게 하세요 — [QUICKSTART.ko.md](QUICKSTART.ko.md) §3 참조.

자세한 local/cloud 설정은 [QUICKSTART.ko.md](QUICKSTART.ko.md)를 보세요.

---

## 문서 안내

- [QUICKSTART.ko.md](QUICKSTART.ko.md) — 설치, config, auth, 첫 session
- [USAGE.ko.md](USAGE.ko.md) — 로그인 이후 UI 전체 사용법

---

## Repository layout

```
codebase/
  backend/     Rust workspace
               crates/{http-api, ws-server, auth, config, pty-backend}
               bin/{gtmux-cli, gen-openapi}
  frontend/    Svelte 5 + Vite + TypeScript browser app
  shared/      생성된 OpenAPI handoff 파일
  smoke/       통합 smoke script
  Makefile     codegen / build / test / smoke / clean
```

---

## 프로젝트 상태

gtmux는 활발히 개발 중입니다. terminal panel, session 관리, canvas layout,
group, snippet, document, asset, import/export, auth, reconnect, local/cloud
기동 경로가 구현되어 있지만, 아직 안정화가 계속되는 프로젝트로 보는 것이
맞습니다.

---

## License

Rust workspace metadata와 동일하게 **MIT OR Apache-2.0** 듀얼 라이선스입니다.
