# 보고서: 문서 정합성 리뷰 (sketch / CONTEXT / plans / adr)

- 일자: 2026-05-13
- 검토 범위: `docs/sketch.md`, `CONTEXT.md`, `docs/plans/0001-research-plan.md`, `docs/plans/0002-work-dispatch.md`, `docs/reports/0001/0004/0005/0010`, `docs/adr/0007–0010`
- 작성자: PM
- 상태: 1차 종료(2026-05-13) — 6건 조치 완료 / **2차 진행 중**(2026-05-13, §7 참조). 2차에서 신규 Blocking 2건·Advisory 5건 확인. 배치 A·B는 §7의 G7·G8 해소 후 진입 권장.

## 요약 (3문장)

배치 A0(ADR-0007~0010)는 grill 보고서 D1~D11의 결정을 정확히 반영했고 5대 불변식 검증을 모두 통과해 **기초 도메인 정의 계층은 정합** 상태다. 다만 (a) ADR-0010의 동반 SSoT 작성 시점이 dispatch §A0.4와 ADR 본문 Open O2 사이에서 **모순**되고, (b) dispatch §A3 프롬프트가 grill D17(인증 결정)을 입력 제약으로 누락했으며, (c) R4 보고서의 영속화 메소드(`POST /layouts`)가 grill D12·ADR-0010 SSoT의 `PUT /api/layout`과 어긋난다 — 모두 배치 A 진입 전에 dispatch 0002를 정정하면 해소된다. 그 외는 markdown 구조·표기 일관성 수준의 사소 항목이며, 배치 A·B 실행에 차단성은 없다.

## 조사 범위와 질문

- 도메인 어휘가 sketch / CONTEXT / ADR 간 일관되는가.
- Grill D1~D17 → ADR 매핑이 빠짐없이 dispatch 0002에 입력 제약으로 흘러갔는가.
- ADR 0007~0010이 서로를 정확한 ID로 상호참조하는가.
- 미작성 산출물(보고서 R2/R3/R6, ADR-0001/0002/0003/0004/0005/0006, SSoT 2건)의 의존 그래프가 dispatch 0002의 실행 순서와 모순 없는가.
- 5대 불변식 검증이 ADR 4개에서 PASS로 떨어지는가.
- 번호·경로·문서 ID 충돌이 있는가.

## 핵심 발견

### 1. 도메인 어휘 정합 — PASS

sketch §4.3, CONTEXT.md "Language", ADR-0007 D1, ADR-0008 D3, ADR-0010 D1~D2가 다음 어휘를 동일하게 정의한다.

| 용어 | sketch §4.3 | CONTEXT.md | ADR 정의 |
|---|---|---|---|
| Server (gtmux Server) | ✓ | ✓ | ADR-0007 D1 |
| Session (tmux) | ✓ | ✓ | ADR-0007 D1 |
| Window (tmux, implementation-only) | ✓ | ✓ | ADR-0008 D3 |
| Pane | ✓ | ✓ | (모든 ADR 전제) |
| Canvas | ✓ | ✓ | ADR-0007 결과 |
| Panel | ✓ | ✓ | ADR-0008/0010 |
| Canvas Layout | ✓ | ✓ | ADR-0010 SSoT |
| Group | ✓ | ✓ | ADR-0010 D1 |
| M / I (직교 active mode) | §4.3 추가됨 | ✓ | (ADR 미반영, dispatch D14에 슬롯) |
| Panel Streaming State | (sketch 미반영) | ✓ | D16 입력 제약, ADR-0001 흡수 예정 |
| Unplaced Panel | (sketch 미반영) | ✓ | ADR-0008 D4 |

sketch.md §6.1이 "gtmux UI 비범위"로, §6.2가 "Group 관리 기능 (web-only, tmux Window 대체)"로 amend 완료된 것이 확인됨 (grill §2 수정 항목 반영).

**관찰**: M/I, Panel Streaming State, Unplaced Panel은 CONTEXT.md에는 정식 정의되어 있지만 sketch.md §4.3에는 명시되지 않은 것으로 보임 — grill §2 amendment 표에 "§4.3에 추가"가 기재되어 있으므로 sketch.md amend가 *부분 완료* 상태일 가능성. 확인 필요.

### 2. Grill D1~D17 → ADR / dispatch 매핑

