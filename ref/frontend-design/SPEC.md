# Infinity Canvas Editor — 구현 명세서

> 대상: 구현 에이전트 / 개발자
> 원본 산출물: `index.html` (단일 HTML, 1920×1080 고정 캔버스, 라이트/다크 토글)
> 디자인 시스템: Figma-inspired (`#000`/`#fff` 인터페이스, 청록 액센트 `#0d99ff`)

---

## 0. 개요

Figma 스타일의 무한 캔버스(infinity canvas) 에디터의 고밀도(high-fidelity) 프로토타입. 다음 구성을 가진다.

- **상단 타이틀 바** (44px) — 메뉴, 메뉴 탭, 파일 경로, 아바타, 테마 토글, Share / Present.
- **상단 툴바** (56px) — Select/Hand, Panel, Figure(Rect/Ellipse/Polygon/Pencil), Text/Doc/Caption.
- **워크스페이스** (남은 영역 전체) — 무한 캔버스 + 좌/우 플로팅 패널 + 하단 뷰포트 컨트롤 + 우클릭 메뉴.
- **좌측 패널** (248px) — Layers 트리(그룹 계층, visible/lock).
- **우측 패널** (268px) — Design / Prototype / Inspect (속성 박스 placeholder).
- **양쪽 패널 접기 버튼** — 패널 옆 가는 레일 버튼.
- **뷰포트 컨트롤** — 줌 in/out/100%/fit, 히스토리.

전체는 `1920×1080` 고정 프레임이며, 뷰포트에 맞춰 `scale-to-fit` 한다.

---

## 1. 레이아웃 (Layout)

### 1.1 프레임 호스트 + 스케일

```css
.frame-host { position: fixed; inset: 0; overflow: hidden; background: #0a0a0a; }
.frame {
  width: 1920px; height: 1080px;
  position: fixed; left: 50%; top: 50%;
  transform-origin: center center;
  transform: translate(-50%, -50%) scale(s);   /* s = min(vw/1920, vh/1080) */
  display: grid;
  grid-template-rows: 44px 56px 1fr 0px;
  overflow: hidden;
}
```

리사이즈 시 JS로 `s` 재계산.

### 1.2 그리드 분할

| 행 | 높이 | 내용 |
|---|---|---|
| 1 | 44px | `.titlebar` |
| 2 | 56px | `.toolbar` |
| 3 | 1fr | `.workspace` (캔버스 + 패널 + 컨트롤) |

### 1.3 워크스페이스 내부 좌표

`.workspace`는 `position: relative; overflow: hidden;`. 그 안에서:

- `.canvas-stage` — `position: absolute; inset: 0;`. **풀-블리드** 캔버스 무대.
- `.side-panel.left` — `top:8px; bottom:8px; left:8px; width:248px;`. **떠 있는 패널** (오버레이).
- `.side-panel.right` — `top:8px; bottom:8px; right:8px; width:268px;`.
- `.rail-toggle.left` — `top:50%; left:264px;` (펼침) / `left:8px` (접힘).
- `.rail-toggle.right` — `top:50%; right:284px;` (펼침) / `right:8px` (접힘).
- `.viewport-ctrl` — `bottom:16px; left:50%; transform: translateX(-50%);`.
- `.ctx-menu` — `position: fixed;`, 이벤트 좌표 기반.
- `.help-bar` — `top:8px; left:50%; transform: translateX(-50%);` 캔버스 단축키 안내 pill.

---

## 2. 디자인 토큰

### 2.1 색 (라이트 / 다크)

라이트:
```css
--bg:#fff; --surface:#fff; --surface-2:#f5f5f5;
--fg:#000; --muted:#6b6b6b;
--border:rgba(0,0,0,.10); --border-strong:rgba(0,0,0,.18);
--glass-dark:rgba(0,0,0,.06); --glass-darker:rgba(0,0,0,.10);
--accent:#0d99ff;
--canvas-bg:#f3f3f3; --grid:rgba(0,0,0,.05);
```

다크 (`html.dark`):
```css
--bg:#1e1e1e; --surface:#2c2c2c; --surface-2:#383838;
--fg:#f5f5f5; --muted:#9a9a9a;
--border:rgba(255,255,255,.10); --border-strong:rgba(255,255,255,.18);
--glass-dark:rgba(255,255,255,.06); --glass-darker:rgba(255,255,255,.10);
--canvas-bg:#1a1a1a; --grid:rgba(255,255,255,.04);
```

