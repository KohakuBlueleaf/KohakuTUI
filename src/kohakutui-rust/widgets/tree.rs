//! `tree`: an expandable outline (files, a graph), with lazily loaded children.

use std::any::Any;

use crossterm::event::KeyCode;
use ratatui::text::{Line, Span};

use super::props::upd_usize;
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get, map};
use crate::proto::Value;
use crate::text::{clip, theme};

/// One row of a [`Tree`]: its depth, label, value and whether it can open.
#[derive(Debug, Clone)]
struct Row {
    depth: usize,
    label: String,
    detail: Option<String>,
    icon: Option<String>,
    value: String,
    /// None: a leaf; Some(open).
    open: Option<bool>,
    tone: Option<String>,
}

/// An outline: items `[{label, value, detail?, icon?, tone?, children?, open?}]`. ↑↓ move, →
/// opens (or steps in), ← closes (or steps out), enter selects (`select` with the value); a
/// click selects, a click on ▸/▾ toggles. An item with `children: true` (no list) can open
/// and reports `change` with `{"expand": value}` so the app can load its children.
#[derive(Debug)]
pub struct Tree {
    items: Value,
    /// Values the user opened or closed, overriding `open` in the items.
    toggled: std::collections::HashMap<String, bool>,
    cursor: usize,
    height: usize,
    top: std::cell::Cell<usize>,
    /// The first row on the last draw (clicks map from it).
    drawn_top: std::cell::Cell<usize>,
}

impl Default for Tree {
    fn default() -> Self {
        Tree {
            items: Value::Array(Vec::new()),
            toggled: std::collections::HashMap::new(),
            cursor: 0,
            height: 0,
            top: std::cell::Cell::new(0),
            drawn_top: std::cell::Cell::new(0),
        }
    }
}

impl Tree {
    fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        fn walk(t: &Tree, items: &Value, depth: usize, out: &mut Vec<Row>) {
            for item in items.as_array().into_iter().flatten() {
                let text = |k: &str| get(item, k).filter(|v| !v.is_nil()).map(display);
                let label = text("label").unwrap_or_else(|| display(item));
                let value = text("value").unwrap_or_else(|| label.clone());
                let kids = get(item, "children");
                let has = kids.is_some_and(|k| {
                    k.as_bool().unwrap_or(false) || k.as_array().is_some_and(|a| !a.is_empty())
                });
                let default_open = get(item, "open").and_then(Value::as_bool).unwrap_or(false);
                let open = has.then(|| *t.toggled.get(&value).unwrap_or(&default_open));
                out.push(Row {
                    depth,
                    label,
                    detail: text("detail"),
                    icon: text("icon"),
                    value,
                    open,
                    tone: text("tone"),
                });
                if open == Some(true) {
                    if let Some(k) = kids.filter(|k| k.as_array().is_some()) {
                        walk(t, k, depth + 1, out);
                    }
                }
            }
        }
        walk(self, &self.items, 0, &mut out);
        out
    }

    fn set_open(&mut self, row: &Row, open: bool, out: &mut Vec<Emit>) {
        self.toggled.insert(row.value.clone(), open);
        if open {
            out.push(Emit::Change(map(vec![("expand", row.value.as_str().into())])));
        }
    }

    fn reveal_cursor(&self) {
        if self.height == 0 {
            return;
        }
        let top = self.top.get();
        if self.cursor < top {
            self.top.set(self.cursor);
        } else if self.cursor >= top + self.height {
            self.top.set(self.cursor + 1 - self.height);
        }
    }

    fn select(&mut self, rows: &[Row], out: &mut Vec<Emit>) {
        if let Some(r) = rows.get(self.cursor) {
            out.push(Emit::Select { index: self.cursor as u32, value: r.value.as_str().into() });
        }
    }
}

