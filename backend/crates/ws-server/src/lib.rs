//! gtmux-ws-server — axum `/ws` upgrade + envelope codec + Hub broadcaster.
//!
//! Post-ADR-0013 (PTY direct, no tmux). The framing layer (envelope codec,
//! varint, payload encoders, subprotocol auth) is unchanged from the
//! pre-Stage-B era — only the *semantics* of the inner payloads have
//! shifted: tmux argv strings are gone, the closed
//! [`gtmux_pty_backend::BackendCommand`] enum is the new compile-time
//! allowlist, and per-pane output bytes flow through the multiplexed
//! [`Hub`] channel that wraps [`gtmux_pty_backend::PtyBackend`].
//!
//! Surface:
//! - [`router`] mounts `GET /ws` with Origin/Host gating and
//!   subprotocol-based bearer auth (ADR-0002 D5 + ADR-0003 D5).
//! - [`Envelope`] is the wire object — see `docs/ssot/wire-protocol.md`
//!   §1.2 for byte layout. Framing is byte-identical to the pre-Stage-B
//!   wire so a frontend that only updates its CTRL `cmd` vocabulary will
//!   keep working (parallel S7-WS-PAYLOAD-SIMPLIFY frontend task tracks
//!   the cmd-string change).
//! - [`Hub`] is the fan-out point — wraps a [`gtmux_pty_backend::PtyBackend`]
//!   and exposes a multiplexed `(PaneId, Bytes)` broadcast for pane output,
//!   a [`gtmux_pty_backend::BackendNotify`] broadcast for lifecycle
//!   events, and a 16-byte ETag broadcast for layout changes.

// `deny(unsafe_code)` (not `forbid`) so the single `libc::raise(SIGTERM)`
// in the kill-session handler can locally `allow` — ADR-0013 D10 amend
// (2026-05-15). All other modules in this crate stay unsafe-free.
#![deny(unsafe_code)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use gtmux_auth::TokenString;
use gtmux_config::Config;
use gtmux_pty_backend::{BackendNotify, PaneId};
use thiserror::Error;
use tokio::time::Instant;
use tracing::{debug, info, warn};

pub mod cmd_router;
mod hub;
mod payload;
mod ring;
mod varint;

pub use cmd_router::{dispatch_ctrl, is_allowed_ctrl_cmd, CtrlOutcome, ALLOWLISTED_CTRL_CMDS};
pub use hub::{
    CookieValidator, Hub, ManipulationEvent, TerminalDiedEvent, TerminalListChangeEvent,
    TerminalSpawnedEvent, HUB_BROADCAST_CAPACITY,
};
pub use ring::{RingBuffer, RING_BUFFER_CAPACITY};

// ─────────────────────────────────────────────────────────────────────────────
//  Constants — calibrated against `docs/ssot/wire-protocol.md` §1.2 and
//  `docs/reports/0010-grill-amendments.md` D15.
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum payload bytes per envelope. The SSoT pins a 1 MiB soft cap for the
/// whole WS message (§1.2), but the codec lives in front of any framing
/// reassembly: a 4 MiB hard ceiling here gives us a defensive 4× headroom so
/// a single attacker-controlled length prefix cannot OOM the decoder.
pub const MAX_PAYLOAD: usize = 4 * 1024 * 1024;

/// Envelope header: 1-byte type + 4-byte little-endian length.
const HEADER_LEN: usize = 5;

/// Heartbeat ping cadence (ADR-0021 D6 / ADR-0019 D6.2). The server pings
/// every 15s; clients are expected to reply with Pong frames before the
/// next ping. Cross-server session-attach leases are refreshed in lock-step
/// with this cadence so a dead webpage releases its session within
/// `PONG_TIMEOUT` after going silent.
const PING_INTERVAL: Duration = Duration::from_secs(15);

/// Pong-grace timeout (ADR-0021 D6). If no pong arrives within this window
/// after the *most recent* ping, the connection is considered dead and the
/// socket is closed with code 1011 (Internal). 30s is the explicit value
/// in ADR-0019 D6.2 / ADR-0021 D6.
const PONG_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-connection pause/resume debounce window (legacy `ADR-0001 D8` +
/// `0010-grill-amendments.md` D16). Identical bytes-on-the-wire to the
/// pre-Stage-B era; the *backend implementation* is now "drop / re-keep
/// the broadcast::Receiver" per ADR-0013 D10 amend (Panel Streaming
/// State → Suspended at the WS layer, not the backend).
const PAUSE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Monotonic connection-id counter (Stage 5-C echo-minus-sender). Minted
/// once per WS handshake so the manipulation broadcast can identify the
/// sender connection and skip its own echo. Need not be unguessable —
/// only unique within a single server boot.
static CONNECTION_ID_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn mint_connection_id() -> Arc<str> {
    let n = CONNECTION_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Arc::from(format!("conn-{n}"))
}

/// WS close codes used by this crate.
mod close_codes {
    pub const NORMAL: u16 = 1000;
    pub const UNSUPPORTED_DATA: u16 = 1003;
    pub const POLICY_VIOLATION: u16 = 1008;
    pub const INTERNAL: u16 = 1011;
}

// ─────────────────────────────────────────────────────────────────────────────
//  Frame type IDs — `docs/ssot/wire-protocol.md` §2.
// ─────────────────────────────────────────────────────────────────────────────

/// Envelope frame type. 1 byte on the wire.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Ctrl = 0x01,
    PaneOutput = 0x02,
    PaneInput = 0x03,
    PaneResize = 0x04,
    PanePause = 0x05,
    PaneResume = 0x06,
    NotifyMirror = 0x07,
    LayoutChanged = 0x80,
    ManipulationSelection = 0x81,
    InputTarget = 0x82,
    ViewportChanged = 0x83,
    FocusMode = 0x84,
    /// `0x85 TERMINAL_DIED` (Stage 5-B, ADR-0021 D5). UUID-carrying death
    /// notification published by the http-api `handle_pane_died` consumer
    /// for every backend `PaneDied` event. Server-wide broadcast — a
    /// single terminal can be mirrored into multiple sessions (ADR-0021
    /// D1), so all attached webpages must see the death. Inner payload:
    /// `varint 0 + UTF-8 JSON {"terminal_id":"<uuid>","reason":"exit"|"killed"}`.
    TerminalDied = 0x85,
    /// `0x87 TERMINAL_LIST_UPDATE` (Stage 5-D path P1, ADR-0021 D3). Pool
    /// delta hint emitted on *attach_confirm* spawn — every webpage
    /// **outside** the triggering session sees `{ added: [...], removed: [] }`
    /// so its Terminal-list sidebar refreshes ahead of the 5-s polling
    /// loop. WS dispatcher filters per-connection via
    /// [`Hub::session_for_cookie`]: the trigger session itself does not
    /// receive this frame (its layout already reflects the spawn). Inner
    /// payload: `varint 0 + UTF-8 JSON {"added":["<uuid>",…],"removed":[…]}`.
    ///
    /// 0x86 MOUNT_CASCADE (the trigger-session companion frame in
    /// path P2 — `[New Terminal]` button) is *reserved by FE decoder*
    /// (`decode.ts:517`) but not yet emitted by BE; see
    /// `docs/reports/0035-be-fe-coordination-stage-5.md` §3.2.
    TerminalListUpdate = 0x87,
    /// `0x88 TERMINAL_SPAWNED` (FE Issue C unblock — 0036(FE) §4). Carries
    /// the UUID ↔ PaneId binding that `AppState::spawn_terminal_with_uuid`
    /// just minted, so the FE can build its `terminalId → paneId` map
    /// without an extra `GET /api/terminals` roundtrip. Server-wide
    /// broadcast — every attached webpage potentially mirrors the new
    /// terminal in its sidebar / layout (ADR-0021 D1). Inner payload:
    /// `varint 0 + UTF-8 JSON {"terminal_id":"<uuid>","pane_id":<u64>}`.
    TerminalSpawned = 0x88,
}

impl FrameType {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::Ctrl,
            0x02 => Self::PaneOutput,
            0x03 => Self::PaneInput,
            0x04 => Self::PaneResize,
            0x05 => Self::PanePause,
            0x06 => Self::PaneResume,
            0x07 => Self::NotifyMirror,
            0x80 => Self::LayoutChanged,
            0x81 => Self::ManipulationSelection,
            0x82 => Self::InputTarget,
            0x83 => Self::ViewportChanged,
            0x84 => Self::FocusMode,
            0x85 => Self::TerminalDied,
            0x87 => Self::TerminalListUpdate,
            0x88 => Self::TerminalSpawned,
            _ => return None,
        })
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// `true` if the slot is web-domain (0x80–0x84).
    pub fn is_web_domain(self) -> bool {
        (self.as_u8() & 0x80) != 0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Codec
// ─────────────────────────────────────────────────────────────────────────────

/// A single wire envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub kind: FrameType,
    pub payload: Bytes,
}

