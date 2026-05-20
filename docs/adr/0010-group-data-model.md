# ADR-0010: Group 데이터 모델 — G-hybrid

- 상태: Accepted (2026-05-14, A0.7 + A4 게이트 통과)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted)
- 결정자: frontend-architect / system-architect (grill 산출)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` (D11, D23 부분 — placement / z-index), `CONTEXT.md` "Group 운영 규칙"
- 관련 ADR: ADR-0008 (single-pane-per-window + Group이 UI 1차 시민이 된 근거), ADR-0006 (Canvas Layout 영속화 — 본 ADR의 SSoT를 입력으로 사용 예정), **ADR-0024 (Layer Tree와 Z-Index 분리 — group은 z-index 없음 명시)**
- 동반 산출물: `docs/ssot/canvas-layout-schema.md` (JSON Schema SSoT — 본 ADR의 페이로드 계약 정본)

> ⚠️ **2026-05-15 G24 grilling amend (by ADR-0024)**: 본 ADR 의 group 정의는 *pure organization* — group 자체는 **z-index field 없음** (`groups[]` 에 z 없음). 자식들의 z 는 *group 공간* 이 아닌 *flat global z 공간* 에 속함. 즉 group sibling 의 자식들끼리도 직접 z 비교 가능. Tree drag reorder = organization 만 변경 (z 영향 X). z mutation 은 ADR-0024 D2 의 4 액션 (Bring/Send to front/back, Bring/Send forward/backward) 으로만.

## 맥락

ADR-0008로 tmux Window가 UI에서 사라지고 사용자 측 Panel 묶음은 web-only **Group**(Figma-식 layer)이 담당하게 됐다. 본 ADR은 Group의 *데이터 모델*, *운영 규칙*, *영속화 계약*을 정식화한다.

본 ADR은 `docs/sketch.md` §4.1.3 (tmux Layout ≠ Canvas Layout) · §4.3 (Group / M / I 정의) · §6.2 (Group 관리 기능) · §6.5 (사이드바 layer panel)과 정합하며, ADR-0007의 placement principle("좌표는 사용자 명시 입력만" — D23에서 *optional + 자동 cascade*로 재정의됨)과도 충돌이 없도록 설계한다.

영속화 페이로드의 *정본 schema*는 본 ADR의 부속 SSoT인 `docs/ssot/canvas-layout-schema.md`에 있다. 본 ADR 본문은 그 schema의 결정 근거와 운영 규칙을 진술하며, 필드 정의가 충돌하는 경우 **SSoT가 canonical**이다.

## 결정 (Decisions)

- **D1.** Group은 **frame을 1차 상태로 저장하지 않는다** (G-hybrid). 자체 상태 = `{label, color, visibility, locked, order}` + 트리 구조 (`parent_id`). 정확한 타입·제약은 SSoT §1 `$defs/Group` 참조.
- **D2.** 트리 구조: **다중 부모 금지**. 한 Panel/Group은 정확히 한 부모 Group 또는 Canvas 루트(`parent_id = null`)에 속한다. 사이클 금지.
- **D3.** 트리 깊이: 명시적 제한 없음 (실용상 사용자가 깊게 만들지 않을 것).
- **D4. 생성·해체 UX**: Panel/Group **다중 선택(M)** → `Group` 액션이 새 Group으로 묶음. Group 단일 선택 → `Ungroup` 액션이 해체하고 자식을 grandparent 또는 Canvas 루트로 reparent.
  - **빈 Group은 별도 생성 경로 없음** (다중 선택 ≥ 1 필수).
  - 자식이 모두 제거되어 발생한 빈 Group은 명시적 사용자 액션(Ungroup/Delete)으로만 제거 — auto-prune 안 함.
- **D5. Drag-reparent**: 사이드바 layer panel 안에서의 드래그만 (MVP). 캔버스 hover 기반 reparent는 P1+.
- **D6. 상태 전파 (AND 전파)**:
  - **effective visibility** = self.visibility AND 모든 ancestor.visibility
  - **effective locked** = self.locked OR 모든 ancestor.locked **중 하나라도 true** (= 한 단계라도 잠겨 있으면 잠금)
  - **label**: self.label이 non-null이면 그 값, null이면 가장 가까운 ancestor의 label을 표시(추론). 영속화는 self 값만 저장.
  - **color**: label과 동일한 inherit 규칙 (D6 label과 같은 정책).
- **D7. Group → M 확장**: 사이드바에서 Group 클릭 = 그 Group의 모든 *후손 Panel*을 M에 등록 (재귀). 캔버스에서 Panel 클릭 = 그 Panel만 M (Group 자동 확장 없음).
- **D8. Group 이동**: *액션*으로만 존재. 사용자가 Group 헤더/사이드바에서 드래그하면 자손 Panel들의 bounding box를 계산하고, 드래그 delta를 모든 자손 Panel 좌표에 동일 적용. effective locked인 자손은 delta 적용 대상에서 제외. ADR-0007 placement principle과 충돌 없음 (드래그가 사용자 명시 입력).
- **D9. Group 리사이즈**: **MVP 미지원** (P1+에서 검토). frame이 1차 상태가 아니므로 자연스러운 부재.
- **D10. Group close (destructive)**: 자손의 모든 Panel을 `kill-pane` 재귀 발급. §7.6 confirm modal 필수. Group 자체는 자손이 모두 사라진 후 명시 삭제 또는 ungroup으로 제거. effective locked 자손은 confirm modal에서 별도 표시(P1+) — MVP는 일괄 kill.

  > ⚠️ **2026-05-15 G25.1 grilling amend (by ADR-0021 D9.3)**: Group close 의 confirm modal 은 **bulk 1 dialog** — 자손 마다 dialog 발동 X. Dialog 옵션 = `[Cancel]` / `[Panels only]` (terminal pool 유지) / `[Panels + Terminals]` (mirror 영향 ⚠ hint). Multi-session 시대의 *"kill-pane 재귀 = terminal kill"* 강제는 폐기 — 사용자 선택. `Settings.behavior.auto_kill_terminal_on_panel_close = true` 시 dialog 없이 `[Panels + Terminals]` 즉시 (default false). 자세한 정합은 ADR-0021 D9.3.

- **D12. Ungroup (비파괴, 2026-05-15 G25 grilling 추가)**: Group 만 삭제, 자손들의 `parent_id = group.parent_id` 로 승격. 자손들의 `self.visibility / self.locked` 보존. **Effective state 는 ancestor 변화로 변할 수 있음** (예: 잠긴 group 안에 있던 자손이 ungroup 후 effective unlock). 비파괴 액션이므로 **confirm 없음**. Layer list group row 의 context menu / more menu 에서 [Ungroup] 노출. Group close 와의 사용자 mental 차이 = *"조직만 풀고 element 들은 보존"* vs *"조직 + element 들도 삭제"*.

- **D13. Multi-session amend (2026-05-15 G25 grilling 명시)**: 본 ADR 의 모든 group propagation 규칙은 **session-local 적용** — 한 session 의 group tree 가 self-contained. Cross-session 영향 없음. 같은 terminal 이 두 session 의 panel 로 mount 된 경우 (ADR-0021 D1), 각 session 의 *그 panel 의 effective state* 는 *그 session 의 group propagation* 에 따라 독립 계산. 예: terminal T 가 session A 의 group X (locked) 자손이고 session B 의 group Y (unlocked) 자손이면, A 의 panel 은 effective locked, B 의 panel 은 effective unlocked. Streaming state (CONTEXT.md) 도 (session, panel) 쌍 단위.
- **D11. Placement·z-index 정합 (D23 반영)**:
  - 신규 Panel의 좌표는 **optional** — 사용자가 명시 입력하면 그 위치, 미지정이면 자동 cascade(origin (0,0)에서 (40,40)px offset 누적, Server 메모리만 유지).
  - **Unplaced Panel 개념은 도입하지 않는다.** 외부 tmux CLI로 생긴 Pane도 즉시 캔버스에 노출(자동 cascade로). 본 데이터 모델에는 "대기 트레이" / "미배치 상태" 같은 필드가 존재하지 않는다.
  - z-index: 모든 Panel은 정수 `z` 필드를 가진다 (SSoT `Panel.z`). 신규 Panel z = 현재 최대 z + 1. M에 들어오는 Panel은 z = 현재 최대 z + 1 (자동 최상위). Overlap 자유 허용.

## 거절된 대안 (Rejected)

- **R1. G-pure-spatial — Group이 frame(x/y/w/h)을 1차 상태로 저장.** 자식 추가·제거·자손 좌표 변경마다 frame 재계산 갱신 로직 + 자식 좌표를 절대값으로 둘지 부모 상대값으로 둘지 결정 부담. MVP 과스코프이며 ADR-0007의 "좌표는 사용자 명시 입력만"과도 의미적으로 어긋남(Group frame이 자손 변화로 *자동* 갱신되는 것은 명시 입력이 아님). 거절. (보고서 D11)
- **R2. G-logical-only — label·visibility·lock만 묶이고 Group 이동은 지원 안 함.** "묶음"의 시각적 의미가 약해 Figma 멘탈 모델(레이어 = 시각 그룹 + 메타 그룹)을 잃는다. 사용자가 "Group을 잡고 옮긴다"를 가장 자주 기대하는 액션이므로, 그 액션이 빠지면 Group은 단순 태그 시스템으로 후퇴한다. 거절. (보고서 D11)
- **R3. "+ New Group" 빈 그룹 생성 어포던스.** 다중 선택 group/ungroup만 두어 빈 Group이 생기는 경로 자체를 차단한다. 명시적 사용자 의도 없이 빈 Group이 트리를 어지럽히는 것을 방지하고, "비어 있는 Group의 시각 표현"이라는 부수 결정도 제거한다. 거절. (사용자 요청)
- **R4. Unplaced Panel 대기 트레이.** 좌표 미지정 Panel을 "미배치 상태"로 사이드바 트레이에 두고 사용자가 배치 액션을 해야만 캔버스에 진입하는 모델. D23에서 placement가 *optional + 자동 cascade*로 재정의되며 폐기. Panel은 생성 즉시 캔버스에 cascade 좌표로 노출되어야 하며, 본 데이터 모델은 "미배치" 같은 partial state를 가지지 않는다. 거절. (보고서 D23)

## SSoT 정렬 — `docs/ssot/canvas-layout-schema.md`

본 ADR의 데이터 모델은 SSoT의 JSON Schema와 1:1로 대응한다. 핵심 필드는 다음 표로 요약하며 **정의는 SSoT가 canonical**이다.

### Group (SSoT `$defs/Group`)

| 필드 | 타입 | 비고 |
|---|---|---|
| `id` | `string`, pattern `^g[0-9a-zA-Z]{1,32}$` | 클라이언트 발급, `g` prefix |
| `parent_id` | `string | null`, 같은 pattern | `null` = Canvas 루트 자식 |
| `label` | `string | null`, maxLength 128 | null 시 ancestor inherit (D6) |
| `color` | `string | null`, pattern `^#[0-9a-fA-F]{6}$` | null 시 ancestor inherit (D6) |
| `visibility` | `boolean` | self 상태. effective는 ancestor AND |
| `locked` | `boolean` | self 상태. effective는 ancestor OR (한 단계라도 잠금이면 잠금) |
| `order` | `integer`, ≥ 0 | 형제 노드 내 정렬 키 (사이드바 layer panel) |

