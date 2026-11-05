//! Dialog components: `confirm` (yes/no), `prompt` (text answer) and `card` (body + actions).

use std::any::Any;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::props::{upd_bool, upd_opt_str, upd_str};
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{clip, display_width, render_markdown, theme, wrap_text};

fn button(label: &str, selected: bool) -> Span<'static> {
    if selected {
        Span::styled(format!(" {label} "), theme::selection().add_modifier(Modifier::BOLD))
    } else {
        Span::styled(format!(" {label} "), theme::muted())
    }
}

/// A yes/no question; submits a bool.
#[derive(Debug)]
pub struct Confirm {
    question: String,
    yes: String,
    no: String,
    choice: bool,
}

impl Default for Confirm {
    fn default() -> Self {
        Confirm { question: String::new(), yes: "Yes".into(), no: "No".into(), choice: false }
    }
}

impl Component for Confirm {
    fn kind(&self) -> &'static str {
        "confirm"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "question", &mut self.question);
        upd_bool(p, "default", &mut self.choice, false);
        if let Some(y) = str_of(p, "yes") {
            self.yes = y.to_owned();
        }
        if let Some(n) = str_of(p, "no") {
            self.no = n.to_owned();
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let mut out = wrap_text(&self.question, theme::bold(), width as usize);
        let buttons = Line::from(vec![
            button(&self.yes, self.choice),
            Span::raw("  "),
            button(&self.no, !self.choice),
            Span::styled("   y/n · ←→ · enter", theme::muted()),
        ]);
        out.push(clip(buttons, width as usize));
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let Input::Key { code, .. } = input else {
            return false;
        };
        match code {
            KeyCode::Char('y' | 'Y') => out.push(Emit::Submit(Value::Boolean(true))),
            KeyCode::Char('n' | 'N') => out.push(Emit::Submit(Value::Boolean(false))),
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                self.choice = !self.choice
            }
            KeyCode::Enter => out.push(Emit::Submit(Value::Boolean(self.choice))),
            _ => return false,
        }
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        let row = wrap_text(&self.question, theme::bold(), m.width as usize).len();
        if m.act != MouseAct::Click || m.row as usize != row {
            return false;
        }
        let yes_end = display_width(&self.yes) + 2;
        let no_start = yes_end + 2;
        let col = m.col as usize;
        let answer = match col {
            c if c < yes_end => true,
            c if (no_start..no_start + display_width(&self.no) + 2).contains(&c) => false,
            _ => return false,
        };
        self.choice = answer;
        out.push(Emit::Submit(Value::Boolean(answer)));
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A question with a text answer; submits the text.
#[derive(Debug, Default)]
pub struct Prompt {
    question: String,
    placeholder: Option<String>,
    multiline: bool,
    editor: TextEditor,
}

impl Prompt {
    fn body(&self, width: u16) -> (Vec<Line<'static>>, (u16, u16)) {
        let avail = (width as usize).saturating_sub(2).max(1);
        let (lines, caret) = self.editor.render(avail, Default::default());
        let mut out = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            let lead = if i == 0 { "› " } else { "  " };
            let mut spans = vec![Span::styled(lead, theme::accent())];
            if self.editor.is_empty() {
                let ph = self.placeholder.clone().unwrap_or_default();
                spans.push(Span::styled(ph, theme::muted()));
            } else {
                spans.extend(line.spans);
            }
            out.push(Line::from(spans));
        }
        (out, (caret.0, caret.1 + 2))
    }

    fn question_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.question.is_empty() {
            return Vec::new();
        }
        wrap_text(&self.question, theme::bold(), width as usize)
    }
}

impl Component for Prompt {
    fn kind(&self) -> &'static str {
        "prompt"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "question", &mut self.question);
        upd_opt_str(p, "placeholder", &mut self.placeholder);
        upd_bool(p, "multiline", &mut self.multiline, false);
        if let Some(v) = get(p, "value") {
            self.editor.set(&display(v));
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let mut out = self.question_lines(width);
        out.extend(self.body(width).0);
        let hint = if self.multiline {
            "  enter submit · alt+enter newline · esc cancel"
        } else {
            "  enter submit · esc cancel"
        };
        out.push(clip(Line::from(Span::styled(hint, theme::muted())), width as usize));
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        edit(&mut self.editor, input, self.multiline, out)
    }

    fn cursor(&self, width: u16) -> Option<(u16, u16)> {
        let q = self.question_lines(width).len() as u16;
        let (_, (row, col)) = self.body(width);
        Some((q + row, col))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Shared editing keys for single answers; Enter submits the text.
fn edit(ed: &mut TextEditor, input: &Input, multiline: bool, out: &mut Vec<Emit>) -> bool {
    match input {
        Input::Paste(s) => {
            let s = if multiline {
                s.clone()
            } else {
                s.replace('\n', " ")
            };
            ed.insert(&s);
            true
        }
        Input::Key { code, mods, .. } => {
            let ctrl = mods.contains(KeyModifiers::CONTROL);
            let newline = mods.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT);
            match code {
                KeyCode::Enter if multiline && newline => ed.insert("\n"),
                KeyCode::Enter => out.push(Emit::Submit(Value::from(ed.text()))),
                KeyCode::Backspace => {
                    ed.backspace();
                }
                KeyCode::Delete => {
                    ed.delete();
                }
                KeyCode::Left => ed.left(),
                KeyCode::Right => ed.right(),
                KeyCode::Home => ed.home(),
                KeyCode::End => ed.end(),
                KeyCode::Up => {
                    ed.up();
                }
                KeyCode::Down => {
                    ed.down();
                }
                KeyCode::Char('a') if ctrl => ed.home(),
                KeyCode::Char('e') if ctrl => ed.end(),
                KeyCode::Char('u') if ctrl => ed.kill_to_line_start(),
                KeyCode::Char('w') if ctrl => ed.delete_word_back(),
                KeyCode::Char(c) if !ctrl || mods.contains(KeyModifiers::ALT) => {
                    ed.insert(c.encode_utf8(&mut [0; 4]))
                }
                _ => return false,
            }
            true
        }
    }
}

