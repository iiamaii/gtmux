# 보고서: R6 — Canvas Layout 영속화 (저장 백엔드·원자성·마이그레이션)

- 일자: 2026-05-13
- 작성: deep-research (B3, plan 0002 §2)
- 입력: `docs/src/prompt_research_handoff.md` §4 R6, `docs/sketch.md` §11.2.D·§10.1·§13.3.5, `docs/reports/0010-grill-amendments.md` D11·D12·D14·D17·D22, `docs/ssot/canvas-layout-schema.md`, `docs/adr/0010-group-data-model.md`, `docs/adr/0011-backend-stack-rust.md` D5
- 후속 ADR: ADR-0006 (Canvas Layout 영속화 storage)

## 요약 (3문장)

Canvas Layout(< 50 KB 일반·256 KB cap)은 **`<XDG_STATE_HOME>/gtmux/<session>.layout.json` 단일 JSON 파일 + `atomic-write-file` 크레이트 기반 rename-over-temp + fsync(file)+fsync(dir) + 권한 0600** 패턴으로 영속화한다. SQLite·redb·sled 등 임베디드 DB는 단일 사용자·단일 라이터·단일 키 한정 워크로드(payload 한 덩어리)에서 startup·운영 복잡도가 명백한 손해이며, ADR-0011이 정한 serde + axum 스택과 가장 마찰 없는 선택은 plain JSON이다. 마이그레이션은 `schema_version` 정수 + lazy upgrade(로드 시 v_n→v_n+1 변환 함수 체인) 패턴을 채택하고, MVP는 v=1 고정이므로 *변환 함수 0개 + reject-on-unknown-version*만 구현하며, 손상·검증 실패 시 reject + `<session>.layout.json.corrupt-<ts>`로 sidecar 격리 후 빈 layout으로 부팅한다.

## 조사 범위와 질문

핸드오프 R6 + plan B3 추가 제약이 정한 9개 의무 조사 항목:

1. 저장 백엔드 후보 (JSON 파일 / SQLite / sled / redb / TOML)
2. 원자성 (rename-over-temp, fsync, partial-write recovery)
3. 동시 쓰기 보호 — D2의 Server:Session:Port 1:1:1 모델에서 추가 락이 필요한가
4. 손상·복구 정책
5. 마이그레이션 정책 (schema_version=1 → n)
6. 파일 권한 0600 (D17 토큰 파일 패턴 재사용)
7. 트리 무결성 검증 라이브러리 (serde_valid vs validator)
8. PUT-and-flush 레이턴시 목표 (D19 < 30 ms HTTP RTT)
9. 사용자 직접 편집 지원 여부

전송 결정(D12 = HTTP, WS는 `0x80 LAYOUT_CHANGED` notify 한정)과 스키마(SSoT `canvas-layout-schema.md`, ETag = 16-byte raw / 32-hex)는 *이미 확정*이며 본 보고서는 인용만 한다.

## 핵심 발견

### F1. 워크로드 형상 — "단일 키 blob"

D12에 따라 `GET/PUT /api/layout`은 *전체 교체* 모델이며 PATCH는 P1+로 deferred. SSoT §3 R9가 256 KB soft cap을 강제. D2의 1:1:1 모델 = 한 Server 프로세스가 한 `<session>.layout.json`을 유일하게 쓴다. 즉 영속화 워크로드는 **(a) 한 파일 (b) 한 라이터 (c) 한 덩어리 blob (d) 정렬 인덱싱·범위 쿼리 없음 (e) 쓰기 빈도 ≤ 1/300 ms (D22 `layout_debounce_ms = 300`)** 다. ACID 중 의미 있는 것은 *Durability + Atomicity*뿐이고 Isolation·트랜잭션 격리는 본질적으로 불필요.

이 형상에서 SQLite/redb는 *전체 페이로드를 BLOB 한 행에 저장*하는 형태로만 의미가 있고, 이는 결국 "DB 안의 큰 JSON blob"이며 startup·바이너리 크기·운영 절차(.db-wal/.db-shm 동반 파일, 백업 시 동일 트랜잭션 캡처 필요)만 늘린다.

