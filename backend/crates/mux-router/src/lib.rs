//! gtmux-mux-router — tmux control mode parser + command argv router.
//!
//! Owns the tmux-side state domain (sessions, windows, panes, output streams).
//! Exposes `Command` enum that mirrors the ADR-0008 allowlist exactly — no
//! variant exists for forbidden commands (`split-window`, `resize-pane`,
//! `select-layout`, `-CC`), so the type system itself enforces invariant #4.
//!
//! ## Parser scope (P0-MUX-1)
//!
//! Line-oriented decoder for the `%`-prefixed notifications emitted by
//! `tmux -C` per `docs/reports/0001-tmux-control-mode.md` §3·§4 and
//! `docs/adr/0001-tmux-integration-control-mode.md` D7. The parser is a
//! pure function over a single line (LF already stripped by the framer);
//! IPC stream attach is owned by Sprint 1 P0-LIFE-1.
//!
//! See [`parse_line`] for the line-level entry point and
//! [`decode_output_payload`] for the `\NNN` octal-escape decoder used by
//! `%output` / `%extended-output` payloads.

#![forbid(unsafe_code)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use thiserror::Error;

/// tmux command allowlist as a closed enum.
///
/// 출처: `docs/adr/0008-single-pane-window-and-group.md` §"tmux command allowlist 표".
/// 새 명령 추가는 본 enum + ADR-0008 표 동시 갱신 필수.
#[derive(Debug, Clone)]
pub enum Command {
    /// `tmux new-window -t <session>` — Pane 생성 (single-pane-per-window 컨벤션).
    NewWindow,
    /// `tmux kill-pane -t %<pid>` — Panel close / Group close 재귀.
    KillPane,
    /// `tmux kill-window -t @<wid>` — gtmux 내부 정리용 (빈 Window 청소).
    KillWindow,
    /// `tmux rename-window -t @<wid> <label>` — Panel label → window name 동기화 (D5).
    RenameWindow,
    /// `tmux send-keys -t %<pid>` — Input Target(I)로 지정된 Panel 입력 전달.
    SendKeys,
    /// `tmux refresh-client -A '%<pid>:pause/continue'` — Panel Streaming State 전이.
    RefreshClientPause,
    /// `tmux refresh-client -B <subscription>` — 포맷 구독 (tmux 3.2+ 푸시 모델).
    RefreshClientSubscribe,
    /// `tmux capture-pane -p -e -J -S -<lines>` — Deep scrollback 회복 (P1+).
    CapturePane,
    /// `tmux list-sessions -F` — 부트스트랩 1회 스냅샷.
    ListSessions,
    /// `tmux list-windows -a -F` — 부트스트랩 1회 스냅샷.
    ListWindows,
    /// `tmux list-panes -a -F` — 부트스트랩 1회 스냅샷.
    ListPanes,
    /// `tmux resize-window -t @<window_id> -x <cols> -y <rows>` — single-pane
    /// 컨벤션(ADR-0008 D1) 하에서 window-size = pane-size, 따라서 pane resize는
    /// `resize-window`로 직접 발급한다. `resize-pane`/`select-layout`은
    /// allowlist에서 영구 제외(ADR-0008 D2).
    ResizeWindow {
        /// `@<window_id>` 대상 — single-pane 컨벤션 하에서 panel 1개 = window 1개.
        window_id: u32,
        /// 새 가로 cell 수. tmux 측은 u16 범위로 충분.
        cols: u16,
        /// 새 세로 cell 수.
        rows: u16,
    },
}

/// 부트스트랩 placeholder — control mode 클라이언트 attach 시그니처.
pub fn connect() -> anyhow::Result<()> {
    todo!("mux-router::connect — control mode attach to be implemented")
}

// ───────────────────────────────────────────────────────────────────────────
// Event model — `%`-prefixed notifications from tmux control mode.
// ───────────────────────────────────────────────────────────────────────────

