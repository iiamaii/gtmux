//! Bridge layer: schema/HTTP UUID ↔ internal pty-backend [`PaneId`] (Stage 4-A).
//!
//! Schema v2 (ADR-0018 D2) addresses every terminal item by a UUID-shaped
//! string — that UUID is what survives in the on-disk session layout. The
//! PTY backend (ADR-0013) issues opaque `PaneId(u64)` values for its own
//! purposes: broadcast keys, wire-protocol frames, kill / resize lookups.
//!
//! These two namespaces are bridged here. Every other module that needs to
//! cross the boundary goes through [`TerminalMap`] — `pty-backend` and
//! `ws-server` keep their u64-only world untouched (handover §4.2 Option B),
//! while `http-api` and the schema layer never see a `PaneId`.
//!
//! Invariants:
//!   * **Bijection** — each UUID maps to at most one [`PaneId`], and vice
//!     versa. Internally enforced by two `HashMap`s under a single
//!     `RwLock`; the public API never exposes either map directly.
//!   * **Commit-on-success** — a UUID is only inserted *after* the backend
//!     reports a successful spawn. Speculative reservations are not allowed,
//!     so a partial failure can never leak a half-registered UUID.
//!   * **Atomic unregister** — both halves drop together. Callers cannot
//!     observe a state where `lookup_pane(uuid).is_some()` while
//!     `lookup_uuid(pane).is_none()`.
//!
//! Concurrency model:
//!   * Reads (`lookup_*`, `snapshot`, `len`) take a read lock.
//!   * Mutations (`register`, `unregister_*`) take a write lock.
//!   * `register` returns a structured error rather than overwriting — the
//!     caller (`AppState::spawn_terminal_with_uuid`) uses this to detect a
//!     concurrent same-UUID spawn race and clean up the duplicate.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use gtmux_pty_backend::PaneId;
use thiserror::Error;
use tokio::sync::RwLock;

/// UUID-shaped string. We keep it as `String` rather than introducing a
/// newtype: the schema layer (`schema::validate`) is the single source of
/// truth for UUID format validation; this module only cares that the value
/// is a unique opaque key.
pub type TerminalUuid = String;

/// Bidirectional UUID ↔ [`PaneId`] map.
#[derive(Default, Debug)]
pub struct TerminalMap {
    inner: RwLock<TerminalMapInner>,
}

#[derive(Default, Debug)]
struct TerminalMapInner {
    by_uuid: HashMap<String, PaneId>,
    by_pane: HashMap<PaneId, String>,
}

impl TerminalMap {
    /// Empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a fresh UUID ↔ PaneId binding.
    ///
    /// Returns `Ok(())` on a new binding, *or* on an idempotent re-register
    /// where the same `(uuid, pane)` pair is already present. Any other
    /// conflict surfaces as [`MapError`] so the caller can recover.
    pub async fn register(&self, uuid: TerminalUuid, pane: PaneId) -> Result<(), MapError> {
        let mut g = self.inner.write().await;
        if let Some(existing) = g.by_uuid.get(&uuid) {
            if *existing == pane {
                return Ok(());
            }
            return Err(MapError::UuidAlreadyBound {
                uuid,
                existing_pane: *existing,
            });
        }
        if let Some(existing_uuid) = g.by_pane.get(&pane) {
            return Err(MapError::PaneAlreadyBound {
                pane,
                existing_uuid: existing_uuid.clone(),
            });
        }
        g.by_uuid.insert(uuid.clone(), pane);
        g.by_pane.insert(pane, uuid);
        Ok(())
    }

    /// Remove a binding by UUID. Returns the previously-bound [`PaneId`] if
    /// it existed, `None` otherwise (idempotent).
    pub async fn unregister_uuid(&self, uuid: &str) -> Option<PaneId> {
        let mut g = self.inner.write().await;
        let pane = g.by_uuid.remove(uuid)?;
        g.by_pane.remove(&pane);
        Some(pane)
    }

