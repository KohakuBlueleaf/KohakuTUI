//! `field`: one form control on one line, for forms inside the transcript or a panel.
//!
//! `control` picks the kind: `slider` and `number` (←/→ step by `step`, pgup/pgdn ten steps,
//! home/end the bounds; a number also takes typed digits), `select` and `radio` (←/→ choose
//! one of `options`), `toggle` and `checkbox` (space, enter or ←/→ flip), `rating` (←/→, 0 to
//! `max`), `text` and `date` (an editable line; `placeholder` while empty).
//!
//! Every change emits `change` with the new value at once: a number for slider / number /
//! rating, the chosen option's value for select / radio, a bool for toggle / checkbox, the text
//! for text / date. The owner usually echoes the value back with `update(value=…)`; an echo of
//! a value this field emitted itself is ignored, so a slow round trip never undoes keys typed
//! since.

use std::any::Any;
use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::editor::TextEditor as Editor;
use super::props::{upd_num, upd_opt_num, upd_opt_str, upd_str};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get, num};
use crate::proto::Value;
use crate::text::{clip, display_width, theme};

const SLIDER_CELLS: usize = 20;
/// Echoes kept to recognise: values emitted but not yet echoed back.
const PENDING: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Control {
    #[default]
    Slider,
    Number,
    Select,
    Radio,
    Toggle,
    Checkbox,
    Rating,
    Text,
    Date,
}

impl Control {
    fn parse(s: &str) -> Option<Control> {
        Some(match s {
            "slider" => Control::Slider,
            "number" => Control::Number,
            "select" => Control::Select,
            "radio" => Control::Radio,
            "toggle" => Control::Toggle,
            "checkbox" => Control::Checkbox,
            "rating" => Control::Rating,
            "text" => Control::Text,
            "date" => Control::Date,
            _ => return None,
        })
    }

    fn hint(self) -> &'static str {
        match self {
            Control::Slider | Control::Number | Control::Rating => "←/→",
            Control::Select | Control::Radio => "←/→ choose",
            Control::Toggle | Control::Checkbox => "space",
            Control::Text | Control::Date => "type · enter",
        }
    }
}

#[derive(Debug)]
pub struct Field {
    control: Control,
    label: String,
    value: Value,
    options: Vec<(String, Value)>,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    placeholder: Option<String>,
    disabled: bool,
    editor: Editor,
    /// Digits typed into a number, until the next step key.
    typed: Option<String>,
    pending: VecDeque<Value>,
}

