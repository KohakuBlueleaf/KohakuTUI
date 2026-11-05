//! `picker`: a filterable list. Typing filters by fuzzy subsequence; enter selects.

use std::any::Any;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::props::{upd_opt_str, upd_str, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, Emit, Input, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{display_width, theme};

#[derive(Debug, Clone)]
struct Item {
    label: String,
    value: Value,
    detail: Option<String>,
}

#[derive(Debug)]
pub struct Picker {
    title: String,
    placeholder: Option<String>,
    items: Vec<Item>,
    filter: TextEditor,
    selected: usize,
    max_visible: usize,
}

impl Default for Picker {
    fn default() -> Self {
        Picker {
            title: String::new(),
            placeholder: None,
            items: Vec::new(),
            filter: TextEditor::default(),
            selected: 0,
            max_visible: 10,
        }
    }
}

/// Fuzzy score of `query` against `label`: None when it is not a subsequence; higher is better.
pub fn fuzzy_score(query: &str, label: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let label: Vec<char> = label.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut pos = 0usize;
    let mut prev: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        let found = (pos..label.len()).find(|&i| label[i] == q)?;
        score += 10;
        if prev.is_some_and(|p| p + 1 == found) {
            score += 15;
        }
        if found == 0 || !label[found - 1].is_alphanumeric() {
            score += 8;
        }
        score -= (found - pos) as i64;
        prev = Some(found);
        pos = found + 1;
    }
    Some(score)
}

impl Picker {
    /// Indices of matching items, best first.
    fn filtered(&self) -> Vec<usize> {
        let q = self.filter.text();
        let mut scored: Vec<(i64, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| fuzzy_score(q, &it.label).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn window(&self, count: usize) -> (usize, usize) {
        let vis = self.max_visible.max(1).min(count.max(1));
        let start = self
            .selected
            .saturating_sub(vis - 1)
            .min(count.saturating_sub(vis));
        (start, (start + vis).min(count))
    }
}

impl Component for Picker {
    fn kind(&self) -> &'static str {
        "picker"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "title", &mut self.title);
        upd_opt_str(p, "placeholder", &mut self.placeholder);
        upd_usize(p, "max_visible", &mut self.max_visible, 10);
        if let Some(items) = get(p, "items") {
            self.items = items
                .as_array()
                .map(|v| v.iter().map(item_of).collect())
                .unwrap_or_default();
            self.selected = 0;
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        match data {
            Value::Array(items) => self.items.extend(items.iter().map(item_of)),
            item => self.items.push(item_of(item)),
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let mut out = Vec::new();
        if !self.title.is_empty() {
            out.push(Line::from(Span::styled(self.title.clone(), theme::bold())));
        }

        let mut filter = vec![Span::styled("› ", theme::accent())];
        let room = width.saturating_sub(2);
        if self.filter.is_empty() {
            let ph = self.placeholder.as_deref().unwrap_or("type to filter");
            filter.push(Span::styled(truncate(ph, room), theme::muted()));
        } else {
            filter.push(Span::raw(truncate(self.filter.text(), room)));
        }
        out.push(Line::from(filter));
        let matches = self.filtered();
        if matches.is_empty() {
            out.push(Line::from(Span::styled("  no matches", theme::muted())));
        }
        let (start, end) = self.window(matches.len());
        for (row, &i) in matches[start..end].iter().enumerate() {
            let item = &self.items[i];
            let selected = start + row == self.selected;
            let marker = if selected { "› " } else { "  " };
            let label = truncate(&item.label, width.saturating_sub(2));
            let style = if selected {
                theme::accent().add_modifier(Modifier::BOLD)
            } else {
                Default::default()
            };
            let mut spans = vec![
                Span::styled(marker, theme::accent()),
                Span::styled(label.clone(), style),
            ];
            if let Some(detail) = &item.detail {
                let room = width.saturating_sub(2 + display_width(&label) + 2);
                if room > 3 {
                    spans.push(Span::styled(
                        format!("  {}", truncate(detail, room)),
                        theme::muted(),
                    ));
                }
            }
            out.push(Line::from(spans));
        }
        let more = matches.len().saturating_sub(end - start);
        let hint = if more > 0 {
            format!("  {} of {} · ↑↓ move · enter select · esc cancel", end - start, matches.len())
        } else {
            "  ↑↓ move · enter select · esc cancel".to_owned()
        };
        out.push(Line::from(Span::styled(truncate(&hint, width), theme::muted())));
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let count = self.filtered().len();
        match input {
            Input::Paste(s) => {
                self.filter.insert(&s.replace('\n', " "));
                self.selected = 0;
            }
            Input::Key { code, mods, .. } => {
                let ctrl = mods.contains(KeyModifiers::CONTROL);
                match code {
                    KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                    KeyCode::Char('p') if ctrl => self.selected = self.selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab => {
                        self.selected = (self.selected + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::Char('n') if ctrl => {
                        self.selected = (self.selected + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::PageUp => {
                        self.selected = self.selected.saturating_sub(self.max_visible)
                    }
                    KeyCode::PageDown => {
                        self.selected =
                            (self.selected + self.max_visible).min(count.saturating_sub(1))
                    }
                    KeyCode::Enter => {
                        if let Some(&i) = self.filtered().get(self.selected) {
                            out.push(Emit::Select {
                                index: i as u32,
                                value: self.items[i].value.clone(),
                            });
                        }
                    }
                    KeyCode::Backspace => {
                        self.filter.backspace();
                        self.selected = 0;
                    }
                    KeyCode::Left => self.filter.left(),
                    KeyCode::Right => self.filter.right(),
                    KeyCode::Char('u') if ctrl => {
                        self.filter.clear();
                        self.selected = 0;
                    }
                    KeyCode::Char(c) if !ctrl || mods.contains(KeyModifiers::ALT) => {
                        self.filter.insert(c.encode_utf8(&mut [0; 4]));
                        self.selected = 0;
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    fn cursor(&self, _width: u16) -> Option<(u16, u16)> {
        let row = u16::from(!self.title.is_empty());
        let col = display_width(&self.filter.text()[..self.filter.cursor()]) as u16;
        Some((row, 2 + col))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn item_of(v: &Value) -> Item {
    match v {
        Value::Map(_) => {
            let label = get(v, "label").map(display).unwrap_or_default();
            Item {
                value: get(v, "value")
                    .cloned()
                    .unwrap_or_else(|| Value::from(label.as_str())),
                detail: str_of(v, "detail").map(str::to_owned),
                label,
            }
        }
        other => Item { label: display(other), value: other.clone(), detail: None },
    }
}
