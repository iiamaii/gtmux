# Plan 0006 — Canvas Workspace Feature Roadmap

- 일자: 2026-05-15
- 작성: agent
- 입력: 사용자 요청 12개 항목, `CONTEXT.md`, `docs/reports/0030-sprint-7-closeout-and-handoff.md`, `docs/plans/0005-figma-layout-overhaul.md`, `docs/adr/0015~0017`, `docs/ssot/canvas-layout-schema.md`
- 상태: Draft
- 목적: 현재 gtmux 의 목적(단일 Session 의 Pane 을 무한 Canvas 에서 조작)과 다음 요청 기능을 사용 흐름 기준으로 재정렬하고, 추가되어야 할 UI 및 frontend/backend 기능을 도출한다.

## 0. 한 줄 목표

gtmux 를 "terminal panel 만 있는 canvas" 에서 **terminal, text, figure, note, image, document, file path 를 함께 다루는 단일 Session Canvas Workspace** 로 확장한다. 핵심은 사용자 흐름을 기준으로 인증, 생성, 선택, 편집, 정리, 동기화, 종료까지 끊김 없이 이어지게 만드는 것이다.

## 1. 현재 기준점

2026-05-15 현재 코드/문서 기준:

- backend 는 tmux 를 폐기하고 PTY-direct 모델을 사용한다. `Pane` 은 `PTY pair + child process` 이고 Server lifetime 에 종속된다.
- frontend 는 Svelte 5 + SvelteFlow 기반 Canvas 를 가진다.
- Titlebar, SessionMenu, ShutdownModal, Sidebar, PaneInfoPanel, HelpBar, ViewportCtrl, ContextMenu, PanelNode close guard, auto-mount 가 도입되어 있다.
- `Canvas Layout` v1 은 `groups[] + panels[]` 중심이다.
- 부팅 시 `panels[]` 는 stale Pane reference 로 strip 된다. 같은 Server lifetime 안에서는 Panel geometry 를 저장하지만, 재기동 후 terminal Panel 복원은 하지 않는다.
- Stage D 의 Toolbar2 는 아직 잔여다.

본 계획은 이 기준점 위에서 다음 큰 확장을 다룬다.

1. 인증 page
2. Canvas item 입력 도구
3. Server authoritative viewport/canvas sync
4. Layer list 와 menu 구체화
5. Terminal Panel metadata UI
6. Panel header/footer/minimize/settings
7. Settings page/modal
8. Viewport navigation
9. UI/UX 고도화

## 2. 제품 원칙

### P1. Session 은 하나, Canvas 도 하나

한 Server 는 하나의 logical Session 에 바인딩되고 하나의 Canvas 를 호스팅한다. Session switching UI 는 만들지 않는다. 사용자가 다른 Session 을 쓰려면 다른 Server 를 다른 port 로 띄운다.

### P2. 실행 상태와 Canvas 상태를 섞지 않는다

- `Pane`: backend 가 소유하는 실행 단위
- `Terminal Panel`: Pane 을 Canvas 위에 보여주는 web-owned visual object
- `Canvas Item`: text, figure, note, image, document, file path 같은 web-owned object

Terminal output/input 은 backend runtime state 이고, Panel title/description/geometry/visibility/lock/minimize 는 web-owned state 다.

### P3. "숨김"과 "종료"는 다르다

- invisible/hide: Canvas 에서 보이지 않을 뿐, 대상이 terminal Panel 이면 Pane process 는 살아 있다.
- minimize: 같은 위치에 header bar 만 남긴다.
- close: terminal Panel 의 경우 Pane process 종료까지 이어지는 destructive action 이다.
- shutdown: Server/Session 종료다.

### P4. Canvas 조작은 직접적이어야 한다

사용자는 toolbar 에서 tool 을 선택하고 Canvas 에서 바로 만든다. 생성 이후에는 layer list, context menu, panel header, settings modal 에서 같은 상태를 조작할 수 있어야 한다.

### P5. 동기화는 사용자가 의식하지 않아야 한다

