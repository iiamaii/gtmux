//! Blocking HTTP client for the ADR-0053 remote-control surface.
//!
//! Server discovery (ADR-0053 D9) reuses the `gtmux status` state-file
//! layout: instances are enumerated from `${XDG_STATE_HOME}/gtmux/*.token`,
//! liveness comes from the pidfile probe, and the bound host/port is read
//! from `${XDG_CONFIG_HOME}/gtmux/<instance>.config.toml` (the same
//! best-effort parse `rotate-token` uses for its Open URL line). Instance
//! selection: explicit `--instance` flag → `GTMUX_SERVER_INSTANCE` env
//! (clap `env` attr — injected into every gtmux-spawned shell) → the single
//! *running* instance → error listing candidates.
//!
//! Auth: `gtmux_auth::load_token(instance)` → `Authorization: Bearer`.
//! Querystring tokens are forbidden (ADR-0003 R(rej)2 / ADR-0053 D6).

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

use crate::state_files::{check_pidfile_liveness, PidLiveness};

/// Read cap for response bodies (mirrors the server's 16 MB layout ceiling
/// plus headroom — `ureq`'s `into_string()` caps at 10 MB which a large
/// layout GET can exceed, so we read via `into_reader()` ourselves).
const RESPONSE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Client-side failure taxonomy. `exit_code()` implements the ADR-0053 D2
/// "one stderr line with a machine-readable code + non-zero exit" contract:
/// auth failures → 5 (EXIT_PERMISSION), missing targets → 3, rest → 1.
#[derive(Debug)]
pub enum CliError {
    /// Server responded with a non-2xx status. `code` is the server's
    /// machine-readable error code (`error`/`code` body field).
    Api {
        status: u16,
        code: String,
        message: String,
    },
    /// Connection-level failure (server down, bad host, timeout).
    Transport(String),
    /// Local pre-flight failure (discovery, token, argument validation).
    Local(String),
}

impl CliError {
    pub fn local(msg: impl Into<String>) -> Self {
        Self::Local(msg.into())
    }

