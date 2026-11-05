//! `picker`: a filterable list. Typing filters by fuzzy subsequence; enter or a click selects;
//! the wheel moves the selection. With `back`, ← at the start of the filter closes it with
//! reason `back` (one level up in a stack of dialogs).
//!
//! Items may carry a `group` (a section header, also matched by the filter as `group/label`),
//! `current` (marked ✓; the list opens on it), `available: false` (dimmed) and `choices`: option
//! groups `[[name, [options]], …]` with the initially `chosen` `{name: option}`. On a row with
//! choices selecting it emits `{"value": …, "choices": {name: option}}`.
//!
//! With `multi`, rows carry ☐/☑: space ticks the highlighted one, enter answers every ticked
//! value in item order (the highlighted one when none is ticked); an item may start `checked`.
//!
//! Keys work on one focused area at a time: **sections** (the sidebar), the **list**, and the
//! highlighted item's **options**. ←/→ (or tab / shift+tab) move the focus across them, ↑↓
//! always act on the focused one (section, item, option value), enter selects from anywhere and
//! typing filters (focus goes to the list). The focused area is highlighted and the hint names
//! only its keys.
//!
//! With `sidebar`, groups become a category column on the left (a tab row when narrow): each
//! category lists only its items (`section` opens on a named one), and while filtering the column counts matches per category
//! and the list moves to one that has any. The body keeps a
//! fixed height so switching categories does not make the dialog jump. `dock: "input"` asks the
//! view to draw the dialog where the input box is instead of over the content.

use std::any::Any;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::editor::TextEditor;
use super::props::{upd_bool, upd_opt_str, upd_str, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{display_width, theme};

#[derive(Debug, Clone)]
struct Item {
    label: String,
    value: Value,
    detail: Option<String>,
    group: Option<String>,
    current: bool,
    available: bool,
    /// Option groups in display order, each with its options.
    choices: Vec<(String, Vec<String>)>,
    /// The chosen option per group (missing: the group's first option).
    chosen: Vec<String>,
    /// Which group ←/→ change.
    active: usize,
    /// Ticked in a `multi` picker.
    checked: bool,
}

impl Item {
    fn haystack(&self) -> String {
        match &self.group {
            Some(g) => format!("{g}/{}", self.label),
            None => self.label.clone(),
        }
    }

    fn cycle(&mut self, delta: isize) {
        let Some((_, options)) = self.choices.get(self.active) else {
            return;
        };
        if options.is_empty() {
            return;
        }
        let at = options
            .iter()
            .position(|o| *o == self.chosen[self.active])
            .unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(options.len() as isize) as usize;
        self.chosen[self.active] = options[next].clone();
    }

    fn select_value(&self) -> Value {
        if self.choices.is_empty() {
            return self.value.clone();
        }
        let chosen = self
            .choices
            .iter()
            .zip(&self.chosen)
            .map(|((name, _), opt)| (Value::from(name.as_str()), Value::from(opt.as_str())))
            .collect();
        Value::Map(vec![
            ("value".into(), self.value.clone()),
            ("choices".into(), Value::Map(chosen)),
        ])
    }
}

/// One visible row: a group header or an item index.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Row {
    Header(usize),
    Item(usize),
}

#[derive(Debug)]
pub struct Picker {
    title: String,
    placeholder: Option<String>,
    items: Vec<Item>,
    filter: TextEditor,
    /// Highlighted position in `view()`.
    selected: usize,
    max_visible: usize,
    back: bool,
    sidebar: bool,
    dock: bool,
    /// Current category (index into `categories()`) when the sidebar is on.
    cat: usize,
    zone: Zone,
    /// Several items can be ticked (space); enter answers all ticked values.
    multi: bool,
}

/// The area the keys act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Zone {
    Sections,
    #[default]
    List,
    Options,
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
            back: false,
            sidebar: false,
            dock: false,
            cat: 0,
            zone: Zone::List,
            multi: false,
        }
    }
}

/// Narrower than this, the category column becomes a tab row.
const SIDEBAR_MIN_WIDTH: usize = 50;

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

fn category_of(item: &Item) -> &str {
    item.group.as_deref().unwrap_or("other")
}