| 결정 | 흡수 위치 | 상태 |
|---|---|---|
| D1·D2·D3 (1:1:1, immutable bind) | ADR-0007 | ✅ |
| D4·D8·D9 (single-pane + Group + rename 동기화) | ADR-0008 | ✅ |
| D5 (active = M의 default seed) | sketch §4.3 / CONTEXT.md "Relationships" | sketch.md 본문 amend 필요 (§4.2·§6) |
| D6 (M/I 직교) | CONTEXT.md, dispatch §A2 D14 슬롯 | sketch §4.3에 정식 추가 미확인 |
| D7 (Placement principle) | CONTEXT.md, ADR-0008 D4 | ✅ |
| D10 (dedicated daemon) | ADR-0009 | ✅ |
| D11 (G-hybrid) | ADR-0010 | ✅ |
| D12 (T-mixed: HTTP layout + WS notify) | dispatch §A2 프롬프트, ADR-0010 SSoT | ✅ (ADR-0002에서 정식화 예정) |
| D13 (MT-3 Live Mirror) | CONTEXT.md, dispatch §A2 프롬프트 | ✅ |
| D14 (WS 0x80–0x8F 슬롯 정의) | dispatch §A2 프롬프트 | ✅ (ADR-0002·SSoT에서 확정 예정) |
| D15 (per-pane 128 KB ring buffer) | dispatch §A1 입력 제약 | ✅ (ADR-0001 흡수 예정) |
| D16 (Panel Streaming State lifecycle) | dispatch §A1 입력 제약 | ✅ |
| **D17 (인증 토큰 정책)** | (없음) | ❌ **GAP** — dispatch §A3 프롬프트 미반영 |

### 3. ADR 상호참조 — PASS

- ADR-0007 → ADR-0008, ADR-0009 (정확)
- ADR-0008 → ADR-0007, ADR-0010, ADR-0001(미작성, 명시) (정확)
- ADR-0009 → ADR-0007, ADR-0001(미작성) (정확)
- ADR-0010 → ADR-0008, ADR-0006(미작성, 명시) (정확)

모든 참조 ID 일관. "미작성 ADR 인용"은 dispatch 0002에서 발행 예정으로 추적되므로 허용 가능.

### 4. 5대 불변식 검증 — PASS

