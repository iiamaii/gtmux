# 보고서: 현재 진행 상태 및 보완 사항 분석 (임시)

- 일자: 2026-05-14
- 작성: PM
- 상태: **임시 문서** — 본 보고서는 의사결정 산출물이 아니라 진행 스냅샷과 보완 권고를 모은 일회성 노트다. 후속 결정이 내려지면 dispatch 0002 또는 각 ADR로 흡수되고 본 문서는 폐기 가능.
- 검토 범위: `docs/sketch.md`, `CONTEXT.md`, `docs/plans/0001/0002`, `docs/adr/0001~0003,0007~0012`(9개 Accepted), `docs/ssot/*`(3개), `docs/reports/0001~0014`(14건), `codebase/` 디렉터리 인벤토리

## 요약 (3문장)

배치 A0/A/B/C가 모두 closeout되어 **9개 ADR Accepted + 3개 SSoT + R1~R8 보고서 + 빌드 가능한 40-파일 코드 스켈레톤**이 정렬된 상태이며, smoke 9단계 중 2단계(`make build`/`make codegen`)만 PASS이고 나머지 6단계는 P0 미구현으로 GATE 상태다 — 즉 **sketch §15 1단계(엔진 연결 검증) 구현 진입 직전**의 상태로 정확히 멈춰 있다. 직전 분기의 차단성 갭(0011 §7 G7/G8 OR-AND 정정, 0013 §3.4 B1 `/auth/bootstrap` 누락, 0013 §3.6 B2 ADR-0006 미작성)은 모두 ADR/SSoT/dispatch에 흡수 또는 명시 deferral되었으며, **현 시점 차단성 갭은 0건**이다. 따라서 즉시 다음 액션은 0014 §dispatch-prompts의 Sprint 0 3-agent 병렬 dispatch(AUTH/CFG/MUX)이며, 그와 병렬로 추적할 carry-forward 11건은 본 보고서 §3·§4·§5에 분류된다.

## 1. 진행 매트릭스 (실측 인벤토리 기반)

`ls`로 확인한 파일 시스템과 0014 §"진행 매트릭스"의 진술을 cross-check.

| 단계 | 산출물 (실측) | 상태 | 비고 |
|---|---|---|---|
| Spec | `docs/sketch.md` (810행) + `CONTEXT.md` (140행) | ✅ amend 완료 | 0011 §7.2.6 G5 PASS |
| Grill | `docs/reports/0010-grill-amendments.md` (D1~D23) | ✅ | 단일 진실 |
| ADRs Accepted | 9개 — `adr/0001`(tmux 통합), `0002`(전송), `0003`(보안), `0007`(1:1:1), `0008`(single-pane+Group), `0009`(daemon 격리), `0010`(Group 모델), `0011`(Rust 스택), `0012`(Svelte 스택) | ✅ | ADR-0004/0005/0006 미작성(아래 §3 참조) |
| SSoTs | 3개 — `wire-protocol.md`, `security-defaults.md`, `canvas-layout-schema.md` | ✅ | OR/AND 정정 반영 여부는 §4 G7 추적 |
| Research reports | R1~R8 = `reports/0001~0008` | ✅ | R4 `POST /layouts` 단정은 ADR-0002에서 supersede 명기 (0011 §G3) |
| Coherence reviews | 0009 (A4), 0011 (A0.7 1차+§7 2차), 0013 (C5) | ✅ | 0011 §7 G7/G8 해소 commit `c0007ad` 반영 (handoff §commit-history 기준) |
| Code skeleton | `codebase/backend/crates/{auth,config,http-api,lifecycle,mux-router,ws-server}` + `codebase/frontend/src/{lib,routes,main.ts,styles}` + `shared/` + `smoke/01_engine_connect.sh` + `Makefile` + `README.md` | ✅ build/codegen/test PASS | 모든 함수 본문 `todo!()` |
| Smoke harness | `codebase/smoke/01_engine_connect.sh` | ✅ 2 PASS / 6 GATE / 1 MANUAL | 0012 §2 표 |
| **P0 구현** | **0건** | ⏳ **다음 단계** | Sprint 0/1/2/3 (§2 참조) |
| GitHub push | — | ⏳ blocked | macOS keychain (`SewingRobot` 캐시 충돌) — 사용자 직접 해결 |

## 2. 즉시 다음 액션 — Sprint 0 dispatch

