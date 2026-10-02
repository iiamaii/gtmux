//! Bounded PTY activity metadata. Output silence is not semantic completion.
//! Reports and OSC 133 shell markers are explicit; prompt matching is heuristic.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A terminal can remain alive after an agent finishes a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Unknown,
    Working,
    Quiet,
    Completed,
    NeedsInput,
}

/// Where a classification came from; consumers must label heuristics as estimates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivitySource {
    Output,
    Heuristic,
    Shell,
    Report,
}

/// Small snapshot, independent of the raw output subscription/ring buffer.
#[derive(Debug, Clone, Serialize)]
pub struct ActivitySnapshot {
    pub state: ActivityState,
    pub source: ActivitySource,
    pub output_seq: u64,
    pub state_seq: u64,
}

/// Scanner memory is bounded even for malicious/unterminated escape sequences.
pub(crate) struct ActivityTracker {
    state: ActivityState,
    source: ActivitySource,
    output_seq: u64,
    state_seq: u64,
    last_output: Option<Instant>,
    line: Vec<u8>,
    osc: Vec<u8>,
    mode: u8,
    saw_work: bool,
}
impl Default for ActivityTracker {
    fn default() -> Self {
        Self {
            state: ActivityState::Unknown,
            source: ActivitySource::Output,
            output_seq: 0,
            state_seq: 0,
            last_output: None,
            line: Vec::new(),
            osc: Vec::new(),
            mode: 0,
            saw_work: false,
        }
    }
}
impl ActivityTracker {
    fn set(&mut self, state: ActivityState, source: ActivitySource) {
        if self.state != state || self.source != source {
            self.state = state;
            self.source = source;
            self.state_seq = self.state_seq.saturating_add(1);
        }
    }
    pub(crate) fn report(&mut self, state: ActivityState) {
        // Repeated completed reports represent distinct turns, even without input.
        self.state_seq = self.state_seq.saturating_add(1);
        self.set(state, ActivitySource::Report);
        self.line.clear();
    }
    pub(crate) fn input(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.line.clear();
        self.saw_work = true;
        self.set(ActivityState::Working, ActivitySource::Output);
    }
    fn shell_marker(&mut self) {
        if self.osc == b"133;C" {
            self.saw_work = true;
            self.set(ActivityState::Working, ActivitySource::Shell);
        } else if self.osc == b"133;D" || self.osc.starts_with(b"133;D;") {
            self.set(ActivityState::Completed, ActivitySource::Shell);
        }
        self.osc.clear();
    }
    pub(crate) fn output(&mut self, bytes: &[u8], now: Instant) {
        let mut printable = false;
        for &b in bytes {
            match self.mode {
                0 => match b {
                    0x1b => self.mode = 1,
                    b'\r' | b'\n' => {
                        self.line.clear();
                    }
                    8 => {
                        self.line.pop();
                    }
                    0x20..=0xff if b != 0x7f => {
                        printable = true;
                        if self.line.len() >= 1024 {
                            self.line.drain(..512);
                        }
                        self.line.push(b);
                    }
                    _ => {}
                },
                1 => match b {
                    b'[' => self.mode = 2,
                    b']' => {
                        self.mode = 3;
                        self.osc.clear();
                    }
                    b'P' | b'_' | b'^' => self.mode = 5,
                    _ => {
                        self.mode = 0;
                    }
                },
                2 => {
                    if (0x40..=0x7e).contains(&b) {
                        if matches!(b, b'J' | b'K' | b'H' | b'f') {
                            self.line.clear();
                        }
                        self.mode = 0;
                    }
                }
                3 => match b {
                    7 => {
                        self.shell_marker();
                        self.mode = 0;
                    }
                    0x1b => self.mode = 4,
                    _ => {
                        if self.osc.len() < 256 {
                            self.osc.push(b);
                        } else {
                            self.osc.clear();
                            self.mode = 5;
                        }
                    }
                },
                4 => {
                    if b == b'\\' {
                        self.shell_marker();
                        self.mode = 0;
                    } else {
                        self.osc.clear();
                        self.mode = 5;
                    }
                }
                5 => {
                    if b == 0x1b {
                        self.mode = 6;
                    } else if b == 7 {
                        self.mode = 0;
                    }
                }
                _ => {
                    self.mode = if b == b'\\' || b == 7 { 0 } else { 5 };
                }
            }
        }
        if printable {
            self.output_seq = self.output_seq.saturating_add(1);
            self.last_output = Some(now);
            self.saw_work = true;
            // Explicit integrations own their state until another report/input/marker.
            if !matches!(self.source, ActivitySource::Report | ActivitySource::Shell) {
                self.set(ActivityState::Working, ActivitySource::Output);
            }
        }
    }
    pub(crate) fn snapshot(&mut self, now: Instant) -> ActivitySnapshot {
        if !matches!(self.source, ActivitySource::Report | ActivitySource::Shell) {
            let quiet_for = self.last_output.map(|t| now.saturating_duration_since(t));
            if quiet_for.is_some_and(|d| d >= Duration::from_millis(500)) {
                let text = String::from_utf8_lossy(&self.line);
                let text = text.trim();
                let lower = text.to_ascii_lowercase();
                let waiting = lower.ends_with("[y/n]")
                    || lower.ends_with("(y/n)")
                    || lower.ends_with("[y/n]:")
                    || lower.ends_with("(y/n):")
                    || lower.contains("do you want to proceed?")
                    || lower.contains("press enter to continue");
                let ready = self.saw_work
                    && (text == "❯"
                        || text == "›"
                        || text.starts_with("❯ ")
                        || text.starts_with("› "));
                if waiting {
                    self.set(ActivityState::NeedsInput, ActivitySource::Heuristic);
                } else if ready {
                    self.set(ActivityState::Completed, ActivitySource::Heuristic);
                } else if quiet_for.is_some_and(|d| d >= Duration::from_secs(2)) {
                    self.set(ActivityState::Quiet, ActivitySource::Output);
                }
            }
        }
        ActivitySnapshot {
            state: self.state,
            source: self.source,
            output_seq: self.output_seq,
            state_seq: self.state_seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_is_not_completion() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        a.output(b"thinking", now);
        assert_eq!(a.snapshot(now).state, ActivityState::Working);
        assert_eq!(
            a.snapshot(now + Duration::from_secs(3)).state,
            ActivityState::Quiet
        );
    }
    #[test]
    fn split_shell_markers_and_cosmetic_output() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        for b in b"\x1b]133;C\x07hello\x1b]133;D;0\x1b\\$ " {
            a.output(&[*b], now);
        }
        let s = a.snapshot(now);
        assert_eq!(s.state, ActivityState::Completed);
        assert_eq!(s.source, ActivitySource::Shell);
        a.input(b"next\r");
        assert_eq!(a.snapshot(now).state, ActivityState::Working);
    }
    #[test]
    fn input_prompt_is_heuristic_and_cleared_on_input() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        a.output(b"Allow command? (y/n)", now);
        let s = a.snapshot(now + Duration::from_secs(1));
        assert_eq!(s.state, ActivityState::NeedsInput);
        assert_eq!(s.source, ActivitySource::Heuristic);
        a.input(b"y");
        assert_ne!(
            a.snapshot(now + Duration::from_secs(4)).state,
            ActivityState::NeedsInput
        );
    }
    #[test]
    fn ready_prompt_is_an_estimate_not_a_report() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        a.output("answer\r\n❯ ".as_bytes(), now);
        let s = a.snapshot(now + Duration::from_secs(1));
        assert_eq!(s.state, ActivityState::Completed);
        assert_eq!(s.source, ActivitySource::Heuristic);
        let sequence = s.state_seq;
        assert_eq!(a.snapshot(now + Duration::from_secs(2)).state_seq, sequence);
    }
    #[test]
    fn reports_survive_output_and_repeated_completions_have_new_sequences() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        a.report(ActivityState::Completed);
        let first = a.snapshot(now).state_seq;
        a.output(b"prompt redraw", now);
        assert_eq!(a.snapshot(now).state, ActivityState::Completed);
        a.report(ActivityState::Completed);
        assert!(a.snapshot(now).state_seq > first);
    }
    #[test]
    fn escape_payloads_do_not_become_prompts_or_unread_output() {
        let now = Instant::now();
        let mut a = ActivityTracker::default();
        a.output(b"\x1b]52;c;Do you want to proceed?\x07\x1b[31m", now);
        assert_eq!(a.snapshot(now).output_seq, 0);
        a.output(&[b'\x1b', b']'], now);
        a.output(&vec![b'a'; 10000], now);
        a.output(b"\x07", now);
        assert!(a.osc.len() <= 256);
        assert!(a.line.len() <= 1024);
        a.output(b"visible", now);
        assert_eq!(a.snapshot(now).output_seq, 1);
    }
}