### Panel (SSoT `$defs/Panel`)

| 필드 | 타입 | 비고 |
|---|---|---|
| `id` | `string`, pattern `^p[0-9a-zA-Z]{1,32}$` | 클라이언트 발급, `p` prefix |
| `parent_id` | `string | null`, pattern `^g...$` | Group만 부모 가능 (Panel은 부모 될 수 없음) |
| `pane_id` | `string`, pattern `^%[0-9]+$` | tmux pane id mirror |
| `x`, `y` | `number` | 캔버스 좌표 (D11 cascade 또는 사용자 명시) |
| `w`, `h` | `number`, > 0 | 크기 |
| `z` | `integer` | z-index (D11) |
| `visibility` | `boolean` | self. effective는 ancestor AND |
| `minimized` | `boolean` | web-only. visibility=true이지만 배지로만 렌더. Panel Streaming State Suspended 트리거 (D16) |
| `locked` | `boolean` | self. effective는 ancestor OR |
| `label` | `string | null`, maxLength 128 | |
| `note` | `string | null`, maxLength 2048 | tmux로 절대 전송되지 않음 |

### 페이로드 envelope (SSoT 최상위)

```json
{
  "etag": "<32 lowercase hex chars>",
  "schema_version": 1,
  "groups": [ /* Group[] */ ],
  "panels": [ /* Panel[] */ ]
}
```

