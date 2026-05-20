# 0050 — Lasso / Selection sync 회귀 시나리오 정리

- 작성일: 2026-05-16
- 종류: regression test plan / manual reproduce playbook
- 관련 ADR: ADR-0024 (Layer tree + z-index separation), ADR-0018 (Canvas item data model v2)
- 관련 report: `0045-refresh-session-reconnect-loop-analysis.md` (effect_update_depth 분석), `0048-fe-refresh-validation-checklist.md`, `0049-session-handover-ui-ux-and-auth-pivot.md` §7 (known issue)
- 관련 commit: `682b584` (UI/UX batch 3 — shift-free lasso), `da7663b` (P0 후속 — effect_update_depth 회피), `5117ef9` (0045 정합)
- 우선: known issue 의 *명문화* — 회귀 발생 시 본 문서로 진단 → 시각/loop 회귀 trigger 판별

---

## 0. 사용 시점

- canvas selection 동작이 의심스러울 때 (lasso 가 빈 set 으로 떨어짐 / Layer click 이 canvas 에 반영 안 됨 / 다중 선택이 일부만 drag commit / `effect_update_depth_exceeded` 재현 등)
- `selectionOnDrag` / `onselectionchange` / `sessionStore.M` / `flowNodes` 의 한 곳을 수정한 PR 의 회귀 검증
- 새 *Node type 추가* / itemToNode 의 signature 필드 변경 / `bind:` 패턴 도입 시 회귀 사전 확인

---

## 1. 현재 selection sync pipeline (정본 mechanism)

### 1.1 진실의 소스

- **`sessionStore.M: Set<string>`** = canvas selection 의 *유일한* 진실. Layer panel, Inspector, RightPanel 등 모든 consumer 가 본 store 로부터 derive.
- SvelteFlow 의 internal `nodes.selected` 는 *복제 view* — `flowNodes` derived 가 매 pass 마다 `M.has(id)` 로 prop 으로 carry.

### 1.2 입력 path (외부 → store M)

| Path | 진입점 | 호출 |
|---|---|---|
| Lasso drag (left-drag 영역) | SvelteFlow internal selection box → `onselectionchange({ nodes })` | `Canvas.svelte:817` → `sessionStore.setM(ids)` |
| 단일 click on node | `onnodeclick` | `sessionStore.setM([id])` (수정자 키 X) |
| Cmd/Ctrl click on node | `onnodeclick` 의 modifier 분기 | `sessionStore.toggleM(id)` |
| Pane click on empty area | `onpaneclick` (select tool 분기) | `sessionStore.clearM()` |
| Layer panel click | `LayerTreeView.svelte:301/307/320` | `sessionStore.toggleM(id)` |
| Context menu / shortcut | 각 진입점 | `addToM` / `removeFromM` / `setM` |

### 1.3 출력 path (store M → 시각)

- `Canvas.svelte:528 flowNodes = $derived.by(() => …)` 가 매 reactive pass 마다 items + M + groupsById 로 재계산.
- id-cache + signature (`makeSignature` line 421-429) 가 *prop identity 안정* 보장 — `selected` 가 signature 의 한 component 라 `M.has(id)` 변화 시 새 node object 생성 (cache miss), 변화 없으면 기존 reference 재사용 (cache hit). SvelteFlow 가 prop unchanged 로 판단 → 내부 측정 effect 가 무한 트리거되지 않음.
- `<SvelteFlow nodes={flowNodes} ... />` 는 **one-way** — `bind:nodes` 폐기 상태 (0045 P0-A 의 회피).

### 1.4 무한 cycle 차단 지점

- `Canvas.svelte:821-830` 의 fast no-op — `onselectionchange` 가 fire 됐을 때 *현재 store M 과 동일한 set* 이면 setM skip. drag 중 매 frame fire 되므로 cycle 차단 필수.
- 차단 조건: `ids.length === M.size` AND 모든 `id ∈ M`. ordering 무관 (Set 비교).

---

## 2. 회귀 위험 분류

### A. `selected` prop 이 양쪽 source 로 갱신되는 case

- 위험: SvelteFlow internal 이 lasso drag *중* node 의 `selected` 를 자체적으로 mutate (시각 미리보기 목적). 우리 derived 가 `M.has(id)` 로 prop 다시 pass → SvelteFlow 가 prop 우선시 → 시각 desync.
- 현재 회피: drag *완료* 시점 (`onselectionchange` 최종 fire) 에서만 store M sync. drag 중 시각은 SvelteFlow internal 이 본인 책임 (raw `selected` mutation), 우리는 *관찰만*.
- 회귀 trigger:
  - `bind:nodes={$flowNodes}` 도입 (양방향 bind → derived 가 internal write 를 다시 source 로 받음 → cycle)
  - `flowNodes` 의 derived 안에서 `M.has` 가 high-frequency 로 호출되며 동시에 SvelteFlow internal 이 자체 mutate (eg. `panOnDrag` 동안)
