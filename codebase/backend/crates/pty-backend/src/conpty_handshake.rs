//! portable-pty enables INHERIT_CURSOR on Windows. ConPTY requests an initial
//! cursor position before starting the shell, even with no browser attached.
//! Consume and answer only that first DSR; later application queries pass through.
#[derive(Default)]
pub(crate) struct CursorHandshake { pending: Vec<u8>, done: bool }
impl CursorHandshake {
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, bool) {
        if self.done { return (bytes.to_vec(), false); }
        const QUERY: &[u8] = b"\x1b[6n";
        let mut output = Vec::with_capacity(bytes.len()); let mut reply = false;
        for &byte in bytes {
            if self.done { output.push(byte); continue; }
            self.pending.push(byte);
            while !QUERY.starts_with(&self.pending) { output.push(self.pending.remove(0)); }
            if self.pending == QUERY { self.pending.clear(); self.done = true; reply = true; }
        }
        (output, reply)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_startup_query_is_answered_once_without_swallowing_other_output() {
        let mut handshake = CursorHandshake::default();
        assert_eq!(handshake.feed(b"hello\x1b["), (b"hello".to_vec(), false));
        assert_eq!(handshake.feed(b"6nready"), (b"ready".to_vec(), true));
        assert_eq!(handshake.feed(b"\x1b[6n"), (b"\x1b[6n".to_vec(), false));
    }
    #[test]
    fn unrelated_escapes_are_preserved() {
        let mut handshake = CursorHandshake::default();
        assert_eq!(handshake.feed(b"\x1b[31mtext\x1b[0m"), (b"\x1b[31mtext\x1b[0m".to_vec(), false));
    }
}