- `etag`: 16바이트 raw가 정본이며, HTTP JSON body에서는 lowercase hex 32자로 인코딩. WS `0x80 LAYOUT_CHANGED` envelope에서는 raw 16바이트. SSoT §2 참조.
- `schema_version`: MVP 고정값 `1`. 마이그레이션 도입 시 ADR-0006에서 확장.

### 검증 규칙 (SSoT §3 R1~R9 요약)

서버는 `PUT /api/layout`을 받아 다음을 순서대로 검증한다.

- R1. JSON Schema 합치 (`additionalProperties: false` — 미정의 필드 거부).
- R2. ID 유일성 (`groups[].id`, `panels[].id` 각각).
- R3. `Panel.pane_id` 존재성 — 서버가 현재 mirror 중인 tmux pane 집합 안에 있을 것.
- R4. 트리 정합성 — `parent_id` 참조가 같은 페이로드의 `Group.id`에 존재하거나 `null`.
- R5. 사이클 금지 (DFS).
- R6. 다중 부모 금지 (각 노드 1개 `parent_id`).
- R7. `Panel.parent_id`는 Group만 참조 (`^g...` pattern).
- R8. `If-Match` ETag 헤더 일치 (불일치 시 412 Precondition Failed).
- R9. 페이로드 크기 ≤ 256 KB (초과 시 413).