`docs/reports/0014-session-handoff.md` §dispatch-prompts에 3개 task의 완전한 프롬프트가 이미 준비되어 있다. 의존 그래프상 모두 leaf 노드이며, 단일 메시지 내 3-agent 병렬 호출이 권장된다.

| Task | 담당 | 산출물 | 계약 |
|---|---|---|---|
| **P0-AUTH-1** | `backend-architect` 또는 `security-engineer` | `crates/auth/src/lib.rs` — `issue/load/save/verify/rotate_token` | ADR-0003 D4·D5, ADR-0011 D8, SSoT security-defaults §1.3·§3, Grill D17 |
| **P0-CFG-1** | `backend-architect` | `crates/config/src/lib.rs` — `Config` struct + figment loader + `derive_mode` | Grill D22, SSoT security-defaults §1·§3, R7 §config-loader |
| **P0-MUX-1** | `backend-architect` | `crates/mux-router/src/lib.rs` — `Event` enum + `parse_line` (winnow) + `decode_output_payload` (8진수) | R1 §3·§4, ADR-0001 D7, Grill D15·D16, R7 §parser |

**Sprint 0 PM 호출 패턴 (참고)**:
```
단일 메시지 내 3 Agent 호출:
  Agent(subagent_type="backend-architect", description="P0-AUTH-1", prompt=handoff §dispatch-prompts Agent #1 본문)
  Agent(subagent_type="backend-architect", description="P0-CFG-1",  prompt=handoff §dispatch-prompts Agent #2 본문)
  Agent(subagent_type="backend-architect", description="P0-MUX-1",  prompt=handoff §dispatch-prompts Agent #3 본문)
```

## 3. Carry-forward 항목 (Sprint 0과 병렬 가능)

handoff 0014 §carry-forward + 0013 §C5 + 0011 §7 잔여를 통합 분류.

### 3.1 ADR 발행 잔여 (3건)

| ADR | 입력 보고서 | 진입 시점 게이트 | 우선순위 |
|---|---|---|---|
| ADR-0004 (터미널 렌더링) | R2 = `reports/0002-terminal-rendering.md` | sketch §15 **3단계** (UX 폴리시) prereq — 0013 §3.6 B2 정정 결론 | P1 |
| ADR-0005 (캔버스 라이브러리) | R3 = `reports/0003-infinite-canvas.md` | 동상 (Svelte Flow @xyflow/svelte v1.5 잠금용) | P1 |
| ADR-0006 (영속화 storage) | R6 = `reports/0006-layout-persistence.md` | 동상 — 3단계 prereq (0013 §3.6에서 *1단계 prereq 아님* 정정 확정) | P1 |

→ **권고**: 3건 모두 plan 0002 §2 Batch B6 발행 큐. Sprint 0~2 진행과 충돌 없음. ADR 작성자(`backend-architect`/`frontend-architect`) 1명에게 직렬로 위임 가능.

### 3.2 0013 (C5) Advisory + Cosmetic 잔여 (7건)

| ID | 분류 | 위치 | 한 줄 |
|---|---|---|---|
| A1 | Advisory | ADR-0011 §Open O7 | enum↔allowlist 1:1 정적 매핑 테스트 measurement 추가 |
| A2 | Advisory | ADR-0011 §Open O5 / ADR-0012 §Open O2 | 코드젠 toolchain 통일(utoipa-only 결정 commit `e35fad7` 반영) 후 잔여 자취 정리 |
| A3 | Advisory | plan 0002 §2 B6 | 후속 보고서 번호 0015+ 사용 (0013·0014 점유) |
| A5 | Advisory | CI workflow `codegen-verify` | drift 감지 명령 명시 (Sprint 2 후 fix 가능) |
| C1 | Cosmetic | `crates/*/Cargo.toml` 또는 frontend 잔여 | schemars 자취 final 정리 |
| C2 | Cosmetic | `codebase/` 디렉터리 청결 | leftover 산출 (target/, node_modules/) gitignore 검증 |
| (0011 G11) | Advisory | plan 0002 §1·§2 | "ADR-NNNN" / "R<N>" / "보고서 NNNN" 인용 컨벤션 1행 추가 |

→ **권고**: Sprint 2 closeout 시 동반 정리. 별도 task 발행 불필요.

### 3.3 운영 잔여 (1건)

