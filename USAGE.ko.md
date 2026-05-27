# gtmux 사용 설명서 — 로그인 이후

> [English](USAGE.md) · **한국어**
>
> Auth dialog 를 지나 workspace 가 열린 다음, canvas 의 각 부분이 무엇을
>하는지에 대한 reference. 설치 / 설정 / 첫 로그인은
> [`QUICKSTART.ko.md`](QUICKSTART.ko.md) 가 짝.

---

## 목차

1. [Session 관리](#1-session-관리)
2. [Architecture: server · terminal server · web app — 그리고 Terminal vs Terminal panel](#2-architecture)
3. [Toolbar — tool 별 세부 기능](#3-toolbar--tool-별-세부-기능)
4. [Group 기능](#4-group-기능)

Appendix:
- [A. Keyboard shortcuts](#a-keyboard-shortcuts)
- [B. Inspector / layer tree / context menu / viewport controls](#b-기타-ui-surface)

---

## 1) Session 관리

**session** 은 한 개의 이름 붙은 영속 workspace 다 — Canvas layout 파일
하나 + 그 안에서 띄운 Terminal 들 + canvas 에 올려놓은 visual item
(note, shape, document 등) 의 모음. gtmux server 1개 안에 session 을
원하는 만큼 둘 수 있지만, **브라우저 탭 하나 당 active session 은
1개** 다.

### 1.1 Session 상태가 저장되는 위치

| 산출물 | 경로 |
|---|---|
| Canvas layout (session 별) | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.json` (schema v2: `groups[]` + `items[]` + `viewport`) |
| Auth token | `${XDG_STATE_HOME}/gtmux/<session>.token` (mode 0600) |
| Pidfile | `${XDG_STATE_HOME}/gtmux/<session>.pid` |
| 업로드된 asset | `<workspace>/.assets/<sha256>` (content-addressed) |
| Server config | `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml` |

Layout 파일은 의미 있는 mutation (drag-commit / inspector edit / paste
…) 마다 300ms debounce 후 rewrite. Terminal 출력은 persist 되지 않고,
**panel 위치 + 메타데이터만** 저장된다.

### 1.2 Auth dialog (가장 먼저 보이는 화면)

브라우저 탭에 active session 이 바인딩되지 않은 상태에서 뜬다 —
첫 로그인, `Switch session` 직후, 또는 서버가 새 identity 로 재기동된
경우.

선택지:

- **[Existing session]** — 목록에서 선택. layout 에 최근 conflict 가
  있으면 *Attach confirm modal* 가 추가로 뜬다.
- **[New session]** — 이름 입력 → 서버가 layout 파일을 만들고 빈 canvas
  로 진입.

### 1.3 Active session dropdown (toolbar 좌측 상단)

Toolbar 의 tool 그룹 좌측에 현재 active session 이름이 표시된 버튼이
있다. 클릭하면 `SessionListModal` 이 바로 열린다 — 다른 session 을
고르면 페이지 리로드 없이 canvas 가 hot-swap (이전 session 의
terminal 들은 detach 되지만 server 의 terminal pool 안에 살아 있어,
다시 돌아오면 live 상태 그대로 re-attach).

### 1.4 Session menu (titlebar kebab)

Titlebar 의 `⋮` 가 `SessionMenu` 다. 항목:

| 항목 | 효과 |
|---|---|
| **New session** | `NewSessionModal` — 이름 입력 후 생성. |
| **Session list** | `SessionListModal` (toolbar dropdown 과 동일). |
| **Import session** | `ImportSessionModal` — `.json` 선택. 서버가 schema validate 후 새 session 파일 작성. 이름 충돌은 409, 이름 바꿔 재시도. |
| **Export session** | `ExportSessionModal` — 현재 session 을 JSON 으로 다운로드. layout (위치/label/note/reference) 은 포함, **terminal 출력은 미포함**, **업로드된 asset 바이트도 미포함**. |
| **Rotate token** | Token 모드 전용. 새 토큰 발급 + 쿠키 재발급. |
| **Settings** | `SettingsOverlay` 열기. 탭: behavior / auth / shortcut / asset 저장. |
| **Shutdown** | Confirm modal → 이 session 의 모든 PTY kill + layout 저장 + exit 0. |
| **Logout** | 3단계 흐름: local reconnect hint clear → `POST /auth/logout` → `/auth` full reload. 쿠키는 서버 측에서 revoke. |

Destructive action (Delete session / Shutdown / Close panel / Delete
group with children) 은 모두 confirm modal 거친다.

### 1.5 Import / Export — 무엇이 들어가고 무엇이 빠지나

`Export` 는 단일 session `.json` 을 쓴다:

- ✅ Group (tree, label, color, visibility, lock).
- ✅ Item (position / size / z / visibility / lock, type 별 payload
  — text content, note body, shape stroke/fill, snippet entries,
  inline document content, file path reference, image asset ID).
- ✅ Viewport (현재 pan / zoom).
- ❌ Live terminal stream — Terminal item 은 `terminal_id` (UUID) 와
  label 만 보유. import 시점에 그 UUID 는 pool 에 없으므로 item 은
  *dangling* 상태 (placeholder 로 렌더, double-click 으로 새 shell
  spawn).
- ❌ 업로드 asset 바이트 (이미지, 임베디드 문서) — ID 는 보존되지만
  바이트는 `<workspace>/.assets/` 에만 있다. 머신 간 옮기려면 따로 복사.

Layout 백업, canvas template 공유, asset store 가 공유된 머신 간 session
이전엔 충분. 전체 workspace archive 는 아니다.

### 1.6 Attach recovery & reconnect

WebSocket 이 끊기면 (노트북 sleep, 서버 재기동, 네트워크 블립):

- 페이지 상단에 **reconnect banner**.
- 1초 grace 후 exponential backoff (cap 30s), 무한 retry.
- 10회 연속 실패 → `Server stopped` 배너로 재기동 / 다른 session attach
  유도.
- WS 복귀 시 FE 가 UUID 기준으로 모든 Terminal panel 을 re-attach —
  pool 에 살아 있으면 live stream + scrollback 즉시 복원, kill 됐거나
  server restart 으로 사라졌으면 panel 에 *dangling* 배지 → 닫을지 새
  shell 을 자리에 spawn 할지 사용자 결정.

---

## 2) Architecture

3개의 논리 tier — 모두 한 `gtmux` Rust binary + 브라우저 탭 하나 안에서
돈다.

```
 ┌────────────────────────┐
 │  Web app (browser)     │  Svelte 5 + xterm.js — canvas / panel
 │  · canvas              │  layout / viewport / selection / inspector
 │  · panels              │  / layer tree / clipboard 소유.
 │  · sidebars            │
 └────────────┬───────────┘
              │ HTTP (REST · layout PUT/GET)  +  WebSocket (live)
              │
 ┌────────────▼───────────┐
 │  gtmux server          │  Rust (axum 0.8 + tokio). --session /
 │  · http-api crate      │  --port 1쌍당 1프로세스.
 │  · ws-server crate     │  Origin / Host / CSRF 미들웨어. Auth.
 │  · auth crate          │  Layout 영속화 (HTTP PUT, 300ms debounce).
 │  · config crate        │
 └────────────┬───────────┘
              │ in-process channel
              │
 ┌────────────▼───────────┐
 │  Terminal server       │  pty-backend crate.
 │  (PTY supervisor)      │  Terminal 1개당 PTY pair + child shell 1.
 │  · portable-pty        │  출력 → tokio::broadcast → 모든 subscriber.
 │  · ring buffer (128KB) │  입력 → master fd writer. resize 시 SIGWINCH.
 │  · child reaper        │  close 시 SIGTERM → 200ms → SIGKILL.
 └────────────────────────┘
```

### 2.1 gtmux server 가 소유하는 것

- 단 하나의 **session 바인딩** — `--session` 으로 지정되고 프로세스
  수명 동안 immutable.
- HTTP API: `GET/PUT /api/layout`, `GET /api/sessions`,
  `POST /api/sessions/import`, asset upload
  (`POST /api/assets/upload`), file picker stat 엔드포인트
  (`GET /api/files/stat`), auth
  (`POST /auth/login`, `POST /auth/logout`).
- WebSocket: 양방향 control + data 프레임. Terminal 별 출력과
  notification 프레임을 multiplex.
- HTTP / WS handshake 마다 cookie auth gate.
- `${XDG_STATE_HOME}/gtmux/<session>.json` 으로 session 별 layout
  persist.

### 2.2 Terminal server (`pty-backend` crate) 가 소유하는 것

- Terminal 마다: PTY master/slave pair (`portable_pty`) + child shell
  프로세스.
- 출력 loop: master fd 는 dedicated std::thread 에서 read, byte 들은
  `tokio::broadcast<Bytes>` 로 모든 구독 WS 연결에 fan-out. Terminal
  별 ring buffer (기본 128 KiB, 설정 가능) 가 신규 subscriber 의
  history replay 담당.
- 입력 loop: foreground panel 의 WS 프레임이 master fd 로 직접 write.
- Lifecycle: tokio child-watcher 가 exit 를 reap; 명시 close 는
  SIGTERM → 200ms grace → SIGKILL.
- Resize: WS `PANE_RESIZE` → `MasterPty::resize()` → TIOCSWINSZ → child
  에게 SIGWINCH.

PTY supervisor 는 gtmux 프로세스 **안에** 있다. 별도 terminal daemon 을
띄울 필요가 없다.

### 2.3 Web app 이 소유하는 것

모든 **시각** 및 **layout** 관련 상태:

- Canvas: pan/zoom 가능한 무한 작업 공간, custom SvelteFlow 위에서
  렌더.
- Panel: 위치 / size / z-index / lock / visibility / minimize /
  maximize / label / color / note.
- Selection 모델: **M** (manipulation selection — 조작 대상 item 들)
  과 **I** (input target — keystroke 를 받는 단 하나의 Terminal panel)
  은 직교.
- Sidebar (좌: layer tree + terminal list, 우: inspector).
- Toolbar, viewport ctrl, command palette, context menu.
- FE 로컬 clipboard (page lifetime), undo/redo stack (50 entries,
  in-memory).
- 커스텀 keyboard shortcut override → `localStorage`.

### 2.4 Terminal vs Terminal panel — 가장 중요한 구분

이 분리가 핵심:

|  | **Terminal** (backend) | **Terminal panel** (frontend) |
|---|---|---|
| 정체 | PTY 1개 + child shell 1개 | Terminal 1개를 렌더하는 canvas item 1개 |
| 식별자 | `terminal_id` (UUID, 서버 발급) | item `id` (UUID, FE 발급) + `terminal_id` reference |
| 소유자 | gtmux server (`pty-backend`) | Canvas layout (`items[]`) |
| Cardinality | 동시 1개 프로세스 | 같은 terminal 을 가리키는 panel 이 N개 가능 |
| Lifecycle | `POST /api/pane/new` 로 spawn. 명시 close (또는 supervisor crash reap) 에만 kill. WS disconnect / 브라우저 reload / session 전환에도 살아남음. | Terminal tool (또는 paste / duplicate) 로 spawn. Panel close 시 사라짐 (기본 동작은 동시에 terminal 도 close). |
| Persist? | In-memory 만. Layout JSON 에 들어가지 않음. 서버 restart = terminal 소멸. | 예, 위치 + label + 참조 `terminal_id` 가 layout JSON 에 들어간다. |
| 출력 스트림 | Terminal 당 `tokio::broadcast` 채널 1개 | 각 panel 이 그 채널의 subscriber 1개 |
| 입력 스트림 | master fd writer 1개 | input target (I) 인 panel 만 write |

#### Mirror 동작

Terminal 마다 broadcast 채널 1개이므로, **같은** `terminal_id` 를
참조하는 Terminal panel 2개는 *mirror* 다 — 출력이 같고, 어느 쪽에서
입력해도 동일 shell 로 간다. 같은 long-running 프로세스를 같은 서버
안의 여러 group / session 에서 동시에 보고 싶을 때 유용.

오늘 기준 mirror 만드는 법: Terminal panel 을 **Cmd/Ctrl+C** 로 copy
한 뒤 context menu 의 `Paste as mirror` (제공되는 경우). 일반 paste
(Cmd/Ctrl+V) 는 **clone** — 새 terminal 이 spawn 된다. 기본값이 clone
인 이유는 다중 입력 panel 이 의도치 않게 생기는 사고를 막기 위함.

#### 무엇이 무엇을 견디나

- **WS 끊김**: terminal 살아 있음, ring buffer 도 계속 채워짐. 재연결
  시 buffer replay + live 재개.
- **브라우저 탭 close**: WS 끊김과 동일 (terminal 살아 있음).
- **같은 탭에서 session 전환**: panel 이 참조를 끊어도 terminal 은
  서버 pool 에 남는다. 다시 돌아오면 panel 이 re-attach.
- **서버 재기동 (`gtmux stop` + `start`)**: terminal 은 **소멸**.
  Layout JSON 은 재연결 시 replay 되지만 모든 panel 이
  *dangling* — double-click 으로 같은 panel slot 에 새 shell spawn.
- **`gtmux teardown --session X`**: layout 파일 + token + state 모두
  제거.

---

## 3) Toolbar — tool 별 세부 기능

Toolbar 는 workspace 상단. 좌 → 우:

```
[Active session dropdown] | [Select · Hand] | [Terminal] |
                            [Rect · Ellipse · Line · Free draw · Text] |
                            [Note · Snippets · Document · Image · File path] |
                                            [Undo · Redo · Lock indicator]
```

12개 canvas tool 이 중앙에 있고, 4개의 semantic 그룹으로 묶여 divider
로 구분된다. Tool 은 기본적으로 **one-shot** — item 1개 spawn 하면
Select 로 자동 복귀. 같은 종류를 연속으로 만들고 싶으면 tool 활성 상태
에서 **Q** 로 lock — **Esc** 까지 유지.

모든 canvas tool 은 active session 이 있어야 동작. 없으면 아이콘이
disabled.

### 3.1 Mode 그룹

#### Select (V)
- 기본 모드. Click 선택, Shift+Click 추가, Cmd/Ctrl+Click 토글, 빈 곳
  drag 로 lasso.
- 선택 집합 = **M** (manipulation selection).
- Inspector (우측 panel) 가 M 으로 채워짐.
- Terminal panel 의 terminal 영역 클릭은 별도로 **I** (input target)
  설정 — keystroke 는 M 크기와 무관하게 그 panel 1개만 수신.

#### Hand (H)
- Click-drag 로 canvas pan. M 은 유지.
- 어떤 tool 에서든 **Space** 를 누른 동안엔 임시 Hand (release 시 원래
  tool 복귀).

### 3.2 Terminal 그룹

#### Terminal (T)
- Canvas 의 한 지점 클릭 → Terminal panel spawn.
- Backend: `POST /api/pane/new` 가 PTY pair 를 열고 `$SHELL` 을 child
  로 spawn, 새 `terminal_id` 반환.
- Frontend: 클릭 지점에 기본 사이즈 (~640×360) panel 이 mount, broadcast
  채널 attach, 입력 target 획득.
- Auto-mount cascade: 연속 spawn 시 FE 가 ~40px 씩 offset 주어 완전
  겹치지 않게.
- 출력 렌더링은 **xterm.js** (256-color, true-color, ring buffer 길이
  까지 scrollback). 입력은 child shell 로 직행.
- Close: panel 의 **×** → `PanelCloseConfirmModal`. Confirm → SIGTERM
  → 200ms grace → SIGKILL → panel 제거 + layout 저장.
- **Delete** 키로 닫아도 동일 흐름.
- Resize: panel handle drag → SIGWINCH 가 shell 로 전파, vim / htop
  같은 풀스크린 TUI 가 정확히 re-flow.

### 3.3 Figure 그룹

Vector 원시 도형. 전부 drag-spawn — 시작 모서리에서 반대편 모서리
까지 drag.

#### Rectangle (R)
- Fill / stroke / 둘 다. Inspector: stroke, stroke width (1–32),
  stroke dash, fill, fill enabled, stroke enabled, corner radius.

#### Ellipse (O)
- Rectangle 과 동일하되 corner radius 없음.

#### Line (L)
- 두 번 click — 시작점, 끝점. Inspector: stroke, stroke width, stroke
  dash (solid / dash / dot).

#### Free draw (P)
- Click-drag 로 자유 stroke. Point 배열을 그대로 보존 (simplification
  없음). Stroke + width 는 사후 Inspector 에서 수정.

#### Text (T)
- Text box 를 drag-spawn 한 뒤 자동 edit 모드 (커서가 box 안). **Esc**
  또는 외부 click 으로 commit.
- Inspector: content, font size (1–96), text align (left / center /
  right), vertical align (top / middle / bottom), color, font weight
  (light / normal / bold), italic, underline, strikethrough.
- ⚠ Shortcut **T** 는 Terminal 과 Text 가 공유 — toolbar 가 현재
  focus 그룹을 보고 결정한다 (Content 그룹에 진입한 상태에서 마지막에
  Text 를 썼다면 **T** 가 Text 로 동작).

### 3.4 Content 그룹

사용자 content 를 담는 item (note, snippet, document, image, file
path). 전부 drag-spawn.

#### Note (N)
- 스티키 노트를 drag-spawn. Title + 멀티라인 body 인라인 편집.
- Inspector: title, body, color (hex picker).

#### Snippets
- Snippet collection 을 drag-spawn. 각 entry 는 `{ key, body }`.
- Key 들은 노드 위에 chip 으로 표시 — chip 클릭 시 body 가 clipboard
  로 복사.
- Per-snippet edit panel (`SnippetEditPanel`) 로 편집. 삭제는 per-snippet
  쓰레기통 → `SnippetDeleteConfirmModal`.

#### Document (D)
- Document viewer 를 drag-spawn. 두 가지 content 모드:
  - Inline: Markdown 을 그대로 입력 / 붙여넣기 (≤ 64 KB, layout JSON
    안에 저장).
  - Asset-backed: 파일 업로드 → 서버가
    `<workspace>/.assets/<sha256>` 에 저장, item 은
    `asset_id` 보유.
- 3가지 render 모드를 노드 위 버튼으로 순환:
  - **Rendered**: Markdown → HTML, DOMPurify 로 sanitize.
  - **Interactive**: sandboxed iframe — 전체 HTML/JS 허용되지만 origin
    격리.
  - **Source**: raw text.

#### Image (I)
- Placeholder drag-spawn → picker 로 이미지 업로드.
- `POST /api/assets/upload` — 서버가 SHA256 hash 후
  `<workspace>/.assets/<sha256>` 에 저장, `asset_id` 반환.
- 지원: PNG, JPEG, WebP, GIF.
- 최대 크기: `[assets].max_size_bytes` (기본 50 MiB, sample 은
  100 MiB).

#### File path (F)
- Drag-spawn 시 **File picker modal** 가 자동 open.
- Picker 는 허용된 root 만 traverse. 기본 home directory 이고 설정 가능.
- Server-side `GET /api/files/stat` 가 메타데이터만 반환.
  파일 내용은 업로드되지 않고, 노드는 *경로 reference*.
- 노드를 double-click 하면 picker 재오픈.

### 3.5 History 그룹 (toolbar 우측)

#### Undo (Cmd/Ctrl+Z)
- 마지막 layout mutation 을 revert: drag commit, inspector edit, align,
  paste, delete, group/ungroup, z-order swap.
- 범위 외: terminal 입력, viewport pan/zoom, session lifecycle.
- Stack 은 session 별 in-memory (50 entries). Reload 시 소멸.
- Terminal item resurrection: undo 가 이미 pool 에서 사라진
  `terminal_id` 의 panel 을 되살리려 하면 거부 + toast (stack 의
  나머지는 보존).

#### Redo (Shift+Cmd/Ctrl+Z)
- Undo 의 역.

### 3.6 Lock indicator

활성 tool 이 Q-lock 일 때 toolbar 우측에 작은 **Q** chip 이 표시.
**Q** 또는 **Esc** 로 해제.

### 3.7 Shortcut 커스터마이즈

`SettingsOverlay → Shortcuts` 에서 single-key binding 을 재할당 가능.
Override 는 `localStorage` 에 저장, 브라우저 scope.
기본 binding:

| 그룹 | 기본 키 |
|---|---|
| Mode | V (Select), H (Hand) |
| Terminal | T |
| Figures | R (Rect), O (Ellipse), L (Line), P (Free draw), T (Text) |
| Content | N (Note), D (Document), I (Image), F (File path) |
| History | Cmd/Ctrl+Z (Undo), Shift+Cmd/Ctrl+Z (Redo) |
| Tool lock | Q |
| Cancel | Esc |

전체 canvas-shortcut 매트릭스는 [Appendix A](#a-keyboard-shortcuts).

---

## 4) Group 기능

**Group** 은 관련 canvas item 을 정리하기 위한 web-only 계층 컨테이너다.
Label, color, visibility, lock, 자식의 순서 list 를 보유. Group 은 자체
geometry 를 **들고 있지 않다**. 그룹의 시각 bound 는 자식들로부터 매
render 마다 derive.

### 4.1 데이터 모델 (`<session>.json` 안)

```
groups[ {id, parent_id, label, color, visibility, locked, order} ]
items[  {id, type, parent_id, x, y, w, h, z, visibility, locked, label, ...} ]
```

`Group.parent_id` 와 `Item.parent_id` 가 `null` 이면 canvas-root 의
자식. Tree depth 상한 없음, schema 는 재귀.

### 4.2 그룹 생성 / 제거

| 액션 | 트리거 | 결과 |
|---|---|---|
| **Group** | M ≥ 1 → context menu **Group** | 새 Group 노드가 M 의 모든 자식을 감싼다. Group 은 geometry 를 상속하지 않음. |
| **Ungroup** | Group 선택 → context menu **Ungroup** | 자식들이 `Group.parent_id` 로 promote. Group 노드 삭제. Non-destructive. |
| **Delete group** | Group 선택 → Delete / Backspace | `GroupCloseConfirmModal`. Confirm → 모든 자손 Terminal item 의 PTY 도 kill (destructive). |
| **Group 안으로 이동** | Layer tree 의 row 를 Group 헤더 위로 drag | Reparent. Group bounds 재계산. |
| **Group 간 이동** | Layer tree 에서 다른 Group 으로 drag | 동일. Canvas-drag reparent 는 P1+ — 지금은 layer tree 가 정본. |

### 4.3 속성

| 속성 | 동작 |
|---|---|
| **Label** | Free-text. Layer tree 에서 double-click 인라인 편집. 빈 값이면 "(unnamed)" placeholder. |
| **Color** | 선택. Layer tree 의 group accent band + 자손 panel header 의 thin tint band 로 표시. Picker 는 context menu → **Change color**. |
| **Visibility** | Layer tree row 의 eye 아이콘. Effective visibility = `self AND all ancestor`. Hidden 그룹은 dim + 자손 panel 렌더 중단. |
| **Lock** | Padlock 아이콘. Effective lock = `self OR any ancestor`. Locked item 은 drag / delete / alignment target 에서 제외. |
| **Order** | Layer tree 의 sibling rank (z 와 무관, §4.5 참조). Tree mode 에서 row drag 로 재정렬. |
| **Geometry / resize** | MVP 에서 1차 상태 아님. Bounds 는 자식들로부터 derive. Group resize 는 P1+ (Group spatial frame). |

### 4.4 Layer tree sidebar

좌측 사이드바의 **Layer tree** 가 Group 관리의 정본.

- **Tree 모드** (기본): 중첩된 Group / Panel. Drag 로 reparent /
  reorder. Row 아이콘: visibility toggle, lock toggle, type 아이콘
  (terminal / shape / text / note 등), context menu (`⋮`).
- **Z 모드** (사이드바 헤더의 토글 버튼): z-index 내림차순 flat
  list. 읽기 전용 — drag 불가. Row 마다 z 배지. "지금 누가 앞에 있나?"
  디버깅, 의도치 않게 묻힌 item 발견 시 유용.

같은 사이드바의 다른 탭 — **Terminal list view** — 는 (어느 group 에
있든) active session 의 모든 Terminal item 을 펼쳐 빠른 선택 / label
rename / *기존 terminal 에 연결* 흐름 (`ChangeTerminalModal`) 을 제공.

### 4.5 Z-index 와 Group

Z-index 와 tree 는 **독립**. Layer tree row 순서 변경은
`order` (조직용) 만 바꾸고 z 는 손대지 않는다. 역으로 z-order 액션
(Bring to front 등) 도 tree 를 건드리지 않는다.

z 는 item 에만 존재 — Group 은 z 없음. 모든 item 이 전역 z-space
하나를 공유하므로 Group A 의 자식이 Group B 의 자식보다 앞에 있어도
tree 구조와 모순되지 않는다.

Mutation:

| 액션 | 키 | 효과 |
|---|---|---|
| Bring forward | `]` | z++ 다음 높은 item 과 swap |
| Send backward | `[` | z-- 다음 낮은 item 과 swap |
| Bring to front | `Shift + ]` | z = 전역 max + 1 |
| Send to back | `Shift + [` | z = 전역 min − 1 |

신규 item 의 z = `전역 max + 1` → 항상 최상위에 등장.

### 4.6 Sub-tree 와 함께 동작하는 Clipboard (Cmd/Ctrl+C / X / V / D)

FE clipboard (page lifetime, in-memory) 는 Group 을 이해한다:

- **Copy** Group → Group + 모든 자손 Group + 모든 자손 Item 의
  sub-tree 전체를 직렬화.
- **Cut** Group → copy + delete, 단일 history entry.
- **Paste** → 모든 Group / Item 에 **새 UUID** 로 sub-tree 재생성.
  위치는 원본 bounding-box 에서 24px offset. Sub-tree 내부의 상대
  위치는 보존.
- **Duplicate** = clipboard 를 건드리지 않는 paste.

Terminal item paste 기본값은 **Clone** (각 panel 에 새 terminal
spawn). 원본 terminal 이 pool 에 살아 있으면 context menu 에 **Paste
as mirror** — 동일 `terminal_id` 를 공유하는 panel 이 paste 된다.

### 4.7 Multi-select 와 bulk 액션

M ≥ 2 일 때:

- Alignment: left / center / right / top / middle / bottom + Distribute
  H / Distribute V — Inspector 와 context menu 의 버튼.
- Layer tree 에서 visibility / lock 일괄 토글 (ancestor 그룹의 toggle
  → AND/OR propagation).
- 일괄 z-order: M 집합에 적용하면서 그 안의 상대 z 순서 보존.
- 일괄 delete: 한 confirm modal 안에 doomed Terminal ID 목록.

---

## A. Keyboard shortcuts

Canvas focus 가 필요 (terminal panel 의 텍스트 영역이 아닌 canvas-root
element).

### Tools
| 키 | Tool |
|---|---|
| V | Select |
| H | Hand |
| T | Terminal (또는 Text — 둘 다 도달 가능할 땐 마지막 사용 우선) |
| R | Rectangle |
| O | Ellipse |
| L | Line |
| P | Free draw |
| N | Note |
| D | Document |
| I | Image |
| F | File path |

### Tool modifier
| 키 | 액션 |
|---|---|
| Q | 활성 tool lock 토글 (sticky) |
| Esc | Lock 해제 + Select 복귀; text-edit / picker modal 도 exit |
| Space (hold) | 임시 Hand pan |
| Cmd/Ctrl + Scroll | Canvas zoom in / out |

### Selection
| 키 | 액션 |
|---|---|
| Click | Item 선택 (Terminal panel 이면 I 도 설정) |
| Shift + Click | 선택에 추가 |
| Cmd/Ctrl + Click | 선택에서 토글 |
| Lasso drag (Select tool, 빈 canvas) | 사각 multi-select |
| Cmd/Ctrl + A | 현재 scope 의 visible item 전체 선택 |
| Esc | 선택 해제 (tool lock exit 이후) |

### Move (nudge)
| 키 | 거리 |
|---|---|
| 화살표 | 1 px |
| Shift + 화살표 | 8 px |
| Cmd/Ctrl + 화살표 | 64 px |

### Clipboard
| 키 | 액션 |
|---|---|
| Cmd/Ctrl + C | 선택 copy (Group 은 sub-tree 동반) |
| Cmd/Ctrl + X | Cut |
| Cmd/Ctrl + V | Paste (24px offset) |
| Cmd/Ctrl + D | Duplicate |

### Z-order
| 키 | 액션 |
|---|---|
| `]` | Bring forward |
| `[` | Send backward |
| Shift + `]` | Bring to front |
| Shift + `[` | Send to back |

### History
| 키 | 액션 |
|---|---|
| Cmd/Ctrl + Z | Undo |
| Shift + Cmd/Ctrl + Z | Redo |

### Lifecycle
| 키 | 액션 |
|---|---|
| Delete / Backspace | 선택 삭제 (자식 있는 Group 은 confirm) |

### Viewport
| 키 | 액션 |
|---|---|
| `0` | Zoom 100% reset |
| Shift + `1` | 모든 item 을 viewport 에 맞춤 |

Single-key binding 은 모두 **Settings → Shortcuts** 에서 재할당
가능.

---

## B. 기타 UI surface

### Titlebar (44px, 상단)

- 좌: Session menu (kebab `⋮`).
- 중앙: `gtmux · <session> · <host>:<port> · <auth-mode>`.
- 우: Theme toggle (light / dark), Focus mode toggle (P1+).

### 좌측 sidebar (248px, dockable)

- Tab 1: **Layer tree** (Group + Item, tree 모드 / Z 모드).
- Tab 2: **Terminal list view** (active session 의 Terminal item flat
  list).

### 우측 panel — Inspector

- M ≥ 1 일 때 표시.
- Common section: x, y, w, h, z, visibility, locked, label.
- Type-specific section: item type 마다 다름 (text style, shape stroke
  / fill, snippet entry, document content, image asset, file path 등).
- Mixed value (M item 들 사이에서 값이 다름) 는 placeholder 로 표시
  (필드를 비우지 않음).
- M ≥ 2 일 때 alignment 버튼 추가.

### Viewport controls (하단 중앙 pill)

- Zoom −, zoom %, zoom +.
- 100% reset.
- Fit all.
- Selection count 배지.

### Context menu (우클릭)

Item / group / 빈 canvas 어디를 클릭했는지에 따라 가변:

- **Item / canvas**: Copy / Cut / Paste / Duplicate, Group /
  Ungroup, Align …, Distribute …, Delete, Bring to front / Send to
  back / Bring forward / Send backward, Lock / Unlock, Show / Hide,
  Minimize / Maximize.
- **Group**: 위 + Change color, Rename, Ungroup.
- **Terminal panel**: 위 + 기존 terminal 에 연결 (`Change terminal`).

### Reconnect banner (32px, 조건부)

WS drop 시 자동 표시. Retry countdown 과 수동 retry 버튼. 연결 복귀
시 자동 hide.

### Command palette (Cmd/Ctrl + K, binding 시)

이름 붙은 모든 액션 — tool 전환, alignment, z-order, shortcut,
group 액션 — list. Shortcut 을 잊었을 때 유용.

---

## 참고

- [`QUICKSTART.ko.md`](QUICKSTART.ko.md) — 설치 / 설정 / 인증 /
  session 생성.
- [`README.ko.md`](README.ko.md) — project 개요.