본 ADR의 데이터 모델이 위 검증 룰과 호환됨을 확인했다 (이 ADR과 SSoT 사이에 사양 불일치 없음 — 2026-05-13 cross-check 완료).

## 결과 (Consequences)

- **긍정**:
  - 데이터 모델 단순 (Group이 frame을 저장하지 않음 → 정합성 코드 0).
  - ADR-0007 placement principle 및 D23 cascade와 자연 정합 (Group 이동이 사용자 명시 입력).
  - Figma 컨벤션과 정렬되어 사용자 멘탈 모델 친숙.
  - HTTP `PUT/GET /api/layout` 페이로드가 작고(< 50 KB 일반, 256 KB 상한) 명료.
  - `additionalProperties: false`로 schema drift 방지 — 미래에 필드 추가 시 SSoT + 본 ADR 동반 갱신 강제.
- **부정/비용**:
  - "Group 리사이즈"는 MVP에서 사용 불가 (P1+).
  - "Group bounding box" 계산은 클라이언트에서 매번 즉시 (자식 좌표 변경마다) — 실용상 무시 가능하나 1000+ 자손 시 O(n) 비용 발생.
  - effective lock/visibility는 ancestor 체인 traversal — 깊이 5+ 트리에서 매 렌더마다 traversal 비용 (Svelte signals의 derived store로 캐시).
- **후속 작업**:
  - ADR-0006 (Canvas Layout 영속화)이 본 SSoT를 storage backend 결정의 입력으로 사용.
  - sketch.md §6.2 (Group 관리 기능) · §6.5 (사이드바 layer panel)가 본 결정을 그대로 인용 (완료).
  - R8 보고서가 Svelte signals 기반 AND/OR 전파 계산 패턴을 검증 (Open O2 참조).

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | PASS — Group은 명백히 web-only. `Panel.pane_id`만 tmux 측 mirror 참조이며 R3에서 정합성 검증. Group/Panel의 visibility/locked/label/note/minimized 등은 tmux에 절대 노출 안 됨. |
| 2 | tmux-native vs web-only 분기 | PASS — Group 모든 상태·연산은 web-only. tmux 액션은 *Group close 시 자손 Pane kill 발급*과 *Panel Streaming State 전이의 `refresh-client -A '%pid:pause/continue'`*만. |
| 3 | tmux Layout ≠ Canvas Layout | PASS — Group은 Canvas 측 묶음. tmux Window 1-pane 컨벤션(ADR-0008)에서 tmux Layout은 trivial이며 Group과 완전 무관. |
| 4 | 보안 기본값 | PASS — HTTP PUT 페이로드의 schema R1~R9 검증(`additionalProperties: false`, pattern 강제, 256 KB cap, ETag 412, pane_id 존재성)이 §13.3 입력 신뢰 금지 정신과 정렬. label/note는 maxLength 강제 + 렌더 시 escape (ADR-0003 보안 디폴트). |
| 5 | control mode 사용 | PASS — Group 자체는 control mode와 무관. Group close가 발급하는 `kill-pane`, Panel Streaming State 전이가 발급하는 `refresh-client -A`만 control mode 채널 사용. |

## 미해결 항목 (Open)

