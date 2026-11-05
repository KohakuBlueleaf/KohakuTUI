//! `composer`: the chat input. Multi-line editing, history, a slash-command popup and paste-burst
//! detection for terminals that deliver a paste as plain keys (design §7.2).

use std::any::Any;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::picker::fuzzy_score;
use super::props::{upd_opt_str, upd_strings};
use super::text::truncate;
use super::{Component, Ctx, Emit, Input, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{display_width, theme};

/// Keys closer together than this are a paste, so an Enter among them is a newline.
pub const PASTE_BURST: Duration = Duration::from_millis(8);

const POPUP_ROWS: usize = 6;

#[derive(Debug, Default)]
pub struct Composer {
    editor: TextEditor,
    placeholder: Option<String>,
    history: Vec<String>,
    hist_pos: Option<usize>,
    draft: String,
    commands: Vec<(String, String)>,
    popup_sel: usize,
    popup_hidden: bool,
    last_char: Option<Instant>,
}

impl Composer {
    /// Commands matching a `/partial` with no space yet, best first.
    fn matches(&self) -> Vec<usize> {
        if self.popup_hidden || self.commands.is_empty() {
            return Vec::new();
        }
        let text = self.editor.text();
        let Some(query) = text.strip_prefix('/') else {
            return Vec::new();
        };
        if query.contains(char::is_whitespace) {
            return Vec::new();
        }
        let mut scored: Vec<(i64, usize)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(i, (name, _))| fuzzy_score(query, name).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn complete(&mut self) -> bool {
        let matches = self.matches();
        let Some(&i) = matches.get(self.popup_sel.min(matches.len().saturating_sub(1))) else {
            return false;
        };
        let name = self.commands[i].0.clone();
        self.editor.set(&format!("/{name} "));
        self.popup_sel = 0;
        true
    }

    fn submit(&mut self, out: &mut Vec<Emit>) {
        let text = self.editor.text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        if self.history.last() != Some(&text) {
            self.history.push(text.clone());
        }
        self.hist_pos = None;
        self.editor.clear();
        self.popup_hidden = false;
        out.push(Emit::Submit(Value::from(text)));
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.draft = self.editor.text().to_owned();
                self.history.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.hist_pos = Some(pos);
        self.editor.set(&self.history[pos].clone());
    }

    fn history_next(&mut self) {
        let Some(p) = self.hist_pos else { return };
        if p + 1 < self.history.len() {
            self.hist_pos = Some(p + 1);
            self.editor.set(&self.history[p + 1].clone());
        } else {
            self.hist_pos = None;
            let draft = std::mem::take(&mut self.draft);
            self.editor.set(&draft);
        }
    }

    fn input_lines(&self, width: u16) -> (Vec<Line<'static>>, (u16, u16)) {
        let avail = (width as usize).saturating_sub(2).max(1);
        let (lines, (row, col)) = self.editor.render(avail, Default::default());
        let mut out = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            let lead = if i == 0 { "› " } else { "  " };
            let mut spans = vec![Span::styled(lead, theme::user())];
            if self.editor.is_empty() {
                let ph = self.placeholder.clone().unwrap_or_default();
                spans.push(Span::styled(truncate(&ph, avail), theme::muted()));
            } else {
                spans.extend(line.spans);
            }
            out.push(Line::from(spans));
        }
        (out, (row, col + 2))
    }
}

impl Component for Composer {
    fn kind(&self) -> &'static str {
        "composer"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "placeholder", &mut self.placeholder);
        upd_strings(p, "history", &mut self.history);
        if let Some(cmds) = get(p, "commands") {
            self.commands = cmds
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|c| match c {
                            Value::Map(_) => (
                                str_of(c, "name")
                                    .unwrap_or_default()
                                    .trim_start_matches('/')
                                    .to_owned(),
                                str_of(c, "desc").unwrap_or_default().to_owned(),
                            ),
                            other => {
                                (display(other).trim_start_matches('/').to_owned(), String::new())
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        if let Some(v) = get(p, "value") {
            self.editor.set(&display(v));
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let (mut out, _) = self.input_lines(width);
        let matches = self.matches();
        let sel = self.popup_sel.min(matches.len().saturating_sub(1));
        let start = sel.saturating_sub(POPUP_ROWS - 1);
        for (row, &i) in matches.iter().enumerate().skip(start).take(POPUP_ROWS) {
            let (name, desc) = &self.commands[i];
            let selected = row == sel;
            let name = format!("  /{name}");
            let room = (width as usize).saturating_sub(display_width(&name) + 2);
            let style = if selected {
                theme::accent()
            } else {
                theme::muted()
            };
            let mut spans = vec![Span::styled(name, style)];
            if !desc.is_empty() && room > 3 {
                spans.push(Span::styled(format!("  {}", truncate(desc, room)), theme::muted()));
            }
            out.push(Line::from(spans));
        }
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let (code, mods, at) = match input {
            Input::Paste(s) => {
                self.editor.insert(s);
                self.last_char = None;
                return true;
            }
            Input::Key { code, mods, at } => (*code, *mods, *at),
        };
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);
        let popup = !self.matches().is_empty();
        let burst = self
            .last_char
            .is_some_and(|t| at.saturating_duration_since(t) < PASTE_BURST);
        match code {
            KeyCode::Enter if mods.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                self.editor.insert("\n")
            }
            KeyCode::Enter if burst => {
                self.editor.insert("\n");
                self.last_char = Some(at);
            }
            KeyCode::Enter if popup => {
                self.complete();
            }
            KeyCode::Enter => self.submit(out),
            KeyCode::Char('j') if ctrl => self.editor.insert("\n"),
            KeyCode::Tab if popup => {
                self.complete();
            }
            KeyCode::Up if popup => self.popup_sel = self.popup_sel.saturating_sub(1),
            KeyCode::Down if popup => {
                self.popup_sel = (self.popup_sel + 1).min(self.matches().len().saturating_sub(1))
            }
            KeyCode::Up => {
                if !self.editor.up() {
                    self.history_prev();
                }
            }
            KeyCode::Down => {
                if !self.editor.down() {
                    self.history_next();
                }
            }
            KeyCode::Esc if popup => self.popup_hidden = true,
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
            KeyCode::Char('a') if ctrl && !alt => self.editor.home(),
            KeyCode::Char('e') if ctrl && !alt => self.editor.end(),
            KeyCode::Char('u') if ctrl && !alt => self.editor.kill_to_line_start(),
            KeyCode::Char('w') if ctrl && !alt => self.editor.delete_word_back(),
            KeyCode::Char(c) if !ctrl || alt => {
                self.editor.insert(c.encode_utf8(&mut [0; 4]));
                self.last_char = Some(at);
                self.popup_hidden = false;
                self.popup_sel = 0;
            }
            _ => return false,
        }
        true
    }

    fn cursor(&self, width: u16) -> Option<(u16, u16)> {
        Some(self.input_lines(width).1)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
