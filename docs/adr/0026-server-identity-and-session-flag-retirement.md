# ADR-0026: Server identity 의 workspace-derived 모델 + `--session` flag retirement

- 상태: Proposed (2026-05-16) — 채택 후 코드 진입은 별 batch. 본 ADR 은 *어떤 identifier 로 어떻게 마이그레이트 하는지* 의 design lock 만.
- 일자: 2026-05-16 (Stage 6 cleanup batch 직후 — Slice D + next-2 + legacy /api/layout 폐기 후의 next-deferred)
- 결정자: agent (system-architect role)
- 근거: `docs/reports/0041-next-session-handover.md` §5.3.1 (4 곳의 session-id 종속 + 3 후보 대안 — workspace name / port / random UUID), `docs/sketch.md` §6, `docs/adr/0019-session-and-workspace-model.md` (Workspace = Server 의 storage dir 의 1:1), `docs/adr/0014-process-supervisor.md` D11 (orphan marker 의 GTMUX_SESSION env)
- 관련 ADR: ADR-0007 (server-session-port-binding, *supersede* — `--session` flag 의 본 ADR 채택 후 폐기), ADR-0019 (Session+Workspace Model, workspace = 1:1 Server identity), ADR-0014 (Process supervisor D11 — orphan reap env marker)
- Supersedes: ADR-0007 의 `--session` flag 정책 (잔존 부분)

## 맥락

`--session <name>` flag 는 ADR-0007 시대 (single-session-per-server) 의 잔재. ADR-0019 의 multi-session pivot 후 "Session" 어휘는 *Workspace 안의 named layout record* 로 재정의 (CONTEXT.md §"Session") — *Server 의 logical 이름* 으로서의 의미는 **이미 폐기**.

그러나 CLI / filesystem layer 에 잔존:

| 사용처 | 현 의존성 |
|---|---|
| `${XDG_STATE_HOME}/gtmux/<session>.pid` | `state_files::pidfile_path_for(&config.server.session)` |
| `${XDG_STATE_HOME}/gtmux/<session>.token` | `state_files::token_path_for(&config.server.session)` |
| `${XDG_STATE_HOME}/gtmux/<session>.layout.json` | `state_files::layout_path_for(&config.server.session)` — **upgrade-from-v1 cleanup 용도 only** (ADR-0006 amend ×2 후) |
| `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` | `state_files::config_path_for(&config.server.session)` |
| Orphan reap env marker | `PtyBackend::with_session(Some(config.server.session.clone()))` → child process 의 `GTMUX_SESSION` env (ADR-0014 D11) |
| Tracing/log context | `tracing::info!(session = %config.server.session, ...)` |
| CLI 서브명령 라우팅 | `gtmux teardown --session <name>` |

본 ADR 은 *server identity 의 새 모델* + *마이그레이션 path* 의 lock.

## 결정 (Decisions)

### D1. 서버 identity = workspace-derived

ADR-0019 가 명시한 *Server : Workspace = 1:1* 가 server identity 의 **canonical 출처**. workspace path 자체가 identifier — `<session>` 의 의미를 *그 workspace 의 short label* 로 좁힘.

#### D1.1 Machine identifier (filesystem path 종속용)

```
machine_id := base16(sha256(canonicalize(workspace_path))[..6])   // 12 hex chars
```

- 12 hex chars = 48 bits = ~280 trillion 명목 collision capacity — single user 환경에서 collision risk 0
- *canonicalize* 처리: symlink resolve + `..` 정리 → 같은 workspace 를 다른 경로로 호출해도 같은 id
- 결정적 (deterministic) — workspace 이동 시 별도 migration 없이 같은 id 재현

#### D1.2 Human label (CLI 사용자 display)

```
human_label := last_segment(canonicalize(workspace_path))
            // e.g., /Users/me/gtmux/work-laptop  →  "work-laptop"
            // e.g., /tmp/test                    →  "test"
            // empty / non-ASCII / 1-char-special → "workspace" (fallback)
```

표시 형식: `{human_label}-{machine_id_short_6}` — e.g., `work-laptop-a4f5b8`.

#### D1.3 Path 의 새 구조

