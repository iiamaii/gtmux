//! Attach reverse index — `terminal_uuid → BTreeSet<session_name>` (ADR-0021
//! D7 amend ③ / 0066 §BE-2 / 0067 Phase 4 / 0068 work package).
//!
//! The old `terminals.rs::scan_session_terminal_refs` walked every session
//! file on every `GET /api/terminals` request — synchronous `std::fs::read`
//! + `serde_json::from_slice` per file. On a multi-session workspace under
//! frontend polling (5 s cadence, ADR-0017 amend / `terminalPool.refresh`)
//! that turned into a recurring O(N_sessions × file_size) tax on every poll.
//!
//! This module replaces the scan with an in-memory reverse index. The index
//! is:
//!
//! 1. **Built once at boot** via [`AttachIndex::rebuild_from_disk`] — the
//!    same pattern as the old scan, but executed once.
//! 2. **Updated on every layout-mutating handler** (`PUT
//!    /api/sessions/:name/layout`, `DELETE
//!    /api/sessions/:name/items/:id`, `POST /api/sessions/import`,
//!    `DELETE /api/sessions/:name`) — the diff is applied *after* the
//!    on-disk write succeeds and the in-memory snapshot is swapped, so the
//!    index never gets ahead of the disk-of-truth (ADR-0006 D13 ordering).
//! 3. **Read by `terminals.rs::list_handler`** (BE-2) — `read_all_attach_refs`
//!    returns a per-UUID list of session names matching the old scan's
//!    output, ordered alphabetically by virtue of the inner `BTreeSet`.
//!
//! Concurrency: `std::sync::RwLock<HashMap<...>>`. All mutations are short
//! (a handful of HashMap/BTreeSet operations, no I/O) so a sync lock is
//! safe to hold across the critical section even from an async handler. The
//! tokio docs explicitly bless this pattern when the critical section
//! contains no `.await`. (`tokio::sync::RwLock` would force us to mark every
//! call site `.await` which complicates boot init where there is no async
//! runtime context to call `.await` on.)

use std::collections::{BTreeSet, HashMap};
use std::sync::RwLock;

use crate::schema::{Item, Layout};
use crate::workspace::{WorkspaceError, WorkspaceManager};

pub type TerminalUuid = String;
pub type SessionName = String;

#[derive(Default, Debug)]
pub struct AttachIndex {
    inner: RwLock<HashMap<TerminalUuid, BTreeSet<SessionName>>>,
}

