//! `composer`: the chat input. Multi-line editing, history, a slash-command popup and paste-burst
//! detection for terminals that deliver a paste as plain keys (design §7.2). With `boxed` it
//! draws inside a rounded border, Claude Code style.

use std::any::Any;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::picker::fuzzy_score;
use super::props::{upd_bool, upd_opt_str, upd_strings};
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
    boxed: bool,
    /// Submit `{text, now}` instead of the text, `now` set by ctrl+enter.
    rich_submit: bool,
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

    /// Whether the input is exactly `/name` of the highlighted command.
    fn exact_command(&self) -> bool {
        let matches = self.matches();
        let sel = self.popup_sel.min(matches.len().saturating_sub(1));
        match (matches.get(sel), self.editor.text().strip_prefix('/')) {
            (Some(&i), Some(typed)) => self.commands[i].0 == typed,
            _ => false,
        }
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
        self.submit_with(out, false);
    }

    fn submit_with(&mut self, out: &mut Vec<Emit>, now: bool) {
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
        let value = match self.rich_submit {
            true => Value::Map(vec![("text".into(), text.into()), ("now".into(), now.into())]),
            false => Value::from(text),
        };
        out.push(Emit::Submit(value));
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

    /// The input rows and the caret within them, boxed or plain.
    fn input_lines(&self, width: u16) -> (Vec<Line<'static>>, (u16, u16)) {
        const MIN_BOX: u16 = 8;
        if !self.boxed || width < MIN_BOX {
            return self.prompt_lines(width, "› ");
        }

        let inner = width - 4;
        let (rows, (row, col)) = self.prompt_lines(inner, "> ");
        let border = theme::muted();
        let rule = "─".repeat(width as usize - 2);
        let mut out = vec![Line::from(Span::styled(format!("╭{rule}╮"), border))];
        for line in rows {
            let pad = (inner as usize).saturating_sub(line.width());
            let mut spans = vec![Span::styled("│ ", border)];
            spans.extend(line.spans);
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(" │", border));
            out.push(Line::from(spans));
        }
        out.push(Line::from(Span::styled(format!("╰{rule}╯"), border)));
        (out, (row + 1, col + 2))
    }

    fn prompt_lines(&self, width: u16, prompt: &'static str) -> (Vec<Line<'static>>, (u16, u16)) {
        let avail = (width as usize).saturating_sub(2).max(1);
        let (lines, (row, col)) = self.editor.render(avail, Default::default());
        let mut out = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            let lead = if i == 0 { prompt } else { "  " };
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
        upd_bool(p, "boxed", &mut self.boxed, false);
        upd_bool(p, "rich_submit", &mut self.rich_submit, false);
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
            KeyCode::Enter if mods.contains(KeyModifiers::CONTROL) && !burst => {
                self.submit_with(out, true)
            }
            KeyCode::Enter if burst => {
                self.editor.insert("\n");
                self.last_char = Some(at);
            }
            // `\` + enter is a newline (Claude Code), for terminals that drop shift+enter
            KeyCode::Enter if self.editor.text()[..self.editor.cursor()].ends_with('\\') => {
                self.editor.backspace();
                self.editor.insert("\n");
            }
            KeyCode::Enter if popup && !self.exact_command() => {
                self.complete();
            }
            KeyCode::Enter => self.submit(out),
            KeyCode::Char('j') if ctrl => self.editor.insert("\n"),
            // ctrl+enter's twin for terminals that send it as a plain enter or ctrl+j
            KeyCode::Char('s') if ctrl && self.rich_submit => self.submit_with(out, true),
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
                // with no line below and no history to walk, ↓ leaves the input
                if !self.editor.down() {
                    if self.hist_pos.is_none() {
                        return false;
                    }
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