### F2. 저장 백엔드 후보 비교

표 기준: gtmux 워크로드(단일 사용자, 단일 키 blob, < 256 KB, < 1 write/300 ms, 단일 라이터).

| 후보 | 동시성·ACID | 바이너리·런타임 비용 | 사용자 직접 편집 | 백업 | gtmux 적합도 |
|---|---|---|---|---|---|
| **plain JSON + atomic write** (권장) | rename atomic + fsync로 *Durable atomic update* 충족 [src1·src2]. 격리 불필요. | rusqlite/redb 추가 의존성 0. serde_json은 ADR-0011 D5에서 이미 선택. | 가능 (텍스트). | `cp`/git 그대로 | **★★★★★** — 워크로드와 1:1 |
| TOML | JSON과 거의 동일하나 SSoT가 JSON Schema라 변환 비용 발생. ETag 헥스/숫자 표현 비표준. | serde_json 외 toml 크레이트 추가. | 가능. | `cp` | ★★ — schema 정합성 손해. config(.toml)은 별도(D22)이므로 layout은 JSON 유지가 자연. |
| **SQLite (`rusqlite`)** | 풀 ACID + WAL. 단일 BLOB row 저장 시 ACID 이점 미사용. | C 의존성 (`libsqlite3` 또는 `bundled` feature → 빌드 ~5 MB, 바이너리 +2 MB). cross-compile 시 `bundled` 강제. [src3·src4] | 불가 (binary file, sqlite3 CLI 필요). | `.dump` + WAL 정합성 주의 | ★★ — over-engineered. 단, P1+ multi-session·다중 백업 슬롯이 필요해지면 재검토. |
| **redb** | ACID + MVCC, 단일 라이터 [src5·src6]. pure Rust, C 의존 없음. | crate ~600 KB. on-disk 포맷 안정 약속 있음. | 불가. | 파일 단일 (.redb) | ★★★ — SQLite보다 깔끔. 그러나 *키 1개*에 B-tree 인프라는 과잉. |
| **sled** | beta. on-disk 포맷이 1.0 전까지 변경 가능 [src7·src8]. | crate 큼. disk 사용량 비효율 보고됨 [src8]. | 불가. | 파일 + 동반 디렉터리 | ★ — pre-1.0 포맷 휘발성·디스크 비효율로 MVP 부적합. |

**결정: plain JSON + atomic write + 0600.** 후술 F3~F7.

### F3. 원자성 — rename-over-temp + fsync(file) + fsync(dir)

POSIX `rename(2)`는 *동일 파일시스템 내* 운영 원자성을 제공하지만 *크래시·전원 단절 시 디스크에 도달했음을 보장하지 않는다* [src1·src9·src10]. ext4 초기엔 zero-length 파일 문제가 있었으나 현행 커널은 `auto_da_alloc`으로 rename-over-temp 패턴을 감지·보호 [src10]. 그럼에도 *포터블*하게 안전하려면:

1. 같은 디렉터리에 `<final>.tmp.<rand>` 생성 (cross-device rename 금지)
2. 페이로드 write
3. `fsync(file_fd)` — 데이터·메타데이터 디스크 도달
4. `rename(tmp, final)` — 원자적 교체
5. `fsync(dir_fd)` — 디렉터리 엔트리 변경 영속화 (이게 빠지면 크래시 후 *옛 파일이 부활*할 수 있다 [src9·src11])

Rust 생태에서 이 5단계를 한 번에 캡슐화하는 표준 크레이트 = **`atomic-write-file` 0.3+** [src12]. Unix에서 `openat`+`fsync`+`renameat`을 사용하고 권한·소유권 보존 옵션 제공. Linux의 `unnamed-tmpfile`(O_TMPFILE) feature는 프로세스가 commit 전에 죽을 때 orphan tmp 파일을 0개로 만든다. `tempfile::NamedTempFile::persist`는 *디렉터리 fsync를 보장하지 않는다* [src13] — 동일 패턴을 직접 작성 시 추가 호출이 필요하므로 `atomic-write-file`을 채택한다.

