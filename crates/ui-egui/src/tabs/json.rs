//! Compact indexes into the output buffer. Folding never duplicates the document.
use std::collections::HashSet;
pub mod feedback;
pub mod long_line;
pub mod selection;
const CLOSE: u32 = 1 << 31;
#[derive(Clone, Copy)]
pub struct Node {
    pub line: u32,
    pub last_line: u32,
    pub start: u32,
    pub end: u32,
    pub count: u32,
    pub array: bool,
}
#[derive(Default)]
pub struct JsonView {
    pub viewport: Option<Viewport>,
    pub selection: selection::Selection,
    pub long_lines: std::collections::HashMap<u32, long_line::LongLine>,
    pub metrics_scale: f32,
    starts: Vec<u32>,
    nodes: Vec<Node>,
    pub visible: Vec<u32>,
    collapsed: bool,
    tree: bool,
    exceptions: HashSet<u32>,
}
struct Open {
    node: Node,
    content: bool,
    commas: u32,
}
impl JsonView {
    pub fn is_structured(&self) -> bool {
        self.tree
    }
    pub fn reset(&mut self, text: &str, tree: bool) {
        if text.is_empty() {
            *self = Self::default();
            return;
        }
        self.viewport = None;
        self.selection = Default::default();
        self.tree = tree;
        self.long_lines.clear();
        self.starts.clear();
        self.nodes.clear();
        self.exceptions.clear();
        self.collapsed = false;
        self.starts.push(0);
        self.starts
            .extend(text.match_indices('\n').filter_map(|(index, _)| {
                (index + 1 < text.len() || !tree).then_some((index + 1) as u32)
            }));
        if !tree {
            self.visible = (0..self.starts.len() as u32).collect();
            return;
        }
        let mut stack: Vec<Open> = Vec::new();
        let (mut line, mut quoted, mut escaped) = (0, false, false);
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line += 1;
            }
            if quoted {
                if byte == b'"' && !escaped {
                    quoted = false;
                }
                escaped = byte == b'\\' && !escaped;
                continue;
            }
            match byte {
                b'"' => {
                    quoted = true;
                    if let Some(top) = stack.last_mut() {
                        top.content = true;
                    }
                }
                b'{' | b'[' => {
                    if let Some(top) = stack.last_mut() {
                        top.content = true;
                    }
                    // Malformed non-JSON output cannot cause an unbounded parser stack.
                    if stack.len() == 512 {
                        self.nodes.clear();
                        break;
                    }
                    stack.push(Open {
                        node: Node {
                            line,
                            last_line: line,
                            start: index as u32,
                            end: 0,
                            count: 0,
                            array: byte == b'[',
                        },
                        content: false,
                        commas: 0,
                    });
                }
                b'}' | b']' => {
                    if let Some(mut top) = stack.pop() {
                        if top.node.array != (byte == b']') {
                            self.nodes.clear();
                            stack.clear();
                            break;
                        }
                        top.node.last_line = line;
                        top.node.end = (index + 1) as u32;
                        top.node.count = if top.content { top.commas + 1 } else { 0 };
                        self.nodes.push(top.node);
                    }
                }
                b',' => {
                    if let Some(top) = stack.last_mut() {
                        top.commas += 1;
                    }
                }
                b' ' | b'\t' | b'\r' | b'\n' => {}
                _ => {
                    if let Some(top) = stack.last_mut() {
                        top.content = true;
                    }
                }
            }
        }
        if !stack.is_empty() || quoted {
            self.nodes.clear();
        }
        // One graphic per paragraph; inline nested containers share their outer graphic.
        self.nodes.sort_unstable_by_key(|n| (n.line, n.start));
        self.nodes.dedup_by_key(|n| n.line);
        self.rebuild_visible();
    }
    pub fn node(&self, line: u32) -> Option<&Node> {
        self.nodes
            .binary_search_by_key(&line, |n| n.line)
            .ok()
            .map(|index| &self.nodes[index])
    }
    pub fn folded(&self, line: u32) -> bool {
        self.collapsed != self.exceptions.contains(&line)
    }
    pub fn all(&mut self, collapsed: bool) {
        self.selection = Default::default();
        self.long_lines.clear();
        self.collapsed = collapsed;
        self.exceptions.clear();
        self.rebuild_visible();
    }
    pub fn toggle(&mut self, line: u32) {
        self.selection = Default::default();
        self.long_lines.clear();
        if !self.exceptions.insert(line) {
            self.exceptions.remove(&line);
        }
        self.rebuild_visible();
    }
    fn rebuild_visible(&mut self) {
        self.visible.clear();
        let mut line = 0u32;
        let mut node = 0;
        while (line as usize) < self.starts.len() {
            self.visible.push(line);
            while self.nodes.get(node).is_some_and(|n| n.line < line) {
                node += 1;
            }
            if self.tree
                && let Some(n) = self.nodes.get(node)
                && n.line == line
                && self.folded(line)
            {
                line = n.last_line + 1;
            } else {
                if self.tree
                    && let Some(n) = self.nodes.get(node)
                    && n.line == line
                    && n.last_line == line
                    && n.count == 0
                {
                    self.visible.push(line | CLOSE);
                }
                line += 1;
            }
        }
    }
    pub fn line<'a>(&self, text: &'a str, line: u32) -> &'a str {
        let line = line & !CLOSE;
        let start = self.starts[line as usize] as usize;
        let end = self
            .starts
            .get(line as usize + 1)
            .map_or(text.len(), |i| *i as usize);
        text[start..end].trim_end_matches(['\n', '\r'])
    }
    pub fn display(&self, text: &str, line: u32) -> Option<String> {
        if !self.tree {
            return None;
        }
        let closing = line & CLOSE != 0;
        let line = line & !CLOSE;
        let node = self.node(line)?;
        let empty_inline = node.count == 0 && node.line == node.last_line;
        if !closing && !self.folded(line) && !empty_inline {
            return None;
        }
        let begin = self.starts[line as usize] as usize;
        let suffix_end = self
            .starts
            .get(node.last_line as usize + 1)
            .map_or(text.len(), |i| *i as usize);
        let suffix = text[node.end as usize..suffix_end].trim_end_matches(['\n', '\r']);
        if closing {
            let indent: String = self
                .line(text, line)
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            return Some(format!(
                "{indent}{}{suffix}",
                if node.array { ']' } else { '}' }
            ));
        }
        if !self.folded(line) && empty_inline {
            return Some(text[begin..node.start as usize + 1].to_owned());
        }
        let noun = if node.array {
            if node.count == 1 { "item" } else { "items" }
        } else if node.count == 1 {
            "field"
        } else {
            "fields"
        };
        Some(format!(
            "{} /* {} {noun} */ {}{suffix}",
            &text[begin..node.start as usize + 1],
            node.count,
            if node.array { ']' } else { '}' }
        ))
    }
    pub fn copy(&self, text: &str, line: u32) -> Option<String> {
        let node = self.node(line)?;
        tomato_core::tools::json_transform(&text[node.start as usize..node.end as usize], "format")
            .ok()
    }
    pub fn visible_text(&self, text: &str) -> String {
        let mut result = String::new();
        for (index, &line) in self.visible.iter().enumerate() {
            if index > 0 {
                result.push('\n');
            }
            if let Some(display) = self.display(text, line) {
                result.push_str(&display);
            } else {
                result.push_str(self.line(text, line));
            }
        }
        result
    }
    pub fn visible_line<'a>(&self, text: &'a str, row: usize) -> std::borrow::Cow<'a, str> {
        let Some(&line) = self.visible.get(row) else {
            return std::borrow::Cow::Borrowed("");
        };
        self.display(text, line)
            .map(std::borrow::Cow::Owned)
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(self.line(text, line)))
    }
    pub fn selected_text(&self, text: &str, selection: &selection::Selection) -> Option<String> {
        let (start, end) = selection.range()?;
        let mut result = String::new();
        for row in start.row..=end.row.min(self.visible.len().saturating_sub(1)) {
            if row > start.row {
                result.push('\n');
            }
            let line = self.visible_line(text, row);
            let left = if row == start.row {
                start.byte.min(line.len())
            } else {
                0
            };
            let right = if row == end.row {
                end.byte.min(line.len())
            } else {
                line.len()
            };
            result.push_str(&line[selection::floor(&line, left)..selection::floor(&line, right)]);
        }
        Some(result)
    }
}
#[derive(Clone, Copy)]
pub struct Viewport {
    pub rect: egui::Rect,
    pub origin: egui::Pos2,
    pub line_height: f32,
    pub gutter: f32,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_output_does_not_allocate_unused_container_indexes() {
        let text = "[{},[],{\"nested\":[{}]}]";
        let mut view = JsonView::default();
        view.reset(text, false);
        assert!(view.nodes.is_empty());
        assert_eq!(view.visible_text(text), text);
        view.reset(text, true);
        assert!(!view.nodes.is_empty());
    }
    #[test]
    fn fold_keeps_nested_state_handles_escaped_brackets_and_copies_original_node() {
        let text = "{\n  \"值\": [\n    {\n      \"括号\": \"[ } \\\"\",\n      \"ok\": true\n    }\n  ],\n  \"empty\": {}\n}";
        let mut view = JsonView::default();
        view.reset(text, true);
        assert_eq!(view.visible.len(), 10);
        assert_eq!(view.node(1).map(|n| n.count), Some(1));
        view.toggle(1);
        assert_eq!(view.visible, vec![0, 1, 7, 7 | CLOSE, 8]);
        assert_eq!(
            view.display(text, 1).as_deref(),
            Some("  \"值\": [ /* 1 item */ ],")
        );
        let copied: serde_json::Value =
            serde_json::from_str(&view.copy(text, 1).expect("array node")).expect("valid JSON");
        assert_eq!(copied[0]["ok"], true);
        view.toggle(0);
        assert_eq!(view.visible, vec![0]);
        view.toggle(0);
        assert_eq!(view.visible, vec![0, 1, 7, 7 | CLOSE, 8]);
        view.all(false);
        assert_eq!(view.visible.len(), 10);
        view.all(true);
        assert_eq!(view.visible, vec![0]);
        view.toggle(0);
        assert_eq!(view.visible, vec![0, 1, 7, 8]);
        view.reset("[]", true);
        assert_eq!(view.visible_text("[]"), "[\n]");
        assert_eq!(view.node(0).map(|n| n.count), Some(0));
    }
}