/// A titled Markdown body with action buttons; selecting an action (enter, its digit or a
/// click) emits its id. With `back`, ← closes it with reason `back` and → / tab move.
#[derive(Debug, Default)]
pub struct Card {
    title: Option<String>,
    body: String,
    actions: Vec<(String, String)>,
    selected: usize,
    back: bool,
}

impl Card {
    /// Lines above the buttons at `width`.
    fn text_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        if let Some(t) = &self.title {
            out.extend(wrap_text(t, theme::bold(), width as usize));
        }
        if !self.body.trim().is_empty() {
            out.extend(
                render_markdown(&self.body, width as usize)
                    .into_iter()
                    .map(|l| l.line),
            );
        }
        out
    }

    /// Each button's (row, first col, end col) below the text, wrapping like `lines`.
    fn button_spots(&self, width: u16) -> Vec<(usize, usize, usize)> {
        let (mut row, mut used) = (0, 0);
        let mut out = Vec::new();
        for (_, label) in &self.actions {
            let w = display_width(label) + 4;
            if used + w > width as usize && used > 0 {
                row += 1;
                used = 0;
            }
            out.push((row, used, used + w - 2));
            used += w;
        }
        out
    }

    fn choose(&self, i: usize, out: &mut Vec<Emit>) {
        let value = Value::from(self.actions[i].0.as_str());
        out.push(Emit::Select { index: i as u32, value });
    }
}

impl Component for Card {
    fn kind(&self) -> &'static str {
        "card"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "title", &mut self.title);
        upd_str(p, "body", &mut self.body);
        upd_bool(p, "back", &mut self.back, false);
        if let Some(actions) = get(p, "actions") {
            self.actions = actions
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|a| match a {
                            Value::Map(_) => {
                                let id = get(a, "id").map(display).unwrap_or_default();
                                let label = str_of(a, "label").map_or(id.clone(), str::to_owned);
                                (id, label)
                            }
                            other => (display(other), display(other)),
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.selected = 0;
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        self.body.push_str(&display(data));
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let mut out = self.text_lines(width);
        if !self.actions.is_empty() {
            let mut spans = Vec::new();
            let mut used = 0;
            for (i, (_, label)) in self.actions.iter().enumerate() {
                let w = display_width(label) + 4;
                if used + w > width as usize && !spans.is_empty() {
                    out.push(Line::from(std::mem::take(&mut spans)));
                    used = 0;
                }
                spans.push(button(label, i == self.selected));
                spans.push(Span::raw("  "));
                used += w;
            }
            out.push(Line::from(spans));
        }
        out
    }

    fn focusable(&self) -> bool {
        !self.actions.is_empty()
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let Input::Key { code, .. } = input else {
            return false;
        };
        let n = self.actions.len();
        if n == 0 {
            return false;
        }
        match code {
            KeyCode::Left if self.back => out.push(Emit::Close("back".into())),
            KeyCode::Left | KeyCode::BackTab => self.selected = (self.selected + n - 1) % n,
            KeyCode::Right | KeyCode::Tab => self.selected = (self.selected + 1) % n,
            KeyCode::Enter => self.choose(self.selected, out),
            KeyCode::Char(c @ '1'..='9') if ((*c as u8 - b'1') as usize) < n => {
                self.choose((*c as u8 - b'1') as usize, out)
            }
            _ => return false,
        }
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        if m.act != MouseAct::Click {
            return false;
        }
        let Some(row) = (m.row as usize).checked_sub(self.text_lines(m.width).len()) else {
            return false;
        };
        let col = m.col as usize;
        let spots = self.button_spots(m.width);
        let hit = spots
            .iter()
            .position(|(r, a, b)| *r == row && (*a..*b).contains(&col));
        let Some(i) = hit else {
            return false;
        };
        self.selected = i;
        self.choose(i, out);
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
