//! Layout components: `stack` (and the five regions), `columns`, `tabs` and `divider`.

use std::any::Any;

use crossterm::event::KeyCode;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_strings, upd_usize};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::Value;
use crate::text::{clip, display_width, theme};

/// Vertical container; children are laid out by the view with `gap` blank lines between them.
#[derive(Debug, Default)]
pub struct Stack {
    pub title: Option<String>,
    pub gap: usize,
}

impl Component for Stack {
    fn kind(&self) -> &'static str {
        "stack"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "title", &mut self.title);
        upd_usize(p, "gap", &mut self.gap, 0);
        Ok(())
    }

    fn lines(&self, _width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        match &self.title {
            Some(t) => vec![Line::from(Span::styled(t.clone(), theme::bold()))],
            None => Vec::new(),
        }
    }

    fn container(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A horizontal rule with an optional label.
#[derive(Debug, Default)]
pub struct Divider {
    label: Option<String>,
}

impl Component for Divider {
    fn kind(&self) -> &'static str {
        "divider"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "label", &mut self.label);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let text = match &self.label {
            Some(l) if display_width(l) + 6 <= width => {
                let rest = width - display_width(l) - 4;
                format!("── {l} {}", "─".repeat(rest))
            }
            _ => "─".repeat(width),
        };
        vec![Line::from(Span::styled(text, theme::muted()))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Children side by side, `count` per row (0: all in one row), `gap` columns apart. Too narrow
/// for that (a column under `min_width`), the children stack instead. With `pack` each child
/// keeps its own width (a row of badges or buttons) and the row wraps when it is full.
#[derive(Debug)]
pub struct Columns {
    pub count: usize,
    pub gap: usize,
    pub min_width: usize,
    pub pack: bool,
}

impl Default for Columns {
    fn default() -> Self {
        Columns { count: 0, gap: 3, min_width: 16, pack: false }
    }
}

impl Component for Columns {
    fn kind(&self) -> &'static str {
        "columns"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_usize(p, "count", &mut self.count, 0);
        upd_usize(p, "gap", &mut self.gap, 3);
        upd_usize(p, "min_width", &mut self.min_width, 16);
        if let Some(v) = crate::proto::value::get(p, "pack") {
            self.pack = v.as_bool().unwrap_or(false);
        }
        Ok(())
    }

    fn lines(&self, _width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn container(&self) -> bool {
        true
    }

    fn side_by_side(&self) -> Option<(usize, usize)> {
        Some((self.count, self.gap))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A row of tab labels over its children, one pane per child; only the `active` one is drawn.
/// While focused ←/→ (or a click on a label) switch, emitting `change` with the new index.
#[derive(Debug, Default)]
pub struct Tabs {
    labels: Vec<String>,
    pub active: usize,
}

impl Tabs {
    fn spans(&self, focused: bool) -> Vec<(usize, Span<'static>)> {
        let mut out = Vec::new();
        for (i, label) in self.labels.iter().enumerate() {
            let style = match (i == self.active, focused) {
                (true, true) => theme::selection().add_modifier(Modifier::BOLD),
                (true, false) => {
                    theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                }
                _ => theme::muted(),
            };
            out.push((i, Span::styled(format!(" {label} "), style)));
        }
        out
    }

    fn choose(&mut self, at: usize, out: &mut Vec<Emit>) {
        if at < self.labels.len() && at != self.active {
            self.active = at;
            out.push(Emit::Change(Value::from(at as u64)));
        }
    }
}

impl Component for Tabs {
    fn kind(&self) -> &'static str {
        "tabs"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_strings(p, "labels", &mut self.labels);
        upd_usize(p, "active", &mut self.active, 0);
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, span) in self.spans(ctx.focused) {
            if i > 0 {
                spans.push(Span::styled("│", theme::faint()));
            }
            spans.push(span);
        }
        if ctx.focused {
            spans.push(Span::styled("  ←/→", theme::faint()));
        }
        vec![clip(Line::from(spans), width as usize)]
    }

    fn container(&self) -> bool {
        true
    }

    fn shows_child(&self, index: usize) -> bool {
        index == self.active.min(self.labels.len().saturating_sub(1))
    }

    fn focusable(&self) -> bool {
        self.labels.len() > 1
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        match input {
            Input::Key { code: KeyCode::Left, .. } => {
                self.choose(self.active.saturating_sub(1), out);
                true
            }
            Input::Key { code: KeyCode::Right, .. } => {
                self.choose(self.active + 1, out);
                true
            }
            _ => false,
        }
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if m.act != MouseAct::Click || m.row != 0 {
            return false;
        }
        let mut col = 0;
        for (i, span) in self.spans(false) {
            let w = display_width(&span.content) + usize::from(i > 0);
            if (m.col as usize) < col + w {
                self.choose(i, out);
                return true;
            }
            col += w;
        }
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