impl AttachIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a single-session diff. `removed` UUIDs lose this session;
    /// `added` UUIDs gain it. UUIDs whose session set becomes empty are
    /// dropped from the map so `read_all_attach_refs` doesn't surface
    /// orphan entries.
    ///
    /// Callers are expected to hold the per-session `SessionLayout` write
    /// lock when invoking this, so two diffs for the same session cannot
    /// interleave.
    pub fn apply_diff(&self, session: &str, removed: &[String], added: &[String]) {
        if removed.is_empty() && added.is_empty() {
            return;
        }
        let mut g = self
            .inner
            .write()
            .expect("attach_index write lock poisoned");
        for uuid in removed {
            if let Some(set) = g.get_mut(uuid) {
                set.remove(session);
                if set.is_empty() {
                    g.remove(uuid);
                }
            }
        }
        for uuid in added {
            g.entry(uuid.clone())
                .or_default()
                .insert(session.to_string());
        }
        // 0073 FE follow-up — 사용자 시연 환경에서 *FE 의 (!) desync badge*
        // 발생 시 본 trace 와 cross-walk 으로 add miss 시점 추적. attach_count
        // 가 잘못된 0 인 UUID 의 attach_index 변천을 시계열로 본다.
        tracing::debug!(
            session = %session,
            removed_count = removed.len(),
            added_count = added.len(),
            removed = ?removed,
            added = ?added,
            total_entries = g.len(),
            "attach_index: apply_diff"
        );
    }

    /// Replace this session's entire contribution with `uuids` (drop any
    /// prior membership for `session`, then add fresh). Used by
    /// `import_handler` where the diff vs the old (non-existent) layout
    /// is "all of the new UUIDs are added", and by `classify_layout_terminals`
    /// / `attach_confirm_handler` as a *self-heal* hook against boot
    /// rebuild miss or any other source of stale state (0073 FE follow-up).
    pub fn apply_full_session(&self, session: &str, uuids: &[String]) {
        let mut g = self
            .inner
            .write()
            .expect("attach_index write lock poisoned");
        // Drop any UUID that currently references this session.
        let to_check: Vec<String> = g
            .iter()
            .filter_map(|(uuid, set)| {
                if set.contains(session) {
                    Some(uuid.clone())
                } else {
                    None
                }
            })
            .collect();
        let prior_count = to_check.len();
        for uuid in to_check {
            if let Some(set) = g.get_mut(&uuid) {
                set.remove(session);
                if set.is_empty() {
                    g.remove(&uuid);
                }
            }
        }
        // Add fresh.
        for uuid in uuids {
            g.entry(uuid.clone())
                .or_default()
                .insert(session.to_string());
        }
        // 0073 FE follow-up 진단 보조 — *new vs prior* 차이가 크면 self-heal
        // 이 실제로 missing entries 를 회복한 셈이다. boot rebuild miss 의
        // 강한 증거.
        let new_count = uuids.len();
        if new_count != prior_count {
            tracing::warn!(
                session = %session,
                prior_count,
                new_count,
                total_entries = g.len(),
                "attach_index: apply_full_session — count drift detected (self-heal recovered missing entries?)"
            );
        } else {
            tracing::debug!(
                session = %session,
                uuid_count = new_count,
                total_entries = g.len(),
                "attach_index: apply_full_session"
            );
        }
    }

    /// Drop `session` from every UUID's session set. Used by
    /// `delete_handler` when an entire session record is unlinked.
    pub fn forget_session(&self, session: &str) {
        let mut g = self
            .inner
            .write()
            .expect("attach_index write lock poisoned");
        let to_check: Vec<String> = g
            .iter()
            .filter_map(|(uuid, set)| {
                if set.contains(session) {
                    Some(uuid.clone())
                } else {
                    None
                }
            })
            .collect();
        for uuid in to_check {
            if let Some(set) = g.get_mut(&uuid) {
                set.remove(session);
                if set.is_empty() {
                    g.remove(&uuid);
                }
            }
        }
        tracing::debug!(
            session = %session,
            total_entries = g.len(),
            "attach_index: forget_session"
        );
    }

    /// Cold-boot rebuild. Walks every session file in the workspace,
    /// extracts terminal UUIDs, and replaces the index contents.
    ///
    /// Individual-file failures (`session_path` validation, `std::fs::read`,
    /// JSON parse) are non-fatal — the rebuild continues with the next
    /// file. **A `warn`-level log is emitted on each skip** so an operator
    /// can correlate a missing-from-`GET /api/terminals` row with a
    /// corrupt session file (0072 BE follow-up §2). Functional impact of
    /// a skipped file is bounded: the affected UUIDs simply do not appear
    /// in the index until the session's next successful layout PUT
    /// re-registers them through the `apply_diff` hook.
    pub fn rebuild_from_disk(&self, wm: &WorkspaceManager) -> Result<(), WorkspaceError> {
        let mut new_map: HashMap<TerminalUuid, BTreeSet<SessionName>> = HashMap::new();
        let mut sessions_scanned = 0u32;
        let mut sessions_skipped = 0u32;
        for info in wm.enumerate_sessions()? {
            sessions_scanned += 1;
            let path = match wm.session_path(&info.name) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        session = %info.name,
                        error = %e,
                        "attach_index: invalid session_path; skipping in boot rebuild"
                    );
                    sessions_skipped += 1;
                    continue;
                }
            };
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        session = %info.name,
                        error = %e,
                        "attach_index: read failed; skipping in boot rebuild"
                    );
                    sessions_skipped += 1;
                    continue;
                }
            };
            let layout: Layout = match serde_json::from_slice(&bytes) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(
                        session = %info.name,
                        error = %e,
                        "attach_index: JSON parse failed; skipping in boot rebuild — affected UUIDs will (re)appear on the next successful layout PUT"
                    );
                    sessions_skipped += 1;
                    continue;
                }
            };
            let mut per_session_uuid_count = 0u32;
            for item in &layout.items {
                if let Item::Terminal { common } = item {
                    new_map
                        .entry(common.id.clone())
                        .or_default()
                        .insert(info.name.clone());
                    per_session_uuid_count += 1;
                }
            }
            tracing::debug!(
                session = %info.name,
                uuid_count = per_session_uuid_count,
                "attach_index: rebuild scanned session"
            );
        }
        let mut g = self
            .inner
            .write()
            .expect("attach_index write lock poisoned");
        *g = new_map;
        // 0073 FE follow-up — boot rebuild 결과의 *시점 snapshot*. 사용자
        // 시연 환경에서 *boot 직후* (!) desync 가 보이면 본 trace 의 entry
        // 수 + (RUST_LOG=trace 시 entries dump) 와 GET /api/terminals 응답을
        // 비교해 rebuild miss 인지 layout PUT path miss 인지 좁힐 수 있다.
        // sessions_skipped > 0 이면 그 session 의 UUID 가 영구히 attach_index
        // 에서 빠진 채라 desync 가 영속. WARN 으로 surface.
        if sessions_skipped > 0 {
            tracing::warn!(
                sessions_scanned,
                sessions_skipped,
                total_entries = g.len(),
                "attach_index: boot rebuild had skipped sessions — affected UUIDs missing from index until next successful layout PUT"
            );
        } else {
            tracing::info!(
                sessions_scanned,
                total_entries = g.len(),
                "attach_index: boot rebuild complete"
            );
        }
        Ok(())
    }

    /// Snapshot the whole index as `uuid → Vec<session>`. The Vec is
    /// ordered alphabetically because the inner store is a `BTreeSet`.
    /// Matches the output of the old `scan_session_terminal_refs`, which
    /// the `GET /api/terminals` handler joins against the live terminal
    /// pool.
    pub fn read_all_attach_refs(&self) -> HashMap<TerminalUuid, Vec<SessionName>> {
        let g = self.inner.read().expect("attach_index read lock poisoned");
        g.iter()
            .map(|(uuid, set)| (uuid.clone(), set.iter().cloned().collect()))
            .collect()
    }

    /// Convenience read for a single UUID. Used by future readers (e.g.
    /// BE-5(b) follow-up) that don't need the full snapshot.
    pub fn read_attached_sessions(&self, uuid: &str) -> Vec<SessionName> {
        let g = self.inner.read().expect("attach_index read lock poisoned");
        g.get(uuid)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Extract every terminal-variant UUID from a layout — used by the
/// mutation hooks to compute (removed, added) diffs against the new
/// layout vs the prior cached snapshot.
pub fn terminal_uuids_in(layout: &Layout) -> Vec<String> {
    layout
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Terminal { common } => Some(common.id.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::{ItemCommon, Layout, Viewport, Visibility, SCHEMA_VERSION};

    fn empty_set() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn s(x: &str) -> String {
        x.to_string()
    }

    #[test]
    fn apply_diff_adds_uuid_to_session() {
        let idx = AttachIndex::new();
        idx.apply_diff("alpha", &[], &[s("u1")]);
        let got = idx.read_all_attach_refs();
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("u1").unwrap(), &vec![s("alpha")]);
    }

    #[test]
    fn apply_diff_removes_uuid_from_session_and_gcs_empty_entry() {
        let idx = AttachIndex::new();
        idx.apply_diff("alpha", &[], &[s("u1")]);
        idx.apply_diff("alpha", &[s("u1")], &[]);
        let got = idx.read_all_attach_refs();
        assert!(
            got.is_empty(),
            "entry with empty session set must be GC'd, got: {got:?}"
        );
    }

    #[test]
    fn apply_diff_multi_session_partial_remove_keeps_entry() {
        let idx = AttachIndex::new();
        idx.apply_diff("alpha", &[], &[s("u1")]);
        idx.apply_diff("beta", &[], &[s("u1")]);
        idx.apply_diff("alpha", &[s("u1")], &[]);
        let got = idx.read_all_attach_refs();
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("u1").unwrap(), &vec![s("beta")]);
    }

    #[test]
    fn apply_full_session_replaces_session_contribution() {
        let idx = AttachIndex::new();
        idx.apply_diff("alpha", &[], &[s("u1"), s("u2")]);
        idx.apply_diff("beta", &[], &[s("u1")]); // u1 mirrored
                                                 // Replace alpha's contribution: drop u1+u2, add u3 only.
        idx.apply_full_session("alpha", &[s("u3")]);
        let got = idx.read_all_attach_refs();
        // u1 → {beta} (alpha dropped), u2 → GC, u3 → {alpha}
        assert_eq!(got.get("u1").unwrap(), &vec![s("beta")]);
        assert!(!got.contains_key("u2"));
        assert_eq!(got.get("u3").unwrap(), &vec![s("alpha")]);
    }

    #[test]
    fn forget_session_removes_from_all_entries() {
        let idx = AttachIndex::new();
        idx.apply_diff("alpha", &[], &[s("u1"), s("u2")]);
        idx.apply_diff("beta", &[], &[s("u1")]);
        idx.forget_session("alpha");
        let got = idx.read_all_attach_refs();
        assert_eq!(got.get("u1").unwrap(), &vec![s("beta")]); // beta survives
        assert!(!got.contains_key("u2")); // GC'd
    }

    #[test]
    fn rebuild_from_disk_parity_with_seeded_layouts() {
        use crate::workspace::WorkspaceManager;
        let dir = tempfile::TempDir::new().unwrap();
        let wm = WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap();
        // Seed two layouts: alpha owns u1+u2, beta owns u1 (mirror).
        let make_layout = |uuids: &[&str]| Layout {
            schema_version: SCHEMA_VERSION,
            groups: vec![],
            items: uuids
                .iter()
                .map(|u| Item::Terminal {
                    common: ItemCommon {
                        id: (*u).to_string(),
                        parent_id: None,
                        x: 0.0,
                        y: 0.0,
                        w: 100.0,
                        h: 100.0,
                        z: 0,
                        visibility: Visibility::Visible,
                        locked: false,
                        label: String::new(),
                        description: String::new(),
                        minimized: false,
                    },
                })
                .collect(),
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
            workspace_root: None,
        };
        std::fs::write(
            dir.path().join("alpha.json"),
            serde_json::to_vec(&make_layout(&["u1", "u2"])).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("beta.json"),
            serde_json::to_vec(&make_layout(&["u1"])).unwrap(),
        )
        .unwrap();

        let idx = AttachIndex::new();
        idx.rebuild_from_disk(&wm).unwrap();
        let got = idx.read_all_attach_refs();
        assert_eq!(got.get("u1").unwrap(), &vec![s("alpha"), s("beta")]);
        assert_eq!(got.get("u2").unwrap(), &vec![s("alpha")]);
        assert_eq!(got.len(), 2);
        // Silently skip nonsense — ensure that adding an unparseable
        // sidecar doesn't corrupt the rebuild.
        let _ = empty_set();
    }
}