단일 사용자라도 여러 탭/창은 mirror view 다. selection, viewport, focus mode, Canvas edits 는 모든 연결에 동기화되어야 한다.

## 3. 주요 사용자 흐름

### Flow A. Session 진입

1. 사용자가 Server URL 을 연다.
2. 인증 상태가 없으면 Auth Page 로 이동한다.
3. 사용자는 token 또는 password 중 하나만 입력한다.
4. 성공 시 Session cookie/authorization bootstrap 이 완료되고 Canvas 로 진입한다.
5. 실패 시 입력값은 보존하지 않고 명확한 에러와 재시도 상태를 보여준다.

필요 UI:
- Session Auth Page
- token/password segmented input 또는 mode switch
- invalid/expired/loading/locked state
- "Local Server" 정보 surface

필요 backend:
- token auth 는 기존 정책 유지
- password auth 를 채택할 경우 hash 저장/검증/rotation/rate limit 필요
- auth 실패 응답 표준화

### Flow B. Terminal 작업 시작

1. 사용자가 Toolbar 의 New Terminal Panel 버튼을 누른다.
2. backend 가 새 Pane 을 spawn 한다.
3. frontend 가 auto-mount 로 Terminal Panel 을 생성한다.
4. Panel header 에 terminal id, title, status 가 표시된다.
5. 사용자가 terminal 영역을 클릭하면 Input Target 이 된다.

필요 UI:
- Toolbar 의 terminal tool
- Terminal Panel header metadata
- Input Target 표시
- layer list 에 Terminal Panel row 표시

필요 backend:
- `pane-spawned` notify 의 metadata 보강
- 기본 title 제공
- alive/dead/status signal 제공

### Flow C. Canvas item 생성

1. 사용자가 Toolbar 에서 Text, Rect, Ellipse, Line, Free draw, Note, Image, Document, File Path tool 을 선택한다.
2. Canvas 에서 click 또는 drag 로 item 을 생성한다.
3. 새 item 은 자동 selection 되고 layer list 에 나타난다.
4. item 은 resize/move/rename/group/lock/invisible 을 지원한다.

필요 UI:
- Tool state
- per-tool creation gesture
- inline text/note edit
- shape resize handles
- image/document/file picker
- layer row 생성

필요 backend:
- Canvas Layout schema v2
- item persistence
- image/document asset storage
- file path security policy

### Flow D. Selection, grouping, layer 관리

1. 사용자가 Canvas 에서 단일 또는 다중 선택한다.
2. Sidebar Layer List 가 selection 을 반영한다.
3. 사용자는 group/ungroup, invisible, lock, rename, reorder 를 수행한다.
4. Group row 는 child item 을 계층으로 표시한다.
5. Group visibility/lock 은 자손에 effective state 로 전파된다.

필요 UI:
- marquee 또는 multi-select key
- layer tree drag/reorder/reparent
- group row expand/collapse
- invisible/lock icon toggle
- selection count / M indicator

필요 backend:
- group/item tree validation
- layout conflict handling

### Flow E. Panel 조작

1. Terminal Panel header 에 title, id, 상태, minimize, maximize, invisible, close 가 있다.
2. minimize 하면 해당 위치에 header bar 만 남고 하단 rounding 이 적용된다.
3. maximize 하면 Canvas viewport 또는 workspace 내에서 크게 표시된다.
4. footer 에 description 이 표시되고 접기/펼치기가 가능하다.
5. header 의 right-click 또는 more button 에 rename, panel settings 를 제공한다.

필요 UI:
- Panel header redesign
- minimize/maximize state
- footer description collapsible
- header context menu
- Panel Settings modal

필요 backend:
- Terminal close/kill ack 안정성 유지
- title/status metadata event
- web-owned metadata persistence

### Flow F. Viewport 이동과 동기화

