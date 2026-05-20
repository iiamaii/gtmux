# ADR-0006: Canvas Layout 영속화 storage — plain JSON file + atomic-write-file + sidecar quarantine

- 상태: Accepted (2026-05-14) — **2026-05-15 amend D15 by ADR-0018**: schema v1 (`groups[] + panels[]`) 에서 schema v2 (`groups[] + items[]`) 로 hard cutover. D14 (`panels[]` boot strip) 은 v2 cutover 로 obsolete — schema v2 는 `items[]` 의 *match-or-spawn 정책* (ADR-0018 D6) 으로 server 재기동 후에도 layout 보존. 영속화 메커니즘 자체 (plain JSON + atomic-write + sidecar) 는 그대로 유지.
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted)
- 결정자: backend-architect (plan 0002 §1 배치 A · S4-A3)
- 근거 보고서: `docs/reports/0006-layout-persistence.md` (이하 *R6*) — D1~D7 결정 7건을 본 ADR이 *그대로 수용*
- 관련 ADR:
  - ADR-0002 (전송 계층) — D9 `GET/PUT /api/layout` + `If-Match` ETag 정신 인계, D3 `0x80 LAYOUT_CHANGED` notify는 본 ADR의 PUT 성공 후 broadcast
  - ADR-0003 (보안 디폴트) + SSoT `docs/ssot/security-defaults.md` — D17 토큰 파일 0600/0700 패턴, `xdg.state_home` 디렉터리 컨벤션
  - ADR-0007 (Server : Session : Port 1:1:1) — D2의 1:1:1 격리가 본 ADR의 단일 라이터·파일별 락 불필요 근거
  - ADR-0008 (single-pane + command allowlist) — 영속화 페이로드에 tmux Layout 문자열이 들어오지 않음을 보장
  - ADR-0009 (tmux daemon 격리) — `teardown` 5단계의 단계 4가 본 ADR의 layout 파일을 정리
  - ADR-0010 (Group 데이터 모델, G-hybrid) — 본 ADR이 영속화하는 페이로드 schema의 정의 ADR
  - ADR-0011 (Rust backend) — D5 serde + D10 `http-api` crate 모듈 경계와 정합
- 부속 SSoT: `docs/ssot/canvas-layout-schema.md` — 본 ADR이 영속화하는 *페이로드 schema 정본*. 본 ADR은 그 SSoT를 *잠근다* (직렬화·검증 디테일을 storage 결정에 맞춰 확정).

## 맥락

`docs/sketch.md` §15 3단계(영속화·재연결)는 *재시작 후 layout 복원*을 1단계 진입의 prerequisite로 명기하고 §6.7은 "Panel 좌표·visibility·잠금·라벨·노트는 페이지 새로고침·서버 재시작·tmux 재기동 후에도 보존된다"를 사용자 약속으로 둔다. ADR-0002 D9가 durable Canvas Layout 운반을 **HTTP `GET/PUT /api/layout` + `If-Match` ETag** 모델로 *채널 수준* 잠갔고, ADR-0010 + 부속 SSoT `canvas-layout-schema.md`가 *페이로드 schema*를 잠갔다. 본 ADR은 그 위에 **(a) 어떤 storage backend에 어떤 경로로 어떤 권한으로 직렬화할지, (b) 크래시·전원 단절·동시 쓰기에 어떤 원자성 보장으로 견딜지, (c) 손상·미지원 버전 파일을 어떻게 처리할지, (d) MVP 이후 schema 진화에 어떤 마이그레이션 frame을 둘지** 네 차원을 잠근다.

본 결정의 7건 D1~D7은 R6 보고서가 의무 조사 9항목을 거쳐 산출한 권고를 *그대로 수용*한 결과다. R6는 "단일 사용자·단일 라이터·단일 키 blob·< 256 KB·< 1 write/300 ms"라는 워크로드 형상(F1)을 분석해 SQLite/redb/sled 같은 풀-임베디드 DB가 startup·바이너리 크기·운영 절차 비용만 늘리는 over-engineering임을 정량 비교(F2 표)했고, 동일 워크로드에서 **plain JSON 파일 + `atomic-write-file` crate가 워크로드와 1:1 매칭**임을 확정했다. 본 ADR은 그 결정 7건을 단정문으로 격상하고, 잠재 후속 v2 schema 시점에 본 ADR을 amend해 변환 함수 체인을 추가하는 *진화 경로*를 명문화한다.