- **GitHub push 차단** (`iiamaii/gtmux`, macOS keychain `SewingRobot` 캐시 충돌) — 사용자가 keychain 정리 또는 SSH URL 전환. 자동 해결 시도 금지(handoff §안티패턴 1).

## 4. 5대 불변식 — 현 시점 평가

| # | 불변식 | 평가 | 근거 |
|---|---|---|---|
| 1 | tmux 상태 ↔ web 상태 분리 | 강화 | ADR-0009 daemon 격리 + ADR-0011 D10 `mux-router` crate 경계가 컴파일 타임 강제 |
| 2 | tmux-native vs web-only 분기 | 강화 | ADR-0008 allowlist 표 + `mux-router::Command` enum 9 variant (split/resize/select-layout 자체 부재) |
| 3 | tmux Layout ≠ Canvas Layout | 기계적 보장 | ADR-0008 single-pane-per-window 컨벤션 |
| 4 | 보안 기본값 | 컴파일 강제 가능 | ADR-0003 + SSoT security-defaults + auth crate API 계약 |
| 5 | control mode (`tmux -C`) | 단일 채널 보장 | ADR-0001 + lifecycle crate spawn 컨벤션 (`-L gtmux-<session>`) |

Sprint 0~2가 ADR 계약대로 구현되면 5대 불변식이 **코드 차원에서 컴파일·테스트로 강제**된다. 1단계 진입 후 회귀 위험은 `code-review-graph` MCP가 추적 가능 (handoff §"코드 그래프": 75 nodes / 210 edges / 8 communities, 빌드 완료).

## 5. 리스크 핫스팟 (P0 구현 시 주의)

코드 작성 전 미리 인지해야 할 함정. handoff §"안티패턴 / 함정"을 정리·확장.

| 리스크 | 발견 위치 | 권고 |
|---|---|---|
| `?token=` 쿼리스트링 토큰 | ADR-0003 R(rej)2 | `/auth/bootstrap` 1회 cookie 교환에서만 합법. ongoing auth는 `Authorization: Bearer` 또는 WS subprotocol. |
| `VIEWPORT_CHANGED` endian | R8 sketch 초안 BE → 정정 LE | Rust `i32::to_le_bytes` + JS `DataView.getInt32(o, true)`. |
| Lock 전파 의미 | 0011 §7 G7/G8 정정 | **lock = OR (cascade-down)**, **visibility = AND**. CONTEXT.md + SSoT + ADR-0010 정합 (commit `c0007ad`). |
| Codegen toolchain | A4 §A2 통일 | **utoipa 5.x + openapi-typescript 단일**. schemars + json-schema-to-typescript는 supersede. R8 §F2는 *역사적 분석*. |
| 금지 명령 발급 | ADR-0008 allowlist | `split-window` · `resize-pane` · `select-layout` enum variant 자체 부재 (컴파일 타임 강제). |
| WS client identity | Grill D13 MT-3 | `client_id` 도입 시도 금지. 모든 ephemeral 상태는 broadcast. |
| Placement 정책 | Grill D23 | `optional + cascade`. 옛 D7("사용자 명시 입력만") + Unplaced Panel 트레이는 폐기. |
| ETag 형식 | 0011 §7.2.4 PASS | 16B raw 정본, JSON 32-hex, WS payload raw 16B. SSoT §2 단일 규칙. |
| JSON Schema round-trip | 0011 §7 G10 advisory | utoipa 산출 schema가 SSoT §1과 byte-equal인지 R7-T6에서 검증 (잠재 위험). |

## 6. 의존 그래프 — Sprint 시퀀스 (handoff 0014 §"다음 단계" 인용)

