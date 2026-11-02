//! Turning a terminal byte stream into plain lines for logs and prompt detection.

/// Removes ANSI escape sequences and control characters from a byte stream,
/// keeping state across chunks so a sequence split between reads is still removed.
#[derive(Debug, Default)]
pub struct Stripper {
    state: State,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

impl Stripper {
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            self.state = match self.state {
                State::Normal => match b {
                    0x1b => State::Esc,
                    b'\n' | b'\r' | b'\t' => {
                        out.push(b);
                        State::Normal
                    }
                    0x00..=0x1f | 0x7f => State::Normal,
                    _ => {
                        out.push(b);
                        State::Normal
                    }
                },
                State::Esc => match b {
                    b'[' => State::Csi,
                    b']' | b'P' | b'_' | b'^' => State::Osc,
                    _ => State::Normal,
                },
                State::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        State::Normal
                    } else {
                        State::Csi
                    }
                }
                State::Osc => match b {
                    0x07 => State::Normal,
                    0x1b => State::OscEsc,
                    _ => State::Osc,
                },
                State::OscEsc => {
                    if b == b'\\' {
                        State::Normal
                    } else {
                        State::Osc
                    }
                }
            };
        }
    }
}

/// Splits stripped output into lines. A lone carriage return discards the
/// line so far, the way a terminal overwrites a spinner.
#[derive(Debug, Default)]
pub struct LineBuffer {
    current: Vec<u8>,
    pending_cr: bool,
}

const MAX_LINE: usize = 4096;

impl LineBuffer {
    pub fn feed(&mut self, input: &[u8], lines: &mut Vec<String>) {
        for &b in input {
            if self.pending_cr {
                self.pending_cr = false;
                if b != b'\n' {
                    self.current.clear();
                }
            }
            match b {
                b'\n' => lines.push(self.take()),
                b'\r' => self.pending_cr = true,
                _ => {
                    self.current.push(b);
                    if self.current.len() >= MAX_LINE {
                        lines.push(self.take());
                    }
                }
            }
        }
    }

    /// The unfinished line, such as a `[y/N]` prompt waiting for an answer.
    pub fn partial(&self) -> String {
        String::from_utf8_lossy(&self.current).into_owned()
    }

    pub fn take(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.current)
            .trim_end()
            .to_string();
        self.current.clear();
        line
    }
}

/// One-shot helper: plain text of a whole buffer.
pub fn strip(input: &str) -> String {
    let mut out = Vec::new();
    Stripper::default().feed(input.as_bytes(), &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colours_and_titles() {
        assert_eq!(strip("\x1b[1;32mok\x1b[0m done"), "ok done");
        assert_eq!(strip("\x1b]0;title\x07after"), "after");
        assert_eq!(strip("\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"), "link");
    }

    #[test]
    fn sequences_split_across_chunks() {
        let mut s = Stripper::default();
        let mut out = Vec::new();
        s.feed(b"a\x1b[3", &mut out);
        s.feed(b"1mb", &mut out);
        assert_eq!(out, b"ab");
    }

    #[test]
    fn lines_and_overwrites() {
        let mut lb = LineBuffer::default();
        let mut lines = Vec::new();
        lb.feed(b"one\r\nspin 1\rspin 2\rdone\nprompt [y/N] ", &mut lines);
        assert_eq!(lines, ["one", "done"]);
        assert_eq!(lb.partial(), "prompt [y/N] ");
    }
}
