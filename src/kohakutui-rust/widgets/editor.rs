//! Multi-line text buffer with a grapheme-safe caret, shared by every text input.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use crate::text::display_width;

/// Text plus a caret at a byte offset that always sits on a grapheme boundary.
#[derive(Debug, Default, Clone)]
pub struct TextEditor {
    text: String,
    cursor: usize,
}

impl TextEditor {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, text: &str) {
        self.text = normalize(text);
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, s: &str) {
        let s = normalize(s);
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, str::len)
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let start = self.prev_boundary();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        true
    }

    pub fn delete(&mut self) -> bool {
        let end = self.next_boundary();
        if end == self.cursor {
            return false;
        }
        self.text.replace_range(self.cursor..end, "");
        true
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary();
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary();
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    /// Moves to the line above at the same display column; false on the first line.
    pub fn up(&mut self) -> bool {
        let start = self.line_start();
        if start == 0 {
            return false;
        }
        let col = display_width(&self.text[start..self.cursor]);
        let prev_end = start - 1;
        let prev_start = self.text[..prev_end].rfind('\n').map_or(0, |i| i + 1);
        self.cursor = at_column(&self.text, prev_start, prev_end, col);
        true
    }

    /// Moves to the line below at the same display column; false on the last line.
    pub fn down(&mut self) -> bool {
        let end = self.line_end();
        if end == self.text.len() {
            return false;
        }

        let col = display_width(&self.text[self.line_start()..self.cursor]);
        let next_start = end + 1;
        let next_end = self.text[next_start..]
            .find('\n')
            .map_or(self.text.len(), |i| next_start + i);
        self.cursor = at_column(&self.text, next_start, next_end, col);
        true
    }

    /// Deletes the word before the caret (and the spaces after it).
    pub fn delete_word_back(&mut self) {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end_matches(' ');
        let start = trimmed.rfind([' ', '\n']).map_or(0, |i| i + 1);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Replaces the `n` bytes before the caret with `s` (a completed word, a pasted block).
    pub fn replace_before(&mut self, n: usize, s: &str) {
        let start = self.cursor.saturating_sub(n);
        let s = normalize(s);
        self.text.replace_range(start..self.cursor, &s);
        self.cursor = start + s.len();
    }

    /// The text before the caret.
    pub fn before(&self) -> &str {
        &self.text[..self.cursor]
    }

    pub fn kill_to_line_start(&mut self) {
        let start = self.line_start();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Lines wrapped by grapheme at `width`, and the caret as (row, col).
    pub fn render(&self, width: usize, style: Style) -> (Vec<Line<'static>>, (u16, u16)) {
        let width = width.max(1);
        let mut rows: Vec<String> = Vec::new();
        let mut caret = (0usize, 0usize);
        let mut offset = 0;
        for logical in self.text.split('\n') {
            let mut row = String::new();
            let mut w = 0;
            for (i, g) in logical.grapheme_indices(true) {
                let gw = display_width(g);
                if w + gw > width && w > 0 {
                    rows.push(std::mem::take(&mut row));
                    w = 0;
                }
                if offset + i == self.cursor {
                    caret = (rows.len(), w);
                }
                row.push_str(g);
                w += gw;
            }
            if offset + logical.len() == self.cursor {
                if w >= width {
                    rows.push(std::mem::take(&mut row));
                    w = 0;
                }
                caret = (rows.len(), w);
            }
            rows.push(row);
            offset += logical.len() + 1;
        }
        let lines = rows
            .into_iter()
            .map(|r| Line::from(Span::styled(r, style)))
            .collect();
        (lines, (caret.0 as u16, caret.1 as u16))
    }
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Byte offset in `text[start..end]` closest to display column `col`.
fn at_column(text: &str, start: usize, end: usize, col: usize) -> usize {
    let mut w = 0;
    for (i, g) in text[start..end].grapheme_indices(true) {
        if w >= col {
            return start + i;
        }
        w += display_width(g);
    }
    end
}