    /// Remove a binding by [`PaneId`]. Returns the previously-bound UUID if
    /// it existed, `None` otherwise (idempotent).
    pub async fn unregister_pane(&self, pane: PaneId) -> Option<String> {
        let mut g = self.inner.write().await;
        let uuid = g.by_pane.remove(&pane)?;
        g.by_uuid.remove(&uuid);
        Some(uuid)
    }

    /// Resolve a UUID to its current PaneId.
    pub async fn lookup_pane(&self, uuid: &str) -> Option<PaneId> {
        let g = self.inner.read().await;
        g.by_uuid.get(uuid).copied()
    }

    /// Resolve a PaneId to its current UUID.
    pub async fn lookup_uuid(&self, pane: PaneId) -> Option<String> {
        let g = self.inner.read().await;
        g.by_pane.get(&pane).cloned()
    }

    /// Point-in-time snapshot of every binding, in arbitrary order. Useful
    /// for `GET /api/terminals` (Batch 4-B) and for diagnostics.
    pub async fn snapshot(&self) -> Vec<(String, PaneId)> {
        let g = self.inner.read().await;
        g.by_uuid.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// Number of bindings.
    pub async fn len(&self) -> usize {
        self.inner.read().await.by_uuid.len()
    }

    /// `true` when the map holds no bindings.
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.by_uuid.is_empty()
    }
}

/// 0040 §5 option A — supply UUID↔PaneId bindings to the ws-server catch-up
/// replay. The handshake calls `alive_bindings()` once on every new connection
/// and emits a `0x88 TERMINAL_SPAWNED` frame per binding *before* the regular
/// pane-spawned NOTIFY / PANE_OUT replay, so reload / reconnect restores the
/// FE `terminalPool.paneIdByUuid` map without a `GET /api/terminals` poll.
#[async_trait]
impl gtmux_ws_server::TerminalUuidProvider for TerminalMap {
    async fn alive_bindings(&self) -> Vec<(u64, Arc<str>)> {
        let g = self.inner.read().await;
        g.by_uuid
            .iter()
            .map(|(uuid, pane)| (pane.0, Arc::<str>::from(uuid.as_str())))
            .collect()
    }
}

/// Mutation-time errors from [`TerminalMap::register`]. Both variants carry
/// the conflicting binding so the caller can either short-circuit (idempotent
/// path) or recover (race cleanup).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MapError {
    /// The UUID is already bound to a *different* PaneId. Typically a sign
    /// that two concurrent attaches both spawned for the same UUID — the
    /// caller should keep the existing binding and kill the loser.
    #[error("uuid {uuid:?} is already bound to pane {existing_pane:?}")]
    UuidAlreadyBound {
        /// UUID that was being registered.
        uuid: String,
        /// PaneId already bound to that UUID.
        existing_pane: PaneId,
    },
    /// The PaneId is already bound to a *different* UUID. An
    /// internal-consistency violation — every `PtyBackend::spawn` should
    /// produce a fresh, never-before-seen PaneId.
    #[error("pane {pane:?} is already bound to uuid {existing_uuid:?}")]
    PaneAlreadyBound {
        /// PaneId that was being registered.
        pane: PaneId,
        /// UUID already bound to that PaneId.
        existing_uuid: String,
    },
}