impl Envelope {
    pub fn new(kind: FrameType, payload: Bytes) -> Self {
        Self { kind, payload }
    }

    /// Encode to wire bytes. Layout: `[type(1)][len(4 LE)][payload(len)]`.
    pub fn encode(&self) -> Result<Bytes, CodecError> {
        let len = self.payload.len();
        if len > MAX_PAYLOAD {
            return Err(CodecError::PayloadTooLarge(len as u32));
        }
        let mut buf = BytesMut::with_capacity(HEADER_LEN + len);
        buf.put_u8(self.kind.as_u8());
        buf.put_u32_le(len as u32);
        buf.extend_from_slice(&self.payload);
        Ok(buf.freeze())
    }

    /// Decode one envelope from `input`.
    pub fn decode(input: &[u8]) -> Result<(Self, usize), CodecError> {
        if input.len() < HEADER_LEN {
            return Err(CodecError::Truncated);
        }
        let type_byte = input[0];
        let kind = FrameType::from_u8(type_byte).ok_or(CodecError::UnknownType(type_byte))?;
        let len_bytes: [u8; 4] = input[1..5].try_into().map_err(|_| CodecError::Truncated)?;
        let len = u32::from_le_bytes(len_bytes);
        if (len as usize) > MAX_PAYLOAD {
            return Err(CodecError::PayloadTooLarge(len));
        }
        let total = HEADER_LEN
            .checked_add(len as usize)
            .ok_or(CodecError::PayloadTooLarge(len))?;
        if input.len() < total {
            return Err(CodecError::Truncated);
        }
        let payload = Bytes::copy_from_slice(&input[HEADER_LEN..total]);
        Ok((Self { kind, payload }, total))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("truncated frame")]
    Truncated,
    #[error("unknown frame type 0x{0:02x}")]
    UnknownType(u8),
    #[error("payload too large: {0} bytes > {MAX_PAYLOAD} max")]
    PayloadTooLarge(u32),
}