/// Decoded tmux control-mode notification (post line framing).
///
/// 출처: `docs/reports/0001-tmux-control-mode.md` §3 (notification table) +
/// `docs/adr/0001-tmux-integration-control-mode.md` D7. Only the subset
/// required for MVP P0 lifecycle/output is materialized; `%begin`/`%end`/
/// `%error` command-response framing is owned by a separate layer and is
/// intentionally *not* an `Event` variant here. Unrecognized but
/// well-formed `%`-prefixed lines fall through to [`Event::Unknown`] so
/// that future tmux versions cannot break us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `%output %<pane-id> <data>` — pane stdout. `bytes` is already
    /// `\NNN`-decoded raw bytes (UTF-8 multi-byte and ANSI escapes preserved).
    Output { pane_id: u32, bytes: Vec<u8> },
    /// `%extended-output %<pane-id> <age-ms> : <data>` — same as `Output`
    /// but with the `age_ms` queue-residency field that `pause-after`
    /// exposes. `age_ms` is telemetry only (R1 §4); the payload uses the
    /// identical octal decoder.
    ExtendedOutput {
        pane_id: u32,
        age_ms: u32,
        bytes: Vec<u8>,
    },
    /// `%pause %<pane-id>` — pane auto-paused by `pause-after` or manual
    /// `refresh-client -A '%<pid>:pause'`.
    Pause { pane_id: u32 },
    /// `%continue %<pane-id>` — pane resumed by
    /// `refresh-client -A '%<pid>:continue'`.
    Continue { pane_id: u32 },
    /// `%session-changed $<id> <name>` — this client attached to a
    /// different session.
    SessionChanged { session_id: u32, name: String },
    /// `%window-add @<wid>` — a window was added to the attached session.
    WindowAdd { window_id: u32 },
    /// `%window-close @<wid>` — a window was closed.
    WindowClose { window_id: u32 },
    /// `%window-renamed @<wid> <name>` — a window was renamed.
    WindowRenamed { window_id: u32, name: String },
    /// `%pane-dead %<pid>` — the pane's process exited (zombie pane,
    /// `pane_dead=1` mirror per grill D21 c4).
    PaneDead { pane_id: u32 },
    /// `%layout-change @<wid> <layout> ...` — used as a *change-detection
    /// trigger only* per ADR-0008 D3 / R1 §7 (not interpreted as canvas
    /// coordinates). The `layout` field carries the raw tmux layout string
    /// so downstream can ignore it without re-parsing.
    LayoutChange { window_id: u32, layout: String },
    /// `%sessions-changed` — a session was created or destroyed (no args).
    SessionsChanged,
    /// `%exit [reason]` — control-mode client is terminating. `reason` is
    /// `None` when tmux emits a bare `%exit`.
    Exit { reason: Option<String> },
    /// Well-formed `%foo` line we do not recognize. Logged at debug;
    /// surfaced as a graceful fallback so a future tmux notification name
    /// does not crash the parser.
    Unknown,
}

// ───────────────────────────────────────────────────────────────────────────
// Errors
// ───────────────────────────────────────────────────────────────────────────

/// Parser error surface. Most malformed input degrades to `Ok(None)`; this
/// enum exists for the rare cases where a caller *requests* strict parsing
/// (e.g. a future test harness or replayer). The public [`parse_line`]
/// entry point itself never returns [`ParseError::Malformed`] — it folds
/// malformed lines into `Ok(None)` so the live stream never crashes.
#[derive(Debug, Error)]
pub enum ParseError {
    /// `\NNN` decoder saw an octal digit out of range. Currently
    /// unreachable from [`decode_output_payload`] (it tolerates any byte
    /// triplet and falls back to verbatim preservation) — kept for the
    /// strict-decoder variant added in Sprint 1.
    #[error("invalid octal escape at byte {0}")]
    InvalidOctal(usize),
    /// Reserved for callers requesting strict parsing in future.
    #[error("malformed line")]
    Malformed,
}

// ───────────────────────────────────────────────────────────────────────────
// Public API: parse_line
// ───────────────────────────────────────────────────────────────────────────