**결정 (Atomicity-A):** `atomic-write-file = "0.3"` (또는 동등) 사용. Linux 빌드는 `unnamed-tmpfile` feature를 켠다. macOS는 fallback 경로 사용.

### F4. 동시 쓰기 보호 — *추가 락 불필요* (D2로 충분)

D2(Server:Session:Port 1:1:1) + D21 c7(port 충돌 시 exit 4)로 한 `<session>.layout.json`을 두 Server가 동시에 열 가능성은 *프로세스 인스턴스 레벨에서 이미 차단*된다. 그러나 사용자가 두 번 `gtmux start --session foo`를 호출한 직후 c7 검사 사이의 *TOCTOU 윈도*가 이론상 존재한다 → 방어 심층화 차원에서 Server 부팅 시 **`<XDG_RUNTIME_DIR>/gtmux/<session>.pid` 위에 `fd-lock`/`fs2` advisory exclusive lock**을 잡고, lock 실패 시 exit 4로 일관 종료한다 (이미 D20·D22가 pid 파일을 정의함을 활용).

이 lock은 *프로세스 단일성*을 보장하므로 layout 파일 자체에는 락이 불필요하다.

**결정 (Concurrency-A):** layout 파일에 락 없음. 프로세스 단일성은 `<session>.pid` advisory lock으로 강제 (`fd-lock` 또는 std 1.81+의 `File::lock_exclusive`).

### F5. 손상·복구 정책

부팅 시 layout 파일 상태 7가지와 처리:

| 상태 | 처리 | 사용자 통지 |
|---|---|---|
| 부재 | 빈 layout (`{etag:"00..0", schema_version:1, groups:[], panels:[]}`) 메모리 초기화. PUT 첫 호출 시 파일 생성. | 부팅 로그 `layout: cold start (no file)` |
| 정상 파싱 + schema valid | 그대로 로드 | (없음) |
| 0 바이트 (이전 크래시) | F3 atomic write가 동작했으면 발생 불가. 발생 시 손상 취급. | corrupt 격리 |
| JSON parse 실패 | 손상 취급. `<file>` → `<file>.corrupt-<unix_ts>` rename, 빈 layout으로 부팅. | stderr WARN 1줄 + `gtmux status` 노출 |
| schema_version 미존재 또는 알 수 없는 정수 | 미지원 버전 취급. 손상 처리와 동일 격리 + 빈 layout. | WARN `unsupported schema_version=X` |
| schema_version=1이지만 SSoT §3 검증 실패 (R2~R7) | 위와 동일 격리. | WARN + 실패 룰 번호 로깅 |
| 권한 != 0600 | 부팅 시 자동 chmod 0600 + WARN. write 시점부터는 atomic-write-file의 권한 보존이 0600 유지. | WARN 1줄 |

**손상 시 자동 복원 미도입 (MVP):** D15가 ring buffer를 디스크에 두지 않은 것과 동일한 사유 — *복원 대상이 사용자 의도(panel 배치 등)이므로 잘못된 복원이 더 큰 손실*. 사용자가 `.corrupt-<ts>` sidecar를 직접 검수·수기 복원하는 escape hatch만 제공.

거절: (a) 자동 `.bak` 회전 — 사용자 git 사용 빈도가 더 높음 (P2 backup preset에서 재검토). (b) checksum 헤더 — JSON 자체가 텍스트로 변조 탐지 어렵고, 이미 `etag`가 hash로 작동하므로 중복.

**결정 (Recovery-A):** reject + sidecar 격리 + 빈 layout 부팅. 자동 백업 회전 없음.

### F6. 마이그레이션 정책

[src14·src15] 기반 권장 패턴 = **정수 `schema_version` + lazy upgrade + 명시 reject of unknown future versions**. gtmux MVP는 `schema_version = 1` 고정 (SSoT §1).