1. 사용자가 pan/zoom 한다.
2. Server 가 viewport 를 단일 진실로 보관하고 모든 연결에 broadcast 한다.
3. 사용자가 layer row 또는 viewport ctrl 에서 "go to selection" 을 누른다.
4. Canvas 는 선택 item 의 bounding box 로 이동한다.
5. fit all, fit selection, reset zoom 을 제공한다.

필요 UI:
- ViewportCtrl 확장
- go to selected item
- fit selection / fit all
- server sync status

필요 backend:
- viewport state WS message 의 authoritative 처리
- broadcast loop
- reconnect 시 current viewport replay

### Flow G. Settings

1. 사용자는 Titlebar/SessionMenu 에서 System Settings page 로 이동한다.
2. system settings 는 Server/session/auth/theme/shortcut/storage/debug 를 다룬다.
3. Panel Settings 는 modal 로 열고 해당 Panel 또는 Canvas Item 의 설정만 다룬다.

필요 UI:
- `/settings` page
- Panel Settings modal
- readonly vs mutable field 구분

필요 backend:
- settings read API
- mutable setting update API
- token/password rotation command
- immutable boot config read-only surface

### Flow H. Shutdown

1. shutdown button 은 SessionMenu 또는 Titlebar 의 명확한 위치에 있다.
2. 사용자가 누르면 confirm modal 이 열린다.
3. active Pane 수, 저장 상태, 종료 효과를 보여준다.
4. 확인하면 backend `kill-session` 으로 graceful shutdown 한다.

필요 UI:
- shutdown 위치 재정리
- modal copy 정리
- ended state banner

필요 backend:
- `kill-session` ack 보장
- graceful shutdown reason 전달

## 4. 추가 UI 인벤토리

### 4.1 Auth

| UI | 설명 | 우선순위 |
|---|---|---|
| Session Auth Page | token/password 로만 진입. mock design page 반영 | P0 |
| Auth mode switch | Token / Password 중 하나 선택 | P0 |
| Auth error panel | invalid, expired, server unavailable | P0 |
| Local server badge | session, host, local/cloud mode | P1 |

### 4.2 Toolbar

| Tool | Gesture | 결과 |
|---|---|---|
| Select | click, shift/cmd click, drag marquee | selection 변경 |
| Hand | drag | viewport pan |
| Terminal | click button | backend spawn + auto-mount |
| Text | click | inline text item 생성 |
| Note | click | sticky note item 생성 |
| Rect | drag | rectangle shape 생성 |
| Ellipse | drag | ellipse shape 생성 |
| Line | drag | line item 생성 |
| Free draw | pointer drag | path item 생성 |
| Image | file picker 또는 drop | image asset item 생성 |
| Document | file picker 또는 drop | document asset item 생성 |
| File Path | paste/type path | file path item 생성 |

### 4.3 Canvas Item UI

공통:
- selection outline
- resize handles
- rotation 은 MVP 제외
- z-index 조정
- lock/invisible
- rename
- duplicate
- delete
- group/ungroup

타입별:
- Text: inline edit, font size, color
- Note: title/body, color
- Rect/Ellipse/Line: stroke/fill/width
- Free draw: stroke color/width, simplify
- Image: thumbnail, original dimensions, replace
- Document: file name, size, open/download action
- File Path: path text, copy, reveal/open은 명시 opt-in

### 4.4 Layer List Panel

필수 기능:
- Group tree 표현
- Terminal Panel 과 Canvas Item 모두 표시
- row click selection
- multi-select reflection
- visible toggle
- lock toggle
- rename inline
- drag reorder
- drag reparent
- group collapse/expand
- type icon 표시
- hidden/locked/minimized/dead 상태 표시

### 4.5 Panel UI

Header:
- terminal id
- title
- status badge
- Input Target marker
- minimize
- maximize
- invisible
- close
- more menu 또는 right-click menu

Footer:
- description
- collapsed/expanded toggle
- empty description affordance

Minimized:
- 같은 x/y/w 위치에 header bar 만 렌더
- height 는 fixed header height
- bottom radius 적용
- terminal stream 은 Suspended 처리

