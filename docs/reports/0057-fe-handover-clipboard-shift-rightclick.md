# Handover (FE) — Clipboard + Shift constraint + Multi-select context menu

> Cold-pickup 가능한 FE agent 인계 문서. 본 batch 의 3 ADR (0030/0031/0032) 의 FE 작업만 정리.
>
> - 생성일: 2026-05-17
> - 대상 agent: FE (Svelte 5 / SvelteFlow / xterm)
> - 동반 BE handover: `docs/reports/0058-be-handover-clipboard-shift-rightclick.md` (BE 작업 minimal — 본 batch 의 거의 모두 FE-only)

---

## 1. Scope (3 ADR)

| ADR | 기능 | FE 양 |
|---|---|---|
| ADR-0030 | Canvas item clipboard (copy/cut/paste) | **High** — store, keymap, ContextMenu, paste 좌표 logic, terminal clone integration |
| ADR-0031 | Figure Shift constraint (rect/ellipse 1:1, line angle hold) | Medium — drawing + NodeResizer 양쪽 modifier wire |
| ADR-0032 | Multi-select context menu | Medium — ContextMenu 의 mode 분기 + batch action wire |

세 ADR 은 ContextMenu 변경에서 *교차* — 같은 PR 또는 인접 PR 으로 land 권장.

## 2. ADR-0030 (Clipboard) — FE 작업

### 2.1 Store

신규 `codebase/frontend/src/lib/stores/clipboardStore.svelte.ts`:

```ts
import type { CanvasItem } from '$lib/types/canvas';

class ClipboardStore {
  /** Page lifetime 안 유지. 새로고침 시 손실 (D1 정합). */
  items = $state<CanvasItem[]>([]);
  /** Cut 후 paste 시 누적 offset 카운트 — 연속 paste 의 (24,24) 누적 (D4). */
  pasteCount = $state(0);

  copy(items: CanvasItem[]): void { this.items = items; this.pasteCount = 0; }
  cut(items: CanvasItem[]): void { this.copy(items); /* delete 는 caller */ }
  clear(): void { this.items = []; this.pasteCount = 0; }
  get hasContent(): boolean { return this.items.length > 0; }
}
export const clipboardStore = new ClipboardStore();
```

### 2.2 Keymap (`bindClipboardShortcuts`)

신규 `codebase/frontend/src/lib/keyboard/clipboardShortcuts.svelte.ts`:

- Cmd/Ctrl+C / X / V 등록 — `bindZShortcuts` / `bindChromeShortcuts` 의 패턴 따라.
- Focus 분기 (ADR-0030 D7): `document.activeElement` 가 `.xterm-helper-textarea` 이면 차단 (xterm 의 native paste 로 routing).
- canvas focus: `.svelte-flow__pane` 또는 selected panel-wrapper.

`+page.svelte` 의 `onMount` 에 `unbindClipboard = bindClipboardShortcuts()` 추가.

### 2.3 Copy / Cut / Paste action 함수

`codebase/frontend/src/lib/canvas/clipboardActions.ts` (신규):

```ts
export function copySelection(): void {
  const items = sessionStore.M.size === 0 ? [] : [...sessionStore.M].map(id => sessionStore.items.get(id)).filter(Boolean);
  clipboardStore.copy(items);
}

export async function cutSelection(): Promise<void> {
  copySelection();
  // delete via existing applyMutation path — ADR-0028 history 1 entry
  const ids = clipboardStore.items.map(it => it.id);
  await sessionStore.applyMutation((cur) => ({ ...cur, items: cur.items.filter(it => !ids.includes(it.id)) }), {...});
}

export async function paste(anchor?: { x: number, y: number }): Promise<void> {
  if (!clipboardStore.hasContent) return;
  clipboardStore.pasteCount += 1;
  const off = clipboardStore.pasteCount * 24;
  // bounding-box top-left (D4)
  const minX = Math.min(...clipboardStore.items.map(it => it.x));
  const minY = Math.min(...clipboardStore.items.map(it => it.y));
  const fresh = clipboardStore.items.map(it => ({
    ...it,
    id: crypto.randomUUID(),
    x: (anchor ? anchor.x : minX + off),
    y: (anchor ? anchor.y : minY + off),
    // terminal: ADR-0030 D3 clone — 새 spawn (POST /api/terminals)
  }));
  // terminal item 의 경우 BE POST + binding (D3 / §2.4)
  // applyMutation 으로 commit — ADR-0028 history 자동 capture
  await sessionStore.applyMutation((cur) => ({ ...cur, items: [...cur.items, ...fresh] }), {...});
}
```

