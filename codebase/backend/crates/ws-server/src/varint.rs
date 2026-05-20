//! Unsigned LEB128 varint encode / decode for envelope inner payloads.
//!
//! 정본: `docs/ssot/wire-protocol.md` §1.3 — gtmux pane ids fit in 5 bytes of
//! varint output (practical upper bound < 2^28). 5-byte oversize is rejected
//! at decode to defang length-of-length amplification attacks.

/// Maximum number of varint bytes accepted on decode (SSoT §1.3).
pub const MAX_VARINT_BYTES: usize = 5;

/// Append the unsigned LEB128 encoding of `value` to `out`. Pane ids and the
/// 0 sentinel are all in range; 64-bit inputs would round-trip but the SSoT
/// caps practical encodings at 5 bytes — callers that exceed it are misusing
/// the API.
pub fn encode_into(value: u64, out: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// Convenience: encode to a fresh `Vec<u8>`. Single allocation, sized for the
/// common single-byte case. Kept public so tests in sibling modules and the
/// future lifecycle command writer can share the implementation without
/// importing `encode_into`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn encode(value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(1);
    encode_into(value, &mut out);
    out
}

/// Decode a single varint from the start of `input`. Returns `(value,
/// bytes_consumed)` on success, or `None` if the encoding is malformed
/// (overrun, runs past [`MAX_VARINT_BYTES`], or trailing continuation bit
/// without further input).
pub fn decode(input: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for (i, &byte) in input.iter().take(MAX_VARINT_BYTES).enumerate() {
        let part = u64::from(byte & 0x7F);
        // Defend against 64-bit overflow on the final shift — but only when
        // a higher byte was actually emitted (a single-byte varint with
        // shift 0 is fine).
        let shifted = part
            .checked_shl(shift)
            .filter(|_| shift < u64::BITS)
            .or_else(|| (part == 0).then_some(0))?;
        value |= shifted;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    // Either ran out of input mid-continuation, or the 5-byte cap was hit
    // without a terminator — both are framing violations.
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_zero() {
        let bytes = encode(0);
        assert_eq!(bytes, vec![0x00]);
        assert_eq!(decode(&bytes), Some((0, 1)));
    }

    #[test]
    fn round_trip_boundary_127() {
        let bytes = encode(127);
        assert_eq!(bytes, vec![0x7F]);
        assert_eq!(decode(&bytes), Some((127, 1)));
    }

    #[test]
    fn round_trip_boundary_128() {
        let bytes = encode(128);
        // 128 = 0b1000_0000 → 0x80 0x01
        assert_eq!(bytes, vec![0x80, 0x01]);
        assert_eq!(decode(&bytes), Some((128, 2)));
    }

    #[test]
    fn round_trip_typical_pane_id() {
        let bytes = encode(37);
        assert_eq!(bytes, vec![0x25]);
        assert_eq!(decode(&bytes), Some((37, 1)));
    }

    #[test]
    fn round_trip_two_byte_pane_id() {
        let bytes = encode(16384);
        assert_eq!(bytes.len(), 3);
        assert_eq!(decode(&bytes), Some((16384, 3)));
    }

    #[test]
    fn decode_truncated_continuation_rejected() {
        // Continuation bit set but no follow-up byte.
        assert_eq!(decode(&[0x80]), None);
    }

    #[test]
    fn decode_oversized_rejected() {
        // Six continuation bytes — exceeds the 5-byte cap.
        let too_long = vec![0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(decode(&too_long), None);
    }

    #[test]
    fn decode_with_trailing_bytes_reports_consumed() {
        let mut bytes = encode(37);
        bytes.extend_from_slice(b"trailing");
        let (v, n) = decode(&bytes).unwrap();
        assert_eq!(v, 37);
        assert_eq!(n, 1);
        assert_eq!(&bytes[n..], b"trailing");
    }
}