### 2.2 그림자

```css
--shadow-sm: 0 1px 2px rgba(0,0,0,.06), 0 0 0 .5px rgba(0,0,0,.06);
--shadow-md: 0 8px 24px rgba(0,0,0,.12), 0 0 0 .5px rgba(0,0,0,.08);
--shadow-lg: 0 20px 48px rgba(0,0,0,.18), 0 0 0 .5px rgba(0,0,0,.08);
```
다크 모드는 알파 0.4/0.5/0.6로 강화.

### 2.3 반경

```
--radius-sm: 4px   (탭, 인풋)
--radius-md: 6px   (메뉴 버튼, 툴 버튼, 컨텍스트 메뉴)
--radius-lg: 8px   (패널, 카드)
--pill:      50px  (CTA, 뷰포트 컨트롤, 아바타)
```

### 2.4 타이포

```
--font-sans: 'SF Pro Display', system-ui, -apple-system, 'Segoe UI', sans-serif;
--font-mono: 'SF Mono', Menlo, 'JetBrains Mono', ui-monospace, monospace;
```

- 본문 기본: 12–13px, weight 400, letter-spacing `-0.14px`, `font-feature-settings: "kern" 1;`
- 모노(섹션 헤더/숫자/단축키): 10–11px, 대문자, letter-spacing `+0.6px`.
- 디스플레이(캔버스 본문 텍스트): variable 가중 540, letter-spacing `-2.5px`까지.

### 2.5 간격

8px 베이스. 사용된 스텝: `2 4 6 8 10 12 14 16 18 20 24 36`.

---

## 3. 타이틀 바 (44px)

### 3.1 그리드

```css
.titlebar { display:grid; grid-template-columns: 1fr auto 1fr; padding: 0 12px; }
```

- **좌** — 햄버거 메뉴 버튼 + 텍스트 탭 5종(`File`, `Edit`, `View`, `Object`, `Help`).
- **중앙** — `Acme Studio / `**`Onboarding Flow — v3`** ` · Saved 2 min ago` (12px, muted, 가운데 굵은 강조).
- **우** — 테마 토글(해/달 아이콘) → 아바타 스택(겹친 26px 원) → `Share` (ghost pill) → `Present` (검은 pill).

### 3.2 컴포넌트

**메뉴 버튼**
```css
.menu-btn { width:32px; height:28px; border-radius:6px; background:transparent; }
.menu-btn:hover { background: var(--glass-dark); }
.menu-btn:focus-visible { outline: 2px dashed var(--accent); outline-offset: 1px; }
```
아이콘 16px stroke 1.5.

**텍스트 탭**
```css
.title-tab { padding: 4px 10px; border-radius: 4px; font-size: 12px; color: var(--muted); }
.title-tab.active { background: var(--glass-dark); color: var(--fg); }
```

**Pill 버튼**
```css
.pill-btn        { height:28px; padding:0 14px; border-radius:50px; background:var(--fg); color:var(--bg); font-size:12px; font-weight:500; }
.pill-btn.ghost  { background:transparent; color:var(--fg); box-shadow: inset 0 0 0 1px var(--border-strong); }
```

**아바타**
```css
.avatar { width:26px; height:26px; border-radius:50%; border:2px solid var(--surface); margin-left:-6px; font-size:10px; font-weight:600; color:#fff; }
```
3개 그라데이션 변형(`a/b/c`). 마지막 아바타는 `+N` 표기.

**테마 토글 동작**: 클릭 시 `html.dark` 토글, 내부 SVG를 해(circle+rays) ↔ 달(crescent) path로 스왑.

---

## 4. 툴바 (56px)

### 4.1 구조

```html
<div class="toolbar">
  <div class="toolbar-left"> Page 1 ▾ </div>     <!-- absolute left -->
  <div class="tool-group"> Select | Hand </div>
  <div class="tool-divider"></div>
  <div class="tool-group"> Panel </div>           <!-- aria-haspopup="true" → 우하단 ▾ 표식 -->
  <div class="tool-divider"></div>
  <div class="tool-group"> Rect | Ellipse | Polygon | Pencil </div>
  <div class="tool-divider"></div>
  <div class="tool-group"> Text | Doc | Caption </div>
  <div class="toolbar-right"> Comment | More </div>  <!-- absolute right -->
</div>
```