현 코드 인벤토리상 `codebase/backend/crates/http-api/src/lib.rs`는 `GET/PUT /api/layout` 핸들러 + ETag(SHA256-128) + `If-Match` 412 검증 + 256 KiB cap을 이미 구현하고 있고, 페이로드는 `RwLock<LayoutSnapshot>`의 *in-memory 보관*에 머문다 — 즉 영속화 storage는 *미구현*이다. 본 ADR이 storage 결정을 잠그면, P0-LAYOUT-STORAGE-1 후속 task(Sprint 4+, `lifecycle` + `http-api` crate wiring)가 본 ADR을 implement한다.

## 결정 (Decisions)

- **D1.** [R6 §F2 표 + §"권장 결정 요약" D1] Canvas Layout의 영속화 backend는 **plain JSON 단일 파일**이다. SQLite·redb·sled·TOML 등 다른 후보는 본 워크로드(단일 키 blob, < 256 KB, < 1 write/300 ms, 단일 라이터, 인덱싱·범위 쿼리 없음)에서 startup·바이너리 크기·운영 복잡도만 늘리는 over-engineering이며, ADR-0011 D5의 serde + serde_json 스택과 *crate 추가 0*으로 정합한다. 파일 내용은 SSoT `canvas-layout-schema.md` §1의 schema에 합치하는 JSON object이며 `serde_json::to_string_pretty`(indent 2 spaces)로 직렬화한다 — 사용자 직접 검수·텍스트 diff·git 백업 친화 (R6 §F10).
- **D2.** [R6 §F2 + ADR-0003 SSoT `xdg.state_home`] Layout 파일 경로 = **`${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.layout.json`**. tmux daemon 소켓·pid 파일·token 파일과 같은 디렉터리 컨벤션(ADR-0003 SSoT §1.10 `xdg.state_home` + ADR-0009 D2 `teardown` 단계 4와 *동일 경로*)을 따른다. `<session>`은 ADR-0007이 잠근 Server-scope 1차 키 — tmux session 이름(SSoT `regex.tmux_session_name` 패턴 강제).
- **D3.** [R6 §F3 + §"권장 결정 요약" D2] **원자적 쓰기는 `atomic-write-file = "0.3"` (또는 동등) crate**를 사용한다. 동일 디렉터리에 `<final>.tmp.<rand>` 생성 → payload write → `fsync(file_fd)` → `rename(2)` → `fsync(dir_fd)` 5단계를 caller가 직접 작성하지 않고 crate가 캡슐화. Linux 빌드는 `unnamed-tmpfile` (O_TMPFILE) feature 활성화 — 프로세스가 commit 전에 죽어도 orphan tmp 파일 0개. macOS는 fallback 경로. `tempfile::NamedTempFile::persist`는 *디렉터리 fsync를 보장하지 않으므로* 채택하지 않는다 (R6 §F3 인용 `tempfile` 0.3 docs caveat).
- **D4.** [SSoT `canvas-layout-schema.md` §1 + R6 §F8] 영속화 페이로드의 schema 정본은 **SSoT `docs/ssot/canvas-layout-schema.md` §1 JSON Schema**다. `schema_version: 1` 고정 (MVP). `etag`(32-hex lowercase), `groups[]`, `panels[]` 세 array가 envelope 최상위. 직렬화는 Rust struct ↔ JSON 양방향 (`#[derive(Serialize, Deserialize)]`) + `#[serde(deny_unknown_fields)]` 로 `additionalProperties: false` 룰(SSoT §3 R1) 컴파일 타임 강제. utoipa 5.x (ADR-0011 D5)가 OpenAPI 산출물을 ADR-0012 frontend로 전파.
- **D5.** [SSoT `canvas-layout-schema.md` §2 + 현 `http-api/src/lib.rs` 인벤토리] ETag 정본 표현은 **SHA-256 digest의 첫 16바이트(SHA256-128)**이며, 채널별 인코딩은 SSoT §2 표를 따른다 — 영속화 파일 안의 `etag` 필드는 *32자 lowercase hex 문자열*. HTTP `ETag` 응답 헤더는 `"<32-hex>"` quoted-string, `If-Match` 요청 헤더는 같은 quoted-string. ETag mismatch는 **412 Precondition Failed** 응답이며 응답 헤더에 *현재 서버 ETag*를 동봉해 클라이언트가 즉시 GET 재발급 없이 rebase 가능(현 `http-api/src/lib.rs` `layout_put_handler` 본문 정합). 비교는 상수시간(ADR-0011 D8 `ring::constant_time`).
- **D6.** [SSoT `canvas-layout-schema.md` §3 R1~R9 + R6 §F8] 서버는 `PUT /api/layout` 페이로드에 대해 **SSoT §3 R1~R9를 순서대로 검증**하고 위반 시 적절한 HTTP 상태 코드(400/412/413)로 reject한다. 특히 그래프 검증 룰 — **R2 (ID 유일성), R4 (parent_id 존재), R5 (사이클 금지), R6 (다중 부모 금지), R7 (Panel.parent_id는 Group만 참조)** — 은 *어느 범용 derive crate도 표현 불가*하므로 (R6 §F8 표) `http-api::validate` 모듈에 DFS 기반 직접 구현하고 100% 단위 테스트 커버. R1 / R3 / R8 / R9는 serde derive · runtime mirror set · ETag middleware · axum body-size middleware로 분산. **검증 실패 시 영속화 파일은 절대 갱신되지 않는다** (in-memory snapshot 갱신도 검증 통과 후에만 atomic swap).
- **D7.** [R6 §F6 + §"권장 결정 요약" D5] **MVP는 `schema_version = 1` 고정, 변환 함수 0개**. 미래 v_N 도입 시 본 ADR을 amend하여 다음 4가지를 추가한다 — (1) `fn migrate_v_k_to_v_k_plus_1(prev: JsonValue) -> Result<JsonValue, MigrationError>` 함수 체인 k = 1..N-1, (2) 로더가 `schema_version`을 읽고 v=N까지 *체인 적용*, (3) 마이그레이션 성공 시 첫 PUT에서 v=N 형식으로 다시 쓰며 직전 파일을 `<file>.v<k>.bak`로 1회 보존, (4) 새 필드는 *optional + default value*로 도입 (필수 필드 추가·타입 변경은 schema_version bump 필수). **MVP는 별도 backup file 미생성** — D3 atomic write가 prev-version overwrite 시점에 *원자 교체*를 보장하므로 single-user single-tab scope에서 충분 (R6 §F5).