/// Parse a single tmux control-mode line.
///
/// Input: one line (LF already stripped) of raw bytes as received from
/// `tmux -C` stdout. The framer upstream is responsible for splitting on
/// `\n` and (per ADR-0001 D12) stripping any incidental `\r`.
///
/// Returns:
/// - `Ok(Some(Event))` for any recognized `%`-prefixed notification, including
///   well-formed-but-unknown ones (mapped to [`Event::Unknown`]).
/// - `Ok(None)` for empty lines, lines that do not begin with `%`
///   (`%begin`/`%end`/`%error` response framing is intentionally left to a
///   different layer), and malformed lines we choose to drop rather than
///   error on. This graceful policy follows R1 §11 / ADR-0001 D12 — never
///   let a single bad line tear down the live stream.
/// - `Err` is reserved; the current implementation does not return it. The
///   `Result` shape is preserved so a strict mode can be added without an
///   API break.
///
/// Note: `%begin`/`%end`/`%error` command-response gating frames are *not*
/// surfaced here because they belong to the command-reply layer (FIFO
/// matching by `command-number n`, ADR-0001 D4). The parser deliberately
/// returns `Ok(None)` for them so the notification dispatcher never sees
/// them.
pub fn parse_line(line: &[u8]) -> Result<Option<Event>, ParseError> {
    // R1 §1: tmux command lines / blank lines / non-`%` lines never carry
    // notifications. Drop them silently (graceful).
    if line.is_empty() || line[0] != b'%' {
        return Ok(None);
    }

    // Dispatch on the longest matching prefix. Order matters where one
    // name is a prefix of another (`%extended-output` before `%output` is
    // not required because the token is space-terminated, but using the
    // exact-token form below makes the precedence explicit and robust).
    if let Some(rest) = strip_token(line, b"%output") {
        return Ok(parse_output(rest));
    }
    if let Some(rest) = strip_token(line, b"%extended-output") {
        return Ok(parse_extended_output(rest));
    }
    if let Some(rest) = strip_token(line, b"%pause") {
        return Ok(parse_pane_only(rest, |pane_id| Event::Pause { pane_id }));
    }
    if let Some(rest) = strip_token(line, b"%continue") {
        return Ok(parse_pane_only(rest, |pane_id| Event::Continue { pane_id }));
    }
    if let Some(rest) = strip_token(line, b"%session-changed") {
        return Ok(parse_session_changed(rest));
    }
    if let Some(rest) = strip_token(line, b"%window-add") {
        return Ok(parse_window_id_only(rest, |window_id| Event::WindowAdd {
            window_id,
        }));
    }
    if let Some(rest) = strip_token(line, b"%window-close") {
        return Ok(parse_window_id_only(rest, |window_id| Event::WindowClose {
            window_id,
        }));
    }
    if let Some(rest) = strip_token(line, b"%window-renamed") {
        return Ok(parse_window_renamed(rest));
    }
    if let Some(rest) = strip_token(line, b"%pane-dead") {
        return Ok(parse_pane_only(rest, |pane_id| Event::PaneDead { pane_id }));
    }
    if let Some(rest) = strip_token(line, b"%layout-change") {
        return Ok(parse_layout_change(rest));
    }
    if let Some(rest) = strip_token(line, b"%sessions-changed") {
        // `%sessions-changed` carries no arguments; tolerate trailing
        // whitespace just in case.
        return Ok(if rest.iter().all(|&b| b == b' ') {
            Some(Event::SessionsChanged)
        } else {
            Some(Event::Unknown)
        });
    }
    if let Some(rest) = strip_token(line, b"%exit") {
        return Ok(Some(parse_exit(rest)));
    }

    // `%begin`/`%end`/`%error` are command-response framing — surfaced via
    // a different channel (ADR-0001 D4). Drop them here so the notification
    // dispatcher does not see them as Unknown noise.
    if line.starts_with(b"%begin") || line.starts_with(b"%end") || line.starts_with(b"%error") {
        return Ok(None);
    }

    // A genuinely unknown `%`-prefixed line — preserved as a graceful
    // fallback so future tmux notification names do not crash the live
    // stream (R1 §11 / ADR-0001 D12).
    Ok(Some(Event::Unknown))
}

// ───────────────────────────────────────────────────────────────────────────
// Per-notification parsers
// ───────────────────────────────────────────────────────────────────────────

/// Parse the tail of a `%output %<pid> <data>` line.
///
/// Returns `None` (which the public [`parse_line`] folds into `Ok(None)`)
/// when the pane id is missing or malformed — that case is the most
/// likely shape of stream corruption and silently dropping it keeps the
/// live channel alive (R1 §11 / ADR-0001 D12). Returns
/// `Some(Event::Unknown)` only for "this looks intentional but I do not
/// understand it" shapes (e.g. a stray byte where the space separator
/// should be), so the dispatcher logs a real anomaly.
fn parse_output(rest: &[u8]) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (pane_id, after_pid) = parse_pane_id(rest)?;
    // Exactly one space separates `%<pid>` from the payload.
    let payload = match after_pid.first() {
        Some(b' ') => &after_pid[1..],
        // An empty payload right after the pane id is legal (a zero-byte
        // burst). Treat anything else as malformed → Unknown so it surfaces
        // in logs.
        None => b"",
        _ => return Some(Event::Unknown),
    };
    Some(Event::Output {
        pane_id,
        bytes: decode_output_payload(payload),
    })
}