- **v=1만 존재 (현재):** 변환 함수 0개. 로더는 `schema_version != 1` → F5의 "미지원 버전 격리" 경로.
- **v=N (미래) 도입 시 ADR-0006 업데이트가 다음을 추가한다:**
  1. `fn migrate_v_k_to_v_k_plus_1(prev: JsonValue) -> Result<JsonValue, MigrationError>` — k = 1..N-1.
  2. 로더가 `schema_version`을 읽고 v=N까지 *체인 적용*. 각 단계는 backward-incompatible 변경(필수 필드 추가, 타입 변경)도 명시적으로 처리 가능.
  3. 마이그레이션 성공 시 메모리 표현은 항상 최신(v=N). 첫 PUT에서 v=N 형식으로 파일을 다시 쓰고, 직전 파일은 `<file>.v<k>.bak`로 1회 보존(P1+ 옵션).
  4. **Backward compat 권고**: 새 필드는 *optional + default value* (예: `additionalProperties: false`는 유지하되 새 필드는 `default`/`Option`으로 도입). 필수 필드 추가·타입 변경은 schema_version 증가 필수.
  5. ETag 정의는 *영속화된 v=N 페이로드의 hash*. v_k 파일을 migration으로 읽었더라도 사용자가 PUT으로 commit하기 전까지는 ETag가 "00..0" 또는 v_k 시점 값 — 이 의미를 ADR-0006 §2에서 명문화.

거절: (a) semver 사용 — 단일 파일 단일 schema이므로 메이저·마이너 구분 가치 없음. 정수 monotonic이 단순. (b) 자동 forward-compat (unknown field 무시) — SSoT §3 R1이 `additionalProperties: false`로 schema drift를 *의도적으로 차단*. 새 필드는 반드시 schema_version bump를 동반해야 함. (c) lazy 마이그레이션 vs eager(부팅 시 강제 재기록) — eager는 부팅 latency·복구 가능성에 손해. lazy 채택.

**결정 (Migration-A):** 정수 `schema_version` + `migrate_v_k_to_v_k_plus_1` 함수 체인 + lazy upgrade + unknown future version reject. MVP는 변환 함수 0개.

### F7. 파일 권한 — 0600 (D17 패턴 재사용)

D17 토큰 파일이 이미 0600(`${XDG_STATE_HOME}/gtmux/<session>.token`)이고, layout 파일은 *세션 라벨·노트·panel 좌표·tmux pane id mirror*를 포함하므로 §13.3.5("저장 데이터 노출")의 보호 대상이다. 같은 디렉터리·같은 권한 규칙으로 통일한다.

Rust 구현은 `std::os::unix::fs::PermissionsExt::from_mode(0o600)` + `fs::set_permissions` [src16]. `atomic-write-file`은 기본적으로 *기존 파일 권한을 보존*하므로, 최초 생성 시점에만 명시 0600 설정 + 부팅 시 audit chmod로 충분. Windows는 MVP 비범위(D20 — tmux unix-only)이므로 ACL 처리 불필요.

부모 디렉터리 `<XDG_STATE_HOME>/gtmux/` 자체는 0700 (D10이 정한 tmux 소켓 디렉터리 컨벤션과 정합).

**결정 (Perm-A):** layout 파일 0600, 부모 디렉터리 0700. 부팅 시 audit + 자동 정정. atomic-write-file 권한 보존 옵션 ON.

### F8. 트리 무결성 검증 — serde + 추가 검증 함수(라이브러리 의존 ↓)

SSoT §3 R1~R9는 9개 룰이며 그 중:

- R1 (JSON Schema 합치): serde의 `#[serde(deny_unknown_fields)]` + `#[derive(Deserialize)]`의 타입 강제로 `additionalProperties: false`·필수 필드·`pattern` 외 대부분 자동 충족. 정규식 pattern은 `regex` 크레이트 + `validator` derive(`#[validate(regex(...))]`)로 처리 가능.
- R2 (ID 유일성), R4 (parent 존재), R5 (사이클 금지), R6 (다중 부모 금지), R7 (Panel.parent_id는 Group만): 도메인-specific 그래프 검증 — *어느 범용 라이브러리도 정확히 표현 못한다*. 직접 구현 (DFS).
- R3 (Panel.pane_id 존재성): 서버 런타임 상태(`mux-router` mirror set) 참조 — 어차피 직접 구현.
- R8 (ETag), R9 (256 KB cap): HTTP 레이어(axum/tower-http).