부가 잠금(D1~D7 직접 파생):

- **D8. 단일 라이터·동시 쓰기**: [R6 §F4 + ADR-0007 D2] Layout 파일 자체에는 advisory lock을 걸지 않는다. 프로세스 단일성은 **`${XDG_RUNTIME_DIR:-/tmp}/gtmux/<session>.pid`** 위에 `fd-lock` (또는 Rust 1.81+ `std::fs::File::lock_exclusive`) advisory exclusive lock으로 강제하며, lock 실패 시 ADR-0009 SSoT의 exit 4로 일관 종료. 단일 사용자 단일 탭은 D9 (브라우저 측 racy write)에서 ETag로 검출.
- **D9. 클라이언트 측 race**: 다중 브라우저 탭 / Devtools 동시 PUT은 SSoT `If-Match` 매칭으로 검출. 412 응답 시 클라이언트는 GET 재발급 후 merge·재send (Pull-through-notify, ADR-0002 D9 인용). 본 ADR은 자동 merge 정책을 *정의하지 않는다* — 사용자 UI 차원의 결정이며 ADR-0012 frontend ADR로 위임.
- **D10. 손상 정책**: [R6 §F5 표] 부팅 시 layout 파일 7가지 상태에 대해 다음 대응 (R6 §F5 표 그대로 수용).

  | 상태 | 처리 | 통지 |
  |---|---|---|
  | 부재 | 빈 layout (`{etag:"00..0", schema_version:1, groups:[], panels:[]}`) 메모리 초기화. 첫 PUT 시 파일 생성. | 부팅 로그 `layout: cold start (no file)` |
  | 정상 + schema valid | 그대로 로드. ETag는 *디스크 페이로드 hash 재계산*으로 메모리 셋업 (사용자 직접 편집을 자연 흡수, R6 §F10) | (없음) |
  | 0 바이트 | 손상 취급. atomic-write-file 정상 동작 시 발생 불가 | corrupt 격리 + WARN |
  | JSON parse 실패 | `<file>` → `<file>.corrupt-<unix_ts>` rename, 빈 layout 부팅 | stderr WARN 1줄 + `gtmux status` 노출 |
  | schema_version 미존재 또는 알 수 없는 정수 | 동일 격리 + 빈 layout | WARN `unsupported schema_version=X` |
  | SSoT §3 R2/R4/R5/R6/R7 검증 실패 | 동일 격리 + 빈 layout | WARN + 실패 룰 번호 |
  | 권한 != 0600 | 부팅 시 자동 chmod 0600 + WARN. 이후 atomic-write-file 권한 보존 옵션이 0600 유지 | WARN 1줄 |

  자동 백업 회전·체크섬 헤더 도입은 거절 (R6 §F5 — `etag`가 이미 hash, 사용자 git 사용이 자연 백업).