### 2.4 Terminal item clone 통합 (ADR-0030 D3)

paste 시 `item.type === 'terminal'` 인 경우 BE `POST /api/terminals` 호출 → 새 `terminal_id` 받음 → 새 item 의 `terminal_id` 에 binding. 기존 `terminalPool.spawn(...)` (있다면) 또는 직접 `http/terminals.ts` 의 spawn endpoint 사용.

**의존성**: BE handover §2.1 의 `POST /api/terminals` 가 이미 존재하는지 확인. 없다면 BE-Slice 선행.

### 2.5 ContextMenu entry 추가 (ADR-0030 D10)

`ContextMenu.svelte` 의 액션 list 에 신규 5 entries (ADR-0032 §D2 매트릭스 정합):
- `[Copy]` / `[Cut]` / `[Paste]` (top)
- 기존 Z 액션 4 위에 위치

### 2.6 Undo/Redo

paste/cut 모두 `applyMutation` 통과 → ADR-0028 historyStore 자동 capture. *추가 작업 없음*. 단 copy 자체는 layout mutation X — history entry 없음.

## 3. ADR-0031 (Shift constraint) — FE 작업

### 3.1 Tool drawing 단계 (`Canvas.svelte`)

`onpanepointerdown` 또는 tool-specific drawing helper 에서:

```ts
const { shiftKey, altKey } = event;
// rect/ellipse: shiftKey 면 width = height (큰 axis 기준)
// line: shiftKey 면 holding angle (D2) — Shift 누른 시점 의 (dx0, dy0) 캡처, 이후 drag 좌표를 그 ray 위 projection
```

drawing 의 holding angle (line):
```ts
let holdAngle: number | null = null;
function onDrawMove(e: PointerEvent) {
  if (e.shiftKey) {
    if (holdAngle === null) {
      holdAngle = Math.atan2(e.clientY - startY, e.clientX - startX);
    }
    const dist = Math.hypot(e.clientX - startX, e.clientY - startY);
    endX = startX + dist * Math.cos(holdAngle);
    endY = startY + dist * Math.sin(holdAngle);
  } else {
    holdAngle = null;  // shift 떼면 재시작 가능
    endX = e.clientX;
    endY = e.clientY;
  }
}
```

### 3.2 NodeResizer wrap (D7 의 옵션 a)

`PanelNode`, `NoteNode`, `ShapeNode`, `ImageNode` 등 NodeResizer 사용처 마다 `onResize` callback wrap:

```svelte
<NodeResizer
  onResize={(e, params) => {
    const evt = e as PointerEvent;
    if (evt.shiftKey && (data.type === 'rect' || data.type === 'ellipse')) {
      const s = Math.max(params.width, params.height);
      params.width = s;
      params.height = s;
    }
    if (evt.shiftKey && data.type === 'line') {
      // line 의 NodeResizer 는 endpoint 의 angle hold — line node 의 custom resize
    }
    // ... onResizeEnd 호출
  }}
/>
```

SvelteFlow `NodeResizer` 의 정확한 callback signature 는 `@xyflow/svelte` 의 type 참고. event 의 `shiftKey` 접근이 가능하지 않으면 `window.addEventListener('pointermove', ...)` 의 별도 hook 필요.

### 3.3 Image (G39) 의 기존 spec 통합

plan-0007 §14.20.6.5 의 image aspect lock 코드가 *이미 land 되어 있다면* 본 ADR-0031 의 generalized 패턴으로 *refactor*. 없다면 신규 + 동일 패턴.

### 3.4 BE / schema 영향

**없음** — modifier 는 좌표 변환 단계만. 최종 commit schema 는 일반 좌표.

## 4. ADR-0032 (Multi-select context menu) — FE 작업

### 4.1 `ContextMenu.svelte` 의 props 확장

현재 `openAt({ clientX, clientY, paneId, panelId })` → 다음 input 도 받음:

```ts
openAt({
  clientX, clientY,
  clickedItemId: string | null,    // null = empty area
  // M 은 sessionStore.M 에서 read — props 로 전달 불필요
})
```

### 4.2 Mode 분기 (ADR-0032 D1)

```ts
const mode = $derived.by((): 'single' | 'multi' | 'empty' => {
  if (clickedItemId === null) return 'empty';
  if (sessionStore.M.size > 1 && sessionStore.M.has(clickedItemId)) return 'multi';
  // M 외 click → M replace + single mode (D9)
  if (!sessionStore.M.has(clickedItemId)) {
    sessionStore.setM(new Set([clickedItemId]));
    return 'single';
  }
  return 'single';
});
```