/// Parse the tail of `%extended-output %<pid> <age-ms> : <data>`.
fn parse_extended_output(rest: &[u8]) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (pane_id, after_pid) = parse_pane_id(rest)?;
    let after_pid = trim_leading_space(after_pid);
    let (age_ms, after_age) = parse_u32(after_pid)?;
    let after_age = trim_leading_space(after_age);
    // tmux puts a literal `:` separator between `<age-ms>` and `<data>`,
    // optionally surrounded by spaces (R1 §3 / §4).
    let after_colon = match after_age.first() {
        Some(b':') => &after_age[1..],
        _ => return Some(Event::Unknown),
    };
    let payload = match after_colon.first() {
        Some(b' ') => &after_colon[1..],
        None => b"",
        _ => after_colon, // tolerate `:foo` with no space
    };
    Some(Event::ExtendedOutput {
        pane_id,
        age_ms,
        bytes: decode_output_payload(payload),
    })
}

/// Parse `%pause %<pid>`, `%continue %<pid>`, `%pane-dead %<pid>` —
/// notifications that carry exactly one pane id.
fn parse_pane_only(rest: &[u8], ctor: fn(u32) -> Event) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (pane_id, tail) = parse_pane_id(rest)?;
    // Tolerate trailing whitespace; anything else makes the line
    // suspicious.
    if !tail.iter().all(|&b| b == b' ') {
        return Some(Event::Unknown);
    }
    Some(ctor(pane_id))
}

/// Parse `%window-add @<wid>` and `%window-close @<wid>`.
fn parse_window_id_only(rest: &[u8], ctor: fn(u32) -> Event) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (window_id, tail) = parse_window_id(rest)?;
    if !tail.iter().all(|&b| b == b' ') {
        return Some(Event::Unknown);
    }
    Some(ctor(window_id))
}

/// Parse `%window-renamed @<wid> <name>`.
fn parse_window_renamed(rest: &[u8]) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (window_id, after_wid) = parse_window_id(rest)?;
    let after_wid = trim_leading_space(after_wid);
    // tmux does not escape names in the notification stream; treat the
    // whole tail as the name verbatim, lossy-converted to UTF-8 (panel
    // labels are user-facing strings, so a non-UTF-8 byte sequence
    // — extremely rare for window names — degrades to U+FFFD rather than
    // dropping the event).
    let name = String::from_utf8_lossy(after_wid).into_owned();
    Some(Event::WindowRenamed { window_id, name })
}

/// Parse `%session-changed $<sid> <name>`.
fn parse_session_changed(rest: &[u8]) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (session_id, after_sid) = parse_session_id(rest)?;
    let after_sid = trim_leading_space(after_sid);
    let name = String::from_utf8_lossy(after_sid).into_owned();
    Some(Event::SessionChanged { session_id, name })
}

/// Parse `%layout-change @<wid> <layout> [<visible-layout> <flags>]`.
///
/// The full tmux notification has up to four fields after the window id
/// (R1 §3) but only the first — the canonical layout string — is needed
/// downstream (we use it as a change-detection trigger per ADR-0008 D3,
/// not as canvas coordinates). Capture it verbatim and ignore the rest.
fn parse_layout_change(rest: &[u8]) -> Option<Event> {
    let rest = trim_leading_space(rest);
    let (window_id, after_wid) = parse_window_id(rest)?;
    let after_wid = trim_leading_space(after_wid);
    // Layout strings never contain whitespace, so split on the first space
    // to drop the trailing visible-layout/flags we do not consume.
    let layout_bytes = match memchr(b' ', after_wid) {
        Some(idx) => &after_wid[..idx],
        None => after_wid,
    };
    Some(Event::LayoutChange {
        window_id,
        layout: String::from_utf8_lossy(layout_bytes).into_owned(),
    })
}