/// Generate a fresh UUID v4 string for a new Terminal. Reuses the same
/// ring-backed mint as [`crate::fresh_server_id`] so no extra dependency is
/// pulled in just for terminal ids.
pub fn fresh_terminal_uuid() -> String {
    crate::session_lock::fresh_server_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_then_lookup_round_trip() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        assert_eq!(map.lookup_pane("uuid-a").await, Some(PaneId(1)));
        assert_eq!(map.lookup_uuid(PaneId(1)).await, Some("uuid-a".into()));
        assert_eq!(map.len().await, 1);
    }

    #[tokio::test]
    async fn idempotent_register_returns_ok() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        // Same pair → idempotent ok.
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        assert_eq!(map.len().await, 1);
    }

    #[tokio::test]
    async fn uuid_conflict_returns_existing_pane() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        let err = map.register("uuid-a".into(), PaneId(2)).await.unwrap_err();
        match err {
            MapError::UuidAlreadyBound {
                uuid,
                existing_pane,
            } => {
                assert_eq!(uuid, "uuid-a");
                assert_eq!(existing_pane, PaneId(1));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
        // Map unchanged.
        assert_eq!(map.lookup_pane("uuid-a").await, Some(PaneId(1)));
        assert_eq!(map.len().await, 1);
    }

    #[tokio::test]
    async fn pane_conflict_returns_existing_uuid() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        let err = map.register("uuid-b".into(), PaneId(1)).await.unwrap_err();
        assert!(matches!(err, MapError::PaneAlreadyBound { .. }));
        // The would-be UUID is *not* in the map.
        assert_eq!(map.lookup_pane("uuid-b").await, None);
    }

    #[tokio::test]
    async fn unregister_uuid_drops_both_halves() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(7)).await.unwrap();
        assert_eq!(map.unregister_uuid("uuid-a").await, Some(PaneId(7)));
        assert_eq!(map.lookup_pane("uuid-a").await, None);
        assert_eq!(map.lookup_uuid(PaneId(7)).await, None);
        assert!(map.is_empty().await);
    }

    #[tokio::test]
    async fn unregister_pane_drops_both_halves() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(7)).await.unwrap();
        assert_eq!(map.unregister_pane(PaneId(7)).await, Some("uuid-a".into()));
        assert_eq!(map.lookup_pane("uuid-a").await, None);
        assert_eq!(map.lookup_uuid(PaneId(7)).await, None);
    }

    #[tokio::test]
    async fn unregister_missing_is_none() {
        let map = TerminalMap::new();
        assert_eq!(map.unregister_uuid("nope").await, None);
        assert_eq!(map.unregister_pane(PaneId(99)).await, None);
    }

    #[tokio::test]
    async fn snapshot_is_a_copy() {
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        map.register("uuid-b".into(), PaneId(2)).await.unwrap();
        let snap = map.snapshot().await;
        assert_eq!(snap.len(), 2);
        // Mutating after snapshot does not affect the snapshot vec.
        map.unregister_pane(PaneId(1)).await;
        assert_eq!(snap.len(), 2);
    }

    #[tokio::test]
    async fn fresh_uuid_is_uuid_v4_shaped() {
        let s = fresh_terminal_uuid();
        // 8-4-4-4-12 hex.
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // Version nibble = 4.
        assert!(parts[2].starts_with('4'));
    }

    // ── TerminalUuidProvider impl (0040 §5 option A) ─────────────────────

    #[tokio::test]
    async fn uuid_provider_empty_when_no_bindings() {
        use gtmux_ws_server::TerminalUuidProvider;
        let map = TerminalMap::new();
        let got = map.alive_bindings().await;
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn uuid_provider_returns_all_bindings() {
        use gtmux_ws_server::TerminalUuidProvider;
        let map = TerminalMap::new();
        map.register("uuid-a".into(), PaneId(1)).await.unwrap();
        map.register("uuid-b".into(), PaneId(2)).await.unwrap();
        map.register("uuid-c".into(), PaneId(7)).await.unwrap();
        let mut got = map.alive_bindings().await;
        got.sort_by_key(|(p, _)| *p);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[0].1.as_ref(), "uuid-a");
        assert_eq!(got[1].0, 2);
        assert_eq!(got[1].1.as_ref(), "uuid-b");
        assert_eq!(got[2].0, 7);
        assert_eq!(got[2].1.as_ref(), "uuid-c");
    }

    #[tokio::test]
    async fn uuid_provider_unregister_drops_from_snapshot() {
        use gtmux_ws_server::TerminalUuidProvider;
        let map = TerminalMap::new();
        map.register("u1".into(), PaneId(1)).await.unwrap();
        map.register("u2".into(), PaneId(2)).await.unwrap();
        map.unregister_pane(PaneId(1)).await;
        let got = map.alive_bindings().await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 2);
        assert_eq!(got[0].1.as_ref(), "u2");
    }
}
