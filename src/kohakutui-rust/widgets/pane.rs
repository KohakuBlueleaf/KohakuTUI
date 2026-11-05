//! Full-screen chrome: `pane` (a resizable side column) and `tabstrip` (a row of tabs).

use std::any::Any;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_usize};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get, num};
use crate::proto::Value;
use crate::text::{clip, display_width, theme};

/// A side column in full screen: `side` left or right, `width` columns (the user drags its
/// divider), `collapsed` to a one-column strip. The view lays it out and reports resizes.
#[derive(Debug)]
pub struct Pane {
    pub side_left: bool,
    pub width: usize,
    pub min: usize,
    pub collapsed: bool,
    pub title: Option<String>,
}

impl Default for Pane {
    fn default() -> Self {
        Pane { side_left: false, width: 32, min: 16, collapsed: false, title: None }
    }
}

impl Component for Pane {
    fn kind(&self) -> &'static str {
        "pane"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        if let Some(v) = get(p, "side") {
            self.side_left = display(v) == "left";
        }
        upd_usize(p, "width", &mut self.width, 32);
        upd_usize(p, "min", &mut self.min, 16);
        self.width = self.width.max(self.min);
        if let Some(v) = get(p, "collapsed") {
            self.collapsed = v.as_bool().unwrap_or(false);
        }
        upd_opt_str(p, "title", &mut self.title);
        Ok(())
    }

    fn lines(&self, _width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn container(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One tab of a [`TabStrip`].
#[derive(Debug, Clone, Default, PartialEq)]
struct Tab {
    label: String,
    value: String,
    /// A status dot before the label, in this tone (none: no dot).
    dot: Option<String>,
    /// A count or short text after the label (unread messages).
    badge: Option<String>,
    closable: bool,
}

/// A row of tabs: click or ←/→ choose (`select` with the tab's value), ctrl+w or a middle click
/// closes a closable one (`close` with its value). Wider than the row, it scrolls to keep the
/// active tab in view.
#[derive(Debug, Default)]
pub struct TabStrip {
    tabs: Vec<Tab>,
    active: usize,
}

impl TabStrip {
    fn label(&self, i: usize) -> Vec<Span<'static>> {
        let t = &self.tabs[i];
        let on = i == self.active;
        let base = if on {
            theme::selection().add_modifier(Modifier::BOLD)
        } else {
            theme::muted()
        };
        let mut spans = vec![Span::styled(" ", base)];
        if let Some(tone) = &t.dot {
            spans.push(Span::styled("● ", base.patch(fg_of(theme::tone(tone)))));
        }
        spans.push(Span::styled(t.label.clone(), base));
        if let Some(b) = t.badge.as_ref().filter(|b| !b.is_empty()) {
            spans.push(Span::styled(format!(" {b}"), base.patch(fg_of(theme::warn()))));
        }
        if t.closable && on {
            spans.push(Span::styled(" ×", base));
        }
        spans.push(Span::styled(" ", base));
        spans
    }

    /// Each tab's (first column, width) on the full, unscrolled row.
    fn spans_at(&self) -> Vec<(usize, usize)> {
        let mut col = 0;
        (0..self.tabs.len())
            .map(|i| {
                let w: usize = self
                    .label(i)
                    .iter()
                    .map(|s| display_width(&s.content))
                    .sum();
                let at = (col, w);
                col += w + 1;
                at
            })
            .collect()
    }

    /// Columns scrolled off the left, whole tabs at a time, so the active tab fits in `width`
    /// beside the ‹ › marks.
    fn offset(&self, width: usize) -> usize {
        let spans = self.spans_at();
        let Some(&(x, w)) = spans.get(self.active) else {
            return 0;
        };
        let end = x + w;
        if end <= width {
            return 0;
        }
        let room = width.saturating_sub(2);
        spans[..=self.active]
            .iter()
            .map(|&(start, _)| start)
            .find(|&start| end - start <= room)
            .unwrap_or(x)
    }

    fn choose(&mut self, i: usize, out: &mut Vec<Emit>) {
        if i < self.tabs.len() {
            self.active = i;
            let value = self.tabs[i].value.clone();
            out.push(Emit::Select { index: i as u32, value: value.into() });
        }
    }

    fn close(&mut self, i: usize, out: &mut Vec<Emit>) {
        if let Some(t) = self.tabs.get(i).filter(|t| t.closable) {
            out.push(Emit::Close(t.value.clone()));
        }
    }
}

fn fg_of(s: Style) -> Style {
    Style { fg: s.fg, ..Style::default() }
}

impl Component for TabStrip {
    fn kind(&self) -> &'static str {
        "tabstrip"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        if let Some(v) = get(p, "tabs") {
            self.tabs = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| {
                    let text = |k: &str| get(t, k).filter(|v| !v.is_nil()).map(display);
                    let label = text("label").unwrap_or_else(|| display(t));
                    Tab {
                        value: text("value").unwrap_or_else(|| label.clone()),
                        label,
                        dot: text("dot"),
                        badge: text("badge"),
                        closable: get(t, "closable").and_then(Value::as_bool).unwrap_or(false),
                    }
                })
                .collect();
        }
        if let Some(v) = get(p, "active").and_then(num) {
            self.active = v.max(0.0) as usize;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let w = width as usize;
        let skip = self.offset(w);
        let mut spans: Vec<Span<'static>> = Vec::new();
        for i in 0..self.tabs.len() {
            if i > 0 {
                spans.push(Span::styled("│", theme::faint()));
            }
            spans.extend(self.label(i));
        }
        let mut line = Line::from(spans);
        if skip > 0 {
            line = drop_columns(line, skip);
            line.spans.insert(0, Span::styled("‹", theme::faint()));
        }
        let more = display_width(&crate::text::plain(&line)) > w;
        let mut line = clip(line, if more { w.saturating_sub(1) } else { w });
        if more {
            line.spans.push(Span::styled("›", theme::faint()));
        }
        vec![line]
    }

    fn focusable(&self) -> bool {
        !self.tabs.is_empty()
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let Input::Key { code, mods, .. } = input else {
            return false;
        };
        match (code, mods.contains(KeyModifiers::CONTROL)) {
            (KeyCode::Left, _) => self.choose(self.active.saturating_sub(1), out),
            (KeyCode::Right, _) => self.choose((self.active + 1).min(self.tabs.len() - 1), out),
            (KeyCode::Home, _) => self.choose(0, out),
            (KeyCode::End, _) => self.choose(self.tabs.len() - 1, out),
            (KeyCode::Char('w'), true) | (KeyCode::Delete, _) => self.close(self.active, out),
            _ => return false,
        }
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if m.row != 0 || !matches!(m.act, MouseAct::Click | MouseAct::Middle) {
            return false;
        }
        // scrolled, column 0 is the ‹ mark and column c shows the row's column c - 1 + skip
        let skip = self.offset(m.width as usize);
        if skip > 0 && m.col == 0 {
            self.choose(self.active.saturating_sub(1), out);
            return true;
        }
        let col = m.col as usize + skip - usize::from(skip > 0);
        let hit = self
            .spans_at()
            .iter()
            .position(|&(x, w)| col >= x && col < x + w);
        match (hit, m.act) {
            (Some(i), MouseAct::Middle) => self.close(i, out),
            (Some(i), _) => self.choose(i, out),
            _ => return false,
        }
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `line` without its first `n` columns.
fn drop_columns(line: Line<'static>, n: usize) -> Line<'static> {
    let mut left = n;
    let mut spans = Vec::new();
    for s in line.spans {
        if left == 0 {
            spans.push(s);
            continue;
        }
        let mut kept = String::new();
        for ch in s.content.chars() {
            let cw = display_width(&ch.to_string());
            if left >= cw && left > 0 {
                left -= cw;
            } else {
                left = 0;
                kept.push(ch);
            }
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, s.style));
        }
    }
    Line::from(spans)
}
