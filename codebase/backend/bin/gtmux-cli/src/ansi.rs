//! ANSI-escape stripping + hex parsing for `gtmux terminal read|send`
//! (ADR-0054 D4). These are CLI-side helpers — the server keeps the pane's
//! raw PTY bytes untouched, so the *client* decides whether to render the raw
//! stream (`--raw`) or a stripped, LLM-readable text (the default).
//!
//! The stripper removes the two escape families that dominate normal command
//! output — CSI (`ESC [ … final`) and OSC (`ESC ] … BEL|ST`) — plus the small
//! two/three-byte escapes (charset selectors, keypad mode, …). It is a
//! best-effort cleanup, not a VT emulator: interleaved cursor-motion in a TUI
//! (vim / a full-screen agent) still yields nonsense, which is the documented
//! lossiness of the raw-ring read (ADR-0054 D1, feasibility report §①).

/// Strip ANSI escape sequences, returning printable text. Escape sequences are
/// pure ASCII, so this walks `char`s and only diverts on `ESC` (`\u{1b}`);
/// every other char (including multi-byte UTF-8) is passed through untouched.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // ESC — dispatch on the following byte.
        match chars.next() {
            // CSI: parameters/intermediates until a final byte 0x40..=0x7E.
            Some('[') => {
                for f in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&f) {
                        break;
                    }
                }
            }
            // OSC: string until BEL (0x07) or ST (ESC \).
            Some(']') => {
                while let Some(f) = chars.next() {
                    if f == '\u{07}' {
                        break;
                    }
                    if f == '\u{1b}' {
                        if matches!(chars.peek(), Some('\\')) {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Charset designator: ESC ( X / ESC ) X — drop the trailing byte.
            Some('(') | Some(')') => {
                chars.next();
            }
            // Any other short escape (ESC =, ESC >, ESC M, …) — the single
            // following byte is already consumed; nothing more to drop.
            Some(_) => {}
            None => break,
        }
    }
    out
}

/// Parse a hex string (whitespace tolerated) into raw bytes — the wire form of
/// `gtmux terminal send --bytes` for control sequences (e.g. `03` = Ctrl-C).
pub fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err("--bytes hex is empty".to_string());
    }
    if compact.len() % 2 != 0 {
        return Err("--bytes hex must have an even number of digits".to_string());
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let pair = &compact[i..i + 2];
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|_| format!("--bytes: invalid hex pair {pair:?} at offset {i}"))?;
        out.push(byte);
        i += 2;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_color_and_cursor() {
        // Colored "OK" plus a cursor-home CSI.
        let raw = "\u{1b}[1;32mOK\u{1b}[0m\u{1b}[H done";
        assert_eq!(strip_ansi(raw), "OK done");
    }

    #[test]
    fn strips_osc_title_bel_and_st() {
        let bel = "\u{1b}]0;my title\u{07}text";
        assert_eq!(strip_ansi(bel), "text");
        let st = "\u{1b}]0;my title\u{1b}\\text";
        assert_eq!(strip_ansi(st), "text");
    }

    #[test]
    fn keeps_plain_text_and_newlines() {
        let raw = "line one\nline two\n";
        assert_eq!(strip_ansi(raw), raw);
    }

    #[test]
    fn strips_charset_and_short_escapes() {
        // Charset designator (ESC ( B) + keypad-normal (ESC >) around text.
        let raw = "\u{1b}(B\u{1b}>hi";
        assert_eq!(strip_ansi(raw), "hi");
    }

    #[test]
    fn passes_multibyte_utf8_through() {
        let raw = "\u{1b}[31m한글\u{1b}[0m ok";
        assert_eq!(strip_ansi(raw), "한글 ok");
    }

    #[test]
    fn trailing_lone_esc_is_dropped() {
        assert_eq!(strip_ansi("done\u{1b}"), "done");
    }

    #[test]
    fn parse_hex_roundtrip_and_whitespace() {
        assert_eq!(parse_hex("03").unwrap(), vec![0x03]);
        assert_eq!(parse_hex("1b 5b 41").unwrap(), vec![0x1b, 0x5b, 0x41]);
        assert_eq!(parse_hex("DEADbeef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_hex_rejects_odd_and_bad() {
        assert!(parse_hex("0").is_err());
        assert!(parse_hex("").is_err());
        assert!(parse_hex("zz").is_err());
    }
}