- 검증: §3.S6 — lasso drag 중간에 frame-level 시각 동기 확인.

### B. `onselectionchange` 무한 callback cycle

- 위험: 우리가 setM → derived rebuild → flowNodes 새 prop → SvelteFlow internal reconcile → `onselectionchange` 재fire (혹시).
- 현재 회피: fast no-op (§1.4) — 동일 set 이면 setM skip.
- 회귀 trigger:
  - no-op 조건 약화 (ids array 비교를 sort/canonicalize 없이 만들면 ordering 차이로 매번 새 set 으로 판단)
  - `setM` 이 *항상* 새 Set 인스턴스를 만들지만 fast no-op 이 *content 비교* 라 정상 — content 비교를 reference 비교로 바꾸는 변경 시 위험
  - `M` 의 element 가 string 이 아닌 객체 ID 되면 `.has(id)` 가 reference 비교로 떨어져 같은 set 으로 인식 안 됨
- 검증: §3.S2 + DevTools `__gtmuxDebug.snapshot()` 의 `flowNodes.rebuild` 카운터 — drag 1회당 1~2회 증가가 정상. 50회+ 면 cycle.

### C. `selectionOnDrag` boolean dynamic toggle

- 위험: `selectionOnDrag` prop 이 `!isSpacePressed && !isHandTool && !isDragTool` 로 dynamic. Space hold / drag-tool 전환 시 prop 토글 → SvelteFlow 의 internal mode 가 lasso↔pan 사이 전환 중 selection state clear 발생 가능성.
- 현재 회피: 명시적인 검증 없음 — SvelteFlow 의 default 동작에 의존.
- 회귀 trigger:
  - lasso 가 진행 중인 상태에서 Space 누름 (mode 가 lasso→pan 으로 전환) — 진행 중인 selection box 가 어떻게 처리되는지 미확정
  - drag-tool 전환 (예: rect 도구) 도중 lasso 이미 영역 잡힌 상태
- 검증: §3.S4, §3.S5.

### D. `elevateNodesOnSelect=true` 와 store M 갱신 race

- 위험: SvelteFlow 의 `elevateNodesOnSelect=true` 가 *선택된 node 의 zIndex 를 internal 으로 boost*. 우리 derived 는 `item.z` 를 zIndex prop 으로 pass. external Layer click 으로 M 변경 시 *어느 쪽 z-index 가 시각에 반영* 되는지 의존성 모호.
- 현재 회피: Layer panel 이 z-index 진실 (ADR-0024). drag/click 의 elevate 는 *transient*.
- 회귀 trigger:
  - Layer panel reorder 직후 lasso 로 다중 선택 → 어떤 항목은 elevate, 어떤 항목은 그대로
  - `item.z` 가 음수 / 0 / 큰 값일 때 elevate offset 계산이 의도 밖
- 검증: §3.S7.

### E. `effect_update_depth_exceeded` 회귀 (0045 의 root issue)

- 위험: derived 의 cache miss 가 잦으면 매 pass 마다 새 Node object → SvelteFlow prop identity 변경 → 내부 측정 effect → parent rebuild → cycle.
- 현재 회피: id-cache + signature (`Canvas.svelte:419-429`) — 모든 mutable field 가 signature 에 포함되어야 함.
- 회귀 trigger:
  - 새 type 추가 시 `makeSignature` 에 type-specific payload 누락 (예: `image.url` / `document.text` 추가 시 signature 갱신 안 함 → stale render, 또는 매번 다른 reference 로 cache invalidate)
  - `JSON.stringify(item)` 가 *non-deterministic key order* 의 객체 포함 (예: Map → object 변환) → 매번 다른 string → cache 항상 miss
  - SvelteFlow upgrade 가 prop diff 정책을 referential 에서 deep 으로 변경 (cache 우회)
- 검증: §3.S8 + DevTools `__gtmuxDebug.snapshot()` 의 `flowNodes.cache.hit` / `cache.miss` 비율. 정상: idle 시 hit ≫ miss. 매 reactive pass 마다 miss 다수 = 회귀.

---

## 3. 재현 시나리오

### Dev instrumentation 활성

```js
// DevTools 콘솔
__gtmuxDebug.enable();
__gtmuxDebug.reset();
// 시나리오 1개 수행
__gtmuxDebug.snapshot();
// → { 'flowNodes.rebuild': N, 'flowNodes.cache.hit': H, 'flowNodes.cache.miss': M, ... }
```