impl Picker {
    /// Indices of matching items, best first.
    fn filtered(&self) -> Vec<usize> {
        let q = self.filter.text();
        let mut scored: Vec<(i64, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| fuzzy_score(q, &it.haystack()).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    /// Categories in the order their first item appears.
    fn categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = Vec::new();
        for item in &self.items {
            let c = category_of(item);
            if !cats.iter().any(|x| x == c) {
                cats.push(c.to_owned());
            }
        }
        cats
    }

    fn sidebar_on(&self) -> bool {
        self.sidebar && self.categories().len() > 1
    }

    /// The listed items: the matches, within the current category when the sidebar is on.
    fn view(&self) -> Vec<usize> {
        let matches = self.filtered();
        if !self.sidebar_on() {
            return matches;
        }
        let cats = self.categories();
        let Some(cat) = cats.get(self.cat) else {
            return matches;
        };
        matches
            .into_iter()
            .filter(|&i| category_of(&self.items[i]) == cat)
            .collect()
    }

    /// After the filter changed: back to the top, and to a category with matches if this one
    /// has none.
    fn refiltered(&mut self) {
        self.selected = 0;
        if self.sidebar_on() && self.view().is_empty() {
            if let Some(&first) = self.filtered().first() {
                let cat = category_of(&self.items[first]).to_owned();
                self.cat = self
                    .categories()
                    .iter()
                    .position(|c| *c == cat)
                    .unwrap_or(0);
            }
        }
    }

    fn switch_category(&mut self, delta: isize) {
        let n = self.categories().len() as isize;
        if n == 0 {
            return;
        }
        self.cat = (self.cat as isize + delta).rem_euclid(n) as usize;
        self.selected = self
            .view()
            .iter()
            .position(|&i| self.items[i].current)
            .unwrap_or(0);
    }

    fn emit_selected(&self, out: &mut Vec<Emit>) {
        let Some(&i) = self.view().get(self.selected) else {
            return;
        };
        if !self.multi {
            out.push(Emit::Select { index: i as u32, value: self.items[i].select_value() });
            return;
        }
        // every ticked value in item order; with none ticked, the highlighted one
        let mut values: Vec<Value> = self
            .items
            .iter()
            .filter(|it| it.checked)
            .map(Item::select_value)
            .collect();
        if values.is_empty() {
            values.push(self.items[i].select_value());
        }
        out.push(Emit::Select { index: i as u32, value: Value::Array(values) });
    }

    /// The highlighted item, if any.
    fn highlighted_mut(&mut self) -> Option<&mut Item> {
        let i = *self.view().get(self.selected)?;
        self.items.get_mut(i)
    }

    /// Rows of the visible window: a header before each run of one group (no sidebar).
    fn rows(&self, matches: &[usize], start: usize, end: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut prev: Option<&Option<String>> = None;
        for &i in &matches[start..end] {
            let group = &self.items[i].group;
            if group.is_some() && prev != Some(group) {
                rows.push(Row::Header(i));
            }
            prev = Some(group);
            rows.push(Row::Item(i));
        }
        rows
    }

    fn render_item(&self, i: usize, selected: bool, width: usize) -> Line<'static> {
        let item = &self.items[i];
        // the list's highlight is strong only while the list (or its options) has the keys
        let focused = selected && self.zone != Zone::Sections;
        let marker = match (selected, focused, item.current) {
            (true, true, _) => Span::styled("› ", theme::accent()),
            (true, false, _) => Span::styled("› ", theme::faint()),
            (false, _, true) => Span::styled("✓ ", theme::ok()),
            _ => Span::raw("  "),
        };
        let style = match (selected, item.available) {
            (_, false) => theme::faint(),
            (true, true) if focused && self.zone == Zone::List => {
                theme::accent().add_modifier(Modifier::BOLD)
            }
            (true, true) => theme::bold(),
            _ => Default::default(),
        };
        let mut spans = vec![marker];
        let mut used = 2;
        if self.multi {
            let (mark, tone) = if item.checked {
                ("☑ ", theme::ok())
            } else {
                ("☐ ", theme::muted())
            };
            spans.push(Span::styled(mark, tone));
            used += 2;
        }
        let label = truncate(&item.label, width.saturating_sub(used));
        used += display_width(&label);
        spans.push(Span::styled(label, style));

        for (g, ((name, _), opt)) in item.choices.iter().zip(&item.chosen).enumerate() {
            let chip = format!("  [{name}={opt}]");
            if used + display_width(&chip) > width {
                spans.push(Span::styled(" …", theme::faint()));
                return Line::from(spans);
            }
            used += display_width(&chip);
            let chip_style = match (selected, self.zone, g == item.active) {
                (true, Zone::Options, true) => theme::selection().add_modifier(Modifier::BOLD),
                (true, _, _) => theme::muted(),
                _ => theme::faint(),
            };
            spans.push(Span::styled(chip, chip_style));
        }
        if let Some(detail) = &item.detail {
            let room = width.saturating_sub(used + 2);
            if room > 3 {
                spans.push(Span::styled(format!("  {}", truncate(detail, room)), theme::muted()));
            }
        }
        if !item.available && used + 14 <= width {
            spans.push(Span::styled("  unavailable", theme::warn()));
        }
        Line::from(spans)
    }