`serde_valid` [src17] vs `validator` 비교:

| 측면 | `serde_valid` 0.24 | `validator` 0.18 |
|---|---|---|
| 모델 | JSON Schema 기반 attr 매핑 | 자체 macro DSL |
| Async | 미지원 | 0.16+ async 지원 |
| 코드 생성 | derive | derive |
| 의존성 무게 | 가벼움 | 가벼움 |
| gtmux 적합 | SSoT가 JSON Schema → 매핑 자연 | 일반적이고 학습 데이터 풍부 |

두 라이브러리 모두 *그래프 검증(R2/R4/R5/R6/R7)*은 표현 불가. 결국 검증의 50%는 직접 코드. 따라서 라이브러리 의존을 *얇게* 가져가는 게 합리.

**결정 (Validation-A):** serde derive + `deny_unknown_fields` + 명시적 검증 함수 모듈 `http-api::validate`. 추가 derive 검증 크레이트는 **`validator`** 1개만 도입(정규식 패턴 강제용 — `Panel.id`, `Group.id`, `pane_id`, `color`). R8/R9는 axum 미들웨어 레이어. 그래프 검증(R2·R4·R5·R6·R7)은 직접 구현하고 100% 단위 테스트 커버.

### F9. 성능 — PUT-and-flush 레이턴시

D19 예산: `Panel drag commit → 모든 연결 sync 완료 < 500 ms`. 이 안에 `PUT /api/layout` RTT 가 들어가야 하며, R6 핸드오프는 *< 30 ms HTTP RTT*를 목표로 설정.

성분:
- JSON 직렬화: serde_json 600–900 MB/s [src18·src19] → 256 KB 페이로드 ≈ 0.3–0.4 ms.
- JSON 역직렬화 (요청 파싱): 500–1000 MB/s → 0.3–0.5 ms.
- 그래프 검증 (R2·R4·R5·R6·R7) over O(N) 노드, N ≤ 256 KB/100 B ≈ 2.5K nodes 상한: 마이크로초대.
- 디스크 write + fsync(file) + fsync(dir) on macOS APFS SSD / Linux ext4 NVMe: 1–10 ms (단일 트랜잭션). 일반 layout(< 50 KB)에서는 1–3 ms 대역.
- axum 라우팅·미들웨어·ETag 검증: < 1 ms.

**합계 추정:** 일반 layout(< 50 KB) 5–8 ms, 256 KB 최악 페이로드 10–15 ms — D19 < 30 ms 예산 안. R7-T3 50-pane 벤치마크에 본 측정 시나리오를 흡수: `wrk` 또는 `oha`로 1,000 회 PUT 실측 → p50/p99 보고.

기각: O_DIRECT, mmap, async fsync 우회 — 단일 writer·한 파일에서 측정 이점 없음 + 복구 난이도만 증가.

**결정 (Perf-A):** atomic-write-file의 동기 fsync 경로 사용. D19 측정은 R7-T3 벤치마크 plan에 추가 항목으로 흡수.

### F10. 사용자 직접 편집 — 지원하되 보증 없음

핸드오프 R6 §9가 직접 묻는 항목. 양면:

**찬성:** Obsidian이 vault를 *플레인 텍스트 마크다운 + .obsidian/workspace.json*로 노출해 백업/git/외부 도구를 자유롭게 한 사례 [src20]. Zed는 settings.json은 직접 편집 가능하지만 workspace state는 SQLite로 잠근 분리 패턴 [src21]. gtmux는 D22가 이미 *config는 사용자 직접 편집 가능, token은 비권장*으로 분리.

**반대:** Server가 살아있는 동안 사용자가 파일을 수정하면 in-memory 상태와 디스크가 분리되고, 다음 PUT이 사용자 수정을 *덮어쓴다* (ETag mismatch도 발생하지 않음 — 클라이언트 측 ETag는 마지막 GET 시점 값).

