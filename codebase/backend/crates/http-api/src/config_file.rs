//! Persistent configuration editing. File text is authoritative; running
//! configuration is immutable until the owner restarts the server.
use std::{io::{Read, Write}, path::PathBuf, sync::Arc};
use axum::{extract::{Request, State}, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Deserialize;
use serde_json::json;
use fs2::FileExt;
use crate::{AppState, auth::{StepUpBody, verify_step_up}};
const LIMIT: usize = 128 * 1024;
// Explicit unlock also releases any transient fork-inherited descriptor before
// exec closes it; closing only this descriptor can retain the shared flock.
struct WriterLock(std::fs::File);
impl Drop for WriterLock {
    fn drop(&mut self) { let _ = FileExt::unlock(&self.0); }
}

/// The embedder must explicitly opt in to file editing by supplying this path.
#[derive(Clone)]
pub struct ConfigFile {
    path: PathBuf,
    boot_settings: Option<gtmux_config::Config>,
    gate: Arc<std::sync::Mutex<()>>,
}
fn revision(text: Option<&str>) -> String {
    match text {
        None => "missing".into(),
        Some(text) => ring::digest::digest(&ring::digest::SHA256, text.as_bytes()).as_ref()
            .iter().map(|b| format!("{b:02x}")).collect(),
    }
}
fn read(path: &std::path::Path) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => return Err("Configuration must be a regular file, not a symlink".into()),
        Ok(m) if m.len() > LIMIT as u64 => return Err("Configuration exceeds 128 KiB".into()),
        _ => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|e| e.to_string())?.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT { return Err("Configuration exceeds 128 KiB".into()); }
    String::from_utf8(bytes).map(Some).map_err(|e| e.to_string())
}
impl ConfigFile {
    /// Bind once at boot. No writes occur until an authenticated save.
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if !path.is_absolute() { return Err("Configuration path must be absolute".into()); }
        let original = read(&path)?;
        Ok(Self { path, boot_settings: original.as_deref().and_then(|text| gtmux_config::parse_document(text).ok()), gate: Arc::default() })
    }
    fn snapshot(&self, running: &gtmux_config::Config) -> Result<serde_json::Value, String> {
        let text = read(&self.path)?;
        let current_revision = revision(text.as_deref());
        let contents = match text {
            Some(text) => text,
            None => toml_edit::ser::to_string_pretty(running).map_err(|e| e.to_string())?,
        };
        let parsed = gtmux_config::parse_document(&contents);
        let restart_values = |config: &gtmux_config::Config| {
            let mut value = serde_json::to_value(config).unwrap_or_default();
            if let Some(object) = value.as_object_mut() { object.remove("behavior"); }
            value
        };
        let restart_required = parsed.as_ref().map(|saved| restart_values(saved) != restart_values(self.boot_settings.as_ref().unwrap_or(running))).unwrap_or(true);
        Ok(json!({"available":true,"path":self.path,"contents":contents,"revision":current_revision,
            "restart_required":restart_required,
            "saved":parsed.as_ref().ok(),"validation_error":parsed.err().map(|e| e.to_string()),
            "running":running}))
    }
    pub(crate) fn save_behavior(&self, behavior: gtmux_config::BehaviorSettings, running: &gtmux_config::Config) -> Result<(), (StatusCode, String)> {
        let snapshot = self.snapshot(running).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let mut doc = snapshot["contents"].as_str().unwrap_or("").parse::<toml_edit::DocumentMut>()
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let values = serde_json::to_value(behavior).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        // Validate shape before indexing; malformed external edits must not panic.
        gtmux_config::parse_document(&doc.to_string()).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        for (key, value) in values.as_object().into_iter().flatten() {
            doc["behavior"][key] = toml_edit::value(value.as_bool().unwrap_or(false));
        }
        self.save(snapshot["revision"].as_str().unwrap_or(""), &doc.to_string(), &running.server.session)
    }
    fn save(&self, expected: &str, text: &str, instance: &str) -> Result<(), (StatusCode, String)> {
        let fail = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
        let cfg = gtmux_config::parse_document(text).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        if cfg.server.session != instance { return Err((StatusCode::BAD_REQUEST, "Server instance name cannot be changed here".into())); }
        if cfg.server.bind.parse::<std::net::IpAddr>().is_err() { return Err((StatusCode::BAD_REQUEST, "Use an IP address for server.bind".into())); }
        for path in [&cfg.server_workspace, &cfg.default_session_workspace].into_iter().flatten() {
            if !path.is_absolute() || !path.is_dir() { return Err((StatusCode::BAD_REQUEST, format!("Workspace must be an existing absolute directory: {}", path.display()))); }
        }
        if let (Some(root), Some(default)) = (&cfg.server_workspace, &cfg.default_session_workspace) {
            if !default.canonicalize().map_err(|e| fail(e.to_string()))?.starts_with(root.canonicalize().map_err(|e| fail(e.to_string()))?) {
                return Err((StatusCode::BAD_REQUEST, "Default session workspace must be inside server workspace".into()));
            }
        }
        let _gate = self.gate.lock().map_err(|_| fail("Config writer unavailable".into()))?;
        let parent = self.path.parent().ok_or_else(|| fail("No parent directory".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| fail(e.to_string()))?;
        let lock_path = self.path.with_extension("toml.gtmux-lock");
        if std::fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(fail("Config writer lock must not be a symlink".into()));
        }
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path).map_err(|e| fail(e.to_string()))?;
        lock.try_lock_exclusive().map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
        let _lock = WriterLock(lock);
        let current = read(&self.path).map_err(fail)?;
        if revision(current.as_deref()) != expected { return Err((StatusCode::CONFLICT, "Configuration changed on disk; reload before saving".into())); }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.permissions().readonly()) { return Err(fail("Configuration file is read-only".into())); }
        let mut file = atomic_write_file::AtomicWriteFile::open(&self.path).map_err(|e| fail(e.to_string()))?;
        file.write_all(text.as_bytes()).map_err(|e| fail(e.to_string()))?;
        // Detect non-cooperating editors during validation/write as well.
        if revision(read(&self.path).map_err(fail)?.as_deref()) != expected { return Err((StatusCode::CONFLICT, "Configuration changed on disk; reload before saving".into())); }
        file.commit().map_err(|e| fail(e.to_string()))?;
        Ok(())
    }
}
fn error(status: StatusCode, message: String) -> Response {
    (status, Json(json!({"error":"config_file_error","message":message}))).into_response()
}
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let Some(file) = state.config_file.clone() else {
        return Json(json!({"available":false,"reason":"This host has not enabled configuration file editing"})).into_response();
    };
    let config = state.config.clone();
    match tokio::task::spawn_blocking(move || file.snapshot(&config)).await {
        Ok(Ok(snapshot)) => Json(snapshot).into_response(),
        Ok(Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save { contents: String, revision: String, credential: Option<String> }
pub(crate) async fn put(State(state): State<AppState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, LIMIT * 2).await { Ok(b) => b, Err(e) => return error(StatusCode::PAYLOAD_TOO_LARGE, e.to_string()) };
    let save: Save = match serde_json::from_slice(&bytes) { Ok(s) => s, Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()) };
    if save.contents.len() > LIMIT { return error(StatusCode::PAYLOAD_TOO_LARGE, "Configuration exceeds 128 KiB".into()); }
    if let Err(e) = verify_step_up(&state, &parts.headers, crate::auth::peer_from_parts(&parts), &StepUpBody { credential: save.credential }).await { return e.into_response(); }
    let Some(file) = state.config_file.clone() else { return error(StatusCode::SERVICE_UNAVAILABLE, "Host has not enabled file editing".into()); };
    let parsed = match gtmux_config::parse_document(&save.contents) {
        Ok(parsed) => parsed, Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()),
    };
    // Serialize with Settings PATCH so it cannot overwrite an accepted TOML save.
    let mut behavior = state.behavior_settings.write().await;
    let config = state.config.clone();
    match tokio::task::spawn_blocking(move || {
        file.save(&save.revision, &save.contents, &config.server.session)?;
        Ok::<_, (StatusCode, String)>(file.snapshot(&config))
    }).await {
        Ok(Ok(snapshot)) => {
            *behavior = parsed.behavior;
            match snapshot { Ok(value) => Json(value).into_response(), Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e) }
        },
        Ok(Err((status, message))) => error(status, message),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preview {
    contents: String,
    port: u16,
    server_workspace: String,
    default_session_workspace: String,
}
/// Edit common fields without losing comments or changing disk state.
pub(crate) async fn preview(Json(body): Json<Preview>) -> Response {
    if body.contents.len() > LIMIT { return error(StatusCode::PAYLOAD_TOO_LARGE, "Configuration exceeds 128 KiB".into()); }
    if let Err(e) = gtmux_config::parse_document(&body.contents) { return error(StatusCode::BAD_REQUEST, e.to_string()); }
    let mut doc = match body.contents.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc, Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let old_port = doc["server"]["port"].as_integer();
    if doc.get("public_origin").and_then(|v| v.as_str()).is_some() {
        if let Some(old) = old_port {
            for key in ["host_allowlist", "cors_origins"] {
                if let Some(values) = doc.get_mut("security").and_then(|v| v.get_mut(key)).and_then(|v| v.as_array_mut()) {
                    for value in values.iter_mut() {
                        if let Some(text) = value.as_str() {
                            for host in ["127.0.0.1", "localhost", "http://127.0.0.1", "http://localhost"] {
                                if text == format!("{host}:{old}") { *value = toml_edit::Value::from(format!("{host}:{}", body.port)); break; }
                            }
                        }
                    }
                }
            }
        }
    }
    doc["server"]["port"] = toml_edit::value(i64::from(body.port));
    for (key, value) in [("server_workspace", body.server_workspace), ("default_session_workspace", body.default_session_workspace)] {
        if value.is_empty() { doc.remove(key); } else { doc[key] = toml_edit::value(value); }
    }
    let contents = doc.to_string();
    match gtmux_config::parse_document(&contents) {
        Ok(config) => Json(json!({"contents":contents,"saved":config})).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = "schema_version = 1\n# keep me\n[server]\nsession = 'test'\nport = 9002\nbind = '127.0.0.1'\n";
    #[tokio::test]
    async fn config_routes_require_auth_and_step_up_and_behavior_persists() {
        use axum::{body::{Body, to_bytes}, http::{Request, header}};
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        std::fs::write(&path, VALID).unwrap();
        let token = gtmux_auth::issue_token().unwrap();
        let state = AppState::new(gtmux_config::parse_document(VALID).unwrap(), token.clone())
            .with_config_file(ConfigFile::new(path.clone()).unwrap());
        let app = crate::router_with_state(state.clone());
        let request = |method: &str, uri: &str, auth: bool, body: serde_json::Value| {
            let mut req = Request::builder().method(method).uri(uri).header(header::HOST, "127.0.0.1:9002")
                .header(header::CONTENT_TYPE, "application/json");
            if auth { req = req.header(header::AUTHORIZATION, format!("Bearer {}", token.0)); }
            req.body(Body::from(body.to_string())).unwrap()
        };
        assert_eq!(app.clone().oneshot(request("GET", "/api/config", false, json!({}))).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let response = app.clone().oneshot(request("GET", "/api/config", true, json!({}))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let snapshot: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 100000).await.unwrap()).unwrap();
        let mut save = json!({"contents":VALID,"revision":snapshot["revision"]});
        assert_eq!(app.clone().oneshot(request("PUT", "/api/config", true, save.clone())).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        save["credential"] = json!(token.0);
        assert_eq!(app.clone().oneshot(request("PUT", "/api/config", true, save)).await.unwrap().status(), StatusCode::OK);
        let response = app.clone().oneshot(request("PATCH", "/api/settings", true,
            json!({"behavior":{"picker_show_hidden":true}}))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.behavior_settings.read().await.picker_show_hidden);
        assert!(gtmux_config::parse_document(&std::fs::read_to_string(&path).unwrap()).unwrap().behavior.picker_show_hidden);
        assert_eq!(state.config_file.as_ref().unwrap().snapshot(&state.config).unwrap()["restart_required"], false);
        let current = state.config_file.as_ref().unwrap().snapshot(&state.config).unwrap();
        let changed = current["contents"].as_str().unwrap().replace("picker_show_hidden = true", "picker_show_hidden = false");
        let response = app.clone().oneshot(request("PUT", "/api/config", true,
            json!({"contents":changed,"revision":current["revision"],"credential":token.0}))).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 100000).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert!(!state.behavior_settings.read().await.picker_show_hidden);
        // Invalid external edits must not be overwritten or applied in memory.
        std::fs::write(&path, "broken=[").unwrap();
        let response = app.oneshot(request("PATCH", "/api/settings", true,
            json!({"behavior":{"picker_show_hidden":true}}))).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!state.behavior_settings.read().await.picker_show_hidden);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken=[");
    }

    #[test]
    fn save_preserves_comments_and_rejects_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.toml");
        std::fs::write(&path, VALID).unwrap();
        let file = ConfigFile::new(path.clone()).unwrap();
        let next = VALID.replace("9002", "9003");
        file.save(&revision(Some(VALID)), &next, "test").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), next);
        assert_eq!(file.save(&revision(Some(VALID)), VALID, "test").unwrap_err().0, StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), next);
        let running = gtmux_config::parse_document(VALID).unwrap();
        assert_eq!(file.snapshot(&running).unwrap()["restart_required"], true);
    }
    #[test]
    fn invalid_config_and_instance_change_leave_disk_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.toml");
        std::fs::write(&path, VALID).unwrap();
        let file = ConfigFile::new(path.clone()).unwrap();
        for text in [VALID.replace("9002", "12"), VALID.replace("'test'", "'other'"), "invalid=[".into()] {
            assert_eq!(file.save(&revision(Some(VALID)), &text, "test").unwrap_err().0, StatusCode::BAD_REQUEST);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        }
    }
    #[test]
    fn missing_file_can_be_created_but_external_creation_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.toml");
        let file = ConfigFile::new(path.clone()).unwrap();
        file.save("missing", VALID, "test").unwrap();
        assert_eq!(file.save("missing", VALID, "test").unwrap_err().0, StatusCode::CONFLICT);
    }
    #[cfg(unix)]
    #[test]
    #[cfg(unix)]
    fn readonly_file_reports_failure_and_symlink_is_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.toml");
        std::fs::write(&path, VALID).unwrap();
        let file = ConfigFile::new(path.clone()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(file.save(&revision(Some(VALID)), VALID, "test").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        let alias = dir.path().join("alias.toml");
        symlink(&path, &alias).unwrap();
        assert!(ConfigFile::new(alias).is_err());
    }
}