impl Default for Field {
    fn default() -> Self {
        Field {
            control: Control::default(),
            label: String::new(),
            value: Value::Nil,
            options: Vec::new(),
            min: None,
            max: None,
            step: 1.0,
            placeholder: None,
            disabled: false,
            editor: Editor::default(),
            typed: None,
            pending: VecDeque::new(),
        }
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

impl Field {
    fn number(&self) -> f64 {
        num(&self.value).unwrap_or(self.min.unwrap_or(0.0))
    }

    fn bounds(&self) -> (f64, f64) {
        match self.control {
            Control::Rating => (0.0, self.max.unwrap_or(5.0)),
            _ => (self.min.unwrap_or(f64::MIN), self.max.unwrap_or(f64::MAX)),
        }
    }

    fn chosen(&self) -> Option<usize> {
        self.options.iter().position(|(_, v)| *v == self.value)
    }

    fn set(&mut self, value: Value, out: &mut Vec<Emit>) {
        if value == self.value {
            return;
        }
        self.value = value.clone();
        if self.pending.len() == PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back(value.clone());
        out.push(Emit::Change(value));
    }

    fn step_by(&mut self, steps: f64, out: &mut Vec<Emit>) {
        let (low, high) = self.bounds();
        let step = if self.control == Control::Rating {
            1.0
        } else {
            self.step
        };
        let next = (self.number() + steps * step).clamp(low, high);
        // land on the step grid from the minimum, so 0.1 steps never drift into 0.30000004
        let base = self.min.unwrap_or(0.0);
        let snapped = base + ((next - base) / step).round() * step;
        let decimals = fmt_num(step).split('.').nth(1).map_or(0, str::len) as i32;
        let factor = 10f64.powi(decimals);
        self.set(number_value((snapped * factor).round() / factor), out);
    }

    fn choose(&mut self, delta: isize, out: &mut Vec<Emit>) {
        let n = self.options.len();
        if n == 0 {
            return;
        }
        let at = self
            .chosen()
            .map_or(0, |i| (i as isize + delta).clamp(0, n as isize - 1) as usize);
        let value = self.options[at].1.clone();
        self.set(value, out);
    }

    fn flip(&mut self, out: &mut Vec<Emit>) {
        let on = self.value.as_bool().unwrap_or(false);
        self.set(Value::from(!on), out);
    }

    fn label_span(&self, focused: bool) -> Option<Span<'static>> {
        if self.label.is_empty() {
            return None;
        }
        let style = if focused {
            theme::accent().add_modifier(Modifier::BOLD)
        } else {
            theme::bold()
        };
        Some(Span::styled(format!("{}  ", self.label), style))
    }

    /// The control's own spans: the value, styled as the selection while focused.
    fn value_spans(&self, focused: bool, width: usize) -> Vec<Span<'static>> {
        let hot = |s: String| {
            if focused {
                Span::styled(s, theme::selection().add_modifier(Modifier::BOLD))
            } else {
                Span::styled(s, theme::accent())
            }
        };
        match self.control {
            Control::Slider => {
                let (low, high) = (self.min.unwrap_or(0.0), self.max.unwrap_or(100.0));
                let share = if high > low {
                    ((self.number() - low) / (high - low)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let cells = SLIDER_CELLS.min(width.saturating_sub(10)).max(5);
                let at = (share * (cells - 1) as f64).round() as usize;
                vec![
                    Span::styled("━".repeat(at), theme::accent()),
                    hot("●".into()),
                    Span::styled("─".repeat(cells - 1 - at), theme::faint()),
                    Span::raw("  "),
                    Span::styled(fmt_num(self.number()), theme::bold()),
                ]
            }
            Control::Number => {
                let shown = self.typed.clone().unwrap_or_else(|| fmt_num(self.number()));
                vec![
                    Span::styled("‹ ", theme::faint()),
                    hot(shown),
                    Span::styled(" ›", theme::faint()),
                ]
            }
            Control::Select => {
                let shown = self
                    .chosen()
                    .map_or_else(|| display(&self.value), |i| self.options[i].0.clone());
                vec![
                    Span::styled("‹ ", theme::faint()),
                    hot(shown),
                    Span::styled(" ›", theme::faint()),
                ]
            }
            Control::Radio => {
                let at = self.chosen();
                let mut spans = Vec::new();
                for (i, (label, _)) in self.options.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw("  "));
                    }
                    if Some(i) == at {
                        spans.push(hot(format!("◉ {label}")));
                    } else {
                        spans.push(Span::styled(format!("○ {label}"), theme::muted()));
                    }
                }
                spans
            }
            Control::Toggle => {
                if self.value.as_bool().unwrap_or(false) {
                    vec![hot("● on".into())]
                } else {
                    vec![hot("○ off".into())]
                }
            }
            Control::Checkbox => vec![],
            Control::Rating => {
                let max = self.max.unwrap_or(5.0).max(1.0) as usize;
                let n = (self.number().max(0.0) as usize).min(max);
                vec![
                    hot("★".repeat(n)),
                    Span::styled("☆".repeat(max - n), theme::faint()),
                ]
            }
            Control::Text | Control::Date => vec![],
        }
    }
}