**채택 정책:**
1. 포맷은 **사람이 읽을 수 있는 JSON (`serde_json::to_string_pretty`)**으로 직렬화 — 인덴트 2 spaces. 텍스트 diff 친화.
2. 사용자 수정은 **Server stopped 상태에서만 지원**. Server running 중 수정 효과는 *없음 + 다음 debounce PUT에 덮어씀*. README와 stderr 부팅 메시지에 1줄 명시.
3. 부팅 시 schema validation을 거치므로, 수기 수정이 schema를 깨면 F5 reject 경로(빈 layout 부팅 + .corrupt 격리)로 자연스럽게 안전망.
4. 사용자 수정 ETag 정합성: 부팅 시 서버가 *내용 hash를 다시 계산해 새 etag로 메모리에 셋업*. 첫 GET 응답이 새 etag 반환. 즉 사용자 편집 후 첫 GET이 곧 ETag 재발급 — 별도 처리 불필요.

거절: 파일에 락 걸어 편집 차단 — 사용자 자유 손해 + Server 종료 후엔 락 불가. README 명시로 충분.

**결정 (Edit-A):** Pretty JSON + Server stopped 상태에서만 안전 + 부팅 시 hash 재계산으로 ETag 자연 재발급. 명시적 락 없음.

## 옵션 비교표

### O1. 저장 백엔드 (워크로드 = 단일 키 blob < 256 KB)

| | plain JSON ★ | TOML | SQLite | redb | sled |
|---|---|---|---|---|---|
| Atomicity (crash) | ★★★★★ (atomic-write-file) | ★★★★★ | ★★★★★ (WAL) | ★★★★★ (CoW B-tree) | ★★★★ (beta) |
| Durability | ★★★★★ | ★★★★★ | ★★★★★ | ★★★★★ | ★★★★ |
| 인덱싱·범위쿼리 | — (불필요) | — | 과잉 | 과잉 | 과잉 |
| Cross-compile | std만 | std만 | bundled feature 필요 | pure Rust | pure Rust |
| 추가 deps | 0 (atomic-write-file는 std-only) | toml 1개 | rusqlite + libsqlite3 | redb 1개 | sled 1개 |
| 사용자 편집 | ★★★★★ | ★★★★ | × | × | × |
| 백업 | `cp`/git | `cp`/git | `.dump` | 파일 단일 | 파일 단일 |
| 포맷 안정성 | 영구 (RFC 8259) | 영구 | 영구 | 약속 있음 | **1.0 전까지 변경 가능** |
| gtmux 적합 | **★★★★★** | ★★ | ★★ | ★★★ | ★ |

### O2. 원자성 패턴

| | 단순 truncate+write | tempfile::persist | atomic-write-file ★ | SQLite WAL |
|---|---|---|---|---|
| Partial-write 안전 | × | △ (dir fsync 누락) | ★ (Unix `openat`+fsync+rename+dir fsync) | ★ (WAL replay) |
| 권한 보존 | 수동 | 수동 | 자동 | n/a |
| 의존 | 0 | tempfile | atomic-write-file | rusqlite |
| 한 줄 사용 API | × | △ | ★ | × |

### O3. 검증 라이브러리

| | serde derive only | serde_valid | validator ★ | jsonschema |
|---|---|---|---|---|
| 정규식 pattern | × | ★ | ★ | ★ |
| 그래프 검증 표현 | × | × | × | × |
| 무게 | 0 | 가벼움 | 가벼움 | 비교적 큼 (런타임 schema) |
| 그래프 룰은 어차피 직접 구현 → 가벼움이 미덕 | | | ★ | |