    fn window(&self, count: usize) -> (usize, usize) {
        let vis = self.max_visible.max(1).min(count.max(1));
        let start = self
            .selected
            .saturating_sub(vis - 1)
            .min(count.saturating_sub(vis));
        (start, (start + vis).min(count))
    }

    /// Width of the category column, 0 when there is none.
    fn sidebar_width(&self, width: usize) -> usize {
        if !self.sidebar_on() || width < SIDEBAR_MIN_WIDTH {
            return 0;
        }
        let longest = self
            .categories()
            .iter()
            .map(|c| display_width(c))
            .max()
            .unwrap_or(0);
        (longest + 8).min(width / 3).min(28)
    }

    /// Category rows for the column: name and item (or match) count, the current one marked.
    fn category_cells(&self, width: usize) -> Vec<Line<'static>> {
        let filtering = !self.filter.is_empty();
        let matches = self.filtered();
        self.categories()
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let n = matches
                    .iter()
                    .filter(|&&i| category_of(&self.items[i]) == name)
                    .count();
                let current = c == self.cat;
                let mark = if current { "› " } else { "  " };
                let count = n.to_string();
                let room = width.saturating_sub(3 + count.len() + 1);
                let label = truncate(name, room);
                let pad = width.saturating_sub(2 + display_width(&label) + count.len() + 1);
                let focused = current && self.zone == Zone::Sections;
                let style = match (current, filtering && n == 0) {
                    (true, _) if focused => theme::selection().add_modifier(Modifier::BOLD),
                    (true, _) => theme::accent().add_modifier(Modifier::BOLD),
                    (false, true) => theme::faint(),
                    _ => Default::default(),
                };
                // the focused section is one solid bar across the column
                let bar = if focused {
                    theme::selection()
                } else {
                    Default::default()
                };
                Line::from(vec![
                    Span::styled(mark, if focused { bar } else { theme::accent() }),
                    Span::styled(label, style),
                    Span::styled(" ".repeat(pad), bar),
                    Span::styled(count, if focused { bar } else { theme::muted() }),
                    Span::styled(" ", bar),
                ])
            })
            .collect()
    }

    /// The tab row that replaces the column on narrow screens.
    fn tab_row(&self, width: usize) -> Line<'static> {
        let mut spans = Vec::new();
        for (c, name) in self.categories().iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(" · ", theme::faint()));
            }
            let style = match c == self.cat {
                true => theme::accent().add_modifier(Modifier::BOLD),
                false => theme::muted(),
            };
            spans.push(Span::styled(name.clone(), style));
        }
        crate::text::clip(Line::from(spans), width)
    }

    /// The areas ←/→ move across, left to right.
    fn zones(&self) -> Vec<Zone> {
        let mut zones = Vec::new();
        if self.sidebar_on() {
            zones.push(Zone::Sections);
        }
        zones.push(Zone::List);
        let has_choices = self
            .view()
            .get(self.selected)
            .is_some_and(|&i| !self.items[i].choices.is_empty());
        if has_choices {
            zones.push(Zone::Options);
        }
        zones
    }

    /// Moves the focus one area left or right; false at the end (the key is not used).
    fn move_zone(&mut self, delta: isize, wrap: bool) -> bool {
        let zones = self.zones();
        let at = zones.iter().position(|z| *z == self.zone).unwrap_or(0) as isize;
        let next = at + delta;
        let next = match (wrap, next) {
            (true, n) => n.rem_euclid(zones.len() as isize),
            (false, n) if n < 0 || n >= zones.len() as isize => return false,
            (false, n) => n,
        };
        self.zone = zones[next as usize];
        true
    }

    /// ←/→ inside the options: the previous or next option group, else out of the area.
    fn options_step(&mut self, delta: isize) -> bool {
        let Some(item) = self.highlighted_mut() else {
            return false;
        };
        let next = item.active as isize + delta;
        if next < 0 || next >= item.choices.len() as isize {
            return false;
        }
        item.active = next as usize;
        true
    }

    fn hint(&self, shown: usize, total: usize, width: usize) -> Line<'static> {
        let zones = self.zones();
        let side = |z: Zone| match z {
            Zone::Sections => "sections",
            Zone::List => "list",
            Zone::Options => "options",
        };
        let at = zones.iter().position(|z| *z == self.zone).unwrap_or(0);
        let mut keys = match self.zone {
            Zone::Sections => String::from("↑↓ section"),
            Zone::List if self.multi => String::from("↑↓ move · space tick"),
            Zone::List => String::from("↑↓ move"),
            Zone::Options => {
                let name = self
                    .view()
                    .get(self.selected)
                    .and_then(|&i| {
                        let it = &self.items[i];
                        it.choices.get(it.active).map(|(n, _)| n.clone())
                    })
                    .unwrap_or_default();
                format!("↑↓ change {name}")
            }
        };
        if let Some(z) = at.checked_sub(1).and_then(|i| zones.get(i)) {
            keys.push_str(&format!(" · ← {}", side(*z)));
        }
        if let Some(z) = zones.get(at + 1) {
            keys.push_str(&format!(" · → {}", side(*z)));
        }
        keys.push_str(" · enter select · esc cancel");
        let text = if shown < total {
            format!("  {shown} of {total} · {keys}")
        } else {
            format!("  {keys}")
        };
        Line::from(Span::styled(truncate(&text, width), theme::muted()))
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
        upd_bool(p, "back", &mut self.back, false);
        upd_bool(p, "sidebar", &mut self.sidebar, false);
        upd_bool(p, "multi", &mut self.multi, false);
        super::upd_dock(p, &mut self.dock);
        if let Some(items) = get(p, "items") {
            self.items = items
                .as_array()
                .map(|v| v.iter().map(item_of).collect())
                .unwrap_or_default();
            // open on the current item: its category, its row
            let current = self.items.iter().position(|it| it.current);
            self.cat = current
                .map(|i| category_of(&self.items[i]).to_owned())
                .and_then(|c| self.categories().iter().position(|x| *x == c))
                .unwrap_or(0);
            self.selected = current
                .and_then(|i| self.view().iter().position(|&x| x == i))
                .unwrap_or(0);
        }
        // `section` opens on a named section instead of the current item's
        if let Some(name) = str_of(p, "section") {
            if let Some(c) = self.categories().iter().position(|x| x == name) {
                self.cat = c;
                self.selected = 0;
            }
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

    fn docked(&self) -> bool {
        self.dock
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

        let view = self.view();
        let (start, end) = self.window(view.len());

        // the focused area is tinted; inside it the selected row is a selection bar
        let list_focused = matches!(self.zone, Zone::List | Zone::Options);
        let item_row = |i: usize, selected: bool, w: usize| -> Line<'static> {
            let line = self.render_item(i, selected, w);
            match (selected, self.zone, list_focused) {
                (true, Zone::List, _) => filled(line, w, theme::selection_bg()),
                (_, _, true) => filled(line, w, theme::focus_area()),
                _ => line,
            }
        };

        if self.sidebar_on() {
            let side = self.sidebar_width(width);
            if side == 0 {
                out.push(self.tab_row(width));
            }
            let right_w = width.saturating_sub(side + if side > 0 { 2 } else { 0 });
            let mut right: Vec<Line<'static>> = view[start..end]
                .iter()
                .enumerate()
                .map(|(row, &i)| item_row(i, start + row == self.selected, right_w))
                .collect();
            if view.is_empty() {
                right.push(Line::from(Span::styled("  no matches", theme::muted())));
            }
            let cats = self.category_cells(side);
            // the column scrolls to keep the current category in sight
            let height = self.max_visible.max(1);
            let cat_start = self
                .cat
                .saturating_sub(height - 1)
                .min(cats.len().saturating_sub(height));
            let side_tint = if self.zone == Zone::Sections {
                theme::focus_area()
            } else {
                Default::default()
            };
            let right_tint = if list_focused {
                theme::focus_area()
            } else {
                Default::default()
            };
            for r in 0..height {
                let mut spans = Vec::new();
                if side > 0 {
                    let cell = cats.get(cat_start + r).cloned().unwrap_or_default();
                    spans.extend(filled(cell, side, side_tint).spans);
                    spans.push(Span::styled("│ ", theme::faint()));
                }
                // rows past the list keep the area's tint so the whole column reads as one
                let line = right.get(r).cloned().unwrap_or_default();
                spans.extend(filled(line, right_w, right_tint).spans);
                out.push(Line::from(spans));
            }
            out.push(self.hint(end - start, view.len(), width));
            return out;
        }

        if view.is_empty() {
            out.push(Line::from(Span::styled("  no matches", theme::muted())));
        }
        if start > 0 {
            out.push(Line::from(Span::styled(format!("  ↑ {start} more"), theme::faint())));
        }
        for row in self.rows(&view, start, end) {
            match row {
                Row::Header(i) => {
                    let name = self.items[i].group.clone().unwrap_or_default();
                    out.push(Line::from(Span::styled(
                        truncate(&name, width),
                        theme::tone("agent").add_modifier(Modifier::BOLD),
                    )));
                }
                Row::Item(i) => {
                    let selected = view.get(self.selected) == Some(&i);
                    out.push(item_row(i, selected, width));
                }
            }
        }
        out.push(self.hint(end - start, view.len(), width));
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let count = self.view().len();
        let last = count.saturating_sub(1);
        // options only exist for an item that has some
        if self.zone == Zone::Options && !self.zones().contains(&Zone::Options) {
            self.zone = Zone::List;
        }
        if self.zone == Zone::Sections && !self.sidebar_on() {
            self.zone = Zone::List;
        }
        match input {
            Input::Paste(s) => {
                self.filter.insert(&s.replace('\n', " "));
                self.zone = Zone::List;
                self.refiltered();
            }
            Input::Key { code, mods, .. } => {
                let ctrl = mods.contains(KeyModifiers::CONTROL);
                let up = matches!(code, KeyCode::Up) || (ctrl && *code == KeyCode::Char('p'));
                let down = matches!(code, KeyCode::Down) || (ctrl && *code == KeyCode::Char('n'));
                match (self.zone, code) {
                    // ↑↓ act on the focused area
                    (Zone::Sections, _) if up || down => {
                        self.switch_category(if up { -1 } else { 1 })
                    }
                    (Zone::Options, _) if up || down => {
                        if let Some(it) = self.highlighted_mut() {
                            it.cycle(if up { -1 } else { 1 });
                        }
                    }
                    (Zone::List, _) if up => self.selected = self.selected.saturating_sub(1),
                    (Zone::List, _) if down => self.selected = (self.selected + 1).min(last),
                    (Zone::List, KeyCode::PageUp) => {
                        self.selected = self.selected.saturating_sub(self.max_visible)
                    }
                    (Zone::List, KeyCode::PageDown) => {
                        self.selected = (self.selected + self.max_visible).min(last)
                    }

                    // ←/→ move across the areas (inside the options: across its groups first)
                    (Zone::Options, KeyCode::Left) if self.options_step(-1) => {}
                    (Zone::Options, KeyCode::Right) => _ = self.options_step(1),
                    (_, KeyCode::Left) if self.move_zone(-1, false) => {}
                    (_, KeyCode::Left) if self.back => out.push(Emit::Close("back".into())),
                    (_, KeyCode::Left) => {}
                    (_, KeyCode::Right) => _ = self.move_zone(1, false),
                    (_, KeyCode::Tab) => _ = self.move_zone(1, true),
                    (_, KeyCode::BackTab) => _ = self.move_zone(-1, true),

                    (_, KeyCode::Enter) => self.emit_selected(out),
                    (Zone::List, KeyCode::Char(' ')) if self.multi => {
                        if let Some(it) = self.highlighted_mut() {
                            it.checked = !it.checked;
                        }
                    }
                    (_, KeyCode::Backspace) => {
                        self.filter.backspace();
                        self.zone = Zone::List;
                        self.refiltered();
                    }
                    (_, KeyCode::Char('u')) if ctrl => {
                        self.filter.clear();
                        self.refiltered();
                    }
                    (_, KeyCode::Char(c)) if !ctrl || mods.contains(KeyModifiers::ALT) => {
                        self.filter.insert(c.encode_utf8(&mut [0; 4]));
                        self.zone = Zone::List;
                        self.refiltered();
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    fn on_mouse(&mut self, m: &Mouse, out: &mut Vec<Emit>) -> bool {
        let view = self.view();
        let count = view.len();
        match m.act {
            MouseAct::Middle => return false,
            MouseAct::ScrollUp => self.selected = self.selected.saturating_sub(1),
            MouseAct::ScrollDown => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1))
            }
            MouseAct::Click => {
                let (start, end) = self.window(count);
                let top = usize::from(!self.title.is_empty()) + 1;
                let Some(row) = (m.row as usize).checked_sub(top) else {
                    return false;
                };
                if self.sidebar_on() {
                    let side = self.sidebar_width(m.width as usize);
                    if side == 0 {
                        return false;
                    }
                    if (m.col as usize) < side {
                        let height = self.max_visible.max(1);
                        let cats = self.categories().len();
                        let cat_start = self
                            .cat
                            .saturating_sub(height - 1)
                            .min(cats.saturating_sub(height));
                        if cat_start + row < cats {
                            self.cat = cat_start + row;
                            self.selected = 0;
                        }
                        return true;
                    }
                    if row < end - start {
                        self.selected = start + row;
                        self.emit_selected(out);
                    }
                    return true;
                }
                let above = usize::from(start > 0);
                let rows = self.rows(&view, start, end);
                let Some(Row::Item(i)) = row.checked_sub(above).and_then(|r| rows.get(r).copied())
                else {
                    return false;
                };
                let Some(pos) = view.iter().position(|&x| x == i) else {
                    return false;
                };
                self.selected = pos;
                self.emit_selected(out);
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

/// `line` padded to `width` with `under` beneath every span; a span's own background wins.
/// The padding is there even without a tint: columns after it must not move.
fn filled(line: Line<'static>, width: usize, under: ratatui::style::Style) -> Line<'static> {
    let used: usize = line.spans.iter().map(|s| display_width(&s.content)).sum();
    let mut spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|s| {
            let style = under.patch(s.style);
            Span::styled(s.content, style)
        })
        .collect();
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), under));
    }
    Line::from(spans)
}

fn item_of(v: &Value) -> Item {
    let mut item = Item {
        label: String::new(),
        value: Value::Nil,
        detail: None,
        group: None,
        current: false,
        available: true,
        choices: Vec::new(),
        chosen: Vec::new(),
        active: 0,
        checked: false,
    };
    let Value::Map(_) = v else {
        item.label = display(v);
        item.value = v.clone();
        return item;
    };

    item.label = get(v, "label").map(display).unwrap_or_default();
    item.value = get(v, "value")
        .cloned()
        .unwrap_or_else(|| Value::from(item.label.as_str()));
    item.detail = str_of(v, "detail").map(str::to_owned);
    item.group = str_of(v, "group").map(str::to_owned);
    item.current = get(v, "current").and_then(Value::as_bool).unwrap_or(false);
    item.available = get(v, "available").and_then(Value::as_bool).unwrap_or(true);
    item.checked = get(v, "checked").and_then(Value::as_bool).unwrap_or(false);
    for group in get(v, "choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some([name, options]) = group.as_array().map(Vec::as_slice) else {
            continue;
        };
        let options: Vec<String> = options
            .as_array()
            .into_iter()
            .flatten()
            .map(display)
            .collect();
        if !options.is_empty() {
            item.choices.push((display(name), options));
        }
    }
    item.chosen = item
        .choices
        .iter()
        .map(|(name, options)| {
            get(v, "chosen")
                .and_then(|c| str_of(c, name))
                .filter(|o| options.iter().any(|x| x == o))
                .map_or_else(|| options[0].clone(), str::to_owned)
        })
        .collect();
    item
}