// ─────────────────────────────────────────────────────────────────────────────
//  BackendNotify → Envelope mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Translate a single backend notification into one `0x07 NOTIFY_MIRROR`
/// envelope. Returns `None` only for variants that have no SSoT-defined
/// JSON kind yet (currently every variant maps; the `None` arm exists for
/// forward-compat when a new BackendNotify variant lands ahead of the
/// frontend handler).
pub fn notify_to_envelope(n: &BackendNotify) -> Option<Envelope> {
    let (pane_id, body) = match n {
        BackendNotify::PaneSpawned { id, request_id } => {
            let body = match request_id {
                Some(rid) => format!(
                    r#"{{"kind":"pane-spawned","request_id":"{}"}}"#,
                    json_escape(rid),
                ),
                None => r#"{"kind":"pane-spawned"}"#.to_string(),
            };
            (*id, body)
        }
        BackendNotify::PaneDied { id, code, signal } => {
            let mut parts = String::from(r#"{"kind":"pane-died""#);
            if let Some(c) = code {
                parts.push_str(&format!(r#","code":{c}"#));
            }
            if let Some(s) = signal {
                parts.push_str(&format!(r#","signal":{s}"#));
            }
            parts.push('}');
            (*id, parts)
        }
        BackendNotify::LayoutChanged => (PaneId(0), r#"{"kind":"layout-changed"}"#.to_string()),
        BackendNotify::ServerReady => (PaneId(0), r#"{"kind":"server-ready"}"#.to_string()),
    };
    let pane_id_u32 = u32::try_from(pane_id.0).unwrap_or(0);
    Some(Envelope::new(
        FrameType::NotifyMirror,
        Bytes::from(payload::encode_notify_mirror(pane_id_u32, &body)),
    ))
}

/// Minimal JSON-string escaper.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Subprotocol parser — RFC 6455 §11.3.4 comma-separated tokens.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSubprotocol {
    pub gtmux_v1: bool,
    pub bearer_token: Option<String>,
}

pub fn parse_subprotocol(header_value: &str) -> Option<ParsedSubprotocol> {
    let mut gtmux_v1 = false;
    let mut bearer_token: Option<String> = None;
    let mut any = false;
    for raw in header_value.split(',') {
        let tok = raw.trim();
        if tok.is_empty() {
            continue;
        }
        any = true;
        if tok == "gtmux.v1" {
            gtmux_v1 = true;
        } else if let Some(t) = tok.strip_prefix("bearer.") {
            if !t.is_empty() && bearer_token.is_none() {
                bearer_token = Some(t.to_string());
            }
        }
    }
    if !any {
        return None;
    }
    Some(ParsedSubprotocol {
        gtmux_v1,
        bearer_token,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Router + handshake
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct WsState {
    token: Arc<TokenString>,
    echo_protocol: HeaderValue,
    hub: Hub,
}

/// Build the WS sub-router.
pub fn router(_config: &Config, token: &TokenString, hub: Hub) -> Router {
    let echo_protocol = HeaderValue::from_static("gtmux.v1");
    let state = WsState {
        token: Arc::new(token.clone()),
        echo_protocol,
        hub,
    };
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(
    State(state): State<WsState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> Response {
    let Some(raw) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
    else {
        return (StatusCode::UPGRADE_REQUIRED, "subprotocol header required").into_response();
    };

    let Some(parsed) = parse_subprotocol(raw) else {
        return (StatusCode::UPGRADE_REQUIRED, "subprotocol header empty").into_response();
    };
    if !parsed.gtmux_v1 {
        return (StatusCode::UPGRADE_REQUIRED, "gtmux.v1 required").into_response();
    }

    // Capture the `gtmux_auth` cookie value before the auth decision so
    // both D10 α (cookie auth) and the existing disconnect-routing path
    // can read the same source. Pre-D10 the cookie was *only* a
    // disconnect-notification key; with D10 α it can also satisfy the
    // upgrade authentication when the http-api layer registered a
    // [`hub::CookieValidator`] on the hub.
    let cookie_value = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|pair| {
                let pair = pair.trim();
                pair.strip_prefix("gtmux_auth=")
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_string())
            })
        });

    // D10 α additive auth: accept the upgrade when **either** the cookie
    // validates **or** the subprotocol bearer matches the canonical
    // token. The legacy bearer path remains the source of truth for
    // automation / CLI scripts; the cookie path covers the browser
    // session flow (ADR-0020 D10). When neither path produces an
    // accept, reply with the most informative 401 message — bearer
    // mismatch is the high-signal case for an attacker probe; missing
    // bearer is the most common path for an unsigned client.
    let cookie_ok = if let Some(c) = cookie_value.as_deref() {
        if let Some(validator) = state.hub.cookie_validator() {
            validator.validate(c).await
        } else {
            false
        }
    } else {
        false
    };

    let bearer_token = parsed.bearer_token.as_deref();
    let bearer_ok = bearer_token
        .map(|t| gtmux_auth::verify_token(t, state.token.as_ref()))
        .unwrap_or(false);

    if !cookie_ok && !bearer_ok {
        if bearer_token.is_some() {
            warn!("ws upgrade rejected: bearer + cookie both invalid");
            return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
        }
        if cookie_value.is_some() {
            warn!("ws upgrade rejected: cookie invalid, no bearer");
            return (StatusCode::UNAUTHORIZED, "invalid cookie").into_response();
        }
        return (StatusCode::UNAUTHORIZED, "bearer token or cookie required").into_response();
    }

    let echo = state.echo_protocol.clone();
    let hub = state.hub.clone();
    // Stage 5-C: mint a per-connection identifier so the manipulation
    // broadcast can implement echo-minus-sender. Monotonic counter is
    // sufficient — uniqueness is required only within a single server
    // boot, and the value is never exposed back to the client.
    let connection_id = mint_connection_id();
    let mut response = ws
        .protocols(["gtmux.v1"])
        .on_upgrade(move |socket| async move {
            handle_socket(socket, hub.clone(), cookie_value.clone(), connection_id.clone()).await;
            if let Some(cookie) = cookie_value {
                if let Some(sink) = hub.disconnect_sink() {
                    // Errors here only mean "no consumer registered" —
                    // the http-api layer hasn't wired its receiver yet
                    // (boot ordering) or has already shut down. Either
                    // way the session lock will be released by the
                    // server-shutdown path; warn for visibility only.
                    if sink.send(cookie).is_err() {
                        tracing::debug!(
                            "ws disconnect sink closed; auto-release skipped"
                        );
                    }
                }
            }
        });
    response
        .headers_mut()
        .insert("sec-websocket-protocol", echo);
    response
}

/// Per-connection loop. Performs catch-up replay on attach (every alive
/// pane's ring buffer is flushed as a 0x02 PANE_OUT envelope, followed
/// by the matching `pane-spawned` NOTIFY so the frontend knows the id
/// is live), then enters the live fan-out: backend notifications +
/// multiplexed pane outputs + layout broadcasts.
async fn handle_socket(
    socket: WebSocket,
    hub: Hub,
    cookie_value: Option<String>,
    connection_id: Arc<str>,
) {
    let (mut sink, mut stream) = socket.split();
    let backend = hub.backend().clone();

    // Snapshot the heartbeat sink once at upgrade time so a sink registered
    // mid-connection only takes effect for subsequent sockets (matches the
    // disconnect-sink behaviour and avoids a half-state where a new sink
    // misses earlier liveness signals from this conn).
    let heartbeat_sink = hub.heartbeat_sink();

    // Subscribe BEFORE catch-up replay so events that arrive while we
    // are flushing snapshots are not lost.
    let mut notify_rx = hub.subscribe_notify();
    let mut layout_rx = hub.subscribe_layout();
    let mut output_rx = hub.subscribe_pane_output();
    let mut terminal_died_rx = hub.subscribe_terminal_died();
    let mut terminal_list_change_rx = hub.subscribe_terminal_list_change();
    let mut terminal_spawned_rx = hub.subscribe_terminal_spawned();
    let mut manipulation_rx = hub.subscribe_manipulation();

    // Send the initial LAYOUT_CHANGED hello so the client knows the
    // server is alive and can decide whether to re-fetch `/api/layout`.
    let hello_etag = [0u8; 16];
    let hello = Envelope::new(
        FrameType::LayoutChanged,
        Bytes::from(payload::encode_layout_changed(&hello_etag)),
    );
    if let Ok(buf) = hello.encode() {
        if sink
            .send(Message::Binary(buf.to_vec().into()))
            .await
            .is_err()
        {
            debug!("ws hello send failed; peer hung up early");
            return;
        }
    }

    // Catch-up: for each alive pane, send a `pane-spawned` NOTIFY (so the
    // frontend's panel store learns the id) followed by the ring-buffer
    // bytes as one or more 0x02 PANE_OUT envelopes.
    for id in backend.pane_ids() {
        let spawned = Envelope::new(
            FrameType::NotifyMirror,
            Bytes::from(payload::encode_notify_mirror(
                u32::try_from(id.0).unwrap_or(0),
                r#"{"kind":"pane-spawned"}"#,
            )),
        );
        if let Ok(buf) = spawned.encode() {
            if sink
                .send(Message::Binary(buf.to_vec().into()))
                .await
                .is_err()
            {
                debug!("ws catch-up spawned send failed; peer hung up during replay");
                return;
            }
        }
        if let Some((replay, _rx)) = backend.subscribe_output(id) {
            if !replay.is_empty() {
                let env = Envelope::new(
                    FrameType::PaneOutput,
                    Bytes::from(payload::encode_pane_out(
                        u32::try_from(id.0).unwrap_or(0),
                        &replay,
                    )),
                );
                if let Ok(buf) = env.encode() {
                    if sink
                        .send(Message::Binary(buf.to_vec().into()))
                        .await
                        .is_err()
                    {
                        debug!("ws catch-up pane-out send failed; peer hung up during replay");
                        return;
                    }
                }
            }
        }
    }

    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ping_timer.tick().await;
    let mut last_pong = Instant::now();

    // Per-pane Streaming State (ADR-0013 D10 amend): set of suspended
    // pane ids. PANE_OUT envelopes for these pane ids are dropped at
    // the WS sink — the backend still emits bytes (they accumulate in
    // the pane's ring buffer) but the WS subscriber stops forwarding
    // them until a PANE_RESUME envelope arrives. This is the
    // post-Stage-B equivalent of the legacy `refresh-client -A pause`.
    let mut suspended: std::collections::HashSet<PaneId> = std::collections::HashSet::new();
    let mut last_pause_event: HashMap<PaneId, Instant> = HashMap::new();

    loop {
        tokio::select! {
            biased;
            maybe_msg = stream.next() => {
                let Some(msg) = maybe_msg else { break };
                match msg {
                    Ok(Message::Binary(buf)) => {
                        let close_now = handle_client_envelope(
                            buf.as_ref(),
                            &backend,
                            &mut suspended,
                            &mut last_pause_event,
                            &mut sink,
                            &hub,
                            cookie_value.as_deref(),
                            &connection_id,
                        ).await;
                        if close_now {
                            return;
                        }
                    }
                    Ok(Message::Text(_)) => {
                        let _ = sink.send(close_frame(
                            close_codes::UNSUPPORTED_DATA,
                            "text frames not supported",
                        )).await;
                        return;
                    }
                    Ok(Message::Pong(_)) => {
                        last_pong = Instant::now();
                        emit_heartbeat(&heartbeat_sink, cookie_value.as_deref());
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = sink.send(Message::Pong(p)).await;
                        emit_heartbeat(&heartbeat_sink, cookie_value.as_deref());
                    }
                    Ok(Message::Close(_)) => {
                        let _ = sink.send(close_frame(
                            close_codes::NORMAL,
                            "peer closed",
                        )).await;
                        return;
                    }
                    Err(e) => {
                        debug!("ws stream error: {e}");
                        return;
                    }
                }
            }
            n = notify_rx.recv() => {
                match n {
                    Ok(notify) => {
                        if let Some(env) = notify_to_envelope(&notify) {
                            if let Ok(buf) = env.encode() {
                                if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "ws notify subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("backend notify closed; ending connection");
                        let _ = sink.send(close_frame(
                            close_codes::INTERNAL,
                            "backend gone",
                        )).await;
                        return;
                    }
                }
            }
            output = output_rx.recv() => {
                match output {
                    Ok((id, bytes)) => {
                        if suspended.contains(&id) {
                            continue;
                        }
                        let env = Envelope::new(
                            FrameType::PaneOutput,
                            Bytes::from(payload::encode_pane_out(
                                u32::try_from(id.0).unwrap_or(0),
                                &bytes,
                            )),
                        );
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "ws pane-output subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("hub pane_output closed");
                    }
                }
            }
            layout = layout_rx.recv() => {
                match layout {
                    Ok(etag) => {
                        let env = Envelope::new(
                            FrameType::LayoutChanged,
                            Bytes::from(payload::encode_layout_changed(&etag)),
                        );
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "ws layout subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Hub dropped — other arms will hit Closed too.
                    }
                }
            }
            died = terminal_died_rx.recv() => {
                match died {
                    Ok(event) => {
                        let env = Envelope::new(
                            FrameType::TerminalDied,
                            Bytes::from(payload::encode_terminal_died(&event.uuid, event.reason)),
                        );
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // Lagged here means a terminal-died event was dropped
                        // for this subscriber. The FE will eventually see the
                        // stale UUID disappear from the next `GET /api/terminals`
                        // poll, so warn but do not tear down the connection.
                        warn!(skipped = n, "ws terminal-died subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Hub dropped — other arms will hit Closed too.
                    }
                }
            }
            manip = manipulation_rx.recv() => {
                match manip {
                    Ok(event) => {
                        // Stage 5-C: echo-minus-sender + session-scoped.
                        //   * own echo → skip
                        //   * no cookie or unattached cookie → skip
                        //   * different session → skip
                        if event.sender_conn_id.as_ref() == connection_id.as_ref() {
                            continue;
                        }
                        let Some(cookie) = cookie_value.as_deref() else {
                            continue;
                        };
                        let Some(my_session) = hub.session_for_cookie(cookie) else {
                            continue;
                        };
                        if my_session.as_str() != event.session_id.as_ref() {
                            continue;
                        }
                        let Some(kind) = FrameType::from_u8(event.frame_type) else {
                            // Defensive — publisher would have to have set
                            // an invalid byte for this to fire. Log and
                            // skip rather than crash the loop.
                            warn!(
                                frame_type = event.frame_type,
                                "ws manipulation event with unknown frame type"
                            );
                            continue;
                        };
                        let env = Envelope::new(kind, event.payload.clone());
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // Manipulation events are *informational* (selection
                        // / viewport / focus state). The next event refreshes
                        // the snapshot — warn but keep the connection alive.
                        warn!(skipped = n, "ws manipulation subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Hub dropped — other arms will hit Closed too.
                    }
                }
            }
            spawned = terminal_spawned_rx.recv() => {
                match spawned {
                    Ok(event) => {
                        // Server-wide broadcast — every attached webpage
                        // potentially mirrors the new terminal. No cookie
                        // filter; the FE decides per-instance whether to
                        // wire an `XtermHost` subscriber against the new
                        // PaneId.
                        let env = Envelope::new(
                            FrameType::TerminalSpawned,
                            Bytes::from(payload::encode_terminal_spawned(
                                &event.terminal_id,
                                event.pane_id,
                            )),
                        );
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // FE will catch up via the next /api/terminals poll,
                        // same fallback as the terminal-list-change arm.
                        warn!(skipped = n, "ws terminal-spawned subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Hub dropped — other arms will hit Closed too.
                    }
                }
            }
            list_change = terminal_list_change_rx.recv() => {
                match list_change {
                    Ok(event) => {
                        // Stage 5-D P1 routing: skip when this connection has
                        // no cookie at all, when the cookie has not yet
                        // attached to any session, or when the cookie is
                        // attached to the trigger session itself (its layout
                        // already reflects the spawn — emitting here would
                        // be redundant noise).
                        let Some(cookie) = cookie_value.as_deref() else {
                            continue;
                        };
                        let Some(my_session) = hub.session_for_cookie(cookie) else {
                            continue;
                        };
                        if my_session.as_str() == event.trigger_session.as_ref() {
                            continue;
                        }
                        // FE's `decodeTerminalListUpdate` validates `added`/
                        // `removed` as string-of-string arrays; rebuild the
                        // Vec<String> view of the Arc<[Arc<str>]> payload to
                        // match `payload::encode_terminal_list_update`.
                        let added_vec: Vec<String> = event
                            .added
                            .iter()
                            .map(|s| s.as_ref().to_string())
                            .collect();
                        let removed_vec: Vec<String> = event
                            .removed
                            .iter()
                            .map(|s| s.as_ref().to_string())
                            .collect();
                        let env = Envelope::new(
                            FrameType::TerminalListUpdate,
                            Bytes::from(payload::encode_terminal_list_update(
                                &added_vec,
                                &removed_vec,
                            )),
                        );
                        if let Ok(buf) = env.encode() {
                            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // Lagged here means a terminal-list-change delta was
                        // dropped. FE will catch up via its 5-s polling loop;
                        // warn but keep the connection alive.
                        warn!(skipped = n, "ws terminal-list-change subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Hub dropped — other arms will hit Closed too.
                    }
                }
            }
            _ = ping_timer.tick() => {
                if last_pong.elapsed() > PONG_TIMEOUT {
                    info!("ws timeout: no pong for {:?}", last_pong.elapsed());
                    let _ = sink.send(close_frame(
                        close_codes::INTERNAL,
                        "heartbeat timeout",
                    )).await;
                    return;
                }
                if sink.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Handle one client-origin binary frame. Returns `true` when the caller
/// must close the connection (a policy violation already sent its close
/// frame on `sink`).
///
/// The argument count grew with Stage 5-C (hub / cookie / connection_id
/// needed for manipulation broadcast) — keeping them as parameters
/// rather than a context struct preserves the per-frame call ergonomics
/// in the `select!` loop. Clippy's 7-arg threshold is informational.
#[allow(clippy::too_many_arguments)]
async fn handle_client_envelope(
    buf: &[u8],
    backend: &gtmux_pty_backend::PtyBackend,
    suspended: &mut std::collections::HashSet<PaneId>,
    last_pause_event: &mut HashMap<PaneId, Instant>,
    sink: &mut (impl SinkExt<Message, Error = axum::Error> + Unpin),
    hub: &Hub,
    cookie_value: Option<&str>,
    connection_id: &Arc<str>,
) -> bool {
    let (env, _) = match Envelope::decode(buf) {
        Ok(t) => t,
        Err(e) => {
            debug!("ws decode error: {e}");
            let _ = sink
                .send(close_frame(
                    close_codes::UNSUPPORTED_DATA,
                    "malformed envelope",
                ))
                .await;
            return true;
        }
    };

    match env.kind {
        FrameType::PaneInput => {
            let Some(p) = payload::decode_pane_in(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_IN",
                    ))
                    .await;
                return true;
            };
            if !p.bytes.is_empty() {
                let _ = backend.send_input(PaneId(u64::from(p.pane_id)), p.bytes.to_vec());
            }
            false
        }
        FrameType::PaneResize => {
            let Some(r) = payload::decode_pane_resize(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_RESIZE",
                    ))
                    .await;
                return true;
            };
            let rows = u16::try_from(r.rows).unwrap_or(u16::MAX);
            let cols = u16::try_from(r.cols).unwrap_or(u16::MAX);
            let _ = backend.resize(PaneId(u64::from(r.pane_id)), rows, cols);
            false
        }
        FrameType::PanePause => {
            let Some(pane_id) = payload::decode_pane_bare_id(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_PAUSE",
                    ))
                    .await;
                return true;
            };
            let id = PaneId(u64::from(pane_id));
            if !debounce_pause(last_pause_event, id) {
                suspended.insert(id);
            }
            false
        }
        FrameType::PaneResume => {
            let Some(pane_id) = payload::decode_pane_bare_id(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_RESUME",
                    ))
                    .await;
                return true;
            };
            let id = PaneId(u64::from(pane_id));
            if !debounce_pause(last_pause_event, id) {
                suspended.remove(&id);
            }
            false
        }
        FrameType::Ctrl => {
            let Some(ctrl) = payload::decode_ctrl_request(&env.payload) else {
                debug!("ws CTRL: invalid JSON");
                let body = payload::encode_ctrl_error(None, "ERR_BAD_REQUEST", "invalid CTRL JSON");
                let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                if let Ok(buf) = err.encode() {
                    let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                }
                return false;
            };
            debug!(
                cmd = %ctrl.cmd,
                id = ?ctrl.id,
                argc = ctrl.args.len(),
                "ws CTRL: decoded request"
            );
            if !is_allowed_ctrl_cmd(&ctrl.cmd) {
                debug!(cmd = %ctrl.cmd, "ws CTRL: rejected (not in allowlist)");
                let body = payload::encode_ctrl_error(
                    ctrl.id.as_deref(),
                    "ERR_NOT_ALLOWED",
                    "command not in allowlist",
                );
                let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                if let Ok(buf) = err.encode() {
                    let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                }
                return false;
            }
            match dispatch_ctrl(backend, ctrl.id.clone(), &ctrl.cmd, &ctrl.args) {
                CtrlOutcome::Ok => {
                    // Ack every CTRL success so the frontend's `sendCtrl`
                    // Promise resolves (otherwise it times out after 5 s).
                    // For `new-pane` the spawned PaneId is *also* surfaced
                    // via the `pane-spawned` NOTIFY broadcast — the ack
                    // body deliberately stays minimal (`{ok:true,id}`) so
                    // the frontend's two-path race (response vs first-sight
                    // mux) keeps its existing semantics. (Bugfix 2026-05-15:
                    // kill-pane was timing out at 5 s because no ack was
                    // sent on the success path.)
                    let body = payload::encode_ctrl_success(ctrl.id.as_deref());
                    let ack = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                    if let Ok(buf) = ack.encode() {
                        let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                    }
                }
                CtrlOutcome::OkAndExit => {
                    // Acknowledge the kill-session request, then raise SIGTERM
                    // on ourselves so axum::serve's graceful_shutdown future
                    // fires and main() drops the PtyBackend (ADR-0014 D7).
                    let body = payload::encode_ctrl_success(ctrl.id.as_deref());
                    let ack = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                    if let Ok(buf) = ack.encode() {
                        let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                    }
                    // SAFETY: libc::raise with a constant signal number is
                    // sound. Process self-signal is the canonical way to
                    // trigger graceful shutdown matching external SIGTERM.
                    #[allow(unsafe_code)]
                    unsafe {
                        libc::raise(libc::SIGTERM);
                    }
                }
                CtrlOutcome::NotAllowed => {
                    let body = payload::encode_ctrl_error(
                        ctrl.id.as_deref(),
                        "ERR_NOT_ALLOWED",
                        "command not in allowlist",
                    );
                    let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                    if let Ok(buf) = err.encode() {
                        let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                    }
                }
                CtrlOutcome::BadRequest => {
                    let body = payload::encode_ctrl_error(
                        ctrl.id.as_deref(),
                        "ERR_BAD_REQUEST",
                        "malformed argv for cmd",
                    );
                    let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                    if let Ok(buf) = err.encode() {
                        let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                    }
                }
                CtrlOutcome::BackendError(e) => {
                    let msg = format!("{e}");
                    let body = payload::encode_ctrl_error(ctrl.id.as_deref(), "ERR_BACKEND", &msg);
                    let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                    if let Ok(buf) = err.encode() {
                        let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                    }
                }
            }
            false
        }
        FrameType::LayoutChanged => {
            let _ = sink
                .send(close_frame(
                    close_codes::POLICY_VIOLATION,
                    "0x80 LAYOUT_CHANGED is server-only",
                ))
                .await;
            true
        }
        FrameType::ManipulationSelection
        | FrameType::InputTarget
        | FrameType::ViewportChanged
        | FrameType::FocusMode => {
            // Stage 5-C: server-side echo-minus-sender. The originating
            // connection's session_for_cookie binding determines the
            // routing scope; we enrich the inbound payload with a trailing
            // varint-len + UTF-8 `session_id` so subscribers can surface
            // the value to the FE without an extra HTTP roundtrip. Drop
            // silently when the connection has no cookie or the cookie
            // has not yet attached — those clients are not addressable
            // for session-scoped delivery.
            let Some(cookie) = cookie_value else {
                debug!(
                    kind = ?env.kind,
                    "ws manipulation dropped: no cookie on connection"
                );
                return false;
            };
            let Some(session_name) = hub.session_for_cookie(cookie) else {
                debug!(
                    kind = ?env.kind,
                    "ws manipulation dropped: cookie has no session attach"
                );
                return false;
            };
            let mut enriched = env.payload.to_vec();
            let session_bytes = session_name.as_bytes();
            let mut len_buf: Vec<u8> = Vec::with_capacity(2);
            varint::encode_into(session_bytes.len() as u64, &mut len_buf);
            enriched.reserve(len_buf.len() + session_bytes.len());
            enriched.extend_from_slice(&len_buf);
            enriched.extend_from_slice(session_bytes);
            hub.publish_manipulation(ManipulationEvent {
                sender_conn_id: connection_id.clone(),
                session_id: Arc::from(session_name.as_str()),
                frame_type: env.kind.as_u8(),
                payload: Bytes::from(enriched),
            });
            false
        }
        FrameType::PaneOutput
        | FrameType::NotifyMirror
        | FrameType::TerminalDied
        | FrameType::TerminalListUpdate
        | FrameType::TerminalSpawned => {
            let _ = sink
                .send(close_frame(
                    close_codes::POLICY_VIOLATION,
                    "frame is server-only",
                ))
                .await;
            true
        }
    }
}

/// Return `true` if this pause/resume event should be debounced (i.e. a
/// previous one fired within [`PAUSE_DEBOUNCE`] for this pane).
fn debounce_pause(state: &mut HashMap<PaneId, Instant>, pane_id: PaneId) -> bool {
    let now = Instant::now();
    match state.get_mut(&pane_id) {
        Some(prev) if now.duration_since(*prev) < PAUSE_DEBOUNCE => true,
        Some(prev) => {
            *prev = now;
            false
        }
        None => {
            state.insert(pane_id, now);
            false
        }
    }
}

/// Forward a heartbeat signal to the registered sink, if any. Send errors
/// (sink closed) are silently absorbed — they signify "the consumer task
/// exited", which can happen during graceful shutdown and isn't a fault.
fn emit_heartbeat(
    sink: &Option<tokio::sync::mpsc::UnboundedSender<String>>,
    cookie: Option<&str>,
) {
    let Some(cookie) = cookie else { return };
    let Some(tx) = sink else { return };
    let _ = tx.send(cookie.to_string());
}

fn close_frame(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert via panic; relaxing the crate-wide deny only inside this module"
)]
mod tests {
    use super::*;
    use gtmux_pty_backend::PtyBackend;

    // ── Subprotocol parser ────────────────────────────────────────────────

    #[test]
    fn parse_subprotocol_both() {
        let p = parse_subprotocol("gtmux.v1, bearer.xyz").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_reverse_order() {
        let p = parse_subprotocol("bearer.xyz, gtmux.v1").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_whitespace_tolerant() {
        let a = parse_subprotocol("gtmux.v1,bearer.xyz").unwrap();
        let b = parse_subprotocol(" gtmux.v1 , bearer.xyz ").unwrap();
        let c = parse_subprotocol("\tgtmux.v1\t,\tbearer.xyz\t").unwrap();
        for p in [a, b, c] {
            assert!(p.gtmux_v1);
            assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
        }
    }

    #[test]
    fn parse_subprotocol_missing_v1() {
        let p = parse_subprotocol("bearer.xyz").unwrap();
        assert!(!p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_missing_bearer() {
        let p = parse_subprotocol("gtmux.v1").unwrap();
        assert!(p.gtmux_v1);
        assert!(p.bearer_token.is_none());
    }

    #[test]
    fn parse_subprotocol_malformed() {
        assert!(parse_subprotocol("").is_none());
        assert!(parse_subprotocol(",").is_none());
        assert!(parse_subprotocol("  ,  ").is_none());
        let p = parse_subprotocol("bearer.").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
        let p = parse_subprotocol("Gtmux.V1, BEARER.xyz").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
    }

    #[test]
    fn parse_subprotocol_ignores_unknown_tokens() {
        let p = parse_subprotocol("gtmux.v1, x-future-extension, bearer.t").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("t"));
    }

    // ── Codec ─────────────────────────────────────────────────────────────

    #[test]
    fn envelope_encode_decode_roundtrip() {
        let inner = payload::encode_pane_out(37, &[0x41u8; 100]);
        let env = Envelope::new(FrameType::PaneOutput, Bytes::from(inner.clone()));
        let buf = env.encode().unwrap();
        assert_eq!(buf.len(), HEADER_LEN + inner.len());
        let (decoded, consumed) = Envelope::decode(&buf).unwrap();
        assert_eq!(consumed, HEADER_LEN + inner.len());
        assert_eq!(decoded.kind, FrameType::PaneOutput);
        assert_eq!(decoded.payload.as_ref(), inner.as_slice());
    }

    #[test]
    fn envelope_decode_truncated_returns_err() {
        assert_eq!(Envelope::decode(&[0x02]), Err(CodecError::Truncated));
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&10u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
        buf.extend_from_slice(b"abc");
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
    }

    #[test]
    fn envelope_decode_unknown_type() {
        let mut buf = vec![0x42u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x42)));
        let mut buf = vec![0x08u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x08)));
        // 0x86 (MOUNT_CASCADE) is *reserved* by the FE decoder
        // (`decode.ts`) for Stage 5-D path P2 but not yet emitted by BE,
        // so the wire codec still rejects it here. The first *fully*
        // unassigned slot is 0x89.
        let mut buf = vec![0x89u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x89)));
    }

    #[test]
    fn envelope_decode_oversized() {
        let oversize = (MAX_PAYLOAD as u32) + 1;
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&oversize.to_le_bytes());
        assert_eq!(
            Envelope::decode(&buf),
            Err(CodecError::PayloadTooLarge(oversize))
        );
    }

    #[test]
    fn envelope_payload_endianness_le() {
        let len_value = 0x000400FFu32;
        let payload = Bytes::from(vec![0u8; len_value as usize]);
        let env = Envelope::new(FrameType::PaneOutput, payload);
        let buf = env.encode().unwrap();
        assert_eq!(&buf[1..5], &len_value.to_le_bytes()[..]);
        assert_ne!(&buf[1..5], &len_value.to_be_bytes()[..]);
    }

    #[test]
    fn envelope_encode_rejects_oversize() {
        let at_cap = Bytes::from(vec![0u8; MAX_PAYLOAD]);
        let env = Envelope::new(FrameType::PaneOutput, at_cap);
        assert!(env.encode().is_ok());
        let over_cap = Bytes::from(vec![0u8; MAX_PAYLOAD + 1]);
        let env = Envelope::new(FrameType::PaneOutput, over_cap);
        assert!(matches!(env.encode(), Err(CodecError::PayloadTooLarge(_))));
    }

    #[test]
    fn frame_type_from_u8_covers_all() {
        for &b in &[
            0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x87,
            0x88,
        ] {
            let ft = FrameType::from_u8(b).expect("known frame");
            assert_eq!(ft.as_u8(), b);
        }
        // 0x86 stays unknown to BE for now — reserved by FE for Stage 5-D
        // path P2 (`MOUNT_CASCADE`) which is still gated on FE/BE
        // coordination per `docs/reports/0035-be-fe-coordination-stage-5.md`.
        for &b in &[0x00u8, 0x08, 0x0F, 0x10, 0x7F, 0x86, 0x89, 0x8F, 0xFE, 0xFF] {
            assert!(FrameType::from_u8(b).is_none(), "byte 0x{b:02x} leaked");
        }
    }

    #[test]
    fn frame_type_web_domain_flag() {
        assert!(!FrameType::PaneOutput.is_web_domain());
        assert!(FrameType::LayoutChanged.is_web_domain());
        assert!(FrameType::ManipulationSelection.is_web_domain());
        // 0x85 TerminalDied keeps the high-bit web-domain marker — the WS
        // dispatcher does not special-case it, but a future router that
        // splits "real-time pane I/O" from "schema metadata" will use the
        // marker.
        assert!(FrameType::TerminalDied.is_web_domain());
        // 0x87 TerminalListUpdate is also web-domain; same marker rule.
        assert!(FrameType::TerminalListUpdate.is_web_domain());
    }

    // ── BackendNotify → Envelope mapping ─────────────────────────────────

    #[test]
    fn notify_pane_spawned_with_request_id() {
        let n = BackendNotify::PaneSpawned {
            id: PaneId(7),
            request_id: Some("r1".into()),
        };
        let env = notify_to_envelope(&n).unwrap();
        assert_eq!(env.kind, FrameType::NotifyMirror);
        // varint(7) = 0x07, then JSON body
        assert_eq!(env.payload[0], 0x07);
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "pane-spawned");
        assert_eq!(json["request_id"], "r1");
    }

    #[test]
    fn notify_pane_spawned_without_request_id() {
        let n = BackendNotify::PaneSpawned {
            id: PaneId(7),
            request_id: None,
        };
        let env = notify_to_envelope(&n).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "pane-spawned");
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn notify_pane_died_carries_exit_code() {
        let n = BackendNotify::PaneDied {
            id: PaneId(5),
            code: Some(0),
            signal: None,
        };
        let env = notify_to_envelope(&n).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "pane-died");
        assert_eq!(json["code"], 0);
        assert!(json.get("signal").is_none());
    }

    #[test]
    fn notify_pane_died_carries_signal() {
        let n = BackendNotify::PaneDied {
            id: PaneId(5),
            code: None,
            signal: Some(15),
        };
        let env = notify_to_envelope(&n).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "pane-died");
        assert_eq!(json["signal"], 15);
    }

    #[test]
    fn notify_layout_changed_uses_pane_id_zero() {
        let env = notify_to_envelope(&BackendNotify::LayoutChanged).unwrap();
        assert_eq!(env.payload[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "layout-changed");
    }

    #[test]
    fn notify_server_ready_uses_pane_id_zero() {
        let env = notify_to_envelope(&BackendNotify::ServerReady).unwrap();
        assert_eq!(env.payload[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "server-ready");
    }

    // ── In-process WS handshake ──────────────────────────────────────────

    use std::net::{Ipv4Addr, SocketAddr};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::{
        client::IntoClientRequest, http::header::SEC_WEBSOCKET_PROTOCOL, protocol::Message as TM,
    };

    static CONFIG_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn test_config() -> Config {
        let toml = r#"schema_version = 1
[server]
session = "tests"
port = 9001
bind = "127.0.0.1"
"#;
        let n = CONFIG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gtmux-ws-test-{}-{}.toml", std::process::id(), n));
        std::fs::write(&path, toml).unwrap();
        let cfg = gtmux_config::load(Some(&path), "tests").unwrap();
        let _ = std::fs::remove_file(&path);
        cfg
    }

    async fn spawn_test_server(token: TokenString) -> (SocketAddr, Hub) {
        let cfg = test_config();
        let backend = PtyBackend::new();
        let hub = Hub::new(backend);
        let app = router(&cfg, &token, hub.clone());
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, hub)
    }

    async fn connect_authed(
        addr: SocketAddr,
        token: &TokenString,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, _resp) = tokio_tungstenite::connect_async(req).await.unwrap();
        ws
    }

    /// Connect to `/ws` with bearer auth + an arbitrary `gtmux_auth` cookie.
    /// Useful for the 0x87 routing matrix (Stage 5-D P1) where each WS in
    /// the test must carry a distinct cookie that the hub session_table
    /// has been pre-bound to a session name.
    async fn connect_authed_with_cookie(
        addr: SocketAddr,
        token: &TokenString,
        cookie: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        req.headers_mut().insert(
            http::header::COOKIE,
            format!("gtmux_auth={cookie}").parse().unwrap(),
        );
        let (ws, _resp) = tokio_tungstenite::connect_async(req).await.unwrap();
        ws
    }

    /// Drain the initial LAYOUT_CHANGED hello + any catch-up PaneSpawned
    /// NOTIFY / PaneOutput envelopes that the server emits before its
    /// `select!` loop starts processing fresh broadcasts. Returns once
    /// `idle_after` elapses without any further frame. The routing tests
    /// below need the socket to be in a quiescent state before publishing
    /// the event-under-test so the receive path is unambiguous.
    async fn drain_initial(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        idle_after: std::time::Duration,
    ) {
        loop {
            match tokio::time::timeout(idle_after, ws.next()).await {
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => break, // idle window elapsed — assume quiescent
            }
        }
    }

    /// Read frames until a `0x87 TERMINAL_LIST_UPDATE` arrives or the timeout
    /// elapses. Non-0x87 frames are silently dropped so the test focuses on
    /// the envelope under inspection. Returns the decoded JSON body.
    async fn expect_terminal_list_update(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        timeout: std::time::Duration,
    ) -> Option<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(TM::Binary(buf)))) => {
                    let Ok((env, _)) = Envelope::decode(buf.as_ref()) else {
                        continue;
                    };
                    if env.kind != FrameType::TerminalListUpdate {
                        continue;
                    }
                    // Inner = varint 0 + UTF-8 JSON. Strip the leading varint
                    // (single byte 0x00 because pane_id = 0 for web-domain
                    // frames) and parse the body.
                    if env.payload.is_empty() || env.payload[0] != 0x00 {
                        continue;
                    }
                    let json: serde_json::Value =
                        serde_json::from_slice(&env.payload[1..]).ok()?;
                    return Some(json);
                }
                Ok(Some(Ok(_))) => continue, // other frame types — ignore
                Ok(Some(Err(_))) | Ok(None) => return None,
                Err(_) => return None,
            }
        }
    }

    // ── Stage 5-D P1: 0x87 TERMINAL_LIST_UPDATE per-connection routing ──

    #[tokio::test]
    async fn terminal_list_update_delivered_to_other_session_only() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;

        // Two cookies, bound to different sessions before the WS upgrade so
        // the dispatcher arm has a stable lookup when the publish lands.
        hub.set_session_for_cookie("cookie-A", "alpha");
        hub.set_session_for_cookie("cookie-B", "beta");

        let mut ws_a = connect_authed_with_cookie(addr, &token, "cookie-A").await;
        let mut ws_b = connect_authed_with_cookie(addr, &token, "cookie-B").await;
        drain_initial(&mut ws_a, std::time::Duration::from_millis(50)).await;
        drain_initial(&mut ws_b, std::time::Duration::from_millis(50)).await;

        // Publish from session α's POV — A is trigger (skipped), B is other.
        hub.publish_terminal_list_change(
            "alpha",
            &["11111111-2222-4333-8444-555555555555".to_string()],
            &[],
        );

        // B must receive the frame; payload must carry the spawned UUID.
        let body = expect_terminal_list_update(&mut ws_b, std::time::Duration::from_millis(500))
            .await
            .expect("session β should receive the fan-out");
        assert_eq!(
            body["added"],
            serde_json::json!(["11111111-2222-4333-8444-555555555555"])
        );
        assert_eq!(body["removed"], serde_json::json!([]));

        // A must NOT receive a 0x87 — drain for a window and require silence
        // on that frame type. Other frames (heartbeat / hello) are ignored.
        let echo = expect_terminal_list_update(&mut ws_a, std::time::Duration::from_millis(150))
            .await;
        assert!(
            echo.is_none(),
            "trigger session must not receive its own 0x87 echo: {echo:?}"
        );
    }

    #[tokio::test]
    async fn terminal_list_update_skipped_when_cookie_absent() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;

        // Bind only cookie-B; ws_no_cookie connects without any cookie at all.
        hub.set_session_for_cookie("cookie-B", "beta");

        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (mut ws_no_cookie, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        drain_initial(&mut ws_no_cookie, std::time::Duration::from_millis(50)).await;

        hub.publish_terminal_list_change("alpha", &["uuid".to_string()], &[]);

        let echo = expect_terminal_list_update(
            &mut ws_no_cookie,
            std::time::Duration::from_millis(200),
        )
        .await;
        assert!(echo.is_none(), "WS without cookie must not receive 0x87");
    }

    #[tokio::test]
    async fn terminal_list_update_skipped_for_unattached_cookie() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;

        // cookie-orphan is intentionally NOT bound to any session — its WS
        // attached BEFORE attach (a normal race window) and must not see
        // any 0x87 until /attach binds it.
        let mut ws_orphan = connect_authed_with_cookie(addr, &token, "cookie-orphan").await;
        drain_initial(&mut ws_orphan, std::time::Duration::from_millis(50)).await;

        hub.publish_terminal_list_change("alpha", &["uuid".to_string()], &[]);

        let echo =
            expect_terminal_list_update(&mut ws_orphan, std::time::Duration::from_millis(200))
                .await;
        assert!(
            echo.is_none(),
            "cookie with no session_for_cookie binding must not receive 0x87"
        );
    }

    #[tokio::test]
    async fn client_origin_terminal_list_update_closes_1008() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        drain_initial(&mut ws, std::time::Duration::from_millis(50)).await;

        // A client trying to spoof a 0x87 must be cut with a policy-violation
        // close — the frame is strictly server-origin (server-only) and the
        // dispatcher returns true from `handle_client_envelope` for it.
        let bad = Envelope::new(
            FrameType::TerminalListUpdate,
            Bytes::from(payload::encode_terminal_list_update(&[], &[])),
        )
        .encode()
        .unwrap();
        ws.send(TM::Binary(bad.to_vec().into())).await.unwrap();

        let mut got_policy_close = false;
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline - tokio::time::Instant::now();
            match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(TM::Close(Some(cf))))) => {
                    assert_eq!(u16::from(cf.code), close_codes::POLICY_VIOLATION);
                    got_policy_close = true;
                    break;
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
            }
        }
        assert!(
            got_policy_close,
            "client-origin 0x87 must be policy-violation closed"
        );
    }

    /// Read frames until a 0x81~0x84 envelope arrives or timeout. Returns
    /// (kind, payload). Drains intervening frames (catch-up / heartbeats).
    async fn expect_manipulation_frame(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        timeout: std::time::Duration,
    ) -> Option<(FrameType, Bytes)> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(TM::Binary(buf)))) => {
                    let Ok((env, _)) = Envelope::decode(buf.as_ref()) else {
                        continue;
                    };
                    if matches!(
                        env.kind,
                        FrameType::ManipulationSelection
                            | FrameType::InputTarget
                            | FrameType::ViewportChanged
                            | FrameType::FocusMode
                    ) {
                        return Some((env.kind, env.payload));
                    }
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) | Ok(None) => return None,
                Err(_) => return None,
            }
        }
    }

    /// Parse the trailing varint-len + UTF-8 session_id at the end of a
    /// Stage 5-C enriched payload. Returns `None` on malformed/missing
    /// trailer — useful so tests can assert the absence path too.
    fn read_trailing_session_id(payload: &[u8]) -> Option<String> {
        if payload.is_empty() {
            return None;
        }
        // The inner layout for 0x81~0x84 is shape-dependent (varint+...).
        // For test purposes we know the leading varint is `0` (single
        // byte). The session_id trailer is appended *after* the original
        // payload, so we scan from the right: the last byte sequence is
        // UTF-8 bytes preceded by the varint length. We try varint
        // decoding from a few candidate offsets from the right and pick
        // the one that exactly consumes the tail.
        for start in (1..payload.len()).rev() {
            let candidate = &payload[start..];
            // Try decoding a varint at `candidate`
            let mut len_bytes = 0usize;
            let mut value: u64 = 0;
            for (i, &b) in candidate.iter().enumerate() {
                value |= (u64::from(b & 0x7F)) << (7 * i);
                len_bytes = i + 1;
                if b & 0x80 == 0 {
                    break;
                }
                if i >= 9 {
                    len_bytes = 0;
                    break;
                }
            }
            if len_bytes == 0 || candidate.len() < len_bytes {
                continue;
            }
            let trailer_total = len_bytes + value as usize;
            if trailer_total != candidate.len() {
                continue;
            }
            let s_bytes = &candidate[len_bytes..];
            if let Ok(s) = std::str::from_utf8(s_bytes) {
                return Some(s.to_string());
            }
        }
        None
    }

    // ── Stage 5-C: echo-minus-sender + session-scoped manipulation routing ──

    #[tokio::test]
    async fn manipulation_skipped_when_sender_has_no_session_attach() {
        // Inbound 0x83 from a connection whose cookie has no session
        // binding must be silently dropped — no broadcast publish.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;

        let mut ws = connect_authed_with_cookie(addr, &token, "no-session-cookie").await;
        drain_initial(&mut ws, std::time::Duration::from_millis(50)).await;

        let mut probe = hub.subscribe_manipulation();
        // Client sends a viewport-changed (0x83) but its cookie has no
        // hub.session_for_cookie binding — dispatcher drops without publish.
        let frame = Envelope::new(
            FrameType::ViewportChanged,
            Bytes::from(payload::encode_viewport_marker_only()),
        )
        .encode()
        .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();

        let racy = tokio::time::timeout(std::time::Duration::from_millis(100), probe.recv()).await;
        assert!(
            racy.is_err(),
            "must not publish manipulation event for unattached cookie: {racy:?}"
        );
    }

    #[tokio::test]
    async fn manipulation_fans_out_to_same_session_minus_sender() {
        // A & B both attach to "alpha". A sends 0x83 — B receives it
        // (with session_id trailer), A does NOT receive its own echo.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;
        hub.set_session_for_cookie("cookie-A", "alpha");
        hub.set_session_for_cookie("cookie-B", "alpha");

        let mut ws_a = connect_authed_with_cookie(addr, &token, "cookie-A").await;
        let mut ws_b = connect_authed_with_cookie(addr, &token, "cookie-B").await;
        drain_initial(&mut ws_a, std::time::Duration::from_millis(50)).await;
        drain_initial(&mut ws_b, std::time::Duration::from_millis(50)).await;

        // A → server: 0x83 with a recognizable body
        let body = payload::encode_viewport_marker_only();
        let frame = Envelope::new(FrameType::ViewportChanged, Bytes::from(body.clone()))
            .encode()
            .unwrap();
        ws_a.send(TM::Binary(frame.to_vec().into())).await.unwrap();

        // B must receive the enriched envelope.
        let (kind, payload_b) =
            expect_manipulation_frame(&mut ws_b, std::time::Duration::from_millis(500))
                .await
                .expect("peer-B must receive 0x83");
        assert_eq!(kind, FrameType::ViewportChanged);
        // First N bytes = original payload; trailing bytes = varint-len +
        // UTF-8 "alpha".
        assert!(payload_b.starts_with(&body));
        let session = read_trailing_session_id(&payload_b)
            .expect("trailing session_id must parse");
        assert_eq!(session, "alpha");

        // A must NOT receive its own echo.
        let echo =
            expect_manipulation_frame(&mut ws_a, std::time::Duration::from_millis(150)).await;
        assert!(
            echo.is_none(),
            "sender must not receive its own manipulation echo: {echo:?}"
        );
    }

    #[tokio::test]
    async fn manipulation_does_not_cross_sessions() {
        // A→alpha, C→beta. A sends 0x83. C must NOT receive it.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;
        hub.set_session_for_cookie("cookie-A", "alpha");
        hub.set_session_for_cookie("cookie-C", "beta");

        let mut ws_a = connect_authed_with_cookie(addr, &token, "cookie-A").await;
        let mut ws_c = connect_authed_with_cookie(addr, &token, "cookie-C").await;
        drain_initial(&mut ws_a, std::time::Duration::from_millis(50)).await;
        drain_initial(&mut ws_c, std::time::Duration::from_millis(50)).await;

        let body = payload::encode_viewport_marker_only();
        let frame = Envelope::new(FrameType::ViewportChanged, Bytes::from(body))
            .encode()
            .unwrap();
        ws_a.send(TM::Binary(frame.to_vec().into())).await.unwrap();

        let leak = expect_manipulation_frame(&mut ws_c, std::time::Duration::from_millis(200))
            .await;
        assert!(
            leak.is_none(),
            "session-β connection must not receive session-α manipulation: {leak:?}"
        );
    }

    #[tokio::test]
    async fn manipulation_session_id_trailer_format_matches_spec() {
        // Wire spec: payload = <original inner> + varint(len) + UTF-8 bytes.
        // This test pins the trailer format so a future codec refactor
        // can't silently drift away from `read_trailing_session_id`.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;
        hub.set_session_for_cookie("cookie-A", "session-with-dashes");
        hub.set_session_for_cookie("cookie-B", "session-with-dashes");

        let mut ws_a = connect_authed_with_cookie(addr, &token, "cookie-A").await;
        let mut ws_b = connect_authed_with_cookie(addr, &token, "cookie-B").await;
        drain_initial(&mut ws_a, std::time::Duration::from_millis(50)).await;
        drain_initial(&mut ws_b, std::time::Duration::from_millis(50)).await;

        let body = payload::encode_viewport_marker_only();
        let frame = Envelope::new(FrameType::ViewportChanged, Bytes::from(body.clone()))
            .encode()
            .unwrap();
        ws_a.send(TM::Binary(frame.to_vec().into())).await.unwrap();

        let (_, payload_b) =
            expect_manipulation_frame(&mut ws_b, std::time::Duration::from_millis(500))
                .await
                .expect("recv");
        // Original body is intact at the start.
        assert_eq!(&payload_b[..body.len()], &body);
        // Trailer = varint("session-with-dashes".len()) + UTF-8 bytes.
        // For 19 bytes the varint is a single byte 0x13.
        assert_eq!(payload_b[body.len()], 0x13);
        assert_eq!(&payload_b[body.len() + 1..], b"session-with-dashes");
    }

    #[tokio::test]
    async fn terminal_list_update_carries_removed_field() {
        // P1 only emits added; this guards the wire shape for the future
        // path that emits {removed: [...]} (kill via http-api would publish
        // a similar event). The decoder must surface both fields.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;
        hub.set_session_for_cookie("cookie-B", "beta");

        let mut ws_b = connect_authed_with_cookie(addr, &token, "cookie-B").await;
        drain_initial(&mut ws_b, std::time::Duration::from_millis(50)).await;

        hub.publish_terminal_list_change(
            "alpha",
            &["u-added".to_string()],
            &["u-removed".to_string()],
        );
        let body = expect_terminal_list_update(&mut ws_b, std::time::Duration::from_millis(500))
            .await
            .expect("receive");
        assert_eq!(body["added"], serde_json::json!(["u-added"]));
        assert_eq!(body["removed"], serde_json::json!(["u-removed"]));
    }

    #[tokio::test]
    async fn ws_upgrade_requires_protocol_header() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let req = url.into_client_request().unwrap();
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "handshake without subprotocol must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_wrong_token() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
            "gtmux.v1, bearer.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .parse()
                .unwrap(),
        );
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "handshake with wrong token must fail");
    }

    // ── Stage 5 D10 α: cookie additive auth path ────────────────────────────

    #[async_trait::async_trait]
    impl CookieValidator for std::collections::HashSet<String> {
        async fn validate(&self, cookie_value: &str) -> bool {
            self.contains(cookie_value)
        }
    }

    fn validator_with(allowed: &[&str]) -> std::sync::Arc<dyn CookieValidator> {
        let mut set = std::collections::HashSet::new();
        for c in allowed {
            set.insert((*c).to_string());
        }
        std::sync::Arc::new(set)
    }

    #[tokio::test]
    async fn ws_upgrade_accepts_cookie_without_bearer_when_validator_set() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token).await;
        hub.set_cookie_validator(validator_with(&["good-cookie"]));

        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        // Subprotocol carries only the marker, no bearer token.
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, "gtmux.v1".parse().unwrap());
        req.headers_mut()
            .insert(http::header::COOKIE, "gtmux_auth=good-cookie".parse().unwrap());
        let (ws, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .expect("cookie-only upgrade should succeed");
        drop(ws);
    }

    #[tokio::test]
    async fn ws_upgrade_rejects_cookie_when_validator_returns_false() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token).await;
        hub.set_cookie_validator(validator_with(&["good-cookie"]));

        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, "gtmux.v1".parse().unwrap());
        req.headers_mut()
            .insert(http::header::COOKIE, "gtmux_auth=stale-cookie".parse().unwrap());
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "invalid cookie + no bearer must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_accepts_bearer_when_no_validator_registered() {
        // Pre-D10 / test-rig parity: a hub without a cookie validator must
        // still accept the legacy bearer-only flow. Verifies additivity.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .expect("legacy bearer upgrade must keep working");
        drop(ws);
    }

    #[tokio::test]
    async fn ws_upgrade_accepts_bearer_when_cookie_invalid_with_validator() {
        // Truly additive: a bad cookie does **not** poison a valid bearer.
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub) = spawn_test_server(token.clone()).await;
        hub.set_cookie_validator(validator_with(&["good-cookie"]));

        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        req.headers_mut()
            .insert(http::header::COOKIE, "gtmux_auth=stale-cookie".parse().unwrap());
        let (ws, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .expect("invalid cookie + valid bearer must still succeed");
        drop(ws);
    }

    #[tokio::test]
    async fn ws_upgrade_rejects_when_neither_path_provided() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        // Marker only — no bearer, no cookie.
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, "gtmux.v1".parse().unwrap());
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "marker-only handshake must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_success_echoes_only_gtmux_v1() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, response) = tokio_tungstenite::connect_async(req)
            .await
            .expect("upgrade should succeed with valid token");
        let echoed = response
            .headers()
            .get(SEC_WEBSOCKET_PROTOCOL)
            .expect("protocol echoed")
            .to_str()
            .unwrap();
        assert_eq!(echoed, "gtmux.v1");
        drop(ws);
    }

    #[tokio::test]
    async fn client_origin_layout_changed_closes_1008() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        let _hello = ws.next().await;
        let bad = Envelope::new(
            FrameType::LayoutChanged,
            Bytes::from(payload::encode_layout_changed(&[0u8; 16])),
        )
        .encode()
        .unwrap();
        ws.send(TM::Binary(bad.to_vec().into())).await.unwrap();
        let mut got_policy_close = false;
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(TM::Close(Some(cf))) => {
                    assert_eq!(u16::from(cf.code), close_codes::POLICY_VIOLATION);
                    got_policy_close = true;
                    break;
                }
                Ok(TM::Close(None)) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        assert!(got_policy_close, "expected explicit 1008 close frame");
    }

    #[tokio::test]
    async fn disallowed_ctrl_cmd_rejected() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        let _hello = expect_binary(&mut ws).await;
        // tmux-era commands are rejected with ERR_NOT_ALLOWED.
        let body = br#"{"id":"r1","cmd":"new-window","args":[]}"#;
        let mut inner = vec![0u8];
        inner.extend_from_slice(body);
        let frame = Envelope::new(FrameType::Ctrl, Bytes::from(inner))
            .encode()
            .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();
        let resp = expect_binary(&mut ws).await;
        let (env, _) = Envelope::decode(&resp).unwrap();
        assert_eq!(env.kind, FrameType::Ctrl);
        assert_eq!(env.payload[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["code"], "ERR_NOT_ALLOWED");
    }

    #[tokio::test]
    async fn kill_unknown_pane_surfaces_backend_error() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        let _hello = expect_binary(&mut ws).await;
        // kill a pane that does not exist → ERR_BACKEND
        let body = br#"{"id":"r2","cmd":"kill-pane","args":["999"]}"#;
        let mut inner = vec![0u8];
        inner.extend_from_slice(body);
        let frame = Envelope::new(FrameType::Ctrl, Bytes::from(inner))
            .encode()
            .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();
        let resp = expect_binary(&mut ws).await;
        let (env, _) = Envelope::decode(&resp).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["code"], "ERR_BACKEND");
    }

    async fn expect_binary(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Vec<u8> {
        loop {
            match ws.next().await {
                Some(Ok(TM::Binary(b))) => return b.to_vec(),
                Some(Ok(TM::Ping(p))) => {
                    let _ = ws.send(TM::Pong(p)).await;
                }
                Some(Ok(TM::Pong(_))) => continue,
                Some(Ok(other)) => panic!("expected Binary, got {other:?}"),
                Some(Err(e)) => panic!("ws err: {e}"),
                None => panic!("ws closed unexpectedly"),
            }
        }
    }
}