/// Parse `%exit [reason]`.
///
/// `%exit` alone → `Event::Exit { reason: None }`. `%exit <text>` →
/// `Event::Exit { reason: Some(text) }`. Per R1 §1 this is always the last
/// notification before the control-mode client terminates.
fn parse_exit(rest: &[u8]) -> Event {
    let rest = trim_leading_space(rest);
    if rest.is_empty() {
        Event::Exit { reason: None }
    } else {
        Event::Exit {
            reason: Some(String::from_utf8_lossy(rest).into_owned()),
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Public API: decode_output_payload
// ───────────────────────────────────────────────────────────────────────────

/// Decode the `\NNN` octal-escape encoding tmux uses in `%output` /
/// `%extended-output` payloads.
///
/// Encoding rules per R1 §4:
/// - bytes < 0x20 and `\` (0x5C) are emitted as `\NNN` (three octal digits)
/// - all other bytes (including the entire ≥0x80 range used by UTF-8
///   multi-byte sequences) are passed through verbatim
///
/// We use a 256-entry classification LUT to dispatch each input byte in
/// one indexed load — the only branchy byte is `\`, and the LUT lets the
/// inner loop stay tight on the dominant non-escape path. Non-triplet
/// escape sequences (e.g. `\1` followed by a space, `\99`, or a stray
/// trailing `\`) are preserved verbatim rather than rejected — R1 §4
/// describes only the well-formed encoding, so a safe fallback is to
/// surface unrecognized bytes unchanged. This also makes us tolerant to
/// any future tmux change that emits a new escape form.
///
/// Allocations: the output `Vec` is pre-sized to the input length, which
/// is the exact upper bound (decoding can only shrink, never grow).
pub fn decode_output_payload(escaped: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(escaped.len());
    let len = escaped.len();
    let mut i = 0;
    while i < len {
        let b = escaped[i];
        // Fast non-escape path — LUT classification of `\` is the only
        // branch we care about; everything else is a single byte copy.
        if !OCTAL_ESC_INIT[b as usize] {
            out.push(b);
            i += 1;
            continue;
        }
        // We are sitting on a backslash. Need three more bytes that are
        // all octal digits 0-7. If any check fails, preserve the
        // backslash verbatim and resume scanning at the next byte.
        if i + 3 < len {
            let d0 = escaped[i + 1];
            let d1 = escaped[i + 2];
            let d2 = escaped[i + 3];
            if OCTAL_DIGIT[d0 as usize] && OCTAL_DIGIT[d1 as usize] && OCTAL_DIGIT[d2 as usize] {
                // Three digits, each 0..=7, max value 7*64 + 7*8 + 7 = 511,
                // which exceeds 255 only when d0 >= 4. tmux only encodes
                // bytes < 0x20 and `\` (0x5C = `\134`) so the maximum
                // legitimate value is 0o134 = 92. Anything larger is
                // either a bug on tmux's side or an attacker-shaped
                // payload — fall back to verbatim preservation to avoid
                // silent truncation.
                let value = (u16::from(d0 - b'0') << 6)
                    | (u16::from(d1 - b'0') << 3)
                    | u16::from(d2 - b'0');
                if value <= 0xFF {
                    out.push(value as u8);
                    i += 4;
                    continue;
                }
            }
        }
        // Verbatim fallback: emit the backslash unchanged and advance one
        // byte. Subsequent bytes will be re-scanned in case the next
        // iteration finds a valid `\NNN`.
        out.push(b);
        i += 1;
    }
    out
}

/// 256-entry classification LUT: `true` for bytes that need the
/// slow-path decode branch. Today that is only `\` (0x5C). Kept as a
/// LUT rather than a single `== b'\\'` comparison so future escape
/// initiators (if tmux ever adds one) cost a table flip rather than an
/// added branch.
const OCTAL_ESC_INIT: [bool; 256] = build_esc_init_lut();

/// 256-entry octal-digit LUT — `true` for `b'0'..=b'7'`. Used to keep
/// the triplet check branch-free per byte.
const OCTAL_DIGIT: [bool; 256] = build_octal_digit_lut();

const fn build_esc_init_lut() -> [bool; 256] {
    let mut t = [false; 256];
    t[b'\\' as usize] = true;
    t
}

const fn build_octal_digit_lut() -> [bool; 256] {
    let mut t = [false; 256];
    let mut c = b'0';
    while c <= b'7' {
        t[c as usize] = true;
        c += 1;
    }
    t
}

// ───────────────────────────────────────────────────────────────────────────
// Low-level byte helpers
// ───────────────────────────────────────────────────────────────────────────

/// If `input` starts with `token` followed by either end-of-input or a
/// space, return the tail after `token`. Otherwise `None`.
///
/// The space-or-EOF anchor prevents `%output` from greedily matching
/// `%outputfoo` (a hypothetical future notification with the same
/// prefix). Without it the dispatcher in [`parse_line`] would have to
/// re-validate every match — folding that check in here keeps the
/// dispatch table flat.
fn strip_token<'a>(input: &'a [u8], token: &[u8]) -> Option<&'a [u8]> {
    if input.len() < token.len() {
        return None;
    }
    if &input[..token.len()] != token {
        return None;
    }
    let tail = &input[token.len()..];
    match tail.first() {
        Some(b' ') | None => Some(tail),
        _ => None,
    }
}

/// Strip exactly the runs of leading 0x20 spaces. tmux never indents
/// notifications, but `\t` or `\r` would be a different beast — we
/// intentionally do not strip those so a malformed line surfaces as
/// `Unknown` rather than getting silently normalized.
fn trim_leading_space(input: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < input.len() && input[i] == b' ' {
        i += 1;
    }
    &input[i..]
}

/// Parse a `%<u32>` token. Returns the parsed id and the byte slice
/// starting at the first byte after the digits, or `None` if the input
/// does not start with `%` or no digits follow.
fn parse_pane_id(input: &[u8]) -> Option<(u32, &[u8])> {
    parse_prefixed_u32(input, b'%')
}

/// Parse a `@<u32>` token. See [`parse_pane_id`].
fn parse_window_id(input: &[u8]) -> Option<(u32, &[u8])> {
    parse_prefixed_u32(input, b'@')
}

/// Parse a `$<u32>` token. See [`parse_pane_id`].
fn parse_session_id(input: &[u8]) -> Option<(u32, &[u8])> {
    parse_prefixed_u32(input, b'$')
}

fn parse_prefixed_u32(input: &[u8], sigil: u8) -> Option<(u32, &[u8])> {
    if input.first().copied() != Some(sigil) {
        return None;
    }
    parse_u32(&input[1..])
}

/// Parse a base-10 `u32` from the start of `input`, returning the value
/// and the slice starting at the first non-digit. Returns `None` if the
/// first byte is not an ASCII digit or if the value would overflow u32.
fn parse_u32(input: &[u8]) -> Option<(u32, &[u8])> {
    let mut i = 0;
    let mut value: u32 = 0;
    while i < input.len() {
        let b = input[i];
        if !b.is_ascii_digit() {
            break;
        }
        // Saturating-style overflow check — tmux ids fit comfortably in
        // u32 (the daemon itself uses int internally), but a hostile or
        // corrupted stream could try to overflow. Reject rather than
        // wrap; the caller surfaces this as `Unknown` via the `None` path.
        value = value.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
        i += 1;
    }
    if i == 0 {
        None
    } else {
        Some((value, &input[i..]))
    }
}

/// Tiny `memchr` shim — finds the first occurrence of `needle` in
/// `haystack`. We avoid pulling in the `memchr` crate for a single
/// space-scan; the byte volume per line is tiny (`%layout-change` lines
/// are < 256 bytes) so the LLVM-vectorized loop is plenty.
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // ── decode_output_payload ────────────────────────────────────────────

    #[test]
    fn decode_ansi_escape() {
        assert_eq!(decode_output_payload(b"\\033"), vec![0x1B]);
        assert_eq!(decode_output_payload(b"hi\\033there"), b"hi\x1Bthere");
    }

    #[test]
    fn decode_backslash() {
        // tmux encodes a literal `\` as `\134`.
        assert_eq!(decode_output_payload(b"\\134"), vec![0x5C]);
    }

    #[test]
    fn decode_octal_boundaries() {
        // 0x00 = \000
        assert_eq!(decode_output_payload(b"\\000"), vec![0x00]);
        // 0xFF = \377 (not a tmux-emitted code, but check the upper edge
        // of representable triplets just to be sure the decoder is
        // self-consistent).
        assert_eq!(decode_output_payload(b"\\377"), vec![0xFF]);
    }

    #[test]
    fn decode_partial_octal_verbatim() {
        // Single octal digit followed by a space — not a valid triplet,
        // must be preserved verbatim.
        assert_eq!(decode_output_payload(b"\\1 "), b"\\1 ");
        // Trailing lone backslash at end-of-input.
        assert_eq!(decode_output_payload(b"foo\\"), b"foo\\");
        // `\99` — second digit is out of range (9 is not octal).
        assert_eq!(decode_output_payload(b"\\99x"), b"\\99x");
    }

    #[test]
    fn decode_passthrough_utf8() {
        // 한글 — Korean UTF-8 multi-byte sequence (each byte ≥ 0x80).
        let utf8 = "한글".as_bytes();
        assert_eq!(decode_output_payload(utf8), utf8.to_vec());
    }

    #[test]
    fn decode_high_byte_passthrough() {
        // Arbitrary 0x80+ bytes (e.g. raw binary in stdout) must pass
        // through untouched per R1 §4.
        let bytes: &[u8] = &[0xE2, 0x80, 0xA2, 0xC2, 0xA9, 0xFE];
        assert_eq!(decode_output_payload(bytes), bytes.to_vec());
    }

    // ── parse_line: %output / %extended-output ───────────────────────────

    #[test]
    fn output_simple() {
        let ev = parse_line(b"%output %1 hello\\012world").unwrap().unwrap();
        // \012 = octal 12 = 0x0A = LF.
        assert_eq!(
            ev,
            Event::Output {
                pane_id: 1,
                bytes: b"hello\nworld".to_vec(),
            }
        );
    }

    #[test]
    fn output_ansi_escape() {
        let ev = parse_line(b"%output %1 hello\\033[31mworld")
            .unwrap()
            .unwrap();
        assert_eq!(
            ev,
            Event::Output {
                pane_id: 1,
                bytes: b"hello\x1b[31mworld".to_vec(),
            }
        );
    }

    #[test]
    fn output_backslash() {
        let ev = parse_line(b"%output %2 a\\134b").unwrap().unwrap();
        assert_eq!(
            ev,
            Event::Output {
                pane_id: 2,
                bytes: b"a\\b".to_vec(),
            }
        );
    }

    #[test]
    fn output_passthrough_utf8() {
        // UTF-8 한글 in the middle of a payload, no escapes.
        let mut line = b"%output %3 ".to_vec();
        line.extend_from_slice("한글".as_bytes());
        let ev = parse_line(&line).unwrap().unwrap();
        assert_eq!(
            ev,
            Event::Output {
                pane_id: 3,
                bytes: "한글".as_bytes().to_vec(),
            }
        );
    }

    #[test]
    fn output_high_byte() {
        // Arbitrary high bytes embedded in the payload (no escapes).
        let mut line = b"%output %4 ".to_vec();
        line.extend_from_slice(&[0xE2, 0x80, 0xA2, 0xC2, 0xA9]);
        let ev = parse_line(&line).unwrap().unwrap();
        assert_eq!(
            ev,
            Event::Output {
                pane_id: 4,
                bytes: vec![0xE2, 0x80, 0xA2, 0xC2, 0xA9],
            }
        );
    }

    #[test]
    fn extended_output_age() {
        let ev = parse_line(b"%extended-output %1 1234 : hello")
            .unwrap()
            .unwrap();
        assert_eq!(
            ev,
            Event::ExtendedOutput {
                pane_id: 1,
                age_ms: 1234,
                bytes: b"hello".to_vec(),
            }
        );
    }

    // ── parse_line: lifecycle notifications ──────────────────────────────

    #[test]
    fn pause_continue() {
        assert_eq!(
            parse_line(b"%pause %2").unwrap().unwrap(),
            Event::Pause { pane_id: 2 }
        );
        assert_eq!(
            parse_line(b"%continue %2").unwrap().unwrap(),
            Event::Continue { pane_id: 2 }
        );
    }

    #[test]
    fn window_lifecycle() {
        assert_eq!(
            parse_line(b"%window-add @5").unwrap().unwrap(),
            Event::WindowAdd { window_id: 5 }
        );
        assert_eq!(
            parse_line(b"%window-close @5").unwrap().unwrap(),
            Event::WindowClose { window_id: 5 }
        );
        assert_eq!(
            parse_line(b"%window-renamed @5 main").unwrap().unwrap(),
            Event::WindowRenamed {
                window_id: 5,
                name: "main".into(),
            }
        );
    }

    #[test]
    fn pane_dead() {
        assert_eq!(
            parse_line(b"%pane-dead %7").unwrap().unwrap(),
            Event::PaneDead { pane_id: 7 }
        );
    }

    #[test]
    fn layout_change() {
        let ev = parse_line(b"%layout-change @1 b25e,80x24,0,0,1")
            .unwrap()
            .unwrap();
        assert_eq!(
            ev,
            Event::LayoutChange {
                window_id: 1,
                layout: "b25e,80x24,0,0,1".into(),
            }
        );
    }

    #[test]
    fn layout_change_with_trailing_fields() {
        // tmux may emit visible-layout + flags after the canonical layout —
        // we capture only the first whitespace-delimited token.
        let ev = parse_line(b"%layout-change @1 b25e,80x24,0,0,1 b25e,80x24,0,0,1 0")
            .unwrap()
            .unwrap();
        assert_eq!(
            ev,
            Event::LayoutChange {
                window_id: 1,
                layout: "b25e,80x24,0,0,1".into(),
            }
        );
    }

    #[test]
    fn session_changed() {
        let ev = parse_line(b"%session-changed $0 work").unwrap().unwrap();
        assert_eq!(
            ev,
            Event::SessionChanged {
                session_id: 0,
                name: "work".into(),
            }
        );
    }

    #[test]
    fn sessions_changed_no_args() {
        assert_eq!(
            parse_line(b"%sessions-changed").unwrap().unwrap(),
            Event::SessionsChanged
        );
    }

    #[test]
    fn exit_with_reason() {
        assert_eq!(
            parse_line(b"%exit normal").unwrap().unwrap(),
            Event::Exit {
                reason: Some("normal".into()),
            }
        );
    }

    #[test]
    fn exit_no_reason() {
        assert_eq!(
            parse_line(b"%exit").unwrap().unwrap(),
            Event::Exit { reason: None }
        );
    }

    // ── parse_line: graceful degradation ─────────────────────────────────

    #[test]
    fn unknown_line_graceful() {
        // Well-formed `%`-prefixed line we do not know — surfaces as
        // `Unknown` so the dispatcher can log it without aborting.
        assert_eq!(
            parse_line(b"%totally-fake-event xyz").unwrap().unwrap(),
            Event::Unknown
        );
    }

    #[test]
    fn empty_line() {
        assert!(parse_line(b"").unwrap().is_none());
    }

    #[test]
    fn comment_line_is_dropped() {
        // tmux does not emit comments, but the spec is to drop any
        // line that does not start with `%`.
        assert!(parse_line(b"#tmux comment").unwrap().is_none());
        assert!(parse_line(b"plain text").unwrap().is_none());
    }

    #[test]
    fn command_response_framing_dropped() {
        // `%begin`/`%end`/`%error` belong to the command-reply layer
        // (ADR-0001 D4) and must not appear as notifications.
        assert!(parse_line(b"%begin 1700000000 42 0").unwrap().is_none());
        assert!(parse_line(b"%end 1700000000 42 0").unwrap().is_none());
        assert!(parse_line(b"%error 1700000000 42 0").unwrap().is_none());
    }

    #[test]
    fn malformed_pane_id_graceful() {
        // `xyz` is not a valid pane id. We choose `Ok(None)` over `Err`
        // to keep the live stream alive (R1 §11 / ADR-0001 D12) — a
        // single corrupted line must never tear down the dispatcher.
        // `Some(Event::Unknown)` is an acceptable alternative per the
        // task contract; we picked the stricter `None` form so the
        // dispatcher does not waste a logging slot on noise.
        assert!(parse_line(b"%output xyz hello").unwrap().is_none());
    }

    // ── decode_output_payload micro-coverage ─────────────────────────────

    #[test]
    fn decode_empty_input() {
        assert_eq!(decode_output_payload(b""), Vec::<u8>::new());
    }

    #[test]
    fn decode_ascii_passthrough() {
        // Sanity: pure ASCII text without escapes round-trips verbatim.
        let s = b"the quick brown fox 0123456789";
        assert_eq!(decode_output_payload(s), s.to_vec());
    }
}