- **D11. 파일·디렉터리 권한**: [R6 §F7 + ADR-0003 SSoT §1.10] Layout 파일 0600, 부모 디렉터리 `${XDG_STATE_HOME}/gtmux/` 0700. 부팅 시 audit + 자동 정정 (D10 표 마지막 행). `atomic-write-file`의 권한 보존 옵션 ON. Windows는 MVP 비범위 (ADR-0009 D2 — tmux unix-only).
- **D12. 페이로드 cap**: 256 KiB (SSoT §3 R9). 현 `http-api/src/lib.rs` `PUT_MAX_BYTES = 256 * 1024` 정합. 영속화 파일은 *cap을 초과해서 디스크에 들어갈 수 없다* — PUT 단계에서 413으로 reject (R9).
- **D13. PUT 성공 후 broadcast**: PUT 성공 = (a) 검증 통과 → (b) 새 ETag 계산 → (c) `atomic-write-file`로 파일 교체 → (d) in-memory snapshot 교체 → (e) WS `0x80 LAYOUT_CHANGED` raw 16바이트 ETag broadcast (ADR-0002 D3). 위 5단계는 서버 `write lock` 안에서 순차 — 두 동시 PUT이 같은 ETag를 관찰할 수 없도록 atomic compare-and-swap.

## 거절된 대안 (Rejected)

- **R1. SQLite (`rusqlite`)** — *바이너리 footprint + C 의존성*. R6 §F2 표: 단일 BLOB 행에 페이로드를 저장하는 구조는 결국 "DB 안의 큰 JSON blob"이며 ACID 이점이 *워크로드에 의해* 미사용(단일 키, 인덱싱·범위 쿼리 없음). `bundled` feature로 cross-compile 시 빌드 +5 MB, 바이너리 +2 MB. `.db-wal`/`.db-shm` 동반 파일이 백업/사용자 검수 절차에 마찰. 단일 사용자·단일 라이터 scope에서 ACID 도입이 *정당화 불가*. 거절. (P1+에서 PATCH 델타·다중 백업 슬롯·다중 layout 도입 시 재검토 — R6 §F2 SQLite 행 "P1+ 재평가 트리거".)
- **R2. redb / sled (pure-Rust 임베디드 KV)** — R6 §F2 표: redb는 SQLite보다 깔끔하나 *키 1개*에 B-tree 인프라는 과잉. sled는 1.0 이전 on-disk 포맷이 변경 가능 + 디스크 비효율 보고 → MVP에 부적합. 거절.
- **R3. TOML** — R6 §F2 표: JSON과 거의 동등하나 SSoT가 *JSON Schema*라 변환 비용 발생. ETag 헥스/숫자 표현이 비표준. config 파일(`<session>.config.toml`, ADR-0003 SSoT D22)이 이미 TOML이므로 layout은 JSON으로 *언어 분리*해 사용자 멘탈 모델을 단순화. 거절.
- **R4. 분산 DB (Postgres/MySQL)** — sketch §13의 *single-user scope* 위반. 인프라 의존성 도입(외부 데몬·네트워크·credential·migration tooling). gtmux 사용 시나리오에서 *완전 비범위*. 거절.
- **R5. Cloud-only (S3/Firebase 등 외부 객체 저장소)** — sketch §13 single-user invariant 위반 + 외부 네트워크 의존 + 영속화 latency가 D19 예산(< 30 ms HTTP RTT) 직접 위배. 거절. (sketch §15 4단계 multi-machine sync 시점에 cloud 옵션 별도 ADR로 재방문 — 본 ADR §"미해결 항목 O3".)
- **R6. Memory-only (no disk persistence)** — 재시작 시 layout 손실 → sketch §15 3단계 prereq 직접 위반. 거절.
- **R7. 단순 `truncate + write` 또는 `tempfile::persist`** — R6 §F3 + 옵션 비교표 O2: 디렉터리 fsync 누락 시 크래시 후 *옛 파일 부활* 가능성 (LWN.net 인용). `tempfile::persist`는 디렉터리 fsync 보장 없음 — 채택하면 caller가 추가 호출을 빠뜨릴 위험 + 5단계를 재발명. `atomic-write-file` crate가 표준화된 5단계를 캡슐화하므로 채택. 거절.
- **R8. `permessage-deflate`/gzip 압축 후 디스크 기록** — 일반 layout(< 50 KB)에서 압축 이득 작고, 사용자 직접 검수·git diff·사용자 수정 친화성을 해친다. 거절.
- **R9. Multi-version 자동 백업 회전 (`.bak.1`/`.bak.2`/...)** — R6 §F5: 사용자 git 사용 빈도가 더 높음 + 회전 보존 카운트 결정 표면 추가. P2 backup preset에서 재검토. 거절.
- **R10. 체크섬 헤더 (CRC32 등) 별도 부착** — `etag`가 이미 hash이고 schema-violating 변조는 R1 검증에서 reject되므로 중복. 거절.
- **R11. Sequence number / vector clock 기반 충돌 해결** — ADR-0002 D11과 동일 정신. ETag optimistic concurrency가 단일 사용자 scope에서 충분 + MT-3 broadcast가 idempotent. 거절.
- **R12. Server stopped 중 사용자 편집 차단 (advisory file lock)** — R6 §F10: 사용자 자유 손해 + Server 종료 후엔 락 불가. README + stderr 부팅 메시지에 "Server stopped 상태에서만 안전 편집" 1줄 명시로 충분. 거절. 부팅 시 hash 재계산이 자연 흡수 (D10 표 두 번째 행).
- **R13. 손상 시 자동 복원 (`.bak`에서 자동 fallback 로드)** — R6 §F5: 복원 대상이 사용자 의도(panel 배치)이므로 *잘못된 복원이 더 큰 손실*. 사용자가 sidecar를 직접 검수·수기 복원 escape hatch만 제공. 거절.