Settings modal:
- title
- description
- visibility
- lock
- size
- terminal display options
- danger zone close

### 4.6 Viewport Control

추가 기능:
- zoom in/out
- reset 100%
- fit all
- fit selected
- go to selected item
- selection count
- sync indicator

## 5. Frontend 개발 항목

### FE-1. Auth Page

- `/auth` route 또는 unauthenticated landing 추가
- mock design page 반영
- token/password input
- bootstrap 성공 후 Canvas redirect
- auth failure state

### FE-2. Toolbar2 + Tool State

- `Toolbar2.svelte` 완성
- `toolStore` 또는 `ephemeralStore.currentTool`
- keyboard shortcuts
- active tool visual state
- existing NewPanelButton 흡수

### FE-3. Canvas Item Model

- `CanvasItem` discriminated union 도입
- `TerminalPanelItem` 과 non-terminal item 분리
- common geometry/state helpers
- stores: `items.svelte.ts` 또는 `panels` store 확장

### FE-4. Item Renderers

- `TextNode`
- `NoteNode`
- `ShapeNode`
- `LineNode`
- `FreeDrawNode`
- `ImageNode`
- `DocumentNode`
- `FilePathNode`

### FE-5. Creation Gestures

- click-to-create
- drag-to-create
- pointer capture
- cancel on Esc
- minimum size threshold
- inline edit start

### FE-6. Layer List V2

- terminal + item unified tree
- group rows
- invisible/lock toggles
- reorder/reparent
- rename
- context menu

### FE-7. Panel Header/Footer V2

- header layout redesign
- title/id/status integration
- minimize/maximize/invisible/close
- footer description collapsible
- header menu with rename/settings

### FE-8. Settings UI

- `/settings` page
- `PanelSettingsModal`
- mutable/read-only fields
- token/password management placeholder

### FE-9. Viewport Sync UI

- ViewportCtrl 확장
- go to selection
- fit selected/all
- sync indicator
- debounce/throttle

### FE-10. UX Polish

- shutdown/new panel 위치 재정렬
- left/right unfold rail gap 동일화
- responsive behavior
- light theme xterm theme
- visual regression target 정리

### FE-11. Tests

- vitest
- store tests
- layer tree tests
- toolbar creation tests
- panel state tests
- Playwright E2E

## 6. Backend 개발 항목

### BE-1. Auth Page Support

- auth page bootstrap endpoint 정리
- token/password only 정책 확정
- password mode 채택 시 hash 저장, verify, rotate
- auth failure response 표준화
- rate limit 또는 exponential delay

### BE-2. Canvas Layout Schema V2

기존 v1:

```json
{
  "schema_version": 1,
  "groups": [],
  "panels": []
}
```

제안 v2:

```json
{
  "schema_version": 2,
  "groups": [],
  "items": []
}
```

`items[]` 는 공통 필드와 타입별 payload 를 가진다.

공통 필드:
- `id`
- `type`
- `parent_id`
- `x`
- `y`
- `w`
- `h`
- `z`
- `visibility`
- `locked`
- `label`
- `description`
- `minimized`
- `maximized`

타입:
- `terminal`
- `text`
- `note`
- `rect`
- `ellipse`
- `line`
- `free_draw`
- `image`
- `document`
- `file_path`

### BE-3. Layout Validation V2

- item id 유일성
- group tree 정합
- terminal item 의 `pane_id` 존재성
- non-terminal item 은 `pane_id` 금지
- asset reference 존재성
- payload cap
- free draw point count cap

### BE-4. Asset Storage

- `${XDG_STATE_HOME}/gtmux/<session>/assets/`
- content hash file names
- metadata JSON
- MIME sniffing
- size cap
- image thumbnail policy
- document download/open policy

### BE-5. File Path Policy

- file path item 은 기본적으로 string metadata
- backend 가 자동 read/open 하지 않음
- open/reveal action 은 explicit opt-in
- path traversal 과 shell concat 금지

### BE-6. WebSocket Sync Extension