```
Sprint 0  (parallel, leaves)
├─ P0-AUTH-1  (auth crate)
├─ P0-CFG-1   (config crate)
└─ P0-MUX-1   (mux-router crate: parser + decoder)
        ↓
Sprint 1  (critical path, sequential)
├─ P0-LIFE-1  (lifecycle::spawn_daemon)  ← AUTH-1, CFG-1
└─ P0-CLI-1   (Cmd::Start binding)        ← LIFE-1, AUTH-1, CFG-1
        ↓
Sprint 2  (parallel)
├─ P0-HTTP-1  (http-api::router + middleware)  ← AUTH-1, CFG-1
├─ P0-HTTP-2  (bootstrap_handler 1회 cookie 교환)  ← AUTH-1  [0013 B1 해소]
├─ P0-WS-1    (ws-server::router + envelope codec)  ← AUTH-1, MUX-1
├─ P0-LIFE-2  (lifecycle::teardown 5단계)
└─ P0-CLI-3   (Cmd::Teardown)  ← LIFE-2
        ↓
smoke re-run (SMOKE_GATE_RUNTIME=0)  →  9 step 모두 PASS 검증
        ↓
Sprint 3  (frontend, parallel with Sprint 2 가능)
├─ FE-1  WS dispatcher (envelope decode + store fan-out)
├─ FE-2  Canvas + 1 Panel (xterm.js mount, R8 F6 옵션)
└─ FE-3  Reconnect banner + grace 1s (Grill D21 c2/c3)
        ↓
sketch §15 1단계 (엔진 연결 검증) 정식 통과
```

ADR-0004/0005/0006 발행은 위 그래프와 직교(P1 작업). Sprint 0~3 진행과 병렬 가능.

## 7. 보완 사항 — PM 권고 (직접 수정은 별도 dispatch 후)

다음은 본 보고서가 *발견*만 한 항목이며, 실제 수정은 별도 task 발행으로 처리해야 한다(본 보고서는 직접 수정을 수행하지 않음).

1. **(즉시)** Sprint 0 dispatch — handoff §dispatch-prompts의 3개 프롬프트를 단일 메시지로 병렬 호출.
2. **(병렬)** ADR-0004/0005/0006 발행 큐를 plan 0002 §2 B6에 정식 task로 분해 — 현재는 0014 §carry-forward에 한 줄 진술만 존재.
3. **(병렬)** 0013 C5 Advisory 7건(A1/A2/A3/A5 + C1/C2 + 0011 G11)을 묶어 `quality-engineer` 또는 `self-review`에 일괄 위임 — Sprint 2 closeout 시점 권장.
4. **(저우선)** GitHub push credential은 사용자 환경 문제이므로 PM 범위 밖 — 안내만 유지.
5. **(검증 발견)** ADR-0011 §Open O7 + ADR-0012 §Open O2의 measurement 강화(0011 G9/G10)는 *Sprint 0 완료 후* `cargo test` 실측이 가능해진 시점에 reopen — Sprint 0 PR review 게이트에 합류.
6. **(문서 정합)** 본 보고서를 dispatch 0002 §0 또는 §3에 *진행 스냅샷 reference*로 1줄 추가 — 다음 세션이 본 문서를 빠르게 찾도록.

## 8. 옵션 비교표 — 없음

본 보고서는 검증/스냅샷 산출물이므로 비교표 생략.

## 9. gtmux에의 함의 (불변식 검증) — §4 참조

5대 불변식 전부 강화 또는 유지. Sprint 0~2가 ADR 계약대로 구현되면 코드 차원에서 컴파일·테스트로 강제 가능. 본 시점에 불변식 위협 0건.

## 10. 미해결 / 후속

- §7의 6개 권고 중 (1) Sprint 0 dispatch가 단일 차단성 액션.
- §3.1 ADR-0004/0005/0006 발행은 1단계 진입을 차단하지 않으나, 3단계 진입 전 완료 필요.
- §3.2 Advisory/Cosmetic 7건은 Sprint 2 closeout 시 묶음 처리.
- 본 임시 문서는 Sprint 0 완료 후 별도 진행 보고서(`reports/0016-…`)로 갱신 권장. 이후 폐기 가능.

## 11. 출처 (URL + 접근일자) — 없음, 내부 문서만

- `docs/sketch.md` — 2026-05-14
- `CONTEXT.md` — 2026-05-14
- `docs/plans/0001-research-plan.md`, `0002-work-dispatch.md` — 2026-05-14
- `docs/adr/0001~0003,0007~0012` (9개) — 2026-05-14
- `docs/ssot/{wire-protocol,security-defaults,canvas-layout-schema}.md` — 2026-05-14
- `docs/reports/0001~0014` (14건, 특히 0010/0011/0012/0013/0014 cross-check) — 2026-05-14
- `codebase/` 디렉터리 인벤토리 (실측) — 2026-05-14

## 12. 변경 이력

- 2026-05-14: 초안 (Sprint 0 dispatch 직전 PM 스냅샷)