## gtmux에의 함의 (§1 5대 불변식 검증)

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | **PASS** — `<session>.layout.json`은 web-only 상태만 포함. `Panel.pane_id`만 tmux 측 mirror 참조이며 R3에서 정합성 검증. tmux 상태(window/pane lifecycle)는 *디스크에 절대 저장 안 됨* (D15 ring buffer 디스크 비저장 정신과 정합). |
| 2 | tmux-native vs web-only 분기 | **PASS** — 영속화 책임은 web-only `http-api` 모듈이 단독으로 진다. `mux-router`는 본 파일에 access 불가 (ADR-0011 D10 dependency graph 강제). |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — 본 파일은 *Canvas Layout만 직렬화*. tmux Layout 문자열은 저장되지 않으며 schema에 그런 필드가 존재하지 않는다. |
| 4 | 보안 디폴트 | **PASS** — (a) 파일 권한 0600·부모 디렉터리 0700 (D17 정합). (b) `additionalProperties: false`로 미정의 필드 거부. (c) `label`/`note` maxLength + 렌더 시 escape (ADR-0003 별도). (d) `pane_id` 정규식 + 서버 mirror set 존재성으로 위변조 거부. (e) 256 KB cap으로 페이로드 폭주 거부 (HTTP 413). (f) 단일 라이터 모델 + atomic-write-file로 race·partial-write 표면 0. |
| 5 | control mode 사용 | **N/A** — 본 결정은 tmux 통신 채널과 무관. 단, layout 파일이 tmux 명령을 발급하지 않음을 명시 (label 변경 → tmux `rename-window` 동기화는 D9 책임이며 본 파일 저장은 부수효과 아님). |

## 미해결 질문 / 후속 ADR 필요 항목

- **O1.** `atomic-write-file` Linux `unnamed-tmpfile` feature가 모든 타겟(특히 cargo-zigbuild로 빌드한 musl)에서 동작하는지 → R7-T1 MSRV·crate compat 검증에 흡수.
- **O2.** macOS APFS의 `F_FULLFSYNC` (fsync보다 강함) 도입 여부 — 단일 사용자 워크스테이션의 비용 대비 효과 작아 MVP는 표준 `fsync`로 충분하나, R7 벤치마크에서 실측 후 결정.
- **O3.** Server stopped 상태에서 사용자가 파일을 편집했을 때 UI 측에 "외부 수정 감지" 알림이 필요한지 — 현 정책은 *부팅 시 hash 재계산으로 자연 흡수*하나 사용자 멘탈 모델 차원에서 알림이 친절할 수 있음. R8 UX 보고서로 검토.
- **O4.** schema_version 증가 시 `<file>.v<k>.bak` 자동 보존 정책의 keep count — MVP는 비실행, ADR-0006 후속 개정에서 결정.
- **O5.** P1+에서 PATCH 델타 도입 시 본 보고서의 단일 키 blob 가정 재검토 필요. 그 시점이 SQLite/redb 재평가 트리거.

## 권장 결정 요약 (ADR-0006 입력)

본 보고서가 ADR-0006에 인계하는 결정 7건:

- **D1.** 저장 백엔드 = `<XDG_STATE_HOME>/gtmux/<session>.layout.json` 단일 JSON 파일 (SSoT 스키마, schema_version=1).
- **D2.** 원자성 = `atomic-write-file` 0.3+ (Linux `unnamed-tmpfile` feature 활성). fsync(file) + fsync(dir) + rename 원자성.
- **D3.** 동시성 = 추가 락 없음. 프로세스 단일성은 `<XDG_RUNTIME_DIR>/gtmux/<session>.pid` advisory exclusive lock (`fd-lock` 또는 std `File::lock_exclusive`)로 강제.
- **D4.** 손상 정책 = reject + `<file>.corrupt-<unix_ts>` sidecar 격리 + 빈 layout 부팅. 자동 백업 회전 없음.
- **D5.** 마이그레이션 = 정수 `schema_version` + lazy `migrate_v_k_to_v_k_plus_1` 체인 + unknown version reject. MVP는 v=1 고정, 변환 함수 0개.
- **D6.** 권한 = layout 파일 0600, 부모 디렉터리 0700. 부팅 시 audit + 자동 정정. `atomic-write-file`의 권한 보존 옵션 ON.
- **D7.** 검증 = serde derive (`deny_unknown_fields`) + `validator` 크레이트(정규식 pattern) + 직접 구현 그래프 검증 모듈(R2·R4·R5·R6·R7 단위 테스트 100%). R8/R9는 axum/tower-http 미들웨어.
- **전송 결정은 D12로 확정 (HTTP `GET/PUT /api/layout` + ETag, WS `0x80 LAYOUT_CHANGED` notify) — 본 보고서는 인용만.**