가운데 정렬(`justify-content: center`), 좌/우는 `position: absolute`로 떠 있음.

### 4.2 툴 버튼

```css
.tool { width:36px; height:36px; border-radius:6px; background:transparent; position:relative; }
.tool svg { width:18px; height:18px; }
.tool:hover  { background: var(--glass-dark); }
.tool.active { background: var(--accent); color: #fff; }
.tool[aria-haspopup="true"]::after { /* 우하단에 작은 ▾ 인디케이터 */ }
```

**툴팁**: 자식 `.tool-label`을 `position:absolute; top:calc(100% + 6px);` 검은 박스, hover 시 `opacity:1`.

### 4.3 아이콘 매핑

| 툴 | 키워드 | SVG 형태 |
|---|---|---|
| select | V | 화살표 커서 (filled triangle pointer) |
| hand   | H | 손바닥 (4개 손가락 stroke) |
| panel  | P | 사이드바 있는 윈도우 (rect + 좌측 컬럼) |
| rect   | R | rounded rect outline |
| ellipse| O | circle outline |
| polygon|   | 오각형 outline |
| pen    | P | 곡선 path (낙서) |
| text   | T | 세리프 "T" filled |
| doc    |   | 문서 + 접힌 모서리 + 본문선 |
| caption|   | 말풍선 + 본문선 |
| comment|   | 말풍선 outline |
| more   |   | 가로 3점 (filled circles) |

### 4.4 상태

활성 툴은 단일. 클릭 시 다른 `.tool.active`에서 클래스 제거 후 자기에 추가. 초기값: `select`.

---

## 5. 워크스페이스 — 캔버스

### 5.1 스테이지

```css
.canvas-stage {
  position: absolute; inset: 0; overflow: hidden;
  background:
    radial-gradient(circle, var(--grid) 1px, transparent 1.5px) 0 0/24px 24px,
    var(--canvas-bg);
}
```

24px 간격의 도트 그리드(라이트 5% 검정 / 다크 4% 흰색).

### 5.2 변환 컨테이너

```css
.canvas-transform {
  position: absolute; top: 50%; left: 50%;
  transform-origin: 0 0;
  transform: translate(tx, ty) scale(scale);
  will-change: transform;
}
```

모든 캔버스 아이템은 이 컨테이너의 자식, **워크스페이스 중앙(0,0)을 원점으로** 좌표가 부여됨 (음수 좌표 허용).

### 5.3 팬 + 줌

- `Space + 드래그`: 팬. `keydown` 시 `spaceDown=true`, `cursor: grab`. 마우스 다운 → 이동량을 `tx,ty`에 가산.
- 마우스 휠 + `⌘/Ctrl`: 줌. 새 스케일 = `clamp(0.1, 4, scale * (1 - deltaY*0.0025))`. **마우스 포인터 기준 줌 포커스** 공식:
  ```
  tx = mx - (mx - tx) * (newScale / scale)
  ty = my - (my - ty) * (newScale / scale)
  ```
- 휠 단독: `tx -= deltaX; ty -= deltaY;` (두 손가락 패닝).
- 휠 보드 전체 캔처를 위해 `{ passive: false }`.

### 5.4 캔버스 아이템 표시

```css
.canvas-item { position: absolute; }
.canvas-item.selected::after {
  content:''; position:absolute; inset:-1px;
  outline: 1.5px solid var(--accent);
}
.canvas-item.selected .handle {
  position:absolute; width:7px; height:7px;
  background:#fff; border:1.5px solid var(--accent);
}
/* handle 위치: tl(top:-4;left:-4), tr(top:-4;right:-4), bl(bottom:-4;left:-4), br */
.canvas-item.hidden { display: none; }
```

### 5.5 헬퍼 바

```html
<div class="help-bar">
  <kbd>Space</kbd> + drag · pan | <kbd>⌘</kbd> + scroll · zoom | <kbd>right-click</kbd> · menu
</div>
```
캔버스 상단 중앙, pill 모양, 11px 모노, pointer-events: none.

### 5.6 샘플 씬 (필수, 데모용 사전 배치)

원점 기준 절대좌표 (좌→우, 위→아래):

