//! Greedy word wrap over styled spans by display width: words break at spaces, wide (CJK)
//! characters break anywhere, and a word longer than the width is split by grapheme.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display width of a string in terminal columns.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// The unstyled text of a line.
pub fn plain(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// A word, a single wide grapheme, or a run of spaces; pieces keep their styles.
struct Atom {
    pieces: Vec<(String, Style)>,
    width: usize,
    space: bool,
}

fn atoms(spans: &[Span<'_>]) -> Vec<Atom> {
    let mut out: Vec<Atom> = Vec::new();
    for span in spans {
        let style = span.style;
        for g in span.content.graphemes(true) {
            let w = display_width(g);
            let space = g.chars().all(char::is_whitespace);
            let standalone = w > 1;
            let joins = match out.last() {
                Some(last) => {
                    last.space == space && !standalone && !(last.width > 0 && is_wide(last))
                }
                None => false,
            };
            if joins {
                let last = out.last_mut().unwrap();
                match last.pieces.last_mut() {
                    Some((text, s)) if *s == style => text.push_str(g),
                    _ => last.pieces.push((g.to_owned(), style)),
                }
                last.width += w;
            } else {
                out.push(Atom { pieces: vec![(g.to_owned(), style)], width: w, space });
            }
        }
    }
    out
}

fn is_wide(atom: &Atom) -> bool {
    atom.pieces.len() == 1 && display_width(&atom.pieces[0].0) > 1
}

#[derive(Default)]
struct Builder {
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    width: usize,
}

impl Builder {
    fn push(&mut self, text: &str, style: Style, w: usize) {
        match self.cur.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => self.cur.push(Span::styled(text.to_owned(), style)),
        }
        self.width += w;
    }

    fn push_atom(&mut self, atom: &Atom) {
        for (text, style) in &atom.pieces {
            self.push(text, *style, display_width(text));
        }
    }

    fn break_line(&mut self) {
        while let Some(last) = self.cur.last_mut() {
            let trimmed = last.content.trim_end().len();
            if trimmed == last.content.len() {
                break;
            }
            last.content.to_mut().truncate(trimmed);
            if last.content.is_empty() {
                self.cur.pop();
            } else {
                break;
            }
        }
        self.lines.push(Line::from(std::mem::take(&mut self.cur)));
        self.width = 0;
    }
}

/// Wraps one logical line of spans to `width` columns. An empty input gives one empty line.
pub fn wrap_spans(spans: &[Span<'_>], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut b = Builder::default();
    let mut started = false;
    for atom in atoms(spans) {
        if atom.space {
            if b.width + atom.width <= width {
                b.push_atom(&atom);
            } else if b.width > 0 {
                b.break_line();
            }
            started = true;
            continue;
        }
        if b.width + atom.width <= width {
            b.push_atom(&atom);
        } else if atom.width <= width {
            b.break_line();
            b.push_atom(&atom);
        } else {
            for (text, style) in &atom.pieces {
                for g in text.graphemes(true) {
                    let w = display_width(g);
                    if b.width + w > width && b.width > 0 {
                        b.break_line();
                    }
                    b.push(g, *style, w);
                }
            }
        }
        started = true;
    }
    if !b.cur.is_empty() || !started || b.lines.is_empty() {
        b.lines.push(Line::from(std::mem::take(&mut b.cur)));
    }
    b.lines
}

/// Cuts a line to `width` columns, keeping styles; a wide grapheme that would straddle the
/// edge is dropped.
pub fn clip(line: Line<'static>, width: usize) -> Line<'static> {
    let mut used = 0;
    let mut spans = Vec::new();

    for span in line.spans {
        if used >= width {
            break;
        }
        if used + display_width(&span.content) <= width {
            used += display_width(&span.content);
            spans.push(span);
            continue;
        }

        let mut text = String::new();
        for g in span.content.graphemes(true) {
            let w = display_width(g);
            if used + w > width {
                break;
            }
            text.push_str(g);
            used += w;
        }
        spans.push(Span::styled(text, span.style));
        break;
    }

    Line::from(spans).style(line.style)
}

/// Wraps text that may contain newlines, one style for all of it.
pub fn wrap_text(text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    text.split('\n')
        .flat_map(|l| wrap_spans(&[Span::styled(l.to_owned(), style)], width))
        .collect()
}