ADR-0007/0008/0009/0010 모두 5×5 = 20칸 전부 PASS 명시. 검증 문구의 강도가 ADR마다 균일하지 않으나 (ADR-0010 #4는 다소 약한 PASS), 부정확한 PASS는 없음.

### 5. 미작성 산출물 의존 그래프 — PASS

| 미작성 | 발행 경로 | 차단 여부 |
|---|---|---|
| ADR-0001/0002/0003 | dispatch §1 배치 A | A0 통과 후 즉시 진행 가능 |
| ADR-0004/0005/0006 | dispatch §2 배치 B4 | R2/R3/R6 보고서 후 |
| 보고서 R2/R3/R6 | dispatch §2 B1/B2/B3 | 배치 A와 병렬 가능 |
| SSoT `wire-protocol.md` | ADR-0002와 동반 | 배치 A 진행 시 |
| SSoT `security-defaults.md` | ADR-0003과 동반 | 배치 A 진행 시 |
| SSoT `canvas-layout-schema.md` | **모순** (G1 참조) | dispatch A0.4 vs ADR-0010 Open O2 |

### 6. 번호·경로 충돌 — 없음

- `reports/0010-grill-amendments.md` ↔ `adr/0010-group-data-model.md`: 다른 디렉터리이므로 충돌 아님. 다만 첫 인용 시 혼동 가능 — 항상 디렉터리 prefix 동반 권장.
- `reports/0011-coherence-review.md` (본 문서)는 dispatch §A0.5의 `0011-batch-a0-coherence-review.md`와 동일 슬롯 점유. **동일 산출물로 간주** — dispatch §A0.5는 본 보고서로 충족된 것으로 처리.

## 갭 (Findings)

### G1 (Blocking). ADR-0010 SSoT 작성 시점 모순

- **현상**: `docs/ssot/canvas-layout-schema.md`의 작성 책임이 두 곳에서 충돌.
  - dispatch §A0.4 DoD: "SSoT의 JSON Schema가 HTTP `PUT /api/layout` 페이로드와 정확히 매칭" → ADR-0010 발행 시 동반.
  - ADR-0010 §Open O2: "SSoT 파일 정식 작성은 ADR-0006 dispatch 시 동반" → 배치 B4 시점.
- **영향**: ADR-0010이 "Proposed"인 현재 SSoT가 부재해, ADR-0002의 `LAYOUT_CHANGED` ETag 형식·ADR-0006의 영속화 스키마가 동일한 참조 대상을 갖지 못한다.
- **권고**:
  - 옵션 A — dispatch §A0.4 DoD에서 SSoT 작성 의무를 제거하고 ADR-0006와 동반(ADR-0010 본문 따름).
  - 옵션 B — 지금 SSoT 초안을 ADR-0010 부속으로 작성(ADR-0010 본문 Open O2 갱신).
  - **추천**: B. ADR-0002(전송)가 ETag 페이로드 형식을 결정하려면 SSoT가 먼저 필요.

### G2 (Advisory→Blocking). dispatch §A3 프롬프트에 grill D17 입력 제약 누락

- **현상**: dispatch §A3 프롬프트는 R5 보고서만 입력으로 명시. 그러나 grill D17은 R5 §"미해결 #1(회전), #4(OS 위임)"를 이미 close 처리:
  - 회전: local=매 시작 재발급 / cloud=영속+명시 회전 명령.
  - OS 위임: MVP 미적용.
  - 부팅 콘솔 URL의 cookie 1회 발급 UX 결정.
- **영향**: ADR-0003 작성자가 D17을 모르면 회전 정책을 "Open"으로 남길 가능성 → grill 결정과 충돌.
- **권고**: dispatch §A3 프롬프트에 "Additional input constraints from grill D17" 블록 추가. (배치 A1과 동일 패턴).

### G3 (Advisory). R4 보고서의 영속화 메소드 불일치

- **현상**: R4 §"불변식 #1" 본문이 `POST /layouts`라 단정. grill D12 / ADR-0010 SSoT는 `PUT /api/layout` + `If-Match: <etag>` (전체 교체, optimistic concurrency).
- **영향**: dispatch §A2 프롬프트는 D12를 인용해 PUT으로 덮으므로 ADR-0002 산출물 자체는 정합. 그러나 R4 보고서 본문이 그대로면 미래 독자가 혼동.
- **권고**: ADR-0002 작성 시 §맥락에서 "R4 §본문의 POST 단정은 D12로 supersede됨"을 명기. R4 보고서 자체는 (1차 자료 보고서이므로) amend 하지 않음.

### G4 (Advisory). ETag 페이로드 형식 모호

- **현상**: ADR-0010 SSoT 예시 `"etag": "<16B hex>"`(JSON 문자열, 32 hex 문자) ↔ grill D14 표 `etag(16B)`(WS binary payload).
- **영향**: 같은 ETag가 HTTP JSON에서는 문자열, WS 이진 envelope에서는 16바이트로 동시 존재. 해석 규칙이 ADR-0002 SSoT(`wire-protocol.md`)에 명시되지 않으면 직렬화 버그.
- **권고**: ADR-0002 SSoT에 "ETag는 16바이트 raw, JSON 표현 시 lowercase hex 문자열로 인코딩" 한 줄 고정.

### G5 (Advisory). sketch.md §4.3 amend 완료 여부 미확인

- **현상**: grill §2 표가 §4.3에 Server/Group/Unplaced Panel/M/I 추가를 지시. sketch.md §4.3 헤딩은 존재하나 amend 완료 여부는 본 리뷰 범위에서 본문 전수 검토 안 함.
- **권고**: sketch.md §4.3 본문이 grill §2 amend 표와 일치하는지 별도 task로 검증. 불일치 시 단순 amend.

### G6 (Cosmetic). dispatch 0002 헤딩 중복

- **현상**: `## 0. 공통 규칙` 과 `## 0. 배치 A0` 두 절이 같은 수준 "0"으로 들어가 있음.
- **권고**: 둘 중 하나를 `## 0.5` 또는 `## 1.0` 으로 재번호. 의미 영향 없음.

## 옵션 비교표 — 없음

본 보고서는 결정이 아닌 검증 산출물이므로 비교표 생략.

## gtmux에의 함의 (불변식 검증 포함)

배치 A0 결과는 5대 불변식에 영향을 주지 않으며, 오히려 ADR-0008이 #3(tmux Layout ≠ Canvas Layout)을 **기계적으로** 보장하는 구조로 격상시켰다. 본 리뷰의 갭 6건 중 어느 것도 불변식을 깨지 않는다.

| # | 불변식 | 현 상태 평가 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | 강화 — ADR-0009의 daemon 격리로 프로세스 경계까지 분리 |
| 2 | tmux-native vs web-only | 강화 — ADR-0008 allowlist 표로 코드 레벨 강제 가능 |
| 3 | tmux Layout ≠ Canvas Layout | 강화 — single-pane 컨벤션으로 trivial 보장 |
| 4 | 보안 기본값 | 변경 없음 — ADR-0003 작성 시 D17 누락(G2) 해소 필요 |
| 5 | control mode 사용 | 변경 없음 — ADR-0001 작성 시 D15/D16 흡수 예정 |

## 미해결 질문 / 후속 ADR 필요 항목

- **G1 해소 방향 결정**(추천: ADR-0010 부속으로 SSoT 초안 작성).
- **G2 dispatch §A3 정정**(D17 입력 제약 추가).
- **G3·G4 ADR-0002 작성 시 §맥락·SSoT 본문에 명시**.
- **G5 sketch.md §4.3 amend 검증** — 별도 task로 발행.
- **G6 헤딩 재번호** — dispatch 0002 사소 수정.
- 본 보고서를 dispatch §A0.5의 산출물(`0011-batch-a0-coherence-review.md`)로 인정할지 PM이 확인. 인정 시 batch A 진입 게이트 통과.

## 출처 (URL + 접근일자)

내부 문서 기반 — 외부 URL 없음.

- `docs/sketch.md` (전체) — 2026-05-13 검토
- `CONTEXT.md` — 2026-05-13
- `docs/plans/0001-research-plan.md` — 2026-05-13
- `docs/plans/0002-work-dispatch.md` (1–240행) — 2026-05-13
- `docs/reports/0001-tmux-control-mode.md` — 2026-05-13
- `docs/reports/0004-transport.md` — 2026-05-13
- `docs/reports/0005-security-model.md` — 2026-05-13
- `docs/reports/0010-grill-amendments.md` — 2026-05-13
- `docs/adr/0007-server-session-port-binding.md` — 2026-05-13
- `docs/adr/0008-single-pane-window-and-group.md` — 2026-05-13
- `docs/adr/0009-tmux-daemon-isolation.md` — 2026-05-13
- `docs/adr/0010-group-data-model.md` — 2026-05-13
- `docs/adr/0011-backend-stack-rust.md` — 2026-05-13 (§7 2차)
- `docs/adr/0012-frontend-stack-svelte.md` — 2026-05-13 (§7 2차)
- `docs/ssot/canvas-layout-schema.md` — 2026-05-13 (§7 2차)

---

## §7. 2차 coherence (A0.7) — Batch A0 6개 ADR 확장 검증

### 7.0 요약 (3문장)

배치 A0가 ADR-0011(Backend = Rust + axum + tokio)·ADR-0012(Frontend = Svelte 5 + Vite + TS) 추가와 ADR-0010 refining(`locked = OR`로 의미 수정), SSoT `canvas-layout-schema.md` 신규 작성으로 확장된 결과를 cross-check한 결과, 1차의 G1–G6은 모두 해소를 유지하지만 **ADR-0010의 OR/AND 정정이 부속 SSoT와 CONTEXT.md "Group 운영 규칙"에 전파되지 않은 상태**(G7) 및 **grill 보고서 D11에도 OR 정정이 반영되지 않은 상태**(G8)가 새로 드러났다. 두 건 모두 ADR-0010 본문은 옳지만 *주변 SSoT/공유 컨텍스트가 옛 정의를 들고 있으므로* 구현자(R7/R8/배치 A·B)가 어느 정의를 따라야 하는지 충돌이 생긴다 — Blocking으로 분류한다. 나머지 5건은 ADR-0011/0012 도구체인 표기 일관화·번호 충돌·5대 불변식 검증 강도 등으로 Advisory 수준에 머무른다.

### 7.1 조사 범위와 질문

- ADR-0007/0008/0009/0010 refined 본문 + ADR-0011/0012 신규 본문 + SSoT `canvas-layout-schema.md` + grill 0010 + CONTEXT.md + sketch.md §4.3·§6.4·§6.5·§7.4·§9.2·§10.1·§11.2·§11.3·§13.3·§14·§15 + plan 0002 (§0/§1/§2/§5)을 전수 cross-check.
- 구체 질문 8개 (PM 지시):
  1. ADR-0010 D6 OR/AND 정정이 CONTEXT.md "Group 운영 규칙" + grill D11 본문에 전파되었는가.
  2. ADR-0008 allowlist 표 ↔ ADR-0011 D10 `mux-router::Command` enum이 1:1 호환되는가.
  3. ADR-0011 D5 (`utoipa`/`schemars`) ↔ ADR-0012 D7 (`json-schema-to-typescript`/`quicktype`) ↔ SSoT JSON Schema 도구체인이 일관되는가.
  4. ADR-0010 + SSoT + ADR-0011 D5 + ADR-0012 D6의 ETag 표현(16B raw / 32 hex)이 단일 규칙으로 굳어졌는가.
  5. ADR-0012 D5의 "DOM-host 필터" 제약이 plan 0002 §2 Task B2(R3)에 명시되었는가.
  6. 1차 G1–G6 잔존 여부 재확인.
  7. ADR 번호·경로 충돌 (`reports/0011` vs `adr/0011` 등) 신규 발생 여부.
  8. 6개 ADR × 5대 불변식 = 30칸이 모두 PASS인지 + 검증 문구가 "근거 있는 PASS"인지(한 줄짜리 placeholder PASS 없음).

### 7.2 핵심 발견

#### 7.2.1 OR/AND 정정 전파 — Q1 (Blocking ×2)

ADR-0010 본문이 채택한 의미는 다음과 같다(D6 본문 + Open O2 + SSoT 정렬 표 명시).

- **effective visibility = self AND 모든 ancestor** (한 단계라도 hidden이면 hidden).
- **effective locked = self OR 모든 ancestor** (한 단계라도 locked이면 locked).

본 의미는 사용자 멘탈모델(잠금은 *cascade down*, 표시는 *cascade off*)과 정렬하며, `docs/adr/0010-group-data-model.md` D6 본문·SSoT 정렬 표(lines 60-62)·Open O2 (a)(c) 사례 모두 OR로 일관 진술됨.

그러나 두 곳에 **옛 AND 정의가 그대로 살아 있다**:

| 파일 | 줄 | 현 본문 | 정정안 |
|---|---|---|---|
| `CONTEXT.md` | 134 | "effective visibility/lock = self AND 모든 ancestor (AND 전파)." | "effective visibility = self AND 모든 ancestor. effective lock = self OR 모든 ancestor (한 단계라도 잠겨 있으면 self도 잠금). label/color는 가장 가까운 ancestor 값 inherit." |
| `docs/ssot/canvas-layout-schema.md` | 87 (§1.1 필드 의미 보강 표) | "`Group.locked / Panel.locked` — self 상태. effective = self AND 모든 ancestor." | "`Group.locked / Panel.locked` — self 상태. effective = self OR 모든 ancestor (한 단계라도 잠금이면 self도 잠금). ADR-0010 D6 참조." |
| `docs/reports/0010-grill-amendments.md` | 84 (§1 D11 본문) | "상태 전파: effective visibility/lock = self AND 모든 ancestor." | "상태 전파: effective visibility = self AND 모든 ancestor. effective lock = self OR (refining 시점 정정, ADR-0010 D6 참조)." |

→ **G7 (Blocking, CONTEXT.md + SSoT)**: 구현 1차 계약 산출물(SSoT)이 ADR과 의미가 다르면 R8/배치 A·B 산출물이 어떤 정의를 채택할지 결정 불가. ADR-0010 §SSoT 정렬 표(line 61)는 OR를 진술하지만 *같은 ADR이 가리키는 SSoT 파일* 본문(line 87)이 AND로 모순 진술 — ADR 안에서도 표면적 자기참조 불일치.
→ **G8 (Blocking, grill 0010 §1 D11)**: grill 보고서는 *근거 보고서*로 ADR이 인용하는 출처. ADR-0010 D6이 "grill D11에서 의미 정정함"이라고 진술하지 않은 한 D11 원문이 AND인 채로 남으면 후속 작성자(R7/R8/A1)가 ADR과 grill을 동시에 인용할 때 어느 쪽 정의를 따를지 모호. 본 보고서는 *grill 보고서는 1차 자료로서 amend 하지 않는다*는 1차 G3 정신과 충돌하지 않도록, 옵션 (a) grill에 "D11 후속 정정: locked=OR (refining 시 ADR-0010 D6에서)" 1줄 보강, 또는 (b) ADR-0010 §맥락에 "본 ADR은 grill D11의 AND 단정 중 lock에 한해 OR로 재정의함 — 근거: 사용자 멘탈모델 cascade-down lock"을 추가 — 두 가지 중 (b) 추천.

#### 7.2.2 ADR-0008 allowlist ↔ ADR-0011 `mux-router::Command` 매핑 — Q2 (PASS, Advisory ×0)

ADR-0008 §"tmux command allowlist 표"의 ALLOW(8행)는 다음과 같다.

| 명령 | 분류 |
|---|---|
| `new-window -t <session>` | mutate |
| `kill-pane -t %<pid>` | mutate |
| `kill-window -t @<wid>` | mutate (gtmux 내부 정리) |
| `rename-window -t @<wid> <label>` | mutate (label 동기화) |
| `send-keys -t %<pid>` | streaming |
| `refresh-client -A '%<pid>:pause/continue'` | streaming control |
| `refresh-client -B <subscription>` | subscription |
| `capture-pane -p -e -J -S -<lines>` | scrollback (P1+) |
| `list-sessions -F` / `list-windows -a -F` / `list-panes -a -F` | bootstrap |

ADR-0011 D10은 `mux-router::Command` enum이 위 표를 1:1 표현한다고만 진술하고 enum variant를 명시 enumeration 하지 않는다. **양쪽 자체 진술은 정합** — ADR-0008이 *정본 표*이고 ADR-0011은 "이 표를 enum으로 반영"이라는 약속만 두므로, R7-T7 scaffolding 단계에서 enum이 표와 1:1인지 검증하면 됨. 다만 ADR-0011 §"미해결" O7에서 R7-T7 산출물의 검증 기준 표현이 "역방향 의존(`mux-router` → `http-api`) 차단 lint"까지만 있고 *enum variant ↔ ADR-0008 표 1:1 검증*은 명시 없음.

→ **권고 (Advisory A1)**: ADR-0011 §Open O7 측정 기준에 "`mux-router::Command` enum의 variant 집합 = ADR-0008 allowlist 표의 ALLOW 행 9개와 정확히 일치(추가도 누락도 없음). 빌드 시 `cargo test`로 검증하는 정적 매핑 테스트 추가" 1행 보강.

#### 7.2.3 Schema 도구체인 일관성 — Q3 (Advisory ×1)

| 위치 | 도구 선택 |
|---|---|
| ADR-0011 D5 | `serde + serde_json` + JSON Schema 산출은 `utoipa` **또는** `schemars` (R7-T6) |
| ADR-0012 D7 | Rust `utoipa` **또는** `schemars` → JSON Schema → `json-schema-to-typescript` (또는 `quicktype`) → TS (R8) |
| SSoT §1 | Draft 2020-12, `additionalProperties: false` |
| SSoT §1 Schema 본문 | `"$schema": "https://json-schema.org/draft/2020-12/schema"` |
| ADR-0011 O5 (Open) | "ADR-0010 SSoT 100% 커버 + 빌드 시간 < 5s" |
| ADR-0012 O2 (Open) | "`utoipa` vs `schemars` 최종 선택은 axum 통합 깊이 + JSON Schema draft 호환성으로 결정" |

→ **A2 (Advisory)**: 두 ADR 모두 *최종 선택을 R7-T6 / R8 O2로 미루고 있어 합의 자체는 OK*. 그러나 (i) `schemars`가 Draft 2020-12를 *지원하지 않는다*(현 시점 안정 버전은 Draft 07만 산출)는 외부 사실이 SSoT의 `$schema` 선언과 충돌할 잠재 위험이 있다. (ii) `utoipa`는 OpenAPI 3.x 중심이며 JSON Schema 직접 산출은 *서브셋*이라 SSoT의 `additionalProperties: false` + `pattern` + `$defs` 조합을 round-trip 손실 없이 잡는지 R7-T6에서 직접 검증 필요. — ADR-0011 §Open O5와 ADR-0012 §Open O2의 측정 기준에 **"산출 JSON Schema가 SSoT §1 본문과 byte-equal (또는 정규화 후 동치)"** 한 줄 보강 권고. 이건 *합의 자체의 결함*은 아니라 측정 강도 보강이므로 Advisory.

#### 7.2.4 ETag 단일 표현 — Q4 (PASS)

네 산출물의 ETag 진술이 모두 같은 의미로 정렬됨.

| 출처 | 표현 |
|---|---|
| SSoT §2 표 | "16-byte raw가 정본. HTTP JSON = 32자 lowercase hex. WS payload = 16B raw. `If-Match`/`ETag` 헤더 = `"<32-hex>"`. 비교는 raw로 환원 후 상수시간." |
| ADR-0010 §SSoT 정렬 (line 91) | "`etag`: 16바이트 raw가 정본이며, HTTP JSON body에서는 lowercase hex 32자로 인코딩. WS `0x80 LAYOUT_CHANGED` envelope에서는 raw 16바이트. SSoT §2 참조." |
| ADR-0011 D3 (HTTP) + D4 (WS) | 직접 등장은 안 하지만 `axum` ETag 핸들러는 RFC 7232 강한 비교 + `If-Match`/412로 잠금 — SSoT §2와 호환. |
| ADR-0012 D7 (HTTPClient) + Open O5 (디바운스+412) | `If-Match` → 412 처리만 명시. raw/hex 변환은 백엔드와 byte-equal 산출(O2)로 자연 정합. |

→ **PASS**. 1차 G4 (ETag 모호) 완전 해소 확인.

#### 7.2.5 ADR-0012 D5 "DOM-host 필터" → plan 0002 §2 Task B2 — Q5 (PASS)

plan 0002 §2 Task B2 (R3) 추가 제약 블록(lines 279–284):

```
Constraint from PM: the cut-off filter is "can host an arbitrary DOM subtree
(xterm.js mounts a <div>) as a node while participating in pan/zoom".
Libraries that force canvas/WebGL rendering of node contents are eliminated
early. State the eliminated set explicitly.
```

ADR-0012 D5와 정확히 같은 제약. → **PASS**.

#### 7.2.6 1차 G1–G6 재검 — Q6

| 갭 | 1차 상태 | 2차 상태 |
|---|---|---|
| G1 (Blocking, SSoT 작성 시점 모순) | dispatch §A0.4와 ADR-0010 O2 모순 | **해소** — `docs/ssot/canvas-layout-schema.md` 작성 완료, ADR-0010 O4가 "해소" 명시 |
| G2 (D17 입력 누락) | dispatch §A3 프롬프트 누락 | **해소** — plan 0002 §A3 프롬프트(lines 206–216)에 D17 토큰 정책 명시 포함 |
| G3 (R4의 POST 단정) | A2 ADR 작성 시 §맥락에서 정리 권고 | **해소(준비)** — plan 0002 §A2 프롬프트(lines 155–157)에 "R4 §본문의 `POST /layouts` 단정은 grill D12에 의해 supersede" 명기 지시 포함 |
| G4 (ETag 페이로드 모호) | SSoT 작성 시 한 줄 고정 권고 | **해소** — SSoT §2가 완전 명세 + ADR-0010 본문 line 91이 동일 규칙 진술 |
| G5 (sketch.md §4.3 amend 검증) | 미확인 | **해소** — sketch.md §4.3 (lines 116–132)이 12개 어휘(Pane / Window / Session / tmux Layout / Server / Canvas / Panel / Canvas Layout / Group / M / I / Panel Streaming State) 모두 정의. 동시에 §6.4 (lines 196–215) z-index/overlap/Streaming 배지·§10.1 (lines 419–429) 백엔드 8개 구성요소·§11.2.D (lines 505–511) HTTP PUT + ETag·§11.3 (lines 513–528) 신규 제외 5항목·§14 (lines 746–755) 8개 난점·§15 (line 761) 선행 ADR 10개 모두 amend 완료 확인 |
| G6 (헤딩 중복) | dispatch 0002 `## 0` 충돌 | **해소** — plan 0002 line 5 `## 공통 규칙`(번호 제거) + line 13 `## 0. 배치 A0` 단일 |

#### 7.2.7 번호·경로 충돌 — Q7

| 경로 | 충돌 여부 |
|---|---|
| `docs/reports/0010-grill-amendments.md` ↔ `docs/adr/0010-group-data-model.md` | 1차에서 인지, 디렉터리 prefix로 분리 — 유지 |
| `docs/reports/0011-coherence-review.md` ↔ `docs/adr/0011-backend-stack-rust.md` | **신규 발생** — 같은 숫자 "0011" 이 reports와 adr 양쪽에서 점유. 디렉터리 prefix가 분리하지만 *짧은 인용*("ADR-0011" vs "보고서 0011")의 의도 명확화 필요 |
| `docs/reports/0012-…` ↔ `docs/adr/0012-frontend-stack-svelte.md` | reports 0012 미발행, 잠재 충돌. 보고서 다음 번호는 0013부터 사용 권고 |
| `docs/ssot/canvas-layout-schema.md` | 충돌 없음 |

→ **A3 (Advisory)**: 인용 컨벤션을 모든 ADR 본문과 plan 0002 §0/§1/§2/§5에서 "ADR-NNNN" / "R<N>" / "보고서 NNNN" 으로 명시 — 이미 대부분 준수. **신규 권고 — 다음 보고서 발행 시 0013부터 사용**(0011/0012 슬롯이 점유된 인상을 피하기 위해). plan 0002 §2 Task B6 후속 보고서 번호도 0013, 0014, 0015 등 사용 권고.

#### 7.2.8 30칸 불변식 검증 — Q8

| ADR | #1 | #2 | #3 | #4 | #5 | 비고 |
|---|---|---|---|---|---|---|
| 0007 | PASS | PASS | PASS | PASS | PASS | 5개 모두 단락 길이 PASS, 1차에서 확인 |
| 0008 | PASS | PASS | PASS | PASS | PASS | allowlist 표가 #2·#4를 기계적으로 보장 |
| 0009 | PASS | PASS | PASS(trivially) | PASS(강함) | PASS | #3는 *trivially* 명시 — 본 ADR 무관 |
| 0010 | PASS | PASS | PASS | PASS | PASS | 강도 보강됨(refining에서 강한 PASS로 격상) |
| 0011 | PASS | PASS | PASS | PASS(강함) | PASS | 모두 코드 경계 + 컴파일 강제 근거 동반 |
| 0012 | PASS | PASS | **PASS(강한 보장)** | **PASS** | N/A | #5 N/A는 의도적 (프론트는 control mode에 직접 접근 안 함) |

→ **PASS(강함)**. 단, ADR-0012 #5 = N/A는 *PASS 아님*. 30칸 중 29 PASS + 1 N/A. N/A의 근거가 본문에 명시("프론트엔드는 tmux control mode 채널에 직접 접근하지 않음. WS envelope(ADR-0002)이 추상화 레이어")되어 있으므로 *허용*. Plan 0002 §A0.6 DoD가 "5대 불변식 PASS"라고 적시했으나 N/A를 인정할지 plan 정정 필요 가능성.

→ **A4 (Advisory)**: plan 0002 §A0.6 DoD에 "control mode 무관 컴포넌트 ADR은 #5 N/A 허용 + 그 사유를 본문에 명시" 한 줄 보강 권고.

### 7.3 갭 (Findings, 2차 신규)

#### G7 (Blocking). ADR-0010 OR/AND 정정이 부속 SSoT + CONTEXT.md "Group 운영 규칙"에 미전파

- **현상**: §7.2.1 표 참조.
- **영향**: 1차 계약(SSoT) ↔ 정의 ADR ↔ 도메인 컨텍스트(CONTEXT.md)의 의미가 *세 개의 다른 문서에서 서로 다름*. R7/R8/배치 A·B 작성자가 어느 정의를 참조할지 무작위.
- **권고 (즉시 fix)**:
  1. `docs/ssot/canvas-layout-schema.md` line 87 정정 — `effective = self OR 모든 ancestor` 명시. (10초 작업)
  2. `CONTEXT.md` line 134 정정 — `effective lock = self OR 모든 ancestor (cascade down)` + `effective visibility = self AND 모든 ancestor`로 분리 진술. (1분 작업)
  3. ADR-0010 §"변경 이력"에 "2026-05-13 (2차): grill D11의 AND 단정 중 lock에 한해 OR로 의미 정정. 사용자 멘탈모델 cascade-down lock 근거." 1행 추가.
- **추천**: 본 보고서 승인 직후 PM 또는 self-review가 위 3개 파일 동시 commit.

#### G8 (Blocking). Grill 보고서 0010 §1 D11이 OR 정정을 반영하지 않음

- **현상**: `docs/reports/0010-grill-amendments.md` line 84가 옛 "AND 전파" 단정 유지. ADR-0010은 grill을 근거로 인용하지만, ADR이 grill 본문과 *어긋난 의미*를 결정하면 추적성 끊김.
- **영향**: G7과 동일 — 어느 출처가 정본인지 결정 불능.
- **권고**:
  - 옵션 (a) — grill 보고서 line 84 본문에 "(refining 시점 정정: lock은 OR. ADR-0010 D6 참조)" 1행 보강. *1차 자료 보고서 amend 금지 정신*과 충돌할 수 있음 (1차 G3 권고 참조).
  - 옵션 (b) — **ADR-0010 §맥락에 "본 ADR은 grill D11의 'effective visibility/lock = AND' 중 lock에 한해 OR로 의미 정정" 명기**. grill 보고서는 amend 없이 그대로 두고, ADR이 *발견적 정정 진술*로 추적성 유지.
- **추천**: 옵션 (b) — 1차 보고서 amend 금지와 정합.

#### G9 (Advisory, §7.2.2). ADR-0011 O7 측정 기준에 enum↔allowlist 1:1 검증 추가

- **현상**: ADR-0011 §"미해결" O7 측정 기준이 의존 그래프 DAG·역방향 lint까지만 명시.
- **권고**: O7 측정 기준에 "`mux-router::Command` enum variant 집합 = ADR-0008 allowlist ALLOW 행 9개와 정확히 일치. 정적 매핑 테스트(`cargo test`)로 강제" 1행 보강.

#### G10 (Advisory, §7.2.3). 도구체인 round-trip 측정 기준 강화

- **현상**: ADR-0011 O5 + ADR-0012 O2가 도구 선택을 R7-T6/R8에 위임. SSoT가 Draft 2020-12 + `$defs` + `additionalProperties: false` + `pattern`을 모두 사용하므로 산출 도구의 schema 호환성 검증이 필요.
- **권고**: 양쪽 Open 항목 통과 기준에 "산출 JSON Schema가 SSoT §1 본문과 byte-equal(또는 jsonschema 정규화 후 동치)"+"산출 TS 타입이 SSoT의 모든 `pattern`/`additionalProperties: false` 제약을 보존" 보강.

#### G11 (Advisory, §7.2.7). 보고서 번호 0011/0012 점유로 인한 인용 모호

- **현상**: `reports/0011` (본 보고서) + `adr/0011` (Backend stack)이 동일 숫자. 향후 `reports/0012`와 `adr/0012` (Frontend stack)도 잠재 충돌.
- **권고**: plan 0002 §2 B6 후속 보고서 번호를 0013부터 사용. 동시에 plan 0002 §1·§2의 *인용 컨벤션 한 줄* 추가 — "ADR-NNNN" vs "R<N>" vs "보고서 NNNN".

#### G12 (Advisory, §7.2.8). plan 0002 §A0.6 DoD의 "5대 불변식 PASS" 표현이 N/A 케이스를 허용하지 않음

- **현상**: ADR-0012 #5 N/A는 합리적이지만 plan 0002 §A0.6 DoD는 "5대 불변식 PASS"라고만 적시.
- **권고**: plan 0002 §A0.6 DoD에 "control mode 무관 컴포넌트 ADR은 #5 N/A 허용 + 그 사유를 본문에 명시" 단서 추가. ADR-0012는 이미 사유를 명시하므로 *현재 통과*.

#### G13 (Cosmetic, §11.2.C 잔존 표현)

- **현상**: sketch.md §11.2.C (line 500)이 "window별 grouping"이라는 옛 표현을 아직 보유. ADR-0008 D3 (tmux Window는 implementation-only, UI 비노출)과 어긋남.
- **권고**: sketch.md §11.2.C line 500 "window별 grouping" → "Group 트리(사이드바 layer panel)"로 교체. 다른 곳(§6.5 등)은 이미 amend 완료.

### 7.4 옵션 비교표 — 없음

본 §7은 검증 산출물이므로 비교표 생략.

### 7.5 gtmux에의 함의 (불변식 검증)

5대 불변식 영향 없음. G7·G8 해소 후 ADR-0010이 SSoT·CONTEXT와 의미적으로 일치하면 #2(tmux-native vs web-only) 검증 강도가 *문서 차원에서도* 일관해진다. ADR-0011의 모듈 분리(D10)·정적 타입 강제는 #1·#2·#4를 *컴파일 타임으로 격상*시켜 1차 평가와 동일하게 강화.

### 7.6 미해결 / 후속

- **G7·G8 해소 commit 필요** (3개 파일 inline 정정 + 1개 ADR ".변경 이력" 1행).
- **G9·G10 후속** — ADR-0011/0012 Open 측정 기준 1줄씩 보강 (Accepted 승격 전 마무리).
- **G11 후속** — plan 0002 §2 B6 후속 보고서 번호 0013+ 사용.
- **G12 후속** — plan 0002 §A0.6 DoD 단서 추가 (사소).
- **G13 후속** — sketch.md §11.2.C line 500 1단어 amend.
- 본 §7로 A0.7 2차 정합 리뷰 완료. **배치 A·B 진입 조건**: G7·G8 fix commit 후. G9–G13은 배치 A·B와 병렬 진행 가능.

### 7.7 변경 이력

- 2026-05-13 (2차): 본 §7 작성. ADR-0011/0012 + SSoT 추가 후 cross-check. 신규 Blocking 2건(G7·G8) + Advisory 5건(G9·G10·G11·G12·G13) 발견.