```
${XDG_STATE_HOME}/gtmux/instances/{machine_id}/
    pid
    token
    layout.json        # upgrade-from-v1 cleanup 용도, 본 ADR 후 신규 write X
${XDG_CONFIG_HOME}/gtmux/instances/{machine_id}/
    config.toml
${XDG_STATE_HOME}/gtmux/instances/{machine_id}/audit/file-open-YYYYMMDD.log   # ADR-0023 D9 정합
```

`instances/{machine_id}/` 의 *directory-per-instance* 구조:
- 같은 사용자가 여러 workspace 운영 시 가시성 (`ls ~/.local/state/gtmux/instances/` 로 일괄 enumerate)
- 마이그레이션 단순화 — 옛 `<session>.{pid,token,layout.json}` 의 flat layout 의 진행성 cleanup
- `gtmux ls` 같은 향후 enumerate 명령의 자연 directory 기반

### D2. `--session <name>` flag retirement 정책

#### D2.1 Phase 1 (본 ADR + 별 batch, 즉시)

- `gtmux start` 의 `--session` flag → `--name <human-label-override>` 로 rename + deprecation warning
- `--session` 그대로 받아들이되 stderr 로 deprecation 경고: "warning: --session is deprecated; use --name (or omit for workspace-derived default)"
- machine_id 는 *항상 workspace path 에서 derive* — `--name` / `--session` 은 *human_label override* 만 (path 의 machine_id 부분 영향 X)
- 기본 동작: 둘 다 미지정 시 `human_label = last_segment(workspace_path)`

#### D2.2 Phase 2 (Stage 7+, 별 ADR amend)

- `--session` flag 완전 제거 (compile-time 거부)
- `--name` 도 *optional only* — `gtmux start` 가 workspace path 만 필요

#### D2.3 CLI 서브명령 적응

- `gtmux teardown --workspace <path>` → workspace 의 machine_id 결정 → instance 디렉터리 정리. `--session` / `--name` 도 그대로 받되 단지 *resolve-helper* 로 동작 (multiple match 시 ambiguous 경고)
- `gtmux ls` (P1+ 신규) — `${XDG_STATE_HOME}/gtmux/instances/` enumerate

### D3. Config file 의 위치 변화

원안 (ADR-0019 / 0007 시대): `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml`

새 정책:
- **본 ADR**: `${XDG_CONFIG_HOME}/gtmux/instances/{machine_id}/config.toml`
- 마이그레이션 시점에 옛 위치의 `config.toml` 이 *first-run reading-only fallback* — boot 시 새 위치 미존재 + 옛 위치 발견 → 옛 contents read → 새 위치에 write → 옛 파일 leave (사용자 명시 cleanup)
- 보조 (선택, P1+): `~/.config/gtmux/global.toml` — XDG_CONFIG_HOME wide 의 *default-of-defaults* layer (figment chain 의 lowest layer)

### D4. Orphan reap marker (ADR-0014 D11) 적응

ADR-0014 D11 의 `GTMUX_SESSION=<session>` + `GTMUX_SERVER_PID=<pid>` env injection 은 *그대로 유지*. 단:

- 변수 이름은 `GTMUX_SESSION` 그대로 (backward compat — 이전 환경 의 잔존 process 도 reap 가능해야)
- 값 의 의미가 *human label* → **machine_id 의 12 hex** 로 변경
- `boot_scanner` (ADR-0014 D11 의 boot 측) 가 `cfg.server.session` 비교 대신 `current_machine_id` 비교 — 본 ADR D1.1 의 algorithm 으로 boot 시 재계산

이 path:
- Backward compat: 옛 (session-id 인코딩된) child shells 도 정상 reap. machine_id 12-hex 와 옛 임의 string 의 매칭 가능성 0 (e.g., `demo` 같은 옛 값은 hex 안 — 매칭 X). 옛 orphan 은 reap 안 되고 그 자리에 남음 — 사용자 명시 kill 필요 (1회성)
- 미래 보장: 본 ADR 후 모든 신규 child shell 은 machine_id 보유 → 다음 boot 의 boot_scanner 가 정상 reap

### D5. Default workspace 의 처리