## 출처

각 인용 번호는 본문 `[src.N]` 표기와 매칭.

1. [How to write/replace files atomically? — Rust Users Forum](https://users.rust-lang.org/t/how-to-write-replace-files-atomically/42821) — 접근일 2026-05-13
2. [atomic-write-file crate docs (docs.rs)](https://docs.rs/atomic-write-file) — 접근일 2026-05-13
3. [rusqlite GitHub README](https://github.com/rusqlite/rusqlite) — 접근일 2026-05-13
4. [SQLite as key-value store for concurrent Rust programs (the-lean-crate discussion)](https://github.com/the-lean-crate/criner/discussions/5) — 접근일 2026-05-13
5. [redb GitHub README (ACID + MVCC)](https://github.com/cberner/redb) — 접근일 2026-05-13
6. [redb crate docs](https://docs.rs/redb) — 접근일 2026-05-13
7. [sled GitHub README (champagne of beta)](https://github.com/spacejam/sled) — 접근일 2026-05-13
8. [Sled HN discussion (status, format stability)](https://news.ycombinator.com/item?id=22375979) — 접근일 2026-05-13
9. [Durability: Linux File APIs — Evan Jones](https://www.evanjones.ca/durability-filesystem.html) — 접근일 2026-05-13
10. [ext4 vs fsync — Alexander Larsson](https://blogs.gnome.org/alexl/2009/03/16/ext4-vs-fsync-my-take/) — 접근일 2026-05-13
11. [Ensuring data reaches disk — LWN.net](https://lwn.net/Articles/457667/) — 접근일 2026-05-13
12. [atomic-write-file 0.3 crates.io](https://crates.io/crates/atomic-write-file) — 접근일 2026-05-13
13. [tempfile::NamedTempFile docs (persist atomicity caveats)](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html) — 접근일 2026-05-13
14. [Schema Evolution & Compatibility Types — Confluent](https://docs.confluent.io/platform/current/schema-registry/fundamentals/schema-evolution.html) — 접근일 2026-05-13
15. [Schema Versioning for JSON Configuration Files — offlinetools.org](https://offlinetools.org/a/json-formatter/schema-versioning-for-json-configuration-files) — 접근일 2026-05-13
16. [std::os::unix::fs::PermissionsExt — Rust std docs](https://doc.rust-lang.org/std/os/unix/fs/trait.PermissionsExt.html) — 접근일 2026-05-13
17. [serde_valid crate (lib.rs)](https://lib.rs/crates/serde_valid) — 접근일 2026-05-13
18. [serde-rs/json-benchmark](https://github.com/serde-rs/json-benchmark) — 접근일 2026-05-13
19. [serde_json README (perf range)](https://github.com/serde-rs/json) — 접근일 2026-05-13
20. [How Obsidian stores data (workspace.json + .obsidian/)](https://help.obsidian.md/data-storage) — 접근일 2026-05-13
21. [How We Rebuilt Settings in Zed (SQLite for workspace state)](https://zed.dev/blog/settings-ui) — 접근일 2026-05-13
22. [XDG Base Directory Specification 0.8 (XDG_STATE_HOME)](https://specifications.freedesktop.org/basedir/latest/) — 접근일 2026-05-13
23. [fd-lock crates.io](https://crates.io/crates/fd-lock) — 접근일 2026-05-13
24. [Rust tracking issue: File lock API (#130994)](https://github.com/rust-lang/rust/issues/130994) — 접근일 2026-05-13
25. [Rename atomicity is not enough — npm/write-file-atomic#64](https://github.com/npm/write-file-atomic/issues/64) — 접근일 2026-05-13

## 변경 이력

- 2026-05-13: 초안 (R6, B3 DoD 충족).