### S1 — Lasso 자체 (정상 path)

1. select tool 활성 (default)
2. canvas 빈 영역에서 left-drag → 사각형 selection box 그려짐
3. 영역 안 node 들이 *시각적으로 selected* (border highlight)
4. mouse-up → `sessionStore.M.size` 가 lasso 영역의 node 수와 일치 (DevTools Sources 에서 `sessionStore.M` 확인)
5. Layer panel 의 해당 row 들도 highlighted
- **기대 카운터**: `flowNodes.rebuild` 가 drag 동안 ~5~30 회, mouse-up 후 1~2회 추가. `cache.hit` 가 `cache.miss` 보다 ≥ 10x.
- **회귀 sign**: rebuild > 100 in mouseup 후 → cycle. cache.miss ≈ rebuild → signature churn.

### S2 — Lasso → 외부(Layer) click 갱신 → Lasso 재실행

1. S1 수행 (3개 선택)
2. Layer panel 에서 다른 row click (Cmd/Ctrl 없이) → 단일 선택으로 reset 되는지 (LayerTreeView 의 click 정책: toggleM 면 추가/제거, setM 면 reset)
3. canvas 의 *시각* 이 store M 과 일치하는지 (canvas 의 highlighted node = Layer 의 highlighted row)
4. canvas 빈 영역 lasso 다시 → fresh selection
- **기대**: 2 → 3 동안 canvas 시각도 즉시 sync. 4 의 결과는 lasso 영역만, 이전 selection 흔적 없음.
- **회귀 sign**: 2 후 canvas 시각에 이전 3개 selection 잔존 (stale `selected` prop) → §2.A trigger.

### S3 — Cmd/Ctrl click 다중 선택 → Lasso 교차

1. node A click (단일 선택)
2. Cmd/Ctrl + node B click (B 추가)
3. canvas 빈 영역 lasso → C+D 영역 잡음
4. 기대: lasso 가 *대체* (replace) 인지 *추가* (union) 인지 — 현재 정책: replace (onselectionchange 의 setM 가 array 그대로 받음, modifier 무관)
- **기대**: lasso 결과는 C+D 만. A/B 는 deselect.
- **회귀 sign**: A/B 가 selected 상태 유지 → onselectionchange 가 modifier-aware union 으로 변질된 case.

### S4 — Lasso 진행 중 Space 누름 (mode 전환 race)

1. canvas 빈 영역에서 left-drag 시작 (lasso 진행 중)
2. 마우스 떼지 않고 Space 누름 → `panOnDragMask` 가 `[0, 1, 2]` 로 전환, `selectionOnDrag` 가 false 로 전환
3. Space 누른 상태에서 drag 계속 → viewport pan 으로 전환되어야
4. Space 떼고 mouse-up
- **기대**: lasso box 는 *취소*, viewport pan 으로 자연 인계. 사용자가 손 떼면 selection 변화 없음.
- **회귀 sign**:
  - lasso 가 *완료* 되어 selection 잡힘 (mode 전환 무시) → §2.C trigger
  - 또는 selection 강제 clear → 의도 미정 — 정책 결정 필요

### S5 — Lasso 진행 중 도구 전환 (예: 'rect' 활성)

1. select tool 에서 lasso 시작
2. 키보드로 rect tool 단축키 (toolStore 갱신) — drag 중에
3. drag 계속
- **기대**: rect 도구는 *down* 이벤트에서 rect 생성 시작 가능. 이미 진행 중인 lasso 는 *완료* 또는 *취소* — 정책 미정. 안전: 이미 시작된 pointer interaction 은 끝까지 lasso 로 유지, 새 down 부터 rect.
- **회귀 sign**: drag 중 instant 전환으로 rect 생성 시도 → 사용자 의도 어긋남.

### S6 — Lasso drag 중 시각 sync (frame-level)

1. 큰 영역 (10+ node) lasso 시작
2. 천천히 영역 확장 (DevTools Sources 에서 Animation panel 활용 또는 slow drag)
3. 각 frame 마다 영역 안으로 *새로 들어온* node 가 즉시 highlighted 되는지
- **기대**: SvelteFlow internal 이 frame-by-frame 으로 selected 시각 갱신.
- **회귀 sign**: 시각 lag (영역 확장 후 한 박자 늦게 highlight) → derived rebuild 가 frame 마다 못 따라감 → §2.A 또는 cache churn.

### S7 — Layer reorder 후 lasso (z-index race)

