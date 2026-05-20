# ADR-0032: Multi-select context menu — batch 액션

- 상태: **Draft** (2026-05-17 신규)
- 관련 ADR: ADR-0017 (chrome — ContextMenu), ADR-0024 (z-index 분리 — 4 z 액션), ADR-0027 (multi-select + alignment), ADR-0028 (Undo/Redo), ADR-0030 (clipboard), ADR-0010 (Group)
- 근거 문서: 본 ADR 이전, `ContextMenu.svelte` 는 *single panel/pane id* 만 input — 다중 선택 (M.size > 1) 상태에서 우 클릭 시 *클릭된 단일 item* 만 대상이 됨. 사용자가 다중 선택 후 batch 액션 (Delete, Hide, Lock, Group, ...) 을 수행할 entry point 가 keyboard shortcut 또는 Inspector 만 — *우 클릭의 자연스러운 mental model 불충족*.

## 결정

### D1. ContextMenu 의 input 확장

ContextMenu 가 받는 input 을 *single item id* + **M (selection set)** 의 *둘 다* 로:

- 클릭 위치의 item 이 *M 안에 있음* → batch mode (M 전체 대상)
- 클릭 위치의 item 이 *M 밖* → M 무시, 클릭된 single item 만 대상 (그리고 M 은 해제 — Figma 컨벤션)
- 클릭 위치가 *빈 area* → empty-area mode (paste / fit-to-view 같은 *non-selection* 액션만)

### D2. Mode 별 액션 매트릭스

| Mode | 액션 |
|---|---|
| **Single-item** (M.size === 1 or empty M + item 클릭) | Copy / Cut / Paste · Z 액션 4 (Bring/Send) · Hide · Lock · Group · Rename · Change terminal · Delete |
| **Multi-item batch** (M.size ≥ 2, clicked-item ∈ M) | Copy (multi) / Cut (multi) / Paste · **Z 액션 4 (batch)** · **Hide all** · **Lock all** · **Group** · **Align (sub-menu)** · **Distribute (sub-menu)** · **Delete all** |
| **Empty area** | Paste · Fit to view · Add (sub-menu — toolbar 와 동일 entries) |

### D3. Per-item 액션의 multi mode 처리

- **Rename**: M.size ≥ 2 시 *hide* (mass rename 의미 모호. P1 의 batch rename 별도 검토).
- **Change terminal** (ADR-0021 D8): M.size ≥ 2 시 *hide* — 다중 terminal item 의 일괄 교체 의도 불명확.
- **Copy pane_id** (ADR-0017 §D2): M.size ≥ 2 시 *hide* — pane_id 다중 복사 의미 모호. *Copy* (item) 와 mental 충돌 회피.

### D4. Z 액션의 batch 동작 (ADR-0024 정합)

ADR-0024 의 4 액션 (Bring to front / Send to back / Bring forward / Send backward) 은 이미 *M-aware* 로 정의되어 있음 (`zStore.bringForward` 등). 본 ADR 은 *ContextMenu entry 노출* 의 결정만 — 동작은 ADR-0024 그대로.

### D5. Group / Ungroup 진입 (ADR-0010 정합)

- **Group**: M.size ≥ 2 (또는 ≥ 1 + parent_id 동일 그룹화) 시 ContextMenu 의 `[Group]` 액션 노출. ADR-0010 D4 의 `Group` 액션 발동.
- **Ungroup**: M 의 single member 가 *Group* type 일 때 `[Ungroup]` 노출 (ADR-0010 D12).

### D6. Align / Distribute (ADR-0027 정합)

ADR-0027 의 multi-select alignment 액션 — Inspector 안에서만 존재. 본 ADR 이 **ContextMenu 의 sub-menu** 로 second entry 제공:

- `[Align ▸]` — Left / Center / Right / Top / Middle / Bottom
- `[Distribute ▸]` — Horizontally / Vertically

M.size ≥ 2 시만 노출. M.size === 2 면 distribute 의 의미 약함 → enable but no-op visually.

### D7. Trigger 패턴

- **Canvas right-click**: `Canvas.svelte` 의 `oncontextmenu` 가 좌표 + clicked-item (또는 null = 빈 area) 을 `contextMenuRef.openAt(...)` 에 전달.
- **Panel/Note header right-click**: PanelNode header 의 `(…)` more button + native right-click 양쪽 모두 같은 ContextMenu 호출. ADR-0017 §D2 의 정합.
- **Layer tree row right-click** (P1): LayerTreeView 의 row right-click 도 같은 ContextMenu 진입 — sidebar 에서 batch 액션 — P1.

### D8. Undo/Redo (ADR-0028 정합)

- batch 액션 (Hide all / Lock all / Delete all / Z batch / Align batch) 은 **단일 `applyMutation` call** 로 표현 → historyStore 가 1 entry capture.
- 사용자는 Cmd+Z 한 번으로 batch 액션 전체 되돌리기.

### D9. M (selection) 의 *click-to-replace* 정합

D1 의 "클릭 위치의 item 이 M 밖 → M 무시" 는 *Figma 컨벤션*. 본 결정의 부작용: 사용자가 *우 클릭 자체를 selection 변경 의도로 활용* 가능 — 좌 클릭 selection 과 정합.

- 클릭된 item 이 M ∈ : M 유지
- 클릭된 item 이 M ∉ : M = {clicked-item} 으로 *replace*

이 동작은 ADR-0027 의 multi-select 좌 클릭 변형 (Cmd/Shift toggle) 과 직교 — 우 클릭은 *항상 single-replace 또는 batch* 둘 중 하나.

## 비채택 대안

- **다중 선택 후 우 클릭 = M 보존 + 클릭된 item 만 대상** (M.size > 1 인데 클릭된 single item 만 작동) — 사용자 의도 mismatch. 거부.
- **다중 선택 후 우 클릭 → 별도 "Multi" 메뉴 root** — sub-menu 분리. 액션 위치 학습 비용. 거부 — 본 ADR 의 평면 매트릭스 (D2) 가 일관.
- **우 클릭 자체로 M 변경 안 함 (M 외 클릭 시 우 클릭 무시)** — 사용자가 우 클릭 한 item 의 액션을 *기대* 한 흐름과 mismatch. 거부.

## 미해결

- **O1.** Empty area 의 `[Add ▸]` sub-menu — toolbar 와 액션 중복. Sub-menu 위치 결정 (toolbar 가 항상 보이므로 ContextMenu 항목 중복은 redundant). P1 의 user research.
- **O2.** Locked item batch 처리 — `[Delete all]` 시 locked 자손 처리는 ADR-0010 D10 의 group close 정합 (Cancel / Delete unlocked only / Delete all force) 패턴 차용? — P1.
- **O3.** Touch 환경의 long-press = 우 클릭 equivalent — mobile P2.

## 변경 이력

- 2026-05-17: 신규 draft. ADR-0017 §D2 의 ContextMenu spec 을 batch 시나리오로 확장.