| ID | 종류 | left, top | w × h | 설명 |
|---|---|---|---|---|
| `(artboard)` | artboard | -720, -440 | 520 × 880 | "📱 Onboarding · iPhone" 라벨 |
| `hero-bg` | rect | -700, -420 | 480 × 280 | 분홍/노랑/하늘 그라데이션 |
| `hero-blob` | ellipse | -540, -360 | 220 × 220 | 핫핑크 방사형 그라데이션 |
| `hero-title` | text | -690, -110 | 460 × 200 | 72px / weight 540 / "Design that moves with you." 3줄 |
| `hero-sub` | text (**selected**) | -690, 90 | 420 × 80 | 18px / weight 340, 회색 |
| `hero-cta` | rect (검정 pill) | -690, 200 | 160 × 48 | "Get started →" 흰 텍스트 |
| `card-1` | rect | 60, -420 | 300 × 220 | 흰 카드, "01 · Canvas — Infinite room…" + 진행 점 3개 |
| `card-2` | rect | 380, -420 | 300 × 220 | 다크 그라데이션 카드, "02 · Layers" + 진행 바 62% |
| `card-3` | rect | 60, -180 | 620 × 200 | 흰 카드 2컬럼, "03 · Collab" + 커서 3명 |
| `sketch` | pen (svg) | -160, 0 | 260 × 140 | 핫핑크 곡선 + 보라 점선 |
| `caption-1` | caption | -160, 140 | 260 × 20 | "↑ Fig. 02 · ideation sketch" mono uppercase |
| `poly-1` | polygon (svg) | 760, -200 | 160 × 160 | 노란 오각형, 검정 stroke 2 |
| `doc-1` | doc | 60, 60 | 620 × 340 | "Brief · Internal — Onboarding rewrite Q2" 2단락 |

초기 뷰: `tx=0, ty=0, scale=0.7`.

---

## 6. 좌측 패널 — Layers (248px)

### 6.1 셸

```css
.side-panel.left {
  position: absolute; top: 8px; bottom: 8px; left: 8px;
  width: 248px;
  background: var(--surface);
  border-radius: 8px;
  box-shadow: var(--shadow-md);
  display: flex; flex-direction: column;
  transition: transform .25s cubic-bezier(.4,0,.2,1), opacity .2s;
}
.side-panel.left.collapsed { transform: translateX(-260px); opacity: 0; pointer-events: none; }
```

### 6.2 탭

```html
<div class="panel-tabs">
  <div class="panel-tab active">Layers</div>
  <div class="panel-tab">Assets</div>
  <div class="panel-tab">Pages</div>
</div>
```
12px, 하단 2px 보더로 활성 표시(마진 -1px로 패널 경계와 정렬).

### 6.3 트리 데이터 모델

```js
{
  id: string,            // 캔버스 아이템 data-id와 1:1
  type: 'group'|'rect'|'ellipse'|'polygon'|'pen'|'text'|'doc'|'caption',
  name: string,
  children?: Node[]       // group일 때만
}
```

상태:
```js
collapsed[id] = boolean    // 그룹 접힘
hidden[id]    = boolean    // 아이템 invisible
locked[id]    = boolean    // 아이템 잠금
selected: Set<string>      // 다중 선택
```

샘플 트리 (구현 시 그대로 시드):

```
▾ Hero / Mobile           (group)
    Background gradient   (rect)
    Pink blob             (ellipse)
    "Design that moves…"  (text)
    Subhead               (text)     ← 초기 선택
    CTA / Get started     (rect)
▾ Feature cards           (group)
    Card · Canvas
    Card · Layers (dark)
    Card · Collaboration
▾ Sketch + caption        (group)
    Free drawing          (pen)
    Caption · Fig 02      (caption)
  Pentagon · yellow       (polygon)
  Doc · Brief Q2          (doc)
```

### 6.4 행(row) 레이아웃

```css
.layer {
  display: grid;
  grid-template-columns: 14px 14px 16px 1fr auto auto;  /* caret | spacer | type-icon | name | lock | vis */
  align-items: center; gap: 4px;
  padding: 4px 8px;
  font-size: 12px;
}
.layer:hover    { background: var(--glass-dark); }
.layer.selected { background: rgba(13,153,255,.12); color: var(--accent); }
.layer.selected .icon { color: var(--accent); }
.layer.invisible { opacity: 0.45; }
```

