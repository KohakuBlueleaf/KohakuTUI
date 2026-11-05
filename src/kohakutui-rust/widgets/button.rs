//! `button`: one focusable line, such as a status line the user can step onto with ↓.
//! Enter, space or a click selects it, emitting `select` with its value (the label when unset).

use std::any::Any;

use crossterm::event::KeyCode;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_str};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::get;
use crate::proto::Value;
use crate::text::{clip, theme};

#[derive(Debug, Default)]
pub struct Button {
    label: String,
    value: Option<Value>,
    /// Muted text after the label while not focused, e.g. "↓ to view".
    hint: Option<String>,
    /// Muted text after the label while focused, e.g. "enter to view".
    focused_hint: Option<String>,
}

impl Button {
    fn select(&self, out: &mut Vec<Emit>) {
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
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
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
        match input {
            Input::Key { code: KeyCode::Enter | KeyCode::Char(' '), .. } => {
                self.select(out);
                true
            }
            _ => false,
        }
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if m.act == MouseAct::Click && m.row == 0 {
            self.select(out);
            return true;
        }
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