1. Layer panel 에서 node A (z=10) 를 node B (z=5) 위로 reorder → A.z 가 새 값으로 갱신
2. canvas 에서 A 가 B 위에 시각적으로 올라옴
3. A+B 둘 다 포함하는 lasso → 두 node 모두 selected
4. `elevateNodesOnSelect` 가 selection 시 internal z 를 boost — A/B 모두 elevate
5. mouse-up 후 selection clear (다른 곳 click) → A/B 의 z 가 reorder 직후 상태로 복원
- **기대**: 5 후 시각 z-order = reorder 후 의도. elevate 가 transient 였음.
- **회귀 sign**: 5 후 z-order 가 elevate 시 boost 된 상태로 frozen → §2.D trigger.

### S8 — `effect_update_depth_exceeded` 회귀 검출

1. 새 page load (`/` 진입)
2. 페이지 reload (`F5` / Cmd-R) — 0045 의 원래 reproduce path
3. DevTools console 에서 `Uncaught Error: …e/effect_update_depth_exceeded` 가 *없어야*
4. `__gtmuxDebug.snapshot()` 의 `flowNodes.rebuild` < 20, `cache.hit / (hit+miss)` > 0.8 (idle 시)
- **기대**: 에러 없음, rebuild bounded.
- **회귀 sign**: 에러 발생 OR rebuild > 100 in 1초 → §2.E.

### S9 — Lasso 후 drag-commit (multi-drag) 의 selection 보존

1. 다중 lasso (3 node)
2. 그 중 한 개를 drag → 세 개 모두 함께 이동
3. drop 직후 *selection 유지* — Layer panel + canvas 둘 다
4. (commit `682b584` 의 회귀 fix — `onnodedragstop` 의 targetNode null 가드 제거. group drag 의 NodeSelection 패턴.)
- **기대**: drag 완료 후에도 M.size = 3.
- **회귀 sign**: drop 후 selection 이 1개로 줄거나 0 — onnodedragstop 가드 회귀.

---

## 4. 회귀 발생 시 진단 절차

1. `__gtmuxDebug.enable()` + `reset()` + 시나리오 재현 + `snapshot()`
2. 카운터 비율로 1차 trigger 분류:
   - `flowNodes.rebuild` >> 시나리오 추정치 → cycle (B/E)
   - `cache.miss / rebuild` 가 높음 → signature churn (E)
   - `canvas.onmove` >> 추정 → viewport effect cycle
3. svelte devtools 의 component inspector 에서 `Canvas` 의 `M` / `flowNodes` reactivity graph 확인
4. SvelteFlow 의 `nodes` prop 의 reference identity 추적 — *동일 reference* 여야 (cache hit). 매번 새 reference 면 cache miss 원인 파악:
   - `JSON.stringify(item)` 의 key ordering 변화
   - 새 type 의 mutable field 가 signature 에 미반영
5. 시각 desync 의심 시 §2.A 회피 (one-way) 가 유지되는지 — `bind:nodes` 도입됐는지 grep
6. cycle 의심 시 fast no-op (`Canvas.svelte:821-830`) 의 조건 점검

---

## 5. 회귀 방지 체크리스트 (PR 단계)

| 변경 영역 | 체크 |
|---|---|
| `Canvas.svelte` 의 `flowNodes` derived | one-way 유지 (`bind:` 금지). signature 가 *모든 mutable field* 포함. |
| `Canvas.svelte` 의 `onselectionchange` | fast no-op (content 비교 / 양방향 무관) 유지. 새 modifier 정책 도입 시 §3.S3 검증. |
| `Canvas.svelte` 의 `selectionOnDrag` 조건 | dynamic toggle 시 §3.S4 / §3.S5 검증. |
| `sessionStore.M` 의 mutator | `setM` 은 항상 *content* 단위, `addToM`/`removeFromM` 은 일관 정책. |
| 새 CanvasItem type 추가 | `itemToNode` 의 case 추가 + `makeSignature` 의 payload coverage 검증. |
| LayerTreeView 의 click 정책 변경 | §3.S2 의 결과 (replace vs union) 정합 검증. |
| SvelteFlow 의존 upgrade | prop diff 정책 변화 (referential → deep 등) 사전 점검 — cache 우회 위험. |

---

## 6. 변경 이력

- 2026-05-16: 초안 — handover 0049 §7 의 known issue (lasso external sync verification) 의 명문화. 0045 의 effect_update_depth 회피 (P0-A id-cache + signature) 를 selection mechanism 차원에서 정합. 본 문서는 *재현* + *회귀 검출* 용 — 회귀 fix 발생 시 §4 진단 결과를 본 문서에 update.