**들여쓰기**: `style.paddingLeft = (8 + depth*14) + 'px'`.

**caret**(그룹만): 8px 화살표. 펼침 = 아래쪽 ▾, 접힘 = `transform: rotate(-90deg)`. transition `.15s`. 클릭 시 `stopPropagation` 후 `collapsed` 토글.

**type icon**: 12px, muted 색. 매핑은 §4.3 참고하되 더 단순한 12×12 outline.

**lock / vis 컨트롤**:
```css
.layer .ctrl { width:18px; height:18px; opacity: 0; }
.layer:hover .ctrl, .layer .ctrl.on { opacity: 1; }
.layer .ctrl:hover { background: var(--glass-darker); color: var(--fg); }
```
- 평소엔 숨김. **호버 시** 모두 표시.
- `on` 상태(잠금/숨김)는 호버 없어도 표시.
- 클릭 시 `stopPropagation`. lock은 단순 토글; vis는 §6.5 참조.

### 6.5 visible 토글 (그룹 전파)

```js
function toggleVisible(id) {
  hidden[id] = !hidden[id];
  const node = findNode(id);
  if (node?.type === 'group') {
    // 모든 후손 동일 값으로 강제 설정
    walk(node, n => n.children?.forEach(c => { hidden[c.id] = hidden[id]; }));
  }
  renderTree();
  reflectSelection();   // 캔버스 .hidden 클래스 동기화
}
```

### 6.6 선택 동작

- 일반 클릭 → `selected = new Set([id])`.
- `⌘/Ctrl/Shift + 클릭` → 토글 추가/제거 (다중 선택).
- 트리와 캔버스 양방향 동기화: 트리 변경 시 `reflectSelection()`가 `.canvas-item.selected` 클래스 재계산.

### 6.7 섹션 헤더

```html
<div class="panel-section-head">
  <span>Page 1 · onboarding</span>
  <span class="add">＋</span>
</div>
```
10px mono, uppercase, letter-spacing 0.6px, muted. `+`는 18×18 호버 시 배경.

---

## 7. 우측 패널 — Design (268px)

### 7.1 셸

좌측과 대칭. 접힘은 `transform: translateX(280px)`.

### 7.2 탭

`Design` (active) / `Prototype` / `Inspect`.

### 7.3 속성 섹션 (`.prop-section`)

```css
.prop-section { padding: 8px 12px 12px; border-bottom: 1px solid var(--border); }
.prop-section:last-child { border-bottom: none; }
.prop-head h4 { font-family: var(--font-mono); font-size: 11px; text-transform: uppercase; letter-spacing: .6px; color: var(--muted); font-weight: 400; margin: 0; }
```

섹션 순서 (placeholder 수준):

1. **Selection** — 우상단 chip `· Text · hero-sub` (`<span class="chip"><span class="dot"></span>...`). X/Y, W/H, R/C 인풋 6개.
2. **Layout** — `Auto` / `Hug ▾`, `Pad 0` / `Gap 8`.
3. **Typography** — `SF Pro Display ▾` (full-width), `Light · 340` / `18 / 27`, `a -0.2` / `Left ▾`.
4. **Fill** — 헤더 우측 `＋`, 행: `[swatch #555] 555555 · 100%`.
5. **Effects** — 헤더 우측 `＋`, 행: `None` (가운데, muted).
6. **Export** — 헤더 우측 `＋`, `1x · PNG` / `Export` (accent 색, pointer).

### 7.4 인풋 스타일

```css
.input {
  height: 28px;
  background: var(--surface-2);
  border: 1px solid transparent;
  border-radius: 4px;
  padding: 0 8px;
  font-size: 11px;
  font-family: var(--font-mono);
  display: flex; align-items: center; gap: 4px;
}
.input:hover { background: var(--glass-dark); }
.input .k { color: var(--muted); }       /* X/Y/W 같은 키 라벨 */
.swatch { width:16px; height:16px; border-radius:3px; border:1px solid var(--border-strong); }
.chip { padding:2px 8px; border-radius:50px; background:var(--surface-2); font-size:11px; color:var(--muted); }
.chip .dot { width:6px; height:6px; border-radius:50%; background: var(--accent); }
```

행 그리드: `.prop-row { grid-template-columns: 1fr 1fr; gap: 6px; }`, 전체 폭은 `.prop-row.full`.

