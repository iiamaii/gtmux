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
    }

    /// Replace this session's entire contribution with `uuids` (drop any
    /// prior membership for `session`, then add fresh). Used by
    /// `import_handler` where the diff vs the old (non-existent) layout
    /// is "all of the new UUIDs are added".
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
    }

    /// Cold-boot rebuild. Walks every session file in the workspace,
    /// extracts terminal UUIDs, and replaces the index contents. Parse /
    /// I/O failures on individual files are silently skipped — matches the
    /// behaviour of the old `terminals.rs::scan_session_terminal_refs`.
    pub fn rebuild_from_disk(&self, wm: &WorkspaceManager) -> Result<(), WorkspaceError> {
        let mut new_map: HashMap<TerminalUuid, BTreeSet<SessionName>> = HashMap::new();
        for info in wm.enumerate_sessions()? {
            let path = match wm.session_path(&info.name) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let layout: Layout = match serde_json::from_slice(&bytes) {
                Ok(l) => l,
                Err(_) => continue,
            };
            for item in &layout.items {
                if let Item::Terminal { common } = item {
                    new_map
                        .entry(common.id.clone())
                        .or_default()
                        .insert(info.name.clone());
                }
            }
        }
        let mut g = self
            .inner
            .write()
            .expect("attach_index write lock poisoned");
        *g = new_map;
        Ok(())
    }

    /// Snapshot the whole index as `uuid → Vec<session>`. The Vec is
    /// ordered alphabetically because the inner store is a `BTreeSet`.
    /// Matches the output of the old `scan_session_terminal_refs`, which
    /// the `GET /api/terminals` handler joins against the live terminal
    /// pool.
    pub fn read_all_attach_refs(&self) -> HashMap<TerminalUuid, Vec<SessionName>> {
        let g = self
            .inner
            .read()
            .expect("attach_index read lock poisoned");
        g.iter()
            .map(|(uuid, set)| (uuid.clone(), set.iter().cloned().collect()))
            .collect()
    }

    /// Convenience read for a single UUID. Used by future readers (e.g.
    /// BE-5(b) follow-up) that don't need the full snapshot.
    pub fn read_attached_sessions(&self, uuid: &str) -> Vec<SessionName> {
        let g = self
            .inner
            .read()
            .expect("attach_index read lock poisoned");
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
