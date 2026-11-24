//! Paragraph indices and original parenthesis folding; no second SQL unless folded.
use std::{collections::BTreeSet, ops::Range};
#[derive(Default)]
pub struct Viewer {
    pub key: Option<(u64, bool)>,
    pub lines: Vec<Range<usize>>,
    pub folds: Vec<(usize, usize)>,
    pub collapsed: BTreeSet<usize>,
    pub visible: Vec<usize>,
    pub text: Option<String>,
}
impl Viewer {
    pub fn reset(&mut self, source: &str, key: (u64, bool)) {
        self.key = Some(key);
        self.lines.clear();
        self.folds.clear();
        self.collapsed.clear();
        self.text = None;
        let mut start = 0;
        for (at, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                self.lines.push(start..at);
                start = at + 1;
            }
        }
        self.lines.push(start..source.len());
        let mut stack = Vec::new();
        for (line, range) in self.lines.iter().enumerate() {
            let mut quoted = false;
            let bytes = source[range.clone()].as_bytes();
            for (at, byte) in bytes.iter().copied().enumerate() {
                if quoted {
                    if byte == b'\'' {
                        quoted = false;
                    }
                    continue;
                }
                if byte == b'\'' {
                    quoted = true;
                    continue;
                }
                if byte == b'-' && bytes.get(at + 1) == Some(&b'-') {
                    break;
                }
                if byte == b'(' {
                    stack.push(line);
                } else if byte == b')'
                    && let Some(start) = stack.pop()
                    && line > start
                {
                    self.folds.push((start, line));
                }
            }
        }
        self.visible.clear();
        self.visible.extend(0..self.lines.len());
    }
    pub fn toggle(&mut self, source: &str, line: usize) {
        if !self.collapsed.remove(&line) {
            self.collapsed.insert(line);
        }
        self.visible.clear();
        let mut hidden_until = None;
        for line in 0..self.lines.len() {
            if hidden_until.is_some_and(|end| line <= end) {
                continue;
            }
            self.visible.push(line);
            if self.collapsed.contains(&line) {
                hidden_until = self
                    .folds
                    .iter()
                    .find(|(start, _)| *start == line)
                    .map(|(_, end)| *end);
            }
        }
        self.text = if self.collapsed.is_empty() {
            None
        } else {
            let mut text = String::with_capacity(
                self.visible
                    .iter()
                    .map(|line| self.lines[*line].len() + 5)
                    .sum(),
            );
            for line in &self.visible {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&source[self.lines[*line].clone()]);
                if self.collapsed.contains(line) {
                    text.push_str(" ...");
                }
            }
            Some(text)
        };
    }
}
