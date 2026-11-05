//! `button`: one focusable line, such as a status line the user can step onto with ↓.
//! Enter, space or a click selects it, emitting `select` with its value (the label when unset).
//!
//! With `segments` (`[{label, value}]`) the line is several buttons in a row: while focused
//! ←/→ choose one (drawn as the selection), and enter, space or a click on a segment selects
//! that segment's value with its index.

use std::any::Any;

use crossterm::event::KeyCode;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_str};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get};
use crate::proto::Value;
use crate::text::{clip, display_width, theme};

const SEPARATOR: &str = "  ·  ";

#[derive(Debug, Default)]
pub struct Button {
    label: String,
    value: Option<Value>,
    /// Muted text after the label while not focused, e.g. "↓ to view".
    hint: Option<String>,
    /// Muted text after the label while focused, e.g. "enter to view".
    focused_hint: Option<String>,
    segments: Vec<(String, Value)>,
    /// The chosen segment.
    at: usize,
}

impl Button {
    fn select(&self, out: &mut Vec<Emit>) {
        if let Some((_, value)) = self.segments.get(self.at) {
            out.push(Emit::Select { index: self.at as u32, value: value.clone() });
            return;
        }
        let value = self
            .value
            .clone()
            .unwrap_or_else(|| Value::from(self.label.as_str()));
        out.push(Emit::Select { index: 0, value });
    }
}

impl Component for Button {
    fn kind(&self) -> &'static str {
        "button"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "label", &mut self.label);
        upd_opt_str(p, "hint", &mut self.hint);
        upd_opt_str(p, "focused_hint", &mut self.focused_hint);
        if let Some(v) = get(p, "value") {
            self.value = (!v.is_nil()).then(|| v.clone());
        }
        if let Some(v) = get(p, "segments") {
            self.segments = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|seg| {
                    let label = get(seg, "label")
                        .map(display)
                        .unwrap_or_else(|| display(seg));
                    let value = get(seg, "value")
                        .cloned()
                        .unwrap_or_else(|| label.as_str().into());
                    (label, value)
                })
                .collect();
            // the chosen segment survives updates while it still exists
            self.at = self.at.min(self.segments.len().saturating_sub(1));
        }
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        if !self.segments.is_empty() {
            let mut spans = Vec::new();
            for (i, (label, _)) in self.segments.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(SEPARATOR, theme::faint()));
                }
                spans.push(match (ctx.focused, i == self.at) {
                    (true, true) => Span::styled(
                        format!(" {label} "),
                        theme::selection().add_modifier(Modifier::BOLD),
                    ),
                    _ => Span::styled(label.clone(), theme::accent()),
                });
            }
            let hint = if ctx.focused {
                &self.focused_hint
            } else {
                &self.hint
            };
            if let Some(h) = hint {
                spans.push(Span::styled(format!(" · {h}"), theme::muted()));
            }
            return vec![clip(Line::from(spans), width as usize)];
        }
        let (label, hint) = match ctx.focused {
            true => {
                let style = theme::selection().add_modifier(Modifier::BOLD);
                (Span::styled(format!(" {} ", self.label), style), &self.focused_hint)
            }
            false => (Span::styled(self.label.clone(), theme::accent()), &self.hint),
        };
        let mut spans = vec![label];
        if let Some(h) = hint {
            spans.push(Span::styled(format!(" · {h}"), theme::muted()));
        }
        vec![clip(Line::from(spans), width as usize)]
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let n = self.segments.len();
        match input {
            Input::Key { code: KeyCode::Enter | KeyCode::Char(' '), .. } => {
                self.select(out);
                true
            }
            Input::Key { code: KeyCode::Left, .. } if n > 1 => {
                self.at = self.at.saturating_sub(1);
                true
            }
            Input::Key { code: KeyCode::Right, .. } if n > 1 => {
                self.at = (self.at + 1).min(n - 1);
                true
            }
            _ => false,
        }
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if m.act != MouseAct::Click || m.row != 0 {
            return false;
        }
        // which segment the click landed on (unfocused widths: label plus separator)
        let mut col = 0;
        for (i, (label, _)) in self.segments.iter().enumerate() {
            let w = display_width(label) + 2;
            if (m.col as usize) < col + w {
                self.at = i;
                break;
            }
            col += w + display_width(SEPARATOR) - 2;
        }
        self.select(out);
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