ADR-0019 D2 의 default `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/`:

- canonicalize 후 결과의 machine_id 가 *유일한 기본값* (한 사용자 = 한 default workspace)
- human_label = "workspace" → 표시 = `workspace-{machine_id_6}`
- 다중 default workspace 운영 의도 시 (드물지만 가능) `--workspace <override>` 사용 — 별 machine_id 자동 할당

### D6. 거절된 대안 (R26)

#### R26-A. Port 를 server identity 로
거절. (a) port 는 *runtime-ephemeral* — server 종료 후 file enumerate 시점에 port 정보 X (사용자가 `lsof` 보고 매칭 어려움). (b) 같은 workspace 의 두 다른 port boot 시 동일 server 의 다른 identity = 의미 모호. (c) port 가 conflict 로 fallback (random or +1) 시 identity 의 *재현성* 깨짐.

#### R26-B. Random UUID v4 (first boot 에 mint, 영속)
거절. (a) opaque — 사용자가 `ls ~/.local/state/gtmux/instances/` 보고 어느 게 어느 workspace 인지 불명. (b) 분실 시 *orphan instance* — workspace 만 알면 다시 매칭 불가, 별 mapping 파일 필요. (c) workspace 가 deterministic 이고 stable 한데 random UUID 를 끼우는 건 의미 없는 indirection.

#### R26-C. Workspace path 의 last segment 만 (hash 없이)
거절. (a) 충돌 — `~/projects/gtmux/workspace` 와 `~/other/gtmux/workspace` 의 last-segment 둘 다 "workspace". (b) ASCII / special-char workspace name 처리 어려움 (`workspace ` trailing space, 한글 등 — filesystem 안 safe 보장 X). 해결책으로 escaping 추가하면 sha256-derived 보다 복잡.

#### R26-D. Hybrid: workspace name unique 일 때만 사용, 충돌 시 hash fallback
거절. (a) 사용자가 두 번째 instance 생성 시 *기존 instance 의 identity 가 자동 변경* — surprise. (b) `gtmux ls` 의 display 일관성 X. (c) sha256-prefix 의 always-suffix 가 더 단순 + 예측가능.

### D7. 마이그레이션 path (Phase 1 → Phase 2)

#### D7.1 Phase 1 (본 ADR 채택 + 별 BE batch, 1-2 일)

1. `Config.server.session` field → `Config.server.name: Option<String>` rename. backward-compat: `session` key 의 figment merge 도 받되 deprecation warning.
2. `state_files::pidfile_path_for(session)` → `state_files::pidfile_path_for(machine_id: &MachineId)`. 4 path function 모두 rename + signature 변경.
3. `instance_dir_for(machine_id)` 신규 — `${XDG_STATE_HOME}/gtmux/instances/{machine_id}/` 보장 (mkdir on first write).
4. `MachineId::from_workspace(&path) -> MachineId` 신규 — D1.1 algorithm.
5. CLI `--session` → `--name` rename (deprecation warning 한 batch).
6. `gtmux teardown` 의 분기 갱신.
7. ADR-0014 D11 의 boot_scanner — `cfg.server.session` 비교 → `current_machine_id` 비교.
8. 회귀 test:
   - 옛 default workspace 의 instance discovery (옛 flat path → 새 instance dir migration 의 첫 boot)
   - workspace path 의 canonicalize edge cases (symlink, `..`, 후행 `/`)
   - 두 다른 workspace 의 machine_id 가 다름

#### D7.2 Phase 2 (Stage 7+, 미래)

1. `--session` flag 완전 제거 — compile-time error.
2. `Config.server.session` field 제거. figment 의 backward-compat warning 도 제거.
3. ADR amend ② (이후).

### D8. 영향

#### D8.1 Code

- `crates/config/src/lib.rs`: `ServerConfig::session` rename + figment migration
- `bin/gtmux-cli/src/state_files.rs`: 4 path function signature 변경
- `bin/gtmux-cli/src/main.rs`: `config.server.session` 의 14 callsite → `machine_id` (D8 보면) 또는 `config.server.name`
- `bin/gtmux-cli/src/process_audit.rs`: `reap_orphans(&machine_id)` 시그니처
- `crates/pty-backend/src/lib.rs`: `PtyBackend::with_session(...)` 의 의미 (env 값) 만 변경, 시그니처 그대로
- 신규: `bin/gtmux-cli/src/instance_id.rs` (machine_id 알고리즘 + canonicalize)