---

## 8. 패널 접기 레일 버튼

```css
.rail-toggle {
  position: absolute; top: 50%; transform: translateY(-50%);
  width: 16px; height: 64px;
  border-radius: 4px;
  background: var(--surface); box-shadow: var(--shadow-sm);
  transition: left .25s cubic-bezier(.4,0,.2,1), right .25s …;
}
.rail-toggle.left          { left: 264px; }   /* 패널 우측 가장자리 옆 */
.rail-toggle.left.collapsed{ left: 8px;   }   /* 워크스페이스 가장자리로 이동 */
.rail-toggle.left.collapsed svg { transform: rotate(180deg); }
/* right는 좌우 반전 */
```

레일 클릭 → 패널과 자기 자신에 `.collapsed` 토글. 패널은 `transform: translateX(-260px)`로 슬라이드 아웃 + `opacity:0` + `pointer-events:none`.

---

## 9. 뷰포트 컨트롤 (하단 중앙)

```css
.viewport-ctrl {
  position: absolute; bottom: 16px; left: 50%; transform: translateX(-50%);
  display: flex; gap: 2px; padding: 4px;
  background: var(--surface); border-radius: 50px;
  box-shadow: var(--shadow-md);
}
.vp-btn  { width:30px; height:30px; border-radius:50%; background:transparent; }
.vp-btn:hover { background: var(--glass-dark); }
.vp-zoom { min-width:56px; text-align:center; font-size:12px; font-family: var(--font-mono); padding: 0 8px; }
.vp-divider { width: 1px; height: 18px; background: var(--border); margin: 0 4px; }
```

내용 (왼→오):

1. `−` Zoom out  → `scale = max(0.1, scale / 1.25)`
2. `100%` zoom 라벨 (모노)
3. `+` Zoom in  → `scale = min(4, scale * 1.25)`
4. divider
5. fit 아이콘 (꺾쇠 4개) → `scale=0.55; tx=0; ty=0`
6. 100% 아이콘 (circle) → `scale=1; tx=0; ty=0`
7. divider
8. history 아이콘 (시계 화살표) — 비동작

줌 변경 시마다 `#zoomLabel`에 `Math.round(scale*100)+'%'` 갱신.

---

## 10. 컨텍스트 메뉴 (우클릭)

### 10.1 스타일

```css
.ctx-menu {
  position: fixed; min-width: 220px;
  background: var(--surface); border-radius: 6px;
  box-shadow: var(--shadow-lg);
  padding: 6px 0; font-size: 12px;
  display: none; z-index: 100;
}
.ctx-menu.open { display: block; }
.ctx-item {
  display: grid; grid-template-columns: 1fr auto;
  padding: 6px 14px 6px 28px;       /* 왼쪽 28px = 체크 마크 공간 */
  cursor: pointer;
}
.ctx-item:hover { background: var(--accent); color: #fff; }
.ctx-item .kbd { font-family: var(--font-mono); font-size: 10px; color: var(--muted); letter-spacing: .4px; }
.ctx-item:hover .kbd { color: rgba(255,255,255,.85); }
.ctx-item.check::before { content: '✓'; position: absolute; left: 10px; }
.ctx-sep { height: 1px; background: var(--border); margin: 4px 0; }
.ctx-section {
  padding: 4px 14px;
  font-family: var(--font-mono); font-size: 9px;
  text-transform: uppercase; letter-spacing: .6px; color: var(--muted);
}
```

### 10.2 항목 순서

```
Copy            ⌘C
Paste here      ⌘V
Duplicate       ⌘D
────
ARRANGE
Bring to front  ]
Send to back    [
────
Group selection ⌘G
Frame selection ⌥⌘G
Hide / Show     ⌘\          (data-action="toggleVis")
Lock / Unlock   ⌘L
────
Add comment
Copy as · PNG
────
Delete          ⌫           (color: #e5484d)
```

### 10.3 동작

- 워크스페이스의 `contextmenu` 이벤트 → `preventDefault()` → `.open` 추가, `left/top`을 마우스 좌표로 설정.
- **뷰포트 밖으로 나가지 않게 보정**: 메뉴 `getBoundingClientRect()` 후 `right > innerWidth`/`bottom > innerHeight` 면 좌상단으로 클램프.
- 메뉴 외부 mousedown → 닫힘.
- `data-action="toggleVis"` 항목 → 현재 `selected` 전체에 `toggleVisible(id)`.

