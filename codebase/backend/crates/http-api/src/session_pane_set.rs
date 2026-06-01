//! `SessionPaneSetProvider` impl on `AppState` (Slice next-2, ADR-0025 D6).
//!
//! Joins the session's `Layout.items` (filtered to `type:"terminal"`)
//! with `TerminalMap.by_uuid` to produce the `HashSet<u64>` of live
//! PaneIds. Used by the WS handler's `pane_output` filter to drop
//! frames whose PaneId isn't in the connection's session.

use std::collections::HashSet;

use async_trait::async_trait;

use crate::schema::Item;
use crate::AppState;

#[async_trait]
impl gtmux_ws_server::SessionPaneSetProvider for AppState {
    async fn pane_ids_for_session(&self, session_name: &str) -> HashSet<u64> {
        // Bail to an empty set on any missing primitive. Each branch
        // is a *legacy demo path* signal — the WS handler treats an
        // empty set as "session has no terminals yet", which is the
        // correct semantic when wm/cache/map are unconfigured (tests)
        // or the session doesn't exist (race vs DELETE).
        let Some(wm) = self.workspace.as_ref() else {
            return HashSet::new();
        };
        let entry = match self.session_cache.get_or_load(wm, session_name).await {
            Ok(arc) => arc,
            Err(_) => return HashSet::new(),
        };
        let uuids: Vec<String> = {
            let guard = entry.read().await;
            guard
                .layout
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Terminal { common, .. } => Some(common.id.clone()),
                    _ => None,
                })
                .collect()
        };
        // Resolve UUIDs → PaneIds via the TerminalMap bulk API (one read
        // lock, O(N) lookups — no full pool clone). 0067 BE-5(a) / 0066
        // §BE-5. Unmatched UUIDs (dangling terminal references, ADR-0021
        // D10) are omitted — the false-negative-is-safe invariant
        // (ADR-0025 D3) ensures they get added later via
        // `0x88 TERMINAL_SPAWNED` once the user clicks the dangling
        // panel to spawn-on-demand.
        self.terminal_map.resolve_uuids_to_panes(&uuids).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ItemCommon, Layout, SCHEMA_VERSION};
    use crate::workspace::WorkspaceManager;
    use gtmux_auth::issue_token;
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use gtmux_pty_backend::PaneId;
    use gtmux_ws_server::SessionPaneSetProvider;

    fn test_state_with_workspace() -> (AppState, tempfile::TempDir) {
        let token = issue_token().unwrap();
        let cfg = Config {
            schema_version: 1,
            server: ServerConfig {
                session: "test".to_string(),
                port: 9001,
                bind: "127.0.0.1".to_string(),
            },
            runtime: RuntimeConfig::default(),
            security: SecurityConfig {
                cors_origins: vec!["http://localhost:9001".to_string()],
                host_allowlist: vec!["127.0.0.1:9001".to_string()],
            },
            cloud: None,
            frontend_dist: None,
            workspace_path: None,
            server_workspace: None,
            default_session_workspace: None,
            auth: gtmux_config::AuthConfig::default(),
            assets: gtmux_config::AssetsConfig::default(),
        };
        let dir = tempfile::TempDir::new().unwrap();
        let wm = WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap();
        let state = AppState::new(cfg, token).with_workspace(wm);
        (state, dir)
    }

    fn seed_session(dir: &std::path::Path, name: &str, terminal_uuids: &[&str]) {
        let items: Vec<crate::schema::Item> = terminal_uuids
            .iter()
            .map(|u| crate::schema::Item::Terminal {
                common: ItemCommon {
                    id: (*u).to_string(),
                    parent_id: None,
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                    z: 0,
                    visibility: crate::schema::Visibility::Visible,
                    locked: false,
                    label: String::new(),
                    description: String::new(),
                    minimized: false,
                },
            })
            .collect();
        let layout = Layout {
            schema_version: SCHEMA_VERSION,
            groups: vec![],
            items,
            viewport: crate::schema::Viewport {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
            workspace_root: None,
        };
        let bytes = serde_json::to_vec(&layout).unwrap();
        std::fs::write(dir.join(format!("{name}.json")), bytes).unwrap();
    }

    #[tokio::test]
    async fn empty_layout_returns_empty_set() {
        let (state, dir) = test_state_with_workspace();
        seed_session(dir.path(), "empty", &[]);
        let set = state.pane_ids_for_session("empty").await;
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn unknown_session_returns_empty_set() {
        let (state, _dir) = test_state_with_workspace();
        let set = state.pane_ids_for_session("nope").await;
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn matched_terminal_uuids_resolve_to_pane_ids() {
        let (state, dir) = test_state_with_workspace();
        let uuid_a = "11111111-2222-4333-8444-555555555aaa";
        let uuid_b = "11111111-2222-4333-8444-555555555bbb";
        seed_session(dir.path(), "alpha", &[uuid_a, uuid_b]);
        state
            .terminal_map
            .register(uuid_a.to_string(), PaneId(7))
            .await
            .unwrap();
        state
            .terminal_map
            .register(uuid_b.to_string(), PaneId(11))
            .await
            .unwrap();
        let set = state.pane_ids_for_session("alpha").await;
        assert_eq!(set, [7u64, 11].into_iter().collect());
    }

    #[tokio::test]
    async fn unmatched_uuid_is_omitted_from_set() {
        // Dangling terminal reference: layout has the UUID but
        // TerminalMap doesn't. Per ADR-0025 D3 (false-negative-is-safe)
        // we silently drop the missing entry — the next
        // `0x88 TERMINAL_SPAWNED` will refresh the set.
        let (state, dir) = test_state_with_workspace();
        let uuid_dangling = "11111111-2222-4333-8444-555555555ccc";
        let uuid_live = "11111111-2222-4333-8444-555555555ddd";
        seed_session(dir.path(), "mixed", &[uuid_dangling, uuid_live]);
        state
            .terminal_map
            .register(uuid_live.to_string(), PaneId(42))
            .await
            .unwrap();
        let set = state.pane_ids_for_session("mixed").await;
        assert_eq!(set, [42u64].into_iter().collect());
    }

    #[tokio::test]
    async fn two_sessions_have_disjoint_sets() {
        // The core ADR-0025 invariant: session A's WS only forwards
        // bytes for PaneIds in A's layout. Verifying it here at the
        // provider level — the WS handler's contains() is then
        // mechanical.
        let (state, dir) = test_state_with_workspace();
        let uuid_a = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let uuid_b = "11111111-2222-4333-8444-555555555fff";
        seed_session(dir.path(), "left", &[uuid_a]);
        seed_session(dir.path(), "right", &[uuid_b]);
        state
            .terminal_map
            .register(uuid_a.to_string(), PaneId(100))
            .await
            .unwrap();
        state
            .terminal_map
            .register(uuid_b.to_string(), PaneId(200))
            .await
            .unwrap();
        let left = state.pane_ids_for_session("left").await;
        let right = state.pane_ids_for_session("right").await;
        assert_eq!(left, [100u64].into_iter().collect());
        assert_eq!(right, [200u64].into_iter().collect());
        assert!(left.is_disjoint(&right));
    }

    #[tokio::test]
    async fn cross_session_mirror_uuid_is_in_both_sets() {
        // ADR-0021 D2 + ADR-0025 D2: same UUID in two sessions'
        // layouts must yield the same PaneId in both filter sets.
        // The kernel broadcast carries one frame per output; both
        // WS handlers' filters must let it through.
        let (state, dir) = test_state_with_workspace();
        let uuid_mirror = "deadbeef-0000-4000-8000-000000000000";
        seed_session(dir.path(), "alpha", &[uuid_mirror]);
        seed_session(dir.path(), "beta", &[uuid_mirror]);
        state
            .terminal_map
            .register(uuid_mirror.to_string(), PaneId(77))
            .await
            .unwrap();
        let a = state.pane_ids_for_session("alpha").await;
        let b = state.pane_ids_for_session("beta").await;
        assert_eq!(a, [77u64].into_iter().collect());
        assert_eq!(b, [77u64].into_iter().collect());
    }
}