- **O1. Drag-reparent UX 디테일 (P1+ 확장 시점)**:
  - 캔버스 hover 기반 reparent의 hover target 판정 규칙 (Group의 visual bounding box를 어떻게 표시? 자손 Panel들의 union? 별도 오버레이?).
  - 사이드바 layer panel 드래그 시 *위치(order)*와 *부모(parent_id)* 동시 변경의 키보드 modifier 정책 (Figma: Tab indent로 부모 변경, drag만으로 형제 reorder).
  - drag 도중 effective locked인 노드를 자식으로 받을 때 시각 피드백 — R8 보고서 항목으로 추가.
- **O2. AND/OR 전파 edge case 사양 — R8 verification 항목**:
  - **locked 전파**: 본 ADR D6은 "OR" (ancestor 중 하나라도 locked=true이면 self는 effective locked). 예: locked Group A의 자식 Panel P가 self.locked=false여도 → P는 effective locked. UI는 잠금 아이콘 표시, drag/resize/close 모두 차단. P에서 직접 unlock 시도 시 동작은? **결정: P의 unlock UI는 disabled (회색)**, hover tooltip "잠긴 그룹 'A' 안에 있어 잠금 해제 불가 — A를 먼저 잠금 해제하세요".
  - **visibility 전파**: AND. ancestor 중 하나라도 visibility=false이면 자손은 effective hidden. 자손 self.visibility=true여도 캔버스에 노출 안 됨. effective hidden Panel의 Streaming State는 Suspended (D16과 정합).
  - **충돌 케이스 (R8 verification)**:
    - (a) Group A locked + Panel P self.locked=false에서 사용자가 Group A를 드래그 → P는 effective locked이지만 ancestor 이동의 *자손 delta 적용 대상*인가? **결정: 대상이 아니다.** D8 본문에 "effective locked인 자손은 delta 적용 대상에서 제외" 명시 (이미 반영). 시각적으로 P만 제자리에 남고 Group A의 나머지 자손은 이동.
    - (b) Group A visibility=false + Panel P self.visibility=true에서 사용자가 사이드바에서 P 클릭 → M 등록되는가? **결정: 등록 안 함.** effective hidden 노드는 M 후보에서 제외. R8에서 사이드바가 effective hidden 자손을 "회색 + 클릭 비활성" 렌더하는지 검증.
    - (c) Group A self.locked=false + 그 부모 Group B locked=true에서 사용자가 A의 lock toggle 시도 → A 자체의 lock 상태는 변경 가능(self 값)하지만, A는 여전히 effective locked. UX 명확성: self.locked 토글 UI는 disabled (회색) + tooltip "상위 그룹 'B'가 잠겨 있어 잠금 토글 불가".
  - 위 (a)(b)(c)는 모두 R8 보고서의 frontend 시나리오 테스트로 검증한다.
- **O3. Group close confirm modal에서 effective locked 자손 표기**: MVP는 일괄 kill (D10). P1+에서 "이 그룹 안에 잠긴 Panel N개 — 그래도 진행?" 확장. R8에서 modal 시안 검증.
- ~~O4. SSoT JSON Schema 파일 작성~~ → **해소** (코히런스 리뷰 G1). `docs/ssot/canvas-layout-schema.md` 초안 완료 (2026-05-13). ADR-0006 dispatch 시 storage backend 결정에 따라 직렬화 디테일 보강 예정.

## 변경 이력

- 2026-05-13: 초안 (grill D11 산출). Proposed.
- 2026-05-13: SSoT 정렬·D23 반영·Open 항목 R8 verification 항목으로 sharpen. Proposed 유지.
- 2026-05-13 (2차 coherence A0.7): grill D11의 통합 "AND" 전파 표현 중 lock semantics에 한해 **OR (cascade-down)**로 정정. 사용자 멘탈모델 = "잠긴 그룹 안의 항목은 잠긴다, 자식이 부모 잠금 해제 불가". SSoT line 87 + CONTEXT.md "Group 운영 규칙" + grill report D11 inline 동시 정정.
- 2026-05-15 (G24 grilling amend by ADR-0024): header amend — group 은 z-index field 없음. Tree drag = organization 만.
- 2026-05-15 (G25 grilling amend): D10 의 confirm modal 이 bulk 1 dialog (3 옵션 + mirror hint) 로 amend (ADR-0021 D9.3 정합). D12 (Ungroup 비파괴) + D13 (multi-session session-local 적용) 신규.
