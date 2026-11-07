//! `cell`: one transcript entry. Its header carries a role icon, title and status; its children
//! render indented under it; a trailer line is printed when it seals. A `collapsed` cell with a
//! title folds to its header and one summary line until the full-screen view opens it; foldable
//! headers carry ▸ (folded) or ▾ (open).
//!
//! `actions` (`[{key, label, value}]`) make a cell reachable in full-screen navigation (ctrl+↑,
//! or a click on it): there its keys send an `action` event named by `value`, with the cell
//! as `focus` (copy, edit, retry on a message), and a hint row lists them.
//!
//! A cell with `batch: n` groups its child cells (tool calls, jobs, files): below n children it is invisible
//! and they render as if they were transcript entries themselves; from n on it folds them under
//! one "n <title>" header ("n items" untitled), like a web chat's tool batch (see `view::render`).

use std::any::Any;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_bool, upd_opt_str, upd_str};
use super::{Component, Ctx, WidgetError};
use crate::proto::value::get;
use crate::proto::Value;
use crate::text::{theme, wrap_spans};

/// Who a cell speaks for; decides its icon and colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
    System,
    Output,
    Error,
}

impl Role {
    fn parse(s: &str) -> Role {
        match s {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            "output" => Role::Output,
            "error" => Role::Error,
            _ => Role::System,
        }
    }
}

#[derive(Debug)]
pub struct Cell {
    pub role: Role,
    pub title: Option<String>,
    pub status: String,
    pub trailer: Option<String>,
    /// Lets the streaming body (its last `markdown` child) reach scrollback before it seals.
    pub stream_flush: bool,
    pub icon: Option<String>,
    pub collapsed: bool,
    /// The folded cell's one line; the trailer (once sealed) when unset.
    pub summary: Option<String>,
    /// Title colour as a theme tone (`tool`, `agent`, `sapphire`, `#rrggbb`).
    pub tone: Option<String>,
    /// Muted text after the title, e.g. a tool's arguments.
    pub detail: Option<String>,
    /// Batch threshold: 0 is a plain cell.
    pub batch: usize,
    /// Keys that act on the cell in navigation: (key, label, action name).
    pub actions: Vec<(char, String, String)>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            role: Role::System,
            title: None,
            status: String::new(),
            trailer: None,
            stream_flush: false,
            icon: None,
            collapsed: false,
            summary: None,
            tone: None,
            detail: None,
            batch: 0,
            actions: Vec::new(),
        }
    }
}