impl Component for Field {
    fn kind(&self) -> &'static str {
        "field"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        if let Some(v) = get(p, "control") {
            let name = display(v);
            self.control = Control::parse(&name).ok_or_else(|| WidgetError::BadProp {
                prop: "control",
                reason: format!("unknown `{name}`"),
            })?;
        }
        upd_str(p, "label", &mut self.label);
        upd_opt_num(p, "min", &mut self.min);
        upd_opt_num(p, "max", &mut self.max);
        upd_num(p, "step", &mut self.step, 1.0);
        if self.step <= 0.0 {
            self.step = 1.0;
        }
        upd_opt_str(p, "placeholder", &mut self.placeholder);
        if let Some(v) = get(p, "disabled") {
            self.disabled = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = get(p, "options") {
            self.options = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| {
                    let value = get(o, "value").cloned().unwrap_or_else(|| o.clone());
                    let label = get(o, "label")
                        .map(display)
                        .unwrap_or_else(|| display(&value));
                    (label, value)
                })
                .collect();
        }
        if let Some(v) = get(p, "value") {
            // an echo of a value this field sent: everything up to it is acknowledged
            if let Some(at) = self.pending.iter().position(|sent| sent == v) {
                self.pending.drain(..=at);
            } else {
                self.pending.clear();
                self.value = v.clone();
                if matches!(self.control, Control::Text | Control::Date) {
                    self.editor.set(&display_text(v));
                }
                self.typed = None;
            }
        }
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        let w = width as usize;
        let focused = ctx.focused && !self.disabled;
        let mut spans: Vec<Span<'static>> = Vec::new();
        if self.control == Control::Checkbox {
            let on = self.value.as_bool().unwrap_or(false);
            let mark = if on { "☑" } else { "☐" };
            let style = if focused {
                theme::selection().add_modifier(Modifier::BOLD)
            } else {
                theme::accent()
            };
            spans.push(Span::styled(mark.to_string(), style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                self.label.clone(),
                if focused {
                    theme::accent()
                } else {
                    theme::bold()
                },
            ));
        } else if matches!(self.control, Control::Text | Control::Date) {
            spans.extend(self.label_span(focused));
            let used = spans
                .iter()
                .map(|s| display_width(&s.content))
                .sum::<usize>();
            // the box leaves room for the key hint drawn after it while focused
            let hint = if focused {
                display_width(self.control.hint()) + 2
            } else {
                0
            };
            let room = w.saturating_sub(used + 2 + hint).clamp(4, 60);
            spans.push(Span::styled("[", theme::faint()));
            if focused {
                // the caret is drawn (a reversed cell): a control in a transcript cell has no
                // terminal cursor
                let chars: Vec<char> = self.editor.text().replace('\n', " ").chars().collect();
                let at = self.editor.cursor().min(chars.len());
                let before: String = chars[..at].iter().collect();
                let under = chars.get(at).map_or(" ".to_string(), |c| c.to_string());
                let after: String = chars
                    .get(at + 1..)
                    .map_or(String::new(), |r| r.iter().collect());
                let used = display_width(&before) + display_width(&under) + display_width(&after);
                spans.push(Span::styled(before, theme::accent()));
                spans.push(Span::styled(under, theme::accent().add_modifier(Modifier::REVERSED)));
                spans.push(Span::styled(after, theme::accent()));
                spans.push(Span::raw(" ".repeat(room.saturating_sub(used))));
            } else if self.editor.is_empty() {
                let hint = self.placeholder.clone().unwrap_or_default();
                spans.push(Span::styled(format!("{hint:<room$}", room = room), theme::faint()));
            } else {
                let text = self.editor.text().replace('\n', " ");
                spans.push(Span::styled(format!("{text:<room$}", room = room), theme::accent()));
            }
            spans.push(Span::styled("]", theme::faint()));
        } else {
            spans.extend(self.label_span(focused));
            spans.extend(self.value_spans(focused, w));
        }
        if focused {
            spans.push(Span::styled(format!("  {}", self.control.hint()), theme::faint()));
        }
        vec![clip(Line::from(spans), w)]
    }

    fn focusable(&self) -> bool {
        !self.disabled
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        if self.disabled {
            return false;
        }
        if matches!(self.control, Control::Text | Control::Date) {
            return self.on_text(input, out);
        }
        let Input::Key { code, mods, .. } = input else {
            return false;
        };
        let fast = if mods.contains(KeyModifiers::SHIFT) {
            10.0
        } else {
            1.0
        };
        match (self.control, code) {
            (Control::Slider | Control::Number | Control::Rating, KeyCode::Left) => {
                self.step_by(-fast, out)
            }
            (Control::Slider | Control::Number | Control::Rating, KeyCode::Right) => {
                self.step_by(fast, out)
            }
            (Control::Slider | Control::Number, KeyCode::PageDown) => self.step_by(-10.0, out),
            (Control::Slider | Control::Number, KeyCode::PageUp) => self.step_by(10.0, out),
            (Control::Slider | Control::Number | Control::Rating, KeyCode::Home) => {
                let (low, _) = self.bounds();
                if low > f64::MIN {
                    self.set(number_value(low), out);
                }
            }
            (Control::Slider | Control::Number | Control::Rating, KeyCode::End) => {
                let (_, high) = self.bounds();
                if high < f64::MAX {
                    self.set(number_value(high), out);
                }
            }
            (Control::Number, KeyCode::Char(c)) if c.is_ascii_digit() || *c == '.' || *c == '-' => {
                let mut typed = self.typed.take().unwrap_or_default();
                typed.push(*c);
                if let Ok(n) = typed.parse::<f64>() {
                    let (low, high) = self.bounds();
                    self.set(number_value(n.clamp(low, high)), out);
                }
                self.typed = Some(typed);
                return true;
            }
            (Control::Number, KeyCode::Backspace) => {
                let mut typed = self.typed.take().unwrap_or_else(|| fmt_num(self.number()));
                typed.pop();
                if let Ok(n) = typed.parse::<f64>() {
                    self.set(number_value(n), out);
                }
                self.typed = Some(typed);
                return true;
            }
            (Control::Select | Control::Radio, KeyCode::Left) => self.choose(-1, out),
            (Control::Select | Control::Radio, KeyCode::Right) => self.choose(1, out),
            (
                Control::Toggle | Control::Checkbox,
                KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right,
            ) => self.flip(out),
            _ => return false,
        }
        self.typed = None;
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if self.disabled || m.act != MouseAct::Click || m.row != 0 {
            return false;
        }
        match self.control {
            Control::Toggle | Control::Checkbox => self.flip(out),
            Control::Select => self.choose(1, out),
            Control::Rating => {
                let label = if self.label.is_empty() {
                    0
                } else {
                    display_width(&self.label) + 2
                };
                let star = (m.col as usize).saturating_sub(label) + 1;
                let max = self.max.unwrap_or(5.0) as usize;
                self.set(number_value(star.min(max) as f64), out);
            }
            _ => return false,
        }
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Field {
    fn on_text(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let before = self.editor.text().to_owned();
        match input {
            Input::Paste(s) => self.editor.insert(&s.replace(['\r', '\n'], " ")),
            Input::Key { code, mods, .. } => {
                let ctrl = mods.contains(KeyModifiers::CONTROL);
                match code {
                    KeyCode::Char('u') if ctrl => self.editor.kill_to_line_start(),
                    KeyCode::Char('w') if ctrl => self.editor.delete_word_back(),
                    KeyCode::Char(c) if !ctrl => self.editor.insert(&c.to_string()),
                    KeyCode::Backspace => {
                        self.editor.backspace();
                    }
                    KeyCode::Delete => {
                        self.editor.delete();
                    }
                    KeyCode::Left => self.editor.left(),
                    KeyCode::Right => self.editor.right(),
                    KeyCode::Home => self.editor.home(),
                    KeyCode::End => self.editor.end(),
                    // enter confirms: the value already went out with every edit
                    KeyCode::Enter => out.push(Emit::Submit(Value::from(self.editor.text()))),
                    _ => return false,
                }
            }
        }
        if self.editor.text() != before {
            let text = self.editor.text().to_owned();
            self.set(Value::from(text.as_str()), out);
        }
        true
    }
}

fn display_text(v: &Value) -> String {
    match v {
        Value::Nil => String::new(),
        other => display(other),
    }
}