## 결과 (Consequences)

- 긍정:
  - **워크로드와 1:1 매칭** — 단일 키 blob·< 256 KiB·< 1 write/300 ms·단일 라이터 형상에 plain JSON + atomic rename이 *정확히* 맞는 도구 (R6 §F1+F2). ACID 중 의미 있는 Durability·Atomicity만 충족, Isolation/Transaction은 *비용 0*.
  - **추가 dependency 거의 0** — `atomic-write-file` 1개. serde·serde_json은 ADR-0011 D5에서 이미 선택. SQLite의 `libsqlite3` C 의존성·cross-compile 마찰·바이너리 +2 MB 회피.
  - **Crash-safe + race-safe** — D3 atomic-write-file가 partial-write·orphan tmp 파일·디렉터리 fsync를 한 번에 캡슐화. D5 ETag optimistic concurrency가 다중 탭 race를 *애플리케이션 레이어에서* 검출. D8 pid 파일 advisory lock이 *프로세스 단일성* 별도 강제.
  - **사용자 친화성** — JSON pretty + 텍스트 diff + git 백업 + `cat`/Obsidian 같은 일반 도구로 검수 가능 (Obsidian vault 컨벤션과 동일 정신, R6 §F10).
  - **불변식 #1 강화** — 본 storage는 *web-only Canvas Layout만* 직렬화. tmux 상태(pane id, output bytes, ring buffer)는 schema에 컴파일 타임 자리가 없으므로 *기계적으로* 디스크 진입 차단 (D4 + ADR-0011 D10 dependency graph).
  - **ADR-0009 teardown과 자연 정합** — `gtmux teardown` 단계 4가 *이미* `<session>.layout.json`을 정리하도록 명시되어 있음 — 본 ADR이 이를 의무로 잠금.
- 부정/비용:
  - **부분 update 어려움** — 256 KiB 최악 페이로드 전체 재직렬화·재쓰기. 일반 layout(< 50 KB)에서는 1–3 ms 디스크 쓰기로 D19 예산 안 (R6 §F9). 50-panel scale에서 측정 필요 (Open O1).
  - **사용자가 Server running 중 파일 편집 시 효과 없음** — 다음 debounce PUT에 덮어씌워짐 (R6 §F10). README + stderr 부팅 메시지 1줄로 명시. 부팅 시 hash 재계산으로 *Server stopped 후 첫 부팅*은 사용자 수정 흡수.
  - **MVP는 단일 schema_version** — v2 도입 시 본 ADR amend 필요. D7이 진화 경로(체인 함수 + `.v<k>.bak` 1회 보존)를 명문화하나 *구현은 v2 발생 시점*.
  - **다중 사용자/cloud sync 비범위** — sketch §15 4단계 multi-machine 시점에 별도 ADR (Open O3).
