# gtmux

> [English](README.md) · **한국어**
>
> Single-user 웹 캔버스 워크스페이스. Rust supervisor 가 PTY 기반 shell
> 들을 spawn 하여 각각을 무한 캔버스 위의 draggable panel 로 펼치고,
> 모든 것을 한 프로세스 뒤 per-session 쿠키로 서빙한다.

```
You → browser → gtmux server (Rust, axum + tokio) → PTY pool → your shells
                            ↓
                   canvas: Terminal panel,
                   shape, note, snippet,
                   document, image, file ref.
```

---

## 이게 뭔가

한 개의 PTY 기반 shell session 을 Figma 스타일 무한 캔버스로 바꿔주는
웹앱. Terminal panel, sticky note, shape, image, snippet collection,
document 를 canvas 의 아무 위치에나 놓을 수 있고, group 은 layer-tree
컨테이너 처럼 동작한다. Auth 와 영속화는 이름 붙은 *session* 에 묶여
`${XDG_STATE_HOME}/gtmux/<session>.json` 에 살아 있다.

런타임에 tmux 는 사용하지 않는다 (이름에도 불구하고). PTY supervisor
가 gtmux binary 안에 함께 들어가 있다 —
[`docs/adr/0013-pty-direct-no-tmux.md`](docs/adr/0013-pty-direct-no-tmux.md)
참조.

---

## Quick install

Local + Cloud, auth, 첫 session 까지의 정식 흐름은
[QUICKSTART.ko.md](QUICKSTART.ko.md). 30초 버전:

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

make codegen                      # OpenAPI → TS 타입
( cd frontend && npm install --no-audit --no-fund && npm run build )
( cd backend  && cargo build --workspace --release )

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session demo
```

stdout 의 `Open URL: …token=…` 을 한 번 연다. 이후로는
`http://127.0.0.1:9001/` 를 북마크.

---

## 화면 구성

[USAGE.ko.md](USAGE.ko.md) 가 정식 walkthrough. 요약:

- **Toolbar** — 4개 semantic 그룹 × 12개 tool:
  - Mode: **Select (V)**, **Hand (H)**.
  - Terminal: **Terminal (T)** — PTY 기반 panel spawn.
  - Figures: **Rectangle (R)**, **Ellipse (O)**, **Line (L)**,
    **Free draw (P)**, **Text (T)**.
  - Content: **Note (N)**, **Snippets**, **Document (D)**,
    **Image (I)**, **File path (F)**.
  - 그 외 **Undo (⌘Z)** / **Redo (⇧⌘Z)** 와 **Q-lock** 표시.
- **Session 관리** — active-session dropdown + titlebar Session
  menu (New / List / Import / Export / Rotate token / Settings /
  Shutdown / Logout).
- **Group 기능** — Figma 스타일 layer tree, drag-reparent,
  AND-visibility / OR-lock 전파, sub-tree clipboard, tree 순서와
  분리된 z-index.
- **Architecture** — 하나의 gtmux 프로세스가 (a) HTTP/WS server,
  (b) Terminal 당 broadcast 채널 1개씩 가진 terminal-server PTY
  supervisor, (c) Svelte 5 웹앱 을 동시 호스팅. Terminal **panel**
  여러 개가 한 **Terminal** 을 mirror 가능 (1 PTY ↔ N panel).

---

## CLI reference

```
gtmux start    --session <name> [--port N] [--workspace PATH] [--config PATH]
gtmux stop     --session <name> [--force]
gtmux teardown --session <name> [--force] [--keep-state] [--keep-config]
gtmux status   [--session <name>]
gtmux rotate-token --session <name>
gtmux set-password / gtmux reset-password
```

각 subcommand 의 정확한 flag 는 `gtmux <subcommand> --help`.

---

## Repository layout

```
codebase/
  backend/     Rust workspace (axum 0.8 + tokio).
               crates/{ws-server, http-api, config, auth, pty-backend}
               bin/{gtmux-cli, gen-openapi}
  frontend/    Svelte 5 + Vite 7 + TypeScript 앱 (ADR-0012).
  shared/      기계 전용 handoff (openapi.yaml + 생성된 TS 타입).
  smoke/       통합 smoke 스크립트.
  Makefile     codegen / build / test / smoke / clean.
```

---

## 프로젝트 상태

활발한 개발 중 — multi-session pivot (plan-0007) 의 Stage 5+ 진행,
[ADR-0019](docs/adr/0019-session-and-workspace-model.md) 아래 Session
attach-recovery + delete-UI 레이어 land 중.

---

## 라이선스

**MIT OR Apache-2.0** 듀얼 라이선스 (Cargo workspace metadata 와 정합).
하위 사용처에 맞는 것을 선택.