impl Component for Tree {
    fn kind(&self) -> &'static str {
        "tree"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        let previous_cursor = self.cursor;
        if let Some(v) = get(p, "items") {
            self.items = v.clone();
        }
        upd_usize(p, "height", &mut self.height, 0);
        if let Some(v) = get(p, "selected").filter(|v| !v.is_nil()) {
            let want = display(v);
            if let Some(i) = self.rows().iter().position(|r| r.value == want) {
                self.cursor = i;
            }
        }
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
        if previous_cursor != self.cursor {
            self.reveal_cursor();
        }
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        let rows = self.rows();
        if rows.is_empty() {
            return vec![Line::from(Span::styled("(empty)", theme::faint()))];
        }
        let (start, end) = if self.height > 0 && rows.len() > self.height {
            let top = self.top.get().min(rows.len() - self.height);
            self.top.set(top);
            (top, top + self.height)
        } else {
            (0, rows.len())
        };
        self.drawn_top.set(start);
        rows[start..end]
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let at = start + i;
                let mark = match r.open {
                    Some(true) => "▾ ",
                    Some(false) => "▸ ",
                    None => "  ",
                };
                let tone = r.tone.as_deref().map(theme::tone).unwrap_or_default();
                let mut spans = vec![
                    Span::raw("  ".repeat(r.depth)),
                    Span::styled(mark, theme::faint()),
                ];
                if let Some(icon) = &r.icon {
                    spans.push(Span::styled(format!("{icon} "), tone));
                }
                spans.push(Span::styled(r.label.clone(), tone));
                if let Some(d) = &r.detail {
                    spans.push(Span::styled(format!("  {d}"), theme::muted()));
                }
                let mut line = clip(Line::from(spans), width as usize);
                if at == self.cursor && ctx.focused {
                    line = line.style(theme::selection());
                } else if at == self.cursor {
                    line = line.style(theme::focus_area());
                }
                line
            })
            .collect()
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let Input::Key { code, .. } = input else {
            return false;
        };
        let rows = self.rows();
        let Some(row) = rows.get(self.cursor).cloned() else {
            return false;
        };
        match code {
            KeyCode::Up if self.cursor > 0 => self.cursor -= 1,
            KeyCode::Down if self.cursor + 1 < rows.len() => self.cursor += 1,
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = rows.len() - 1,
            KeyCode::Right if row.open == Some(false) => self.set_open(&row, true, out),
            KeyCode::Right if row.open == Some(true) && self.cursor + 1 < rows.len() => {
                self.cursor += 1;
            }
            KeyCode::Left if row.open == Some(true) => self.set_open(&row, false, out),
            KeyCode::Left if row.depth > 0 => {
                // step out to the parent row
                if let Some(p) = rows[..self.cursor]
                    .iter()
                    .rposition(|r| r.depth < row.depth)
                {
                    self.cursor = p;
                }
            }
            KeyCode::Enter => self.select(&rows, out),
            KeyCode::Char(' ') if row.open.is_some() => {
                self.set_open(&row, row.open != Some(true), out)
            }
            _ => return false,
        }
        self.reveal_cursor();
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        let rows = self.rows();
        match m.act {
            MouseAct::ScrollUp | MouseAct::ScrollDown => {
                if self.height == 0 {
                    return false;
                }
                let top = self.top.get();
                self.top.set(if m.act == MouseAct::ScrollUp {
                    top.saturating_sub(3)
                } else {
                    (top + 3).min(rows.len().saturating_sub(self.height))
                });
                return true;
            }
            MouseAct::Click => {}
            _ => return false,
        }
        let at = self.drawn_top.get() + m.row as usize;
        let Some(row) = rows.get(at).cloned() else {
            return false;
        };
        self.cursor = at;
        let mark = row.depth * 2;
        if row.open.is_some() && (mark..mark + 2).contains(&(m.col as usize)) {
            self.set_open(&row, row.open != Some(true), out);
        } else {
            self.select(&rows, out);
        }
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
