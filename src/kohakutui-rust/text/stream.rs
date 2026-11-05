//! Streaming Markdown source: which prefix is stable (renders the same whatever comes next).
//!
//! A line is stable once its newline arrives, except table rows, which are held back until the
//! table ends because their column widths depend on every row.

use super::markdown::render;
use super::wrap::wrap_text;
use super::MdLine;
use ratatui::style::Style;
use ratatui::text::Line;

/// Accumulates deltas and tracks the stable prefix `src[..stable_end]`.
#[derive(Debug, Default, Clone)]
pub struct MdStream {
    src: String,
    stable_end: usize,
    scanned: usize,
    in_fence: bool,
    table_start: Option<usize>,
    finished: bool,
}

impl MdStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// A finished stream over complete source.
    pub fn complete(src: &str) -> Self {
        let mut s = Self::new();
        s.push(src);
        s.finish();
        s
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    pub fn stable_end(&self) -> usize {
        self.stable_end
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn push(&mut self, delta: &str) {
        if self.finished || delta.is_empty() {
            return;
        }
        self.src.push_str(delta);
        while let Some(nl) = self.src[self.scanned..].find('\n') {
            let start = self.scanned;
            let end = start + nl + 1;
            self.scan_line(start, end);
            self.scanned = end;
        }
        let end = self.table_start.unwrap_or(self.scanned);
        self.stable_end = self.stable_end.max(end);
    }

    fn scan_line(&mut self, start: usize, end: usize) {
        let line = self.src[start..end].trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            self.in_fence = !self.in_fence;
            self.table_start = None;
        } else if !self.in_fence && line.starts_with('|') {
            self.table_start.get_or_insert(start);
        } else {
            self.table_start = None;
        }
    }

    /// Ends the stream: everything is stable.
    pub fn finish(&mut self) {
        if !self.src.is_empty() && !self.src.ends_with('\n') {
            self.src.push('\n');
        }
        self.finished = true;
        self.table_start = None;
        self.scanned = self.src.len();
        self.stable_end = self.src.len();
    }

    /// The stable prefix rendered as Markdown.
    pub fn stable_lines(&self, width: usize) -> Vec<MdLine> {
        render(&self.src[..self.stable_end], width)
    }

    /// The unstable tail rendered as plain wrapped text.
    pub fn pending_lines(&self, width: usize) -> Vec<Line<'static>> {
        let tail = self.src[self.stable_end..].trim_end_matches('\n');
        if tail.trim().is_empty() {
            return Vec::new();
        }
        wrap_text(tail, Style::default(), width.max(1))
    }
}