---

## 11. 인터랙션 매트릭스

| 입력 | 결과 |
|---|---|
| 툴 버튼 클릭 | 활성 툴 변경 (단일 active) |
| 타이틀 탭 / 패널 탭 클릭 | 부모 내 active 갱신 |
| 테마 토글 클릭 | `html.dark` 토글 + 아이콘 스왑 |
| 레일 버튼 클릭 | 해당 패널 + 버튼에 `.collapsed` 토글 |
| 캔버스 아이템 클릭 | 단일 선택 (수정자키 시 다중) |
| 빈 캔버스 클릭 | 선택 해제 |
| 트리 행 클릭 | 단일 선택 (수정자키 시 다중) |
| 트리 그룹 caret 클릭 | 펼침/접힘 토글 |
| 트리 lock 아이콘 클릭 | locked 토글 |
| 트리 vis 아이콘 클릭 | hidden 토글 (그룹이면 후손 전파) |
| 우클릭(워크스페이스) | ctx 메뉴 오픈 (좌표 클램프) |
| ctx 외부 클릭 | 메뉴 닫힘 |
| ctx "Hide / Show" | 선택된 모든 아이템 vis 토글 |
| `Space` + 드래그 | 캔버스 팬 |
| `⌘/Ctrl` + 휠 | 마우스 포커스 줌 |
| 휠 단독 | 두 방향 팬 |
| 뷰포트 −/+ | 1.25× / 0.8× 줌 |
| fit 버튼 | `scale=0.55, tx=ty=0` |
| 100% 버튼 | `scale=1, tx=ty=0` |

---

## 12. 접근성 / 포커스

- 모든 인터랙티브에 `:focus-visible { outline: 2px dashed var(--accent); outline-offset: 1px; }` (Figma의 점선 시그너처).
- 아이콘 전용 버튼은 `title` 또는 `aria-label`을 가짐.
- 메뉴는 키보드 단축키 칸을 시각화하지만 실제 키 바인딩은 미구현 (단, `Space`/`⌘+wheel`만 동작).

---

## 13. 파일 구조 및 출력 컨트랙트

- 단일 파일 `index.html` (`<!doctype html>` ~ `</html>`).
- CSS 인라인, 스크립트 1개 인라인. 외부 폰트/이미지 없음.
- 모든 캔버스 컨텐츠는 인라인 SVG 또는 그라데이션. 외부 자산 금지.

---

## 14. 자가 점검 (P0 — 반드시 통과)

- [ ] 뷰포트 어떤 크기든 1920×1080 프레임이 잘리지 않고 들어옴.
- [ ] 라이트/다크 토글 시 모든 토큰이 부드럽게 전환된다.
- [ ] 좌/우 패널 접기 버튼이 패널 슬라이드 + 자체 위치 이동.
- [ ] 트리 그룹 caret로 자식 행이 펼침/접힘.
- [ ] 트리 행 lock/vis 아이콘이 호버 시 나타나고, 토글 후엔 `.on`으로 계속 노출.
- [ ] vis 토글이 그룹일 때 후손 전체에 전파.
- [ ] 트리 선택 ↔ 캔버스 선택이 양방향 동기화.
- [ ] 우클릭 시 ctx 메뉴가 마우스 위치에 열리고 화면 밖으로 나가면 클램프.
- [ ] `Space + 드래그` 팬, `⌘ + 휠` 줌, 휠 패닝 모두 동작.
- [ ] 뷰포트 줌 라벨이 실시간으로 백분율 표시.

---

## 15. 5-Dim 비평 기준

1. **Philosophy** — Figma DS 충실도: 인터페이스는 흑백, 액센트는 청록 1색.
2. **Hierarchy** — 캔버스(중앙) > 패널 > 툴바/타이틀. 한 화면에서 시선이 자연스레 캔버스로 떨어진다.
3. **Execution** — 토큰값 일관, 그림자 3단계, pill/circle 도형 규칙.
4. **Specificity** — 샘플 씬 카피는 실제 브리프 형태 ("Onboarding rewrite — Q2" 등). 더미 텍스트 금지.
5. **Restraint** — 액센트는 선택 윤곽/핸들/CTA에만. 그라데이션은 캔버스 콘텐츠 내부에만.