- 후속 작업:
  - **P0-LAYOUT-STORAGE-1** (Sprint 4+) — `lifecycle` + `http-api` crate wiring. 현 in-memory `RwLock<LayoutSnapshot>` 위에 **(a) 부팅 시 파일 로드 + D10 표 7가지 분기**, **(b) PUT 성공 시 D13 5단계 atomic swap**, **(c) D8 pid 파일 advisory lock**, **(d) D11 0600/0700 audit + chmod**, **(e) D6 R2/R4/R5/R6/R7 DFS 검증 모듈** 추가. `atomic-write-file` crate 의존성 추가 (ADR-0011 O1 MSRV 호환 확인).
  - **ADR-0011 amend** — D7 crate set에 `atomic-write-file` 추가. R7-T1 MSRV 검증에 흡수.
  - **R7 benchmark** — D19 예산(< 30 ms HTTP RTT for PUT) 50-panel 페이로드 실측 추가 (Open O1).
  - **README · `gtmux status` 출력** — R6 §F10 "Server stopped 상태에서만 안전 편집" 1줄 + sidecar 격리 발생 시 안내.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | **PASS (강함)** — 본 storage는 web-only Canvas Layout 전용. SSoT §1 schema에 tmux 상태(pane lifecycle/output/ring buffer/tmux Layout 문자열)를 직렬화할 자리가 정의되어 있지 않으며 `additionalProperties: false` + serde `deny_unknown_fields`가 *컴파일 타임 + 검증 시점 이중 차단*. `Panel.pane_id`만 tmux 측 mirror 참조 + R3 (SSoT §3) 정합성 검증 통과 시에만 수용. ADR-0011 D10 dependency graph가 `mux-router → http-api` 방향을 금지하므로 *tmux 코드가 영속화 파일에 access 자체 불가*. |
| 2 | tmux-native vs web-only 분기 | **PASS** — `PUT /api/layout` 경로는 web-only 책임. ADR-0011 D10이 `http-api` crate를 web-only 도메인으로 둠. 본 ADR의 D6 검증·D13 5단계 어디에도 tmux command 발급 없음. tmux pane lifecycle 변경(생성·종료)은 별도 control mode 채널을 통해서만 발생하며 그 결과 page mirror가 `Panel.pane_id` 검증(R3) 통과를 좌우. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — SSoT §1 schema는 Canvas Layout(`panels[].x/y/w/h/z` + `groups[]` 트리)만 정의. tmux Layout 문자열 슬롯 *부재*. ADR-0008가 `select-layout` 발급을 영구 금지 → 운반·저장할 메시지 *카테고리 자체 부재*. 본 storage 파일을 `cat`해도 tmux split 표현은 절대 나타나지 않는다. |
| 4 | 보안 기본값 | **PASS (강함)** — (a) **권한 0600 + 부모 디렉터리 0700** (D11, ADR-0003 SSoT §1.10 token 파일 패턴과 동일), (b) **HTTP 미들웨어 체인** — `Origin`/`Host` allowlist + `Authorization: Bearer` 검증 + body size 256 KiB cap (현 `http-api/src/lib.rs` 정합), (c) **검증 우선** — D6 SSoT §3 R1~R9 순차 검증 통과 *전* 디스크 진입 금지 (검증 실패 = 파일 변경 0), (d) **atomic write** — D3가 partial-write로 인한 *손상 + 권한 변화* 표면 0, (e) **변조 탐지** — `etag`가 SHA-256 기반 hash이므로 사용자 직접 편집·수동 변조도 부팅 시 재계산·schema 검증으로 자연 흡수 또는 D10 격리, (f) **dependency 최소화** — `atomic-write-file` 1개 추가, 공격 표면 ↓, (g) **fail-closed** — 손상 시 빈 layout 부팅(D10) — 손상된 데이터로 *부분 기동* 절대 없음, ADR-0003 SSoT §5 startup 체크리스트 정신과 정렬. |
| 5 | control mode 사용 | **N/A** — 본 ADR은 tmux 통신 채널과 무관한 *web-only 영속화 결정*. 본 ADR의 어떤 D도 tmux control mode 명령을 발급하지 않는다. (ADR-0001 D7의 `%output` → ring buffer → `0x02 PANE_OUT` 파이프라인은 본 storage 파일에 *진입하지 않는다* — pane output은 디스크에 저장되지 않으며 ring buffer는 ADR-0001 D7 메모리 전용.) |

## 미해결 항목 (Open)