### 4.3 Mode 별 entry 렌더 (ADR-0032 D2 매트릭스)

ContextMenu 의 `{#each entries as e}` 마다 `e.visibleInMode: ('single' | 'multi' | 'empty')[]` 체크. 또는 mode 분기 `{#if mode === 'single'}` 블록.

### 4.4 Batch action wire

각 액션 (Hide all / Lock all / Delete all / Z batch / Align batch / Distribute batch) 은 *기존 single-item action 의 M 전체 iterate 변형*. 모두 `sessionStore.applyMutation` 단일 호출 — ADR-0028 정합.

### 4.5 Align / Distribute sub-menu (ADR-0032 D6)

ADR-0027 의 alignment 액션을 Inspector 외에 ContextMenu sub-menu 로 second entry 제공. action 함수 자체는 *재사용* — Inspector callsite 와 동일 함수 호출.

## 5. Coupling / 의존성

- ContextMenu 변경은 **세 ADR 모두 공유** — 한 PR 또는 series 의 첫 PR 에서 ContextMenu 의 generic refactor (mode + entry 매트릭스) 진행, 이후 각 ADR 의 액션 wire.
- ADR-0030 의 terminal clone (§2.4) 만 BE 의존 — 다른 모두는 FE-only.
- ADR-0031 은 schema 변경 없어 *isolated*. 가장 빠르게 land 가능.

## 6. 권장 진행 순서 (FE)

1. **Phase 1 — ADR-0031 (Shift constraint)** — schema/BE 영향 0, 시각 효과 즉시. tool drawing + NodeResizer 두 callsite.
2. **Phase 2 — ContextMenu generic refactor (ADR-0032 의 D1/D2 frame)** — mode 분기 frame 만, 액션은 기존 그대로. 회귀 방지 baseline.
3. **Phase 3 — ADR-0030 (Clipboard) Slice A: non-terminal item only** — store + keymap + paste 좌표. terminal item 은 disable (paste 시 skip 또는 toast).
4. **Phase 4 — ADR-0030 Slice B: terminal clone** — BE handover §2.1 의 endpoint 가 land 후. POST /api/terminals 의 새 spawn + binding.
5. **Phase 5 — ADR-0032 batch actions (D2 매트릭스 완성)** — Hide/Lock/Delete/Z/Group/Align/Distribute 의 batch wire.

## 7. Test plan

- **Phase 1**: Storybook 또는 manual — rect/ellipse drag 시 Shift 누른 채 1:1 유지 / line drag 시 holding angle / NodeResizer corner drag 시 Shift aspect lock.
- **Phase 3+**: Cmd+C/X/V 의 focus 분기 (canvas vs xterm), paste 좌표 offset, multi-paste 누적 offset, Undo 의 cut 복귀.
- **Phase 5**: 다중 선택 후 우 클릭 → batch hide / lock / delete 의 Undo 정합 (1 history entry).

## 8. 변경 영향 파일 (예상)

| ADR | 파일 |
|---|---|
| 0030 | `lib/stores/clipboardStore.svelte.ts` (신규) / `lib/keyboard/clipboardShortcuts.svelte.ts` (신규) / `lib/canvas/clipboardActions.ts` (신규) / `routes/+page.svelte` (bind/unbind) / `lib/chrome/ContextMenu.svelte` (entry) |
| 0031 | `lib/canvas/Canvas.svelte` (drawing) / `lib/canvas/{Panel,Note,Shape,Image}Node.svelte` (NodeResizer onResize wrap) |
| 0032 | `lib/chrome/ContextMenu.svelte` (mode + matrix) / `lib/canvas/Canvas.svelte` (oncontextmenu 의 clickedItemId 결정) |

## 9. Open questions (FE 결정 사항)

- **Q1.** Terminal paste 의 Slice B 시점 — Phase 4 land 전엔 disable vs warning toast vs note 로 fallback. *권장: disable + toast "Terminal copy/paste is not yet supported"*.
- **Q2.** Line 의 NodeResizer wrap — endpoint drag 의 *angle hold* 가 NodeResizer 외 별도 line endpoint handle 일 가능성. LineNode 의 현 구현 확인 후 결정.
- **Q3.** Empty area 의 `[Add ▸]` sub-menu (ADR-0032 O1) — toolbar redundant. 본 Phase 에서 *생략*, P1 결정 후 추가.