- viewport authoritative state
- selection broadcast
- item edit notify
- settings notify
- reconnect state replay

### BE-7. Conflict Handling

- 현재 ETag PUT 유지
- drag/draw/edit 는 debounce
- free draw 는 batch
- 필요 시 PATCH 또는 operation log ADR 검토

### BE-8. Terminal Metadata

- pane id
- generated title
- current process/shell status
- dead/alive
- optional OSC title support

### BE-9. Settings API

- `GET /api/settings`
- mutable setting update
- token rotation
- password change
- immutable boot config read-only

### BE-10. Performance/Safety

- payload size cap
- drawing point simplification
- asset lazy load
- server memory budget
- upload cap
- audit logs

## 7. ADR / SSoT 필요 항목

### ADR-0018 후보: Canvas Item Data Model

필요 이유:
- v1 schema 를 넘어서는 하드 변경
- terminal Panel 과 non-terminal item 의 관계가 명확해야 함
- layer/group/visibility/lock/z-index 공통 모델 확정 필요

결정할 것:
- `panels[] + canvas_items[]` vs unified `items[]`
- terminal item 과 Pane 의 lifetime 관계
- minimized/maximized/footer state 의 저장 위치

추천:
- unified `items[]`
- terminal 은 `type: "terminal"` item
- `pane_id` 는 terminal item 에만 허용

### ADR-0019 후보: Asset Storage

필요 이유:
- image/document 는 파일 저장과 보안 표면을 만든다.

결정할 것:
- 저장 경로
- hash naming
- upload cap
- MIME policy
- delete/orphan cleanup

### ADR-0020 후보: Canvas Operation Sync

필요 이유:
- free draw, drag, resize, viewport sync 는 전체 PUT 만으로 부담이 생길 수 있다.

결정할 것:
- HTTP PUT 유지 범위
- WS operation 도입 여부
- server authoritative viewport
- reconnect replay

### ADR-0021 후보: Auth Page Model

필요 이유:
- token/password only 정책은 보안과 UX가 얽힌다.

결정할 것:
- local token flow 유지
- password mode 저장 방식
- password rotation
- rate limit

### SSoT 갱신

- `docs/ssot/canvas-layout-schema.md` v2
- `docs/ssot/wire-protocol.md` viewport/item operation 추가
- `docs/ssot/security-defaults.md` password/asset/upload 정책 추가

## 8. 구현 단계 제안

### Stage 1. UX Foundation

- Auth Page
- Toolbar2
- Tool state
- shutdown/new panel 위치 정리
- rail gap polish

완료 기준:
- 사용자가 인증 후 Canvas 로 진입
- toolbar 에서 Select/Hand/New Terminal 전환 가능
- shutdown flow 가 명확함

### Stage 2. Panel Metadata + Header/Footer

- title/description fields
- header redesign
- footer collapsible
- minimize/maximize/invisible
- header context menu
- panel settings modal

완료 기준:
- terminal Panel 이 사용자 친화적 이름/설명을 가진다.
- minimize 시 header bar 만 남는다.
- close/shutdown의 의미가 시각적으로 분리된다.

### Stage 3. Layer List V2

- group tree
- multi-select
- invisible/lock toggles
- rename/reorder/reparent
- canvas selection 연동

완료 기준:
- Canvas 와 layer list 가 양방향 selection sync 된다.
- group layer 와 item layer 가 정확히 표현된다.

### Stage 4. Canvas Layout Schema V2

- ADR-0018
- SSoT schema v2
- Rust validation
- TS type generation
- migration path

완료 기준:
- terminal item 과 non-terminal item 을 같은 layout tree 에 저장할 수 있다.

### Stage 5. Basic Canvas Items

- text
- note
- rect
- ellipse
- line

완료 기준:
- toolbar 로 생성, 이동, resize, rename, layer 표시, 저장 가능.

### Stage 6. Viewport Server Sync

- server authoritative viewport
- fit selected/all
- go to selection
- reconnect replay