- **O1.** 50-panel scale에서 PUT-and-flush latency 실측 — R6 §F9가 일반 layout(< 50 KB) 5–8 ms / 256 KB 최악 10–15 ms로 추정. D19 < 30 ms 예산 안이나 macOS APFS의 `F_FULLFSYNC` 도입 여부(R6 §"미해결 O2")는 R7 benchmark 후 결정. 결과에 따라 본 ADR D3 amend (`atomic-write-file`의 fsync 강도 옵션).
- **O2.** v2 schema 도입 시점에 본 ADR amend — D7의 (1)~(4) 4가지를 *구체 변환 함수 시그너처와 함께* 작성. 트리거 = sketch §15 3단계 이후 새 필드 도입 PR. backup 보존 카운트(R6 §"미해결 O4") 동시 결정.
- **O3.** Multi-machine sync (Cloud 모드) 영속화 정책 — sketch §15 4단계. 본 ADR scope 밖, 별도 ADR 발행. cloud 객체 저장소 vs CRDT 기반 sync vs 사용자 수동 export/import 중 결정.
- **O4.** Server stopped 중 외부 수정 감지 알림 — R6 §"미해결 O3". UX 차원(R8 보고서 검토 대상). 현 정책은 *부팅 시 hash 재계산으로 자연 흡수*하나 사용자 멘탈 모델 차원의 추가 알림이 친절할 수 있음.
- **O5.** P1+ PATCH 델타 도입 시 backend 재평가 — R6 §F1 가정(단일 키 blob)이 무너지면 SQLite/redb의 *부분 update 트랜잭션* 이점이 다시 의미를 가질 수 있다. PATCH 도입 PR이 본 ADR의 supersession 트리거.

## 2026-05-15 Amend ×1 — D14 신규: panels[] strip on boot

PTY-direct 시대 (ADR-0013) 에서는 모든 Pane 이 Server 의 child process 이고 graceful shutdown (ADR-0014 D5) 또는 crash 시 *함께 소멸* 한다. 디스크에 영속화된 layout 의 `panels[]` 에 있는 `Panel.pane_id` (`%2`, `%3`, ...) 는 다음 Server 부팅 시 fresh PtyBackend 가 `%1` 부터 재할당하기 때문에 **stale reference** 가 된다.

D10 표의 7가지 상태 처리 위에 본 amend 는 *load 후 panels[] strip* 을 추가한다 — 손상이 아닌 *정합* 처리.

### D14. boot-time panels[] strip

`LayoutStore::load` 가 schema 검증을 통과한 후, `body["panels"]` 가 비어있지 않으면 **빈 array 로 교체** 한다. `groups[]` / `schema_version` / 추후 viewport 상태 등은 그대로 보존.

```rust
let panels_before = body.get("panels").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
if panels_before > 0 {
    if let Some(panels) = body.get_mut("panels") { *panels = Value::Array(Vec::new()); }
    tracing::info!(stripped = panels_before, "layout: stripped {} stale Panel(s) on boot", panels_before);
}
```

- `ETag` 는 strip 후의 body 로 *재계산* — 첫 PUT 시 자연스럽게 새 hash 로 commit.
- 사용자 측 UX: 재기동 후 *빈 캔버스* 로 진입. 명시 "New Panel" 액션으로 fresh Pane + Panel 추가.
- groups + canvas viewport 는 보존 — Group 트리 / pan/zoom 위치는 살아남음.

### 거절 (R14)

- **R14 옵션 B**: frontend 가 grace 후 orphan 자동 제거 — backend 가 *layout 무지* 정합 (ADR-0006 정신) 이지만 *frontend race + 2s wait* 복잡도. 또한 backend 가 hub.subscribe 시 pane-spawned NOTIFY replay 추가 surface 필요. backend strip 이 더 단순.
- **R14 옵션 C**: 자동 re-spawn — sketch §6.7 의 Panel 좌표 보존 약속에 가장 부합. 그러나 (a) terminal content 손실 (b) backend 가 layout schema 알아야 하거나 frontend 가 remap helper 필요 (c) race / failure 처리 복잡. P1+ 결정.

### 정합 검증

- sketch §6.7 "재시작 후 Panel 좌표 보존" 약속은 *tmux 시대* 의 promise — ADR-0013 채택으로 *Pane 자체가 재기동 비범위* 가 됐으므로 Panel 좌표 보존도 *직접 의미 없음*. groups + canvas viewport 의 보존이 sketch 약속의 *부분 충실*.
- ADR-0014 D9 (Server 재기동 시 layout 보존 / process state 비보존) 와 정합.
- ADR-0015 (frontend cascade auto-mount) 와 정합 — boot 직후 빈 layout 에 사용자가 New Panel 시 cascade origin 부터 자연스럽게 추가.

## 변경 이력