impl Cell {
    /// The icon span for the header, or for the first line when there is no title.
    pub fn icon_span(&self, ctx: &Ctx) -> Option<Span<'static>> {
        if let Some(icon) = &self.icon {
            return Some(Span::styled(icon.clone(), theme::accent()));
        }
        match self.status.as_str() {
            "running" => {
                return Some(Span::styled(theme::spinner(ctx.ms()), theme::tone("running")))
            }
            s if s.starts_with("waiting") => return Some(Span::styled("⚙", theme::warn())),
            "interrupted" => return Some(Span::styled("○", theme::warn())),
            "ok" | "done" => return Some(Span::styled("✓", theme::ok())),
            "error" | "failed" => return Some(Span::styled("✗", theme::danger())),
            _ => {}
        }
        match self.role {
            Role::User => Some(Span::styled("›", theme::user())),
            Role::Assistant => Some(Span::styled("●", theme::assistant())),
            Role::Tool => Some(Span::styled("◆", theme::accent())),
            Role::System => Some(Span::styled("•", theme::muted())),
            Role::Error => Some(Span::styled("✗", theme::danger())),
            Role::Output => None,
        }
    }

    /// Indentation of children; output cells are flush left.
    pub fn indent(&self) -> usize {
        if self.role == Role::Output {
            0
        } else {
            2
        }
    }

    /// Title row: icon, bold title, status note, and the fold marker when `fold` is given
    /// (true: folded). None without a title.
    pub fn header(&self, width: u16, ctx: &Ctx, fold: Option<bool>) -> Vec<Line<'static>> {
        let Some(title) = &self.title else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        if let Some(icon) = self.icon_span(ctx) {
            spans.push(icon);
            spans.push(Span::raw(" "));
        }
        let style = match (&self.tone, self.role) {
            (Some(t), _) => theme::tone(t).add_modifier(Modifier::BOLD),
            (None, Role::User) => theme::user(),
            (None, Role::Error) => theme::danger().add_modifier(Modifier::BOLD),
            _ => theme::bold(),
        };
        spans.push(Span::styled(title.clone(), style));
        if let Some(d) = &self.detail {
            spans.push(Span::styled(format!("  {d}"), theme::muted()));
        }
        // statuses the icon already says are not repeated as text
        let quiet = matches!(
            self.status.as_str(),
            "ok" | "done" | "running" | "interrupted" | "error" | "failed"
        );
        if !self.status.is_empty() && !quiet {
            spans.push(Span::styled(format!("  {}", self.status), theme::muted()));
        }
        if let Some(folded) = fold {
            spans.push(Span::styled(fold_marker(folded), theme::muted()));
        }
        wrap_spans(&spans, width as usize)
    }

    /// The hint for navigation on this cell: "c copy · e edit".
    pub fn actions_hint(&self) -> Option<String> {
        if self.actions.is_empty() {
            return None;
        }
        let keys: Vec<String> = self
            .actions
            .iter()
            .map(|(k, l, _)| format!("{k} {l}"))
            .collect();
        Some(keys.join(" · "))
    }

    /// The closing line written when the cell seals.
    pub fn trailer_lines(&self, width: u16) -> Vec<Line<'static>> {
        match &self.trailer {
            Some(t) => {
                let spans = [
                    Span::styled("  └ ", theme::muted()),
                    Span::styled(t.clone(), theme::muted()),
                ];
                wrap_spans(&spans, width as usize)
            }
            None => Vec::new(),
        }
    }

    /// Whether the cell can fold: it asks to and has a header to fold under.
    pub fn foldable(&self) -> bool {
        self.collapsed && self.title.is_some()
    }

    /// Whether the cell renders folded: foldable, and not opened (expand-all or the user's
    /// toggle, which inverts expand-all).
    pub fn folded(&self, ctx: &Ctx, toggled: bool) -> bool {
        self.foldable() && !(ctx.expand_all ^ toggled)
    }

    /// The folded cell's line under its header.
    pub fn summary_lines(&self, width: u16, sealed: bool) -> Vec<Line<'static>> {
        let text = match (&self.summary, &self.trailer) {
            (Some(s), _) => s.clone(),
            (None, Some(t)) if sealed => t.clone(),
            _ if sealed => "done".to_owned(),
            _ => "running…".to_owned(),
        };
        let spans = [
            Span::styled("  ⎿  ", theme::muted()),
            Span::raw(text),
            Span::styled("  (ctrl+o to expand)", theme::muted()),
        ];
        wrap_spans(&spans, width as usize)
    }

    pub fn body_style(&self) -> Style {
        match self.role {
            Role::Error => theme::danger(),
            _ => Style::default(),
        }
    }
}

impl Component for Cell {
    fn kind(&self) -> &'static str {
        "cell"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        if let Some(r) = get(p, "role") {
            self.role = Role::parse(r.as_str().unwrap_or("system"));
        }
        upd_opt_str(p, "title", &mut self.title);
        upd_str(p, "status", &mut self.status);
        upd_opt_str(p, "trailer", &mut self.trailer);
        upd_bool(p, "stream_flush", &mut self.stream_flush, false);
        upd_opt_str(p, "icon", &mut self.icon);
        upd_bool(p, "collapsed", &mut self.collapsed, false);
        upd_opt_str(p, "summary", &mut self.summary);
        upd_opt_str(p, "tone", &mut self.tone);
        upd_opt_str(p, "detail", &mut self.detail);
        super::props::upd_usize(p, "batch", &mut self.batch, 0);
        if let Some(v) = get(p, "actions") {
            self.actions = v
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|a| {
                            let key = crate::proto::value::str_of(a, "key")?.chars().next()?;
                            let label = crate::proto::value::str_of(a, "label").unwrap_or_default();
                            let value = crate::proto::value::str_of(a, "value").unwrap_or(label);
                            Some((key, label.to_owned(), value.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        self.header(width, ctx, None)
    }

    fn container(&self) -> bool {
        true
    }

    fn animating(&self) -> bool {
        self.status == "running" && self.icon.is_none()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The fold marker after a foldable header.
pub fn fold_marker(folded: bool) -> &'static str {
    if folded {
        "  ▸"
    } else {
        "  ▾"
    }
}
