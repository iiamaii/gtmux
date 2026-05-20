# ADR-0031: Figure 입력 modifier-key constraint (Shift / Alt)

- 상태: **Draft** (2026-05-17 신규)
- 관련 ADR: ADR-0018 (canvas-item-data-model — shape types), ADR-0027 (multi-select + alignment)
- 관련 plan: plan-0007 §14.20.6.5 (image G39 의 NodeResizer + Shift+drag = aspect ratio lock — *부분 spec*)
- 근거 문서: 본 ADR 이전, image (G39) 만 Shift+drag aspect lock spec 존재. rect / ellipse / line 등 다른 figure 의 modifier 동작은 미정의 — 사용자가 정확한 정사각형, 정원, 수평/수직 line 그리기에 *번거로움*.

## 용어 (정본)

- **"Shift constraint"** / **"modifier-key constraint"** — modifier 키를 누른 상태에서 *입력 자유도 축소* 의 일반 용어.
- **"aspect ratio lock"** (= uniform scaling) — width = height 비율 강제 (rect/ellipse 의 1:1).
- **"angle snap"** / **"axis lock"** — line 의 각도 제약 (특정 각도 들로 snap 또는 holding angle 보존).
- Figma / Sketch / Adobe Illustrator 공통 컨벤션 — "Hold Shift to constrain proportions / axis".

## 결정

### D1. 적용 단계 (drawing + resize 양쪽)

modifier-key constraint 는 *두 단계* 모두에 적용:

- **(a) Tool drawing**: Toolbar 에서 figure tool (rect / ellipse / line) 선택 후 canvas 에 mouse drag 로 생성하는 단계.
- **(b) Item resize**: 이미 mount 된 figure 의 NodeResizer corner/edge handle drag 단계.

(a) 와 (b) 의 modifier 동작은 *동일* (예측 가능성).

### D2. Shift modifier 동작 (per type)

| Type | Shift 효과 |
|---|---|
| `rect` | **Aspect ratio 1:1 lock** — drag 의 dominant axis 기준 동일 size (정사각형). |
| `ellipse` | **Aspect ratio 1:1 lock** — width = height (정원). |
| `line` | **Angle snap** — *holding angle* 보존 = Shift 누른 시점의 각도 기억, 이후 drag 좌표를 그 각도의 ray 위로 projection. *15° increment snap* 은 본 ADR 비채택 (D5). |
| `image` (G39) | 기존 plan-0007 §14.20.6.5 의 aspect ratio lock 그대로 — image 의 source aspect 보존. **본 ADR 이 통합**. |
| `panel` / `note` / `text` / `file_path` / `caption` / `document` | Resize 에 1:1 lock 적용 불가 (의도 모호) — *no-op*. 본 ADR 은 figure (shape + image) 에 한정. |

**Line 의 *holding angle* 근거**: 사용자 명시 의도 ("shift 누른 시점 각도 유지"). Figma 의 15° snap 보다 직관적 — 사용자가 *원하는 각도로* 시작한 후 그 각도 유지하며 길이만 조절. Drawing 단계에서 angle 은 *drag 의 시작 시점부터* 자연스럽게 결정되므로 holding 가 자연.

### D3. Alt modifier 동작 (P1)

- **Center-anchored resize** — corner/edge handle 의 반대편 anchor 가 *opposite corner* 가 아닌 *center*. 양쪽 동시 확장.
- Drawing 단계: 클릭 시작점이 center 가 되고 drag 가 *반대편 corner* 까지의 거리. mouse 가 center 기준 대칭으로 확장.
- 본 ADR 의 P0 scope 외. 결정만 잠그고 구현 P1.

### D4. Shift + Alt 조합 (P1)

- Center-anchored + aspect ratio 1:1 (또는 line angle snap).
- 두 modifier 의 *결합 적용*. P1.

### D5. 비채택 — Line 의 15° increment snap

Figma 컨벤션은 line 의 Shift = 15° 의 배수 (0, 15, 30, 45, 60, 75, 90...) 로 snap. 본 ADR 은 *holding angle* (D2) 으로 결정 — 사용자 의도 우선. 두 패턴 모두 valid 하나 mixed 사용은 혼란.

**대안 — User preference 로 toggle** (Settings.behavior.line_shift_mode = "hold" | "snap_15") — P1 검토.

### D6. UI feedback

- Drawing 단계에서 Shift 누른 *순간* 의 cursor 변경: rect/ellipse 는 그대로, line 은 그 시점의 각도를 시각화 (extension ray 표시 — P1).
- Resize 단계에서 Shift 누른 *순간* 의 visual cue: 현재 ratio 가 aspect-lock 적용 후 다른 size 라면 즉시 snap (이미 reflow 자체가 visual cue).
- Toast / hint 라벨 노출 안 함 — modifier-key UX 는 *조용함* 이 표준.

### D7. 구현 위치 (FE)

- **Tool drawing**: `Canvas.svelte` 의 `onpanepointerdown` 흐름 + 각 tool 의 drawing helper. event 의 `shiftKey` 검사 → constrain logic 적용.
- **NodeResizer wrap**: SvelteFlow 의 `<NodeResizer>` 가 self-contained — Shift 처리를 외부 hook 으로 inject 어려움. 두 옵션:
  - (a) `<NodeResizer>` 의 onResize callback 안에서 `event.shiftKey` 확인 + width/height adjust + `event.preventDefault` 또는 controlled re-set.
  - (b) NodeResizer 대체 자체 corner handle 구현.
  - **default = (a)** — overhead 최소.

### D8. Schema / BE 영향

**없음**. modifier 는 *입력 단계의 좌표 변환* — 최종 commit 되는 schema 좌표는 일반 좌표와 동일 (1:1 사각형도 그냥 w = h 인 rect). BE 변경 0.

## 비채택 대안

- **Shift 가 line 의 perpendicular drawing** (0/90/180/270 snap 만): D5 의 hybrid 변형. *너무 제약* — 사용자 의도와 mismatch.
- **Modifier 없이 자동 1:1 snap** (예: 일정 ratio 근처면 auto-lock): 의도 결정의 명시성 손실. 거부.
- **Modifier 가 *반대* 의미 (Shift = free, default = constrained)**: 일반 컨벤션 (Figma/Adobe) 위반. 거부.

## 미해결

- **O1.** P1 의 Alt center-anchor 동작과 ADR-0027 의 alignment / distribute 액션 의 시너지 — center-anchor resize 후 distribute 가 같은 의미 인지.
- **O2.** Touch device 에서 Shift 대안 (long-press, two-finger 등) — 본 ADR 의 P0 scope 외, mobile P2.

## 변경 이력

- 2026-05-17: 신규 draft. plan-0007 §14.20.6.5 의 image G39 spec 을 본 ADR 로 통합 + figure 전체로 generalize.
