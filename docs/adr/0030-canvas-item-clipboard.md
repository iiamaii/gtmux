# ADR-0030: Canvas item clipboard — copy / cut / paste

- 상태: **Accepted** (2026-05-19 — Draft (2026-05-17) → Accepted promote, wire 진입 prerequisite)
- 관련 ADR: ADR-0018 (canvas-item-data-model), ADR-0021 (terminal pool + mirror), ADR-0024 (z-index 분리), ADR-0028 (Undo/Redo), ADR-0017 (chrome — ContextMenu)
- 관련 plan: plan-0007 §14 (FE-NEW-1 + keyboard shortcut)
- 근거 문서: 본 ADR 이전, ContextMenu 에는 *"Copy pane_id"* 텍스트 복사만 존재 (ADR-0017 §D2). canvas item 자체의 clipboard (item duplicate, paste, cut) 는 미설계 — 사용자가 잘못 만든 item 을 재현할 때 처음부터 다시 만들어야 하는 UX 결함.

## 결정

### D1. Clipboard scope = FE in-memory only

브라우저 `navigator.clipboard` API **미사용** — 다음 이유:
- canvas item 의 serialized 표현은 schema-specific JSON (ADR-0018) 으로, 다른 앱과의 교차 paste 의미 없음.
- 보안 (ADR-0003) — clipboard API 가 권한 모달 트리거.
- terminal item 의 server-side reference (`terminalPool`) 는 brower clipboard 로 전달 불가.

→ **`clipboardStore.svelte.ts`** 신규 (FE-only, page lifetime 안 유지). 새로고침 시 손실 — Figma / Sketch 도 동일.

### D2. 대상 item types

ADR-0018 의 모든 `CanvasItem` variant 가 대상. type 별 정책:

| Type | Copy/Cut | Paste 동작 |
|---|---|---|
| `note` | ○ | 새 UUID + 좌표 offset + 내용 복제 |
| `shape` (rect/ellipse/line) | ○ | 새 UUID + 좌표 offset + style 복제 |
| `text` | ○ | 새 UUID + 좌표 offset + 내용 복제 |
| `file_path` | ○ | 새 UUID + 좌표 offset + path 복제 |
| `image` (G39) | ○ | 새 UUID + 좌표 offset + asset reference 복제 |
| `caption` / `document` (plan-0011) | ○ | 동일 |
| **`terminal`** | △ | **D3 — 새 spawn (clone) default. mirror attach 는 P1** |

### D3. Terminal item paste 정책

Terminal 은 server-pool 자원 (ADR-0021). FE clipboard 가 "복제" 의도를 가진 두 모드 중 default:

- **(a) Clone (default)**: paste 시 BE `POST /api/terminals` 로 *새 terminal* spawn. clipboard 의 원본 terminal id 는 *visual seed* 만, 새 item 은 새 terminal 과 binding.
- **(b) Mirror (P1)**: 같은 terminal_id 의 *추가 attach* — 두 panel 이 동일 stream 미러링. P1 별도 ADR (ADR-0021 D7 의 mirror 정책 확장).

본 ADR 의 default 행위 = (a). UI 노출 = ContextMenu `[Paste]` (clone) + P1 의 `[Paste as mirror]` 분리 entry.

**근거**: clipboard 의 사용자 mental model 은 *"독립 사본"*. mirror 는 *"같은 것의 다른 보기"* — 의미 차이 명시. 사용자가 mirror 의도 시 ADR-0024 의 "Duplicate as mirror" entry 사용.

### D4. Paste 좌표

- **단일 paste**: 원본 좌표 + (24, 24) offset (Figma 컨벤션).
- **다중 paste**: 다중 선택 copy 의 *bounding-box 의 top-left* 를 anchor 로 (24, 24) offset, 다른 item 들은 *상대 위치 보존*.
- **연속 paste**: 같은 clipboard 를 N 회 paste 시 매 회 (24, 24) 누적 offset.
- 좌표가 viewport 밖이면 그대로 진행 (사용자가 ViewportCtrl 의 focus 로 이동 가능).

### D5. Cut 동작

- Cut = Copy + Delete. clipboard 에 보관 후 원본 즉시 제거.
- ADR-0028 정합 — Delete 와 동일하게 `applyMutation` 의 single history entry 로 capture. **Undo 시 cut 된 item 복귀 + clipboard 는 변경 없음** (Figma 와 동일).
- Cut 후 paste 의 시각 효과: 같은 좌표 (원본 위치) — offset 없이 paste 도 가능하지만 단순화를 위해 *항상 (24, 24) offset 적용* (Figma 표준).

### D6. ID 생성

paste 시 모든 item 의 `id` 는 새 UUID. terminal item 의 경우 `terminal_id` 도 새로 spawn 된 server-side id 로 binding.

### D7. 단축키 (ADR-0017 §D6 amend)

| 단축키 | 액션 | 조건 |
|---|---|---|
| `Cmd/Ctrl+C` | Copy | canvas focus + M.size ≥ 1 |
| `Cmd/Ctrl+X` | Cut | canvas focus + M.size ≥ 1 (locked item 은 제외) |
| `Cmd/Ctrl+V` | Paste | canvas focus + clipboard 비어있지 않음 |

