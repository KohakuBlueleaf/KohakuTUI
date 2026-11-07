//! `composer`: the chat input. Multi-line editing, history, a slash-command popup and paste-burst
//! detection for terminals that deliver a paste as plain keys (design §7.2). With `boxed` it
//! draws inside a rounded border, Claude Code style.
//!
//! The rest is opt-in, each by a prop:
//!
//! * `triggers` (e.g. `["@"]`): a word starting with one asks the app for completions with a
//!   `change` `{"complete": {"trigger", "query"}}`; the app answers with `suggestions`
//!   (`[{label, value, detail}]`), shown in the popup; tab / enter puts the value in place of
//!   the word (a value ending in `/` keeps completing, as a folder does).
//! * ctrl+r: reverse search through the history (typing narrows, ctrl+r goes older, enter or
//!   → takes the match, esc goes back to what was typed).
//! * `fold_paste` (lines): a longer paste shows as `[Pasted text #n +L lines]`; submit sends the
//!   whole text, backspace after the token removes the paste.
//! * `modes` (`[{prefix, label}]`): text starting with the prefix (`!`) shows the prefix as the
//!   prompt and the label on the box.
//! * `attachments` (labels): chips above the text; backspace at the start of an empty input
//!   asks to drop the last one with `change` `{"detach": index}`.
//! * ctrl+g: `change` `{"edit": text}`, for an external editor (the app sets `value` back).
//! * `report_draft`: `change` `{"draft": text}` whenever the text changes, so the app can keep
//!   a draft per conversation (a `value` it sets is not reported back).

use std::any::Any;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::picker::fuzzy_score;
use super::props::{upd_bool, upd_opt_str, upd_strings, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, Emit, Input, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{display_width, theme};

/// Keys closer together than this are a paste, so an Enter among them is a newline.
pub const PASTE_BURST: Duration = Duration::from_millis(8);

const POPUP_ROWS: usize = 6;
/// A paste longer than this many characters folds too, whatever its line count.
const FOLD_CHARS: usize = 2000;

#[derive(Debug, Default, Clone, PartialEq)]
struct Suggestion {
    label: String,
    value: String,
    detail: String,
}

/// The ctrl+r search: what is typed, the history entry it matches, the text it replaced.
#[derive(Debug, Default)]
struct Search {
    query: String,
    hit: Option<usize>,
    saved: String,
}

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
    pub(crate) boxed: bool,
    /// Submit `{text, now}` instead of the text, `now` set by ctrl+enter.
    rich_submit: bool,
    triggers: Vec<String>,
    suggestions: Vec<Suggestion>,
    /// The (trigger, query) last asked for, so a word is asked about once.
    asked: Option<(String, String)>,
    search: Option<Search>,
    fold_paste: usize,
    /// (token shown, text it stands for)
    pastes: Vec<(String, String)>,
    modes: Vec<(String, String)>,
    attachments: Vec<String>,
    /// Report the text as `change` `{"draft": text}` whenever it changes.
    report_draft: bool,
    reported: String,
}

/// What the popup lists.
enum Popup {
    None,
    Commands(Vec<usize>),
    Suggestions(Vec<usize>),
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

    /// The word before the caret when it starts with a trigger: (trigger, query, word bytes).
    fn word(&self) -> Option<(String, String, usize)> {
        if self.triggers.is_empty() {
            return None;
        }
        let before = self.editor.before();
        let start = before
            .rfind(char::is_whitespace)
            .map_or(0, |i| i + before[i..].chars().next().map_or(1, char::len_utf8));
        let word = &before[start..];
        let trigger = self
            .triggers
            .iter()
            .find(|t| word.starts_with(t.as_str()))?;
        Some((trigger.clone(), word[trigger.len()..].to_owned(), word.len()))
    }