#### D8.2 ADR / Docs

- ADR-0007: `--session` 정책 부분 supersede 명시
- ADR-0019: D1 의 *Workspace = 1:1 Server identity* 가 본 ADR 의 D1 source 임을 cross-ref
- ADR-0014 D11: amend — boot_scanner 의 machine_id 비교 변경
- CONTEXT.md: *Server* 어휘에 "machine_id (workspace-derived 12-hex) 가 filesystem identity" 한 줄 추가
- handover §5.3.1: 본 ADR 로 dispatch 마크

#### D8.3 사용자 영향

- Phase 1 후 첫 boot: 옛 `~/.local/state/gtmux/<session>.{pid,token}` 의 *idle*. 새 `~/.local/state/gtmux/instances/{id}/{pid,token}` 가 동작. 사용자는 옛 파일 manual cleanup 가능 (안 해도 무해 — gtmux 가 거기 안 씀).
- `gtmux teardown --session <oldname>` 도 backward-compat warning 후 `--name <oldname>` 처럼 동작 — multi-instance 환경에서 last-segment 매칭 → 적절한 machine_id resolve.

### D9. 보안 / 측정

#### D9.1 보안

- machine_id 의 sha256 prefix — single-user 환경에서 충분. multi-user 환경 (P3+) 진입 시 별 review 필요 (machine_id 가 user 별 *고유한가* — 다른 사용자가 같은 workspace path 사용 가능? 그러나 ADR-0007 D2 의 1:1:1 이 user 별 home 으로 분리되어 자연 격리).
- canonical path → symlink swap attack (사용자가 boot 후 workspace symlink 를 다른 경로로 옮김) 방어 X. 단 single-user 환경의 self-trust 정책 — 가능하나 자연한 사용자 행동 X.

#### D9.2 측정

| 지표 | 측정 | 기대 |
|---|---|---|
| `--session` flag 의 outbound 호출 횟수 (CLI 사용자) | telemetry 미존재 → 사용자 보고 | Phase 1 후 deprecation warning 출력 ~0 회 / month → Phase 2 진입 가능 |
| Instance dir creation 의 boot latency 증가 | benchmark | < 5 ms (mkdir + 2 write) |
| Backward compat: 옛 path 에 token 있을 시 boot | 회귀 test | 옛 path read + 새 path write, exit 0 |
| Machine id collision | analytical | 12 hex = 48 bits, single user → 0 |

### D10. Open questions

- **O1. workspace path 의 canonicalize 시점**: boot-time 1회 vs 모든 path access 마다. 권장: boot 1회 (`MachineId::from_workspace` 호출 직후 `OnceLock` 캐싱). symlink 가 boot 후 변경 시 *불일치* 가능하나 이미 process-level의 일관성이 더 중요.
- **O2. Backward compat 의 grace period**: Phase 1 + N versions 후 Phase 2? `N` 값 — handover 의 별 amend 결정.
- **O3. Audit log 의 path 변경 (ADR-0023 D9)**: 본 ADR 의 `instances/{id}/audit/` 가 ADR-0023 D9 의 `${XDG_STATE_HOME}/gtmux/audit/` 보다 더 좁음 — ADR-0023 D9 amend 필요. 본 ADR 채택 시 동시 amend.
- **O4. `gtmux ls` 등 enumerate 명령**: 별 ADR / sketch entry — P1+ 후속.

## 변경 이력

- 2026-05-16: 초안 (Proposed). Stage 6 cleanup batch (commits `21ea4ea` 등) 직후 — Slice D / next-2 / legacy /api/layout 폐기 의 다음 deferred 항목. 채택 시 Phase 1 별 batch 진입 + 코드 변화 11 file (config + state_files + main + process_audit + 신규 instance_id) + 1-2 일 작업. Phase 2 는 Stage 7+ 별 amend.