**Focus 분기**: xterm focus 안에서는 *terminal 의 paste* 로 routing (xterm v6 의 default OS clipboard). 즉 `document.activeElement` 가 xterm 의 helper-textarea 면 단축키 차단. canvas focus = `.svelte-flow__pane` 또는 selected panel-wrapper.

기존 keyboard shortcut 시스템 (`bindZShortcuts`, `bindChromeShortcuts`) 의 패턴 따라 `bindClipboardShortcuts` 별도 wire.

### D8. Multi-clipboard semantics

다중 선택 (M.size > 1) 후 copy = 한 clipboard 안에 array 보관. paste 는 array 일괄 출력 (D4 의 bounding-box anchor).

### D9. Undo/Redo (ADR-0028 정합)

- Paste / Cut / Copy 모두 `sessionStore.applyMutation` 단일 entry 통과 — historyStore 가 자동 capture.
- Copy 자체는 layout mutation X (clipboard 만 변경) — history entry 없음.
- Cut = Delete mutation 1 회 + clipboard 변경. Undo 는 Delete 만 되돌림.

### D10. ContextMenu 진입 (ADR-0032 정합)

ContextMenu 에 신규 액션:
- `[Copy]` — selection 비어있지 않을 때
- `[Cut]` — selection 비어있지 않을 때 (모두 locked 면 비활성)
- `[Paste]` — clipboard 비어있지 않을 때. canvas 빈 영역 right-click 시도 노출
- `[Paste here]` (P1) — right-click 좌표를 paste anchor 로 사용

ADR-0024 의 4 z 액션 위에 신규 5 액션 (Copy/Cut/Paste 가 가장 위 위치).

### D11. Duplicate (Cmd/Ctrl+D) — 2026-05-19 amend

Clipboard 우회 1-step shortcut — selection 의 in-place clone.

- 동작 = paste 와 동일 절차 (D4 의 bbox + (24, 24) offset, D6 의 새 UUID, D3 의 terminal clone-spawn). 단 paste offset 의 `pasteCount` 누적과는 *독립* — Duplicate 는 매 호출마다 고정 (24, 24).
- **Clipboard 미오염** — `clipboardStore.entries` / `pasteCount` 둘 다 변경 0. 사용자가 직전 copy 한 내용은 그대로 다음 Cmd+V 의 source.
- 선택은 새 item 으로 교체 (Figma 패턴, ADR-0030 D4 multi-paste 의 selection 정합).
- ADR-0030 D5 정합 — locked item 은 source 에서 제외.
- ADR-0030 D9 정합 — 단일 `applyMutation` PUT = 1 history entry.

근거: Cmd+C → Cmd+V 두 단계의 1-key 단축. Figma / Sketch / Miro 의 default. clipboard 가 매번 갱신되는 Sketch 모드는 *직전 copy* 를 망가뜨려 사용자 mental model 과 mismatch — Figma 의 분리 모드 채택.

UI 노출 = ADR-0017 D6 amend ⑧ 매트릭스. ContextMenu 의 별 `[Duplicate]` entry 는 P1 (현 `[Copy]` + `[Paste]` 조합으로 동등).

## 비채택 대안

- **Browser Clipboard API** — schema mismatch + 권한 모달. 거부.
- **OS-level drag-and-drop 으로 paste** — drag 의 mental model 은 "옮기기" 가 강함. 거부.
- **Cut 시 clipboard 가 비워지면 원본 제거 안 함 (Figma 일부 변형)** — undo 정합 복잡. 거부 — 항상 Delete 동작.
- **Mirror 를 default paste 로** — terminal 의 sharing 의미는 *비일반적* — 사용자 명시 의도일 때만. (D3 근거 참고)

## 미해결

- **O1.** Terminal mirror paste (P1) 의 ADR-0021 D7 정합 — `attached_sessions` 의 multi-session 의미 확장 필요.
- **O2.** Paste anchor 좌표 = *마우스 위치* vs *bounding-box (24, 24)* — D4 의 default 는 후자, but right-click context 에서 paste 시 *마우스 위치* 를 anchor 로 하는 게 자연스러움. P1 의 `[Paste here]` 항목 spec 보완.
- **O3.** 잘라낸 (cut) item 의 *원본 z-index* 보존 여부 — 본 ADR 은 paste 시 새 z = max(z) + 1. cut 의 *원래 z* 정합은 추후.

## 변경 이력

- 2026-05-17: 신규 draft. ADR-0028 (Undo/Redo) Phase 3 land 후 follow-up.
- 2026-05-19: Promote Draft → Accepted. Spec 본문 변경 없음 — D1~D10 그대로. ADR-0017 D6 amend ⑤ (D6 amend ⑦ 의 ⑦번) 의 (b) Copy/Cut/Paste cross-link 가 본 ADR 의 spec 을 정본으로 지목하므로, wire 진입 (clipboardStore + bindClipboardShortcuts + ContextMenu [Copy]/[Cut]/[Paste] entry + +page.svelte bind) 의 ADR-before-code rule 정합을 위해 promote. 미해결 O1/O2/O3 그대로 유지.
- 2026-05-19 ②: D11 신규 — Duplicate (Cmd/Ctrl+D). Clipboard 미오염 1-step in-place clone. ADR-0017 D6 amend ⑧ 매트릭스 와 짝. paste 와 동일 동작 (D4/D6/D3 재활용) + clipboard state 변경 0.