    /// Exit code per the grill-D20 matrix reused by the remote surface.
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Api { status, .. } => match status {
                401 | 403 => 5, // EXIT_PERMISSION
                404 => 3,       // EXIT_SESSION_MISSING family (target absent)
                _ => 1,
            },
            CliError::Transport(_) | CliError::Local(_) => 1,
        }
    }

    /// One-line stderr rendering: `gtmux <ctx>: <code>: <message>`.
    pub fn print(&self, ctx: &str) {
        match self {
            CliError::Api {
                status,
                code,
                message,
            } => eprintln!("gtmux {ctx}: {code} (HTTP {status}): {message}"),
            CliError::Transport(msg) => eprintln!("gtmux {ctx}: transport_error: {msg}"),
            CliError::Local(msg) => eprintln!("gtmux {ctx}: {msg}"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Server discovery
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the target instance name (ADR-0053 D9). `explicit` carries the
/// `--instance` flag value, already env-backed by clap
/// (`GTMUX_SERVER_INSTANCE`) so the flag wins over the env, which wins over
/// auto-detection of the single running instance.
pub fn resolve_instance(explicit: Option<String>) -> Result<String, CliError> {
    if let Some(name) = explicit {
        return Ok(name);
    }
    let state_dir = crate::status_state_dir()
        .ok_or_else(|| CliError::local("cannot resolve XDG_STATE_HOME (and $HOME is unset)"))?;
    let all = crate::enumerate_instances(&state_dir, None)
        .map_err(|e| CliError::Local(format!("enumerating instances: {e}")))?;
    let alive: Vec<String> = all
        .into_iter()
        .filter(|name| matches!(check_pidfile_liveness(name), Ok(PidLiveness::Alive(_))))
        .collect();
    match alive.len() {
        0 => Err(CliError::local(
            "no running gtmux instance found — start one with `gtmux start --name <name>` \
             or pass --instance",
        )),
        1 => Ok(alive.into_iter().next().expect("len checked")),
        _ => Err(CliError::Local(format!(
            "multiple running instances ({}) — set GTMUX_SERVER_INSTANCE or pass --instance",
            alive.join(", ")
        ))),
    }
}

/// Cheap line-walk parse of `bind = "…"` / `port = NNNN` out of a config
/// TOML (same tolerance as `infer_open_url` — figment env overrides don't
/// apply here, which matches the offline-tolerant status/rotate paths).
pub fn parse_bind_port(raw: &str) -> (Option<String>, Option<u16>) {
    let mut bind: Option<String> = None;
    let mut port: Option<u16> = None;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("bind") {
            let rest = rest.trim_start_matches([' ', '=']).trim();
            let rest = rest.trim_matches('"');
            if !rest.is_empty() {
                bind = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("port") {
            let rest = rest.trim_start_matches([' ', '=']).trim();
            if let Ok(n) = rest.parse::<u16>() {
                port = Some(n);
            }
        }
    }
    (bind, port)
}

/// `http://<host>:<port>` for an instance, from its per-instance config
/// file. Unspecified binds (`0.0.0.0` / `::`) map to loopback; IPv6 hosts
/// are bracketed.
pub fn server_base_url(instance: &str) -> Result<String, CliError> {
    let dir = crate::config_dir_for_humanise()
        .ok_or_else(|| CliError::local("cannot resolve XDG_CONFIG_HOME (and $HOME is unset)"))?;
    let path = dir.join(format!("{instance}.config.toml"));
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        CliError::Local(format!(
            "cannot determine the server address for instance '{instance}': reading {}: {e} \
             (the remote commands need the per-instance config file for the bound port)",
            path.display()
        ))
    })?;
    let (bind, port) = parse_bind_port(&raw);
    let Some(port) = port else {
        return Err(CliError::Local(format!(
            "config {} has no `port` — cannot determine the server address",
            path.display()
        )));
    };
    let host = match bind.as_deref() {
        Some("0.0.0.0") | Some("::") | None => "127.0.0.1".to_string(),
        Some(v6) if v6.contains(':') => format!("[{v6}]"),
        Some(other) => other.to_string(),
    };
    Ok(format!("http://{host}:{port}"))
}

/// Discovery + token load in one step — every remote command starts here.
pub fn connect(explicit_instance: Option<String>) -> Result<Client, CliError> {
    let instance = resolve_instance(explicit_instance)?;
    let base = server_base_url(&instance)?;
    let token = gtmux_auth::load_token(&instance).map_err(|e| {
        CliError::Local(format!(
            "loading token for instance '{instance}': {e} \
             (has `gtmux start --name {instance}` run on this host?)"
        ))
    })?;
    Ok(Client::new(base, token.0))
}

// ─────────────────────────────────────────────────────────────────────────────
// Client
// ─────────────────────────────────────────────────────────────────────────────

/// Thin bearer-authenticated JSON/multipart client over `ureq`.
pub struct Client {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(base: String, token: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        Self { base, token, agent }
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        self.agent
            .request(method, &format!("{}{}", self.base, path))
            .set("Authorization", &format!("Bearer {}", self.token))
    }

    /// GET → parsed JSON body.
    pub fn get_json(&self, path: &str) -> Result<Value, CliError> {
        finish(self.request("GET", path).call())
    }

    /// JSON-body request (POST/PUT/PATCH). `headers` piggybacks per-call
    /// extras (e.g. `X-Gtmux-Password` for the D6 session gate).
    pub fn send_json(
        &self,
        method: &str,
        path: &str,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> Result<Value, CliError> {
        let mut req = self.request(method, path).set("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.set(k, v);
        }
        finish(req.send_string(&body.to_string()))
    }

    /// Body-less request (POST without payload / DELETE).
    pub fn send_empty(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Result<Value, CliError> {
        let mut req = self.request(method, path);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        finish(req.call())
    }

    /// `multipart/form-data` POST (fs upload — ADR-0053 D14).
    pub fn post_multipart(&self, path: &str, body: MultipartBody) -> Result<Value, CliError> {
        let (content_type, bytes) = body.finish();
        finish(
            self.request("POST", path)
                .set("Content-Type", &content_type)
                .send_bytes(&bytes),
        )
    }
}

/// Convert a ureq outcome into parsed JSON, mapping non-2xx to
/// [`CliError::Api`] with the server's machine-readable code.
fn finish(result: Result<ureq::Response, ureq::Error>) -> Result<Value, CliError> {
    match result {
        Ok(resp) => read_json_body(resp),
        Err(ureq::Error::Status(status, resp)) => {
            let body = read_json_body(resp).unwrap_or(Value::Null);
            let code = body
                .get("error")
                .or_else(|| body.get("code"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("http_{status}"));
            let mut message = body
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| match &body {
                    Value::Null => String::new(),
                    other => other.to_string(),
                });
            if let Some(idx) = body.get("failed_index").and_then(Value::as_u64) {
                message = format!("{message} (failed op index {idx})");
            }
            Err(CliError::Api {
                status,
                code,
                message,
            })
        }
        Err(ureq::Error::Transport(t)) => Err(CliError::Transport(t.to_string())),
    }
}

/// Read a response body (capped) and parse it as JSON. Empty bodies
/// (204 / HEAD-ish responses) yield `Value::Null`.
fn read_json_body(resp: ureq::Response) -> Result<Value, CliError> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(RESPONSE_MAX_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| CliError::Transport(format!("reading response body: {e}")))?;
    if buf.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&buf).map_err(|e| {
        CliError::Local(format!(
            "server returned non-JSON body ({e}): {}",
            String::from_utf8_lossy(&buf[..buf.len().min(200)])
        ))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Multipart encoder (manual — keeps the dependency surface at ureq alone)
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal `multipart/form-data` body builder for `POST /api/fs/upload`
/// (fields: `dir`, optional `on_conflict`, one `file` part — the server
/// sniffs MIME from the bytes so no per-part Content-Type is required for
/// text fields; the file part carries a generic octet-stream).
pub struct MultipartBody {
    boundary: String,
    buf: Vec<u8>,
}

impl MultipartBody {
    pub fn new() -> Self {
        // Boundary uniqueness: pid + monotonic-ish nanos. Collision with
        // payload bytes is theoretically possible but the payload would have
        // to contain the exact dashed boundary line — accepted for a local
        // single-user tool (same tradeoff every browser makes).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Self {
            boundary: format!("gtmux-cli-{}-{}", std::process::id(), nanos),
            buf: Vec::new(),
        }
    }

    pub fn text(&mut self, name: &str, value: &str) {
        self.buf
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.buf.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    pub fn file(&mut self, name: &str, filename: &str, bytes: &[u8]) {
        // Header-safe filename: quotes / CR / LF cannot be smuggled into the
        // part header. The server re-sanitizes anyway (`sanitize_filename`).
        let safe: String = filename
            .chars()
            .map(|c| if c == '"' || c == '\r' || c == '\n' { '_' } else { c })
            .collect();
        self.buf
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.buf.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{safe}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        self.buf.extend_from_slice(bytes);
        self.buf.extend_from_slice(b"\r\n");
    }

    /// Close the body — returns `(content_type, bytes)`.
    pub fn finish(mut self) -> (String, Vec<u8>) {
        self.buf
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (
            format!("multipart/form-data; boundary={}", self.boundary),
            self.buf,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bind_port_reads_both() {
        let raw = "[server]\nsession = \"x\"\nbind = \"127.0.0.1\"\nport = 9001\n";
        assert_eq!(
            parse_bind_port(raw),
            (Some("127.0.0.1".to_string()), Some(9001))
        );
    }

    #[test]
    fn parse_bind_port_tolerates_missing_fields() {
        assert_eq!(parse_bind_port("[server]\n"), (None, None));
    }

    #[test]
    fn exit_codes_map_auth_and_not_found() {
        let auth = CliError::Api {
            status: 401,
            code: "credential_required".into(),
            message: String::new(),
        };
        assert_eq!(auth.exit_code(), 5);
        let forbidden = CliError::Api {
            status: 403,
            code: "password_required".into(),
            message: String::new(),
        };
        assert_eq!(forbidden.exit_code(), 5);
        let missing = CliError::Api {
            status: 404,
            code: "session_not_found".into(),
            message: String::new(),
        };
        assert_eq!(missing.exit_code(), 3);
        let bad = CliError::Api {
            status: 400,
            code: "bad_request".into(),
            message: String::new(),
        };
        assert_eq!(bad.exit_code(), 1);
        assert_eq!(CliError::local("x").exit_code(), 1);
    }

    #[test]
    fn multipart_encoding_shape() {
        let mut m = MultipartBody::new();
        m.text("dir", "/tmp/ws");
        m.text("on_conflict", "rename");
        m.file("file", "a\"b.txt", b"hello");
        let (ct, bytes) = m.finish();
        let body = String::from_utf8(bytes).unwrap();
        let boundary = ct
            .strip_prefix("multipart/form-data; boundary=")
            .expect("content type shape");
        assert!(body.starts_with(&format!("--{boundary}\r\n")));
        assert!(body.contains("Content-Disposition: form-data; name=\"dir\"\r\n\r\n/tmp/ws\r\n"));
        assert!(body.contains("name=\"on_conflict\"\r\n\r\nrename\r\n"));
        // Quote in the filename is neutralized.
        assert!(body.contains("name=\"file\"; filename=\"a_b.txt\""));
        assert!(body.contains("Content-Type: application/octet-stream\r\n\r\nhello\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }
}
