# Handover (BE) — Clipboard + Shift constraint + Multi-select context menu

> Cold-pickup 가능한 BE agent 인계 문서. 본 batch 의 3 ADR (0030/0031/0032) 의 BE 작업만 정리.
>
> - 생성일: 2026-05-17
> - 대상 agent: BE (Rust / axum / mux-router)
> - 동반 FE handover: `docs/reports/0057-fe-handover-clipboard-shift-rightclick.md`

---

## 1. Scope summary

본 batch 의 **BE 변경은 거의 0** — 세 ADR 모두 *입력 단계의 좌표 변환* 또는 *FE-only state/UI* 패턴.

| ADR | BE 영향 |
|---|---|
| ADR-0030 (Clipboard) | △ **Terminal item clone (Slice B)** 에서만 — 기존 `POST /api/terminals` (terminal spawn) endpoint 가 *이미 존재* 한다면 변경 0. 없거나 signature mismatch 면 §2 의 작업. |
| ADR-0031 (Shift constraint) | **0** — schema/wire 변경 없음. modifier 의 좌표 변환은 FE 단계, 최종 commit schema 는 일반 좌표. |
| ADR-0032 (Multi-select context menu) | **0** — batch 액션은 `applyMutation` 의 *single PUT* 로 표현 (ADR-0028 정합). 기존 `PUT /api/sessions/<name>/layout` endpoint 그대로 사용. |

→ **본 batch 의 BE Slice 는 ADR-0030 의 Terminal clone 한 항목**.

## 2. ADR-0030 Slice B — Terminal clone endpoint

### 2.1 Endpoint 점검

FE 의 paste 흐름 (FE handover §2.4) 에서 terminal item paste 시 다음 endpoint 호출 의도:

```
POST /api/terminals
Body: { label?: string, cwd?: string, command?: string[] }
Response: { id: "uuid", pane_id: number, label, ... }
```

**작업**:
1. `codebase/backend/crates/http-api/src/terminals.rs` (또는 라우팅 정본 파일) 에서 `POST /api/terminals` 핸들러 존재 확인.
2. 존재 시: signature 가 위와 호환 인지 검증 (특히 response 의 `id` 가 layout schema 의 `terminal_id` 와 동일 타입 — ADR-0018 D3 정합).
3. 부재 시: 신규 추가. 정본 = ADR-0021 D8 의 [Change terminal] 의 *spawn 경로* 와 동일 — 기존 코드 재사용 가능.

### 2.2 Authorization / rate-limit

- Endpoint 는 cookie auth (ADR-0020) 통과 + active session 의 terminal pool 안에 spawn.
- Rate limit: 사용자가 빠르게 Cmd+V 반복 시 spawn 폭주 가능. 별도 rate limit 검토 (예: 1 second 내 N spawn 제한). 단 *별도 ADR* 필요 시 본 batch 외.

### 2.3 Mirror paste (P1)

ADR-0030 D3 의 (b) "Paste as mirror" 는 P1 — 본 batch 외. 추후 별도 ADR (ADR-0021 D7 의 mirror 정책 확장).

### 2.4 Test plan (BE)

- `POST /api/terminals` 의 unit test — body 검증, response shape, error path (active session 없음 → 4xx).
- Integration test — FE paste 흐름 의 새 terminal_id 가 후속 `PUT /layout` 의 item.terminal_id 와 match 되는지.

## 3. ADR-0031 (Shift constraint) — BE 작업

**없음**. 명시적 0 — modifier 가 좌표 변환만, schema 변경 없음. 기존 `PUT /api/sessions/<name>/layout` 의 `{ x, y, w, h }` 그대로.

## 4. ADR-0032 (Multi-select context menu) — BE 작업

**없음**. batch 액션 (Hide all / Lock all / Delete all / Z batch / Align batch) 은 모두 `applyMutation` 의 *single PUT* 로 표현 — ADR-0028 의 Undo/Redo 정합 으로 1 history entry. BE 의 `PUT /layout` etag rebase 가 그대로 동작.

**검증 항목**: BE 의 `PUT /layout` 이 *대량* item 변경 (예: 50 items 의 z batch update) 시 성능 회귀 없는지. ADR-0028 의 layoutSnapshot/applyMutation 의 payload 크기 — 큰 회귀 발견 시 별도 plan.

## 5. 권장 진행 순서 (BE)

1. **Phase 0 — Endpoint 점검 (§2.1)** — 1-2 hour 작업. 존재 시 *no-op*, 부재 시 Slice 후 BE-NEW.
2. **Phase 1 — Rate-limit 결정 (§2.2)** — 필요 시 별도 ADR. 본 batch 외.
3. **Phase 2 — Performance regression test (§4)** — `PUT /layout` 의 batch payload 시뮬레이션. 결과 양호 시 *no-op*.

## 6. Coupling / 의존성

- FE Phase 4 (handover §6) 가 본 Slice B 의 endpoint 의존. FE Phase 3 까지는 BE 변경 없이 진행 가능 — terminal paste 만 disable 한 채.
- 본 batch 의 *다른 BE 작업* 없음 — FE land 와 *완전 독립*.

## 7. Open questions (BE 결정 사항)

- **Q1.** `POST /api/terminals` 의 *최소 body* — clone 시 원본 terminal 의 cwd / command 를 복사 하는 게 자연 인지, 빈 shell 로 시작 인지. 사용자 mental model = *"새 sibling shell"* 이라면 빈 shell 도 OK. *원본 inherit* 가 자연 이라면 FE 가 body 에 cwd 전달.
- **Q2.** Rate-limit 의 spec (§2.2) — 별도 ADR 필요한지 또는 ADR-0030 amend 로 충분한지.