완료 기준:
- 두 탭의 viewport 가 같은 위치/zoom 을 유지한다.
- 선택 item 으로 이동할 수 있다.

### Stage 7. Asset Items

- image
- document
- file path
- asset storage
- security policy

완료 기준:
- image/document 는 layout 에 base64 로 들어가지 않는다.
- file path 는 안전한 metadata 로만 저장된다.

### Stage 8. Free Draw + Performance

- free draw renderer
- point simplification
- batching
- persistence debounce

완료 기준:
- 빠르게 그려도 UI/WS/PUT 가 과부하되지 않는다.

### Stage 9. Settings

- system settings page
- token/password management
- panel settings modal 완성
- debug/status view

완료 기준:
- system setting 과 panel setting 의 책임 경계가 UI 에서 분명하다.

### Stage 10. Test + Stabilization

- vitest
- Playwright
- visual regression
- multi-tab sync tests
- layout conflict tests

완료 기준:
- 주요 사용자 흐름 A~H 가 자동화 테스트 또는 체크리스트로 검증된다.

## 9. 우선순위 표

| 우선순위 | 기능 | 이유 |
|---|---|---|
| P0 | Auth Page | 앱 진입 흐름의 첫 화면 |
| P0 | Toolbar2 Select/Hand/New Terminal | 현재 Stage D 잔여이자 모든 Canvas 작업의 시작점 |
| P0 | Panel header/footer/minimize/metadata | terminal workspace 의 기본 사용성 |
| P0 | Layer List V2 selection/visibility/lock | Canvas item 이 늘어나면 탐색/정리가 필수 |
| P0 | Viewport go to selection | 큰 Canvas 에서 길 잃는 문제 해결 |
| P1 | Text/Note/Rect/Ellipse/Line | terminal 외 Canvas 입력의 최소 가치 |
| P1 | Schema v2 | Canvas item 저장의 전제 |
| P1 | Server viewport sync | 다중 탭 mirror 정책 정합 |
| P1 | Settings page/modal | 고급 설정의 위치 정리 |
| P2 | Image/Document/File path | asset storage/security 필요로 범위 큼 |
| P2 | Free draw | 성능/동기화 부담 큼 |
| P2 | Operation sync/PATCH | v2 안정화 후 필요성 재평가 |

## 10. 주요 리스크

| 리스크 | 설명 | 대응 |
|---|---|---|
| Schema churn | `panels[]` 에서 multi item 모델로 확장 | ADR-0018 + SSoT v2 먼저 |
| PUT 빈도 | drag/free draw 가 전체 PUT 을 과도하게 발생 | debounce + batch + operation sync ADR |
| Asset 보안 | image/document/file path 는 로컬 파일 접근 표면 생성 | asset ADR + size/MIME/path 정책 |
| UI 복잡도 | toolbar/layer/settings/menu 가 급격히 커짐 | 사용자 흐름별 stage 분리 |
| 다중 탭 충돌 | selection/viewport/layout edit 동시 발생 | server authoritative ephemeral state + ETag rebase |
| Terminal lifecycle 혼동 | minimize/invisible/close/shutdown 의미 혼동 | header/footer/menu copy 와 버튼 위치 분리 |

## 11. 바로 다음 작업 후보

1. **ADR-0018 Canvas Item Data Model 초안**
2. **Auth Page mock 반영**
3. **Toolbar2 구현**
4. **Panel header/footer redesign**
5. **Layer List V2 설계 상세화**

추천 순서는 2 → 3 → 4 → 5 → 1 이다. 이유는 사용자가 즉시 체감하는 UX foundation 을 먼저 정리하고, 그 다음 schema v2 로 큰 구조 변경을 들어가는 편이 시연과 검증이 쉽기 때문이다.

## 12. 변경 이력

- 2026-05-15: 초안. 사용자 요청 12개 항목을 사용 흐름 중심으로 재구성하고 frontend/backend 개발 항목, ADR/SSoT 필요 항목, 단계별 구현 순서를 정리.
