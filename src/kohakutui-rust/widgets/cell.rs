//! `cell`: one transcript entry. Its header carries a role icon, title and status; its children
//! render indented under it; a trailer line is printed when it seals.

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
            "running" => return Some(Span::styled(theme::spinner(ctx.ms()), theme::accent())),
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

    /// Title row: icon, bold title, status note. None without a title.
    pub fn header(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        let Some(title) = &self.title else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        if let Some(icon) = self.icon_span(ctx) {
            spans.push(icon);
            spans.push(Span::raw(" "));
        }
        let style = match self.role {
            Role::User => theme::user(),
            Role::Error => theme::danger().add_modifier(Modifier::BOLD),
            _ => theme::bold(),
        };
        spans.push(Span::styled(title.clone(), style));
        if !self.status.is_empty() && !matches!(self.status.as_str(), "ok" | "done") {
            spans.push(Span::styled(format!("  {}", self.status), theme::muted()));
        }
        wrap_spans(&spans, width as usize)
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
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        self.header(width, ctx)
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