    /// Suggestions matching the word being completed, best first.
    fn suggested(&self) -> Vec<usize> {
        if self.popup_hidden || self.suggestions.is_empty() {
            return Vec::new();
        }
        let Some((_, query, _)) = self.word() else {
            return Vec::new();
        };
        let mut scored: Vec<(i64, usize)> = self
            .suggestions
            .iter()
            .enumerate()
            .filter_map(|(i, s)| fuzzy_score(&query, &s.label).map(|sc| (sc, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn popup(&self) -> Popup {
        if self.search.is_some() {
            return Popup::None;
        }
        let cmds = self.matches();
        if !cmds.is_empty() {
            return Popup::Commands(cmds);
        }
        let sug = self.suggested();
        if sug.is_empty() {
            Popup::None
        } else {
            Popup::Suggestions(sug)
        }
    }

    fn popup_len(&self) -> usize {
        match self.popup() {
            Popup::None => 0,
            Popup::Commands(v) | Popup::Suggestions(v) => v.len(),
        }
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

    /// Takes the highlighted popup entry.
    fn complete(&mut self, out: &mut Vec<Emit>) -> bool {
        match self.popup() {
            Popup::None => false,
            Popup::Commands(matches) => {
                let i = matches[self.popup_sel.min(matches.len() - 1)];
                let name = self.commands[i].0.clone();
                self.editor.set(&format!("/{name} "));
                self.popup_sel = 0;
                true
            }
            Popup::Suggestions(matches) => {
                let i = matches[self.popup_sel.min(matches.len() - 1)];
                let value = self.suggestions[i].value.clone();
                let Some((trigger, _, len)) = self.word() else {
                    return false;
                };
                let folder = value.ends_with('/');
                let text = if folder {
                    format!("{trigger}{value}")
                } else {
                    format!("{trigger}{value} ")
                };
                self.editor.replace_before(len, &text);
                self.popup_sel = 0;
                if !folder {
                    self.suggestions.clear();
                }
                self.ask(out);
                true
            }
        }
    }

    /// Asks the app for completions when the word being typed changed.
    fn ask(&mut self, out: &mut Vec<Emit>) {
        let now = self.word().map(|(t, q, _)| (t, q));
        if now == self.asked {
            return;
        }
        self.asked = now.clone();
        if let Some((trigger, query)) = now {
            self.popup_hidden = false;
            let ask = Value::Map(vec![
                ("trigger".into(), trigger.into()),
                ("query".into(), query.into()),
            ]);
            out.push(Emit::Change(Value::Map(vec![("complete".into(), ask)])));
        }
    }

    /// The text with every paste token replaced by what it stands for.
    fn expanded(&self) -> String {
        let mut text = self.editor.text().to_owned();
        for (token, content) in &self.pastes {
            text = text.replacen(token.as_str(), content, 1);
        }
        text
    }

    fn paste(&mut self, s: &str) {
        let lines = s.lines().count();
        let fold =
            self.fold_paste > 0 && (lines > self.fold_paste || s.chars().count() > FOLD_CHARS);
        if !fold {
            self.editor.insert(s);
            return;
        }
        let token = format!("[Pasted text #{} +{lines} lines]", self.pastes.len() + 1);
        self.editor.insert(&token);
        self.pastes.push((token, s.to_owned()));
    }

    /// Backspace: a paste token goes whole; at the start of an empty input, the last chip.
    fn backspace(&mut self, out: &mut Vec<Emit>) {
        let before = self.editor.before();
        if let Some(i) = self
            .pastes
            .iter()
            .position(|(t, _)| before.ends_with(t.as_str()))
        {
            let len = self.pastes[i].0.len();
            self.editor.replace_before(len, "");
            self.pastes.remove(i);
            return;
        }
        if self.editor.is_empty() && !self.attachments.is_empty() {
            let last = self.attachments.len() - 1;
            out.push(Emit::Change(Value::Map(vec![("detach".into(), Value::from(last as f64))])));
            return;
        }
        self.editor.backspace();
    }

    fn submit(&mut self, out: &mut Vec<Emit>) {
        self.submit_with(out, false);
    }

    fn submit_with(&mut self, out: &mut Vec<Emit>, now: bool) {
        let text = self.expanded();
        if text.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        if !text.trim().is_empty() && self.history.last() != Some(&text) {
            self.history.push(text.clone());
        }
        self.hist_pos = None;
        self.editor.clear();
        self.pastes.clear();
        self.suggestions.clear();
        self.asked = None;
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

    /// The newest history entry at or before `from` containing `query`.
    fn find(&self, query: &str, from: usize) -> Option<usize> {
        let q = query.to_lowercase();
        (0..=from.min(self.history.len().checked_sub(1)?))
            .rev()
            .find(|&i| self.history[i].to_lowercase().contains(&q))
    }

    /// ctrl+r keys; returns whether the search took the key.
    fn search_key(&mut self, code: KeyCode, ctrl: bool) -> bool {
        let Some(mut search) = self.search.take() else {
            return false;
        };
        let last = self.history.len().saturating_sub(1);
        match code {
            KeyCode::Char('r') if ctrl => {
                let from = search.hit.map_or(last, |h| h.saturating_sub(1));
                if search.hit != Some(0) {
                    search.hit = self.find(&search.query, from).or(search.hit);
                }
            }
            KeyCode::Esc | KeyCode::Char('g' | 'c') if code == KeyCode::Esc || ctrl => {
                self.editor.set(&search.saved);
                return true;
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::Right | KeyCode::End => {
                let text = search.hit.map(|h| self.history[h].clone());
                self.editor.set(&text.unwrap_or(search.saved));
                return true;
            }
            KeyCode::Backspace => {
                search.query.pop();
                search.hit = self.find(&search.query, last);
            }
            KeyCode::Char(c) if !ctrl => {
                search.query.push(c);
                search.hit = self.find(&search.query, last);
            }
            _ => {}
        }
        self.search = Some(search);
        true
    }

    fn key(&mut self, code: KeyCode, mods: KeyModifiers, at: Instant, out: &mut Vec<Emit>) -> bool {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);
        let popup = self.popup_len() > 0;
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
            KeyCode::Enter if self.editor.before().ends_with('\\') => {
                self.editor.backspace();
                self.editor.insert("\n");
            }
            KeyCode::Enter if popup && !self.exact_command() => {
                self.complete(out);
            }
            KeyCode::Enter => self.submit(out),
            KeyCode::Char('j') if ctrl => self.editor.insert("\n"),
            // ctrl+enter's twin for terminals that send it as a plain enter or ctrl+j
            KeyCode::Char('s') if ctrl && self.rich_submit => self.submit_with(out, true),
            KeyCode::Char('r') if ctrl && !alt => {
                let saved = self.editor.text().to_owned();
                let hit = self.history.len().checked_sub(1);
                self.search = Some(Search { query: String::new(), hit, saved });
            }
            KeyCode::Char('g') if ctrl && !alt => {
                let text = self.expanded();
                out.push(Emit::Change(Value::Map(vec![("edit".into(), text.into())])));
            }
            KeyCode::Tab if popup => {
                self.complete(out);
            }
            KeyCode::Up if popup => self.popup_sel = self.popup_sel.saturating_sub(1),
            KeyCode::Down if popup => {
                self.popup_sel = (self.popup_sel + 1).min(self.popup_len().saturating_sub(1))
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
            KeyCode::Backspace => self.backspace(out),
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

    /// The mode whose prefix the text starts with.
    fn mode(&self) -> Option<&(String, String)> {
        let text = self.editor.text();
        self.modes
            .iter()
            .find(|(p, _)| !p.is_empty() && text.starts_with(p.as_str()))
    }

    /// The input rows and the caret within them, boxed or plain.
    fn input_lines(&self, width: u16) -> (Vec<Line<'static>>, (u16, u16)) {
        const MIN_BOX: u16 = 8;
        if !self.boxed || width < MIN_BOX {
            let (mut rows, (row, col)) = self.prompt_lines(width, "› ");
            let chips = self.chips(width as usize);
            let shift = u16::from(chips.is_some());
            if let Some(chips) = chips {
                rows.insert(0, chips);
            }
            return (rows, (row + shift, col));
        }

        let inner = width - 4;
        let (mut rows, (row, col)) = self.prompt_lines(inner, "> ");
        let chips = self.chips(inner as usize);
        let shift = u16::from(chips.is_some());
        if let Some(chips) = chips {
            rows.insert(0, chips);
        }
        let border = match self.mode() {
            Some(_) => theme::warn(),
            None => theme::muted(),
        };
        let top = match self.mode() {
            Some((_, label))
                if !label.is_empty() && (width as usize) > display_width(label) + 6 =>
            {
                let label = format!(" {label} ");
                let rest = (width as usize - 2).saturating_sub(display_width(&label) + 1);
                format!("╭─{label}{}╮", "─".repeat(rest))
            }
            _ => format!("╭{}╮", "─".repeat(width as usize - 2)),
        };
        let rule = "─".repeat(width as usize - 2);
        let mut out = vec![Line::from(Span::styled(top, border))];
        for line in rows {
            let pad = (inner as usize).saturating_sub(line.width());
            let mut spans = vec![Span::styled("│ ", border)];
            spans.extend(line.spans);
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(" │", border));
            out.push(Line::from(spans));
        }
        out.push(Line::from(Span::styled(format!("╰{rule}╯"), border)));
        (out, (row + 1 + shift, col + 2))
    }

    /// The attachment chips as one row, or None without attachments.
    fn chips(&self, width: usize) -> Option<Line<'static>> {
        if self.attachments.is_empty() {
            return None;
        }
        let text = self
            .attachments
            .iter()
            .map(|a| format!("[⎘ {a}]"))
            .collect::<Vec<_>>()
            .join(" ");
        Some(Line::from(Span::styled(truncate(&text, width), theme::accent())))
    }

    fn prompt_lines(&self, width: u16, prompt: &'static str) -> (Vec<Line<'static>>, (u16, u16)) {
        let avail = (width as usize).saturating_sub(2).max(1);
        if let Some(search) = &self.search {
            let found = search
                .hit
                .and_then(|h| self.history.get(h))
                .map(|t| t.lines().next().unwrap_or_default().to_owned());
            let label = match found {
                Some(_) => "search history",
                None if search.query.is_empty() => "search history",
                None => "no match",
            };
            let head = format!("({label}) {}", search.query);
            let col = display_width(&head) as u16 + 2;
            let mut spans = vec![
                Span::styled("⌕ ", theme::accent()),
                Span::styled(head.clone(), theme::accent()),
            ];
            if let Some(found) = found {
                let room = avail.saturating_sub(display_width(&head) + 3);
                spans.push(Span::styled(format!(" › {}", truncate(&found, room)), theme::muted()));
            }
            return (vec![Line::from(spans)], (0, col));
        }
        let (lines, (row, col)) = self.editor.render(avail, Default::default());
        let lead_style = match self.mode() {
            Some(_) => theme::warn(),
            None => theme::user(),
        };
        let mut out = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            let lead = if i == 0 { prompt } else { "  " };
            let mut spans = vec![Span::styled(lead, lead_style)];
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

fn suggestions_of(v: &Value) -> Vec<Suggestion> {
    v.as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| match item {
                    Value::Map(_) => {
                        let label = str_of(item, "label").unwrap_or_default().to_owned();
                        Suggestion {
                            value: str_of(item, "value")
                                .map_or_else(|| label.clone(), str::to_owned),
                            detail: str_of(item, "detail").unwrap_or_default().to_owned(),
                            label,
                        }
                    }
                    other => Suggestion {
                        label: display(other),
                        value: display(other),
                        detail: String::new(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

fn modes_of(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|items| {
            items
                .iter()
                .map(|m| {
                    (
                        str_of(m, "prefix").unwrap_or_default().to_owned(),
                        str_of(m, "label").unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
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
        upd_strings(p, "triggers", &mut self.triggers);
        upd_usize(p, "fold_paste", &mut self.fold_paste, 0);
        upd_strings(p, "attachments", &mut self.attachments);
        upd_bool(p, "report_draft", &mut self.report_draft, false);
        if let Some(v) = get(p, "suggestions") {
            self.suggestions = suggestions_of(v);
            self.popup_sel = 0;
        }
        if let Some(v) = get(p, "modes") {
            self.modes = modes_of(v);
        }
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
            self.pastes.clear();
            self.search = None;
            // set by the app: not reported back
            self.reported = self.editor.text().to_owned();
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let (mut out, _) = self.input_lines(width);
        let rows: Vec<(String, String)> = match self.popup() {
            Popup::None => Vec::new(),
            Popup::Commands(m) => m
                .iter()
                .map(|&i| (format!("  /{}", self.commands[i].0), self.commands[i].1.clone()))
                .collect(),
            Popup::Suggestions(m) => m
                .iter()
                .map(|&i| {
                    let s = &self.suggestions[i];
                    (format!("  {}", s.label), s.detail.clone())
                })
                .collect(),
        };
        let sel = self.popup_sel.min(rows.len().saturating_sub(1));
        let start = sel.saturating_sub(POPUP_ROWS - 1);
        for (row, (name, desc)) in rows.iter().enumerate().skip(start).take(POPUP_ROWS) {
            let selected = row == sel;
            let name = name.clone();
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
        let consumed = self.on_input_inner(input, out);
        if self.report_draft && self.search.is_none() {
            let text = self.expanded();
            if text != self.reported {
                out.push(Emit::Change(Value::Map(vec![("draft".into(), text.clone().into())])));
                self.reported = text;
            }
        }
        consumed
    }

    fn cursor(&self, width: u16) -> Option<(u16, u16)> {
        Some(self.input_lines(width).1)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Composer {
    fn on_input_inner(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let (code, mods, at) = match input {
            Input::Paste(s) => {
                if self.search.is_some() {
                    return true;
                }
                self.paste(s);
                self.last_char = None;
                self.ask(out);
                return true;
            }
            Input::Key { code, mods, at } => (*code, *mods, *at),
        };
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        if self.search_key(code, ctrl) {
            return true;
        }
        let consumed = self.key(code, mods, at, out);
        if consumed {
            self.ask(out);
        }
        consumed
    }
}