- 2026-05-13: 초안 (plan 0002 §1 배치 A S4-A3 dispatch). Proposed.
- 2026-05-14: R6 보고서 7개 결정 수용 + SSoT `canvas-layout-schema.md` 잠금 + 현 `http-api/src/lib.rs` 인벤토리와 cross-check. Accepted.
- 2026-05-15: amend ×1 — D14 신규 (boot-time panels[] strip, PTY-direct 시대 정합). 거절안 R14 (frontend orphan cleanup / auto-respawn) 명시.
- 2026-05-17: amend ×3 — **D13 의 "5단계 sync 안 write lock 보유" 정합 보존 + blocking I/O 의 `spawn_blocking` 분리** (0066 §BE-4 / 0067 Phase 3). 옛 정책에서 D13 의 (c) `atomic-write-file` 호출이 *async task* 안 sync I/O 로 실행 → `tokio` worker 가 disk latency 동안 block. layout PUT 의 write lock 도 그 시간 동안 보유 → 동일 session 의 다른 reader (`/api/sessions/:name/layout` GET) 가 무한 대기. amend ③:
  - **D13.c (atomic-write) 만 `tokio::task::spawn_blocking` 으로 분리** — write lock 은 `.await` 동안 *그대로 보유* (disk-first invariant + CAS atomicity 보존). 다른 reader 는 여전히 disk write 의 latency 만큼 기다리나, *tokio worker thread 는 free* — N 개 동시 layout PUT 이 worker pool 고갈 회피.
  - **GET 경로의 canonical serialize 를 read lock *밖* 으로 이동** (D6 검증과 무관, 0066 §BE-4 의 read-side 부담 해소): 옛 코드는 read lock 보유 동안 `canonical_bytes(&snap.layout)` 직렬화 (큰 layout 시 ms 단위). amend 후: read lock 안에서 *etag_hex 의 If-None-Match 검사* + *Layout::clone()* 만, lock drop 후 serialize. 304 short-circuit 은 lock 안 그대로.
  - **PUT 의 double-serialize 제거**: 옛 코드는 `SessionLayout::new(layout)` 내부 + 1413 의 `canonical_bytes(&new_snap.layout)` 두 번 직렬화. amend: 한 번 serialize 후 그 bytes 를 `SessionLayout::new_with_bytes(layout, &bytes)` 신규 helper 로 재사용 — etag 계산도 같은 bytes 로.
  - **invariant 보존**: D5 (ETag SHA-256 first 16 bytes), D6 (검증 우선, 디스크 진입 전), D8 (no file-level advisory lock), D11 (0600/0700 권한), D13.(a)~(e) 의 순서. CAS atomicity 와 disk-first ordering 모두 변함 없음 — *worker block 회피와 read 동시성* 만 amend.
  - 회귀 가드: 신규 unit test 2 종 (`layout_get_releases_lock_before_serialization` — 동시 reader 가 큰 layout PUT 의 disk-write window 동안 block 되지 않음, `layout_put_uses_spawn_blocking_for_disk_write` — runtime worker 가 disk-write 동안 다른 task 의 progress 차단 안 함). 기존 CAS / If-Match / 412 / atomic-write 의 test 전부 통과.

- 2026-05-16: amend ×2 (Stage 6 cleanup, handover §5.3.3) — **legacy `/api/layout` v1 (singular endpoint) + `LayoutStore` + `LayoutSnapshot` 폐기**. schema v2 의 multi-session 진실 (`/api/sessions/<name>/layout` + `SessionLayout` + `SessionCache`) 가 single source of truth. FE 가 v2 완전 migrate (FE 의 `lib/http/sessions.ts` only, no `lib/http/layout.ts`) 로 BE 측 v1 surface 가 *progressive dead code*. 제거 영향: `AppState.layout` / `AppState.store` 필드 제거, `crates/http-api/src/storage.rs` 모듈 제거, `LayoutSnapshot` struct + v1 handler 2 개 + helper (`canonical_serialize`, `compute_etag`, `parse_etag_header`, `minimal_layout_check`) 제거, `AppState::with_hub_and_path()` constructor 제거 (`with_hub_and_workspace` 가 대체). CLI `state_files::layout_path_for()` 는 보존 — `gtmux teardown` 이 *upgrade-from-v1* 환경에서 잔존 파일 cleanup 용도로 계속 호출. 회귀 보호: 23 v1 unit test 제거 (워크스페이스 388 → 365 PASS, regression 0), smoke 02_stage5.sh 12/12 PASS — v2 endpoint 정합 보존. 정합: ADR-0019/0018 의 v2 진실, ADR-0006 D6/D7 의 ETag 정책 (이제 sessions.rs::sha256_128 + parse_etag_header) 그대로 보존.
