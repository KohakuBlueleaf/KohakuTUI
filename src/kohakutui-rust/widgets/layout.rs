//! Layout components: `stack` (and the five regions) and `divider`.

use std::any::Any;

use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_usize};
use super::{Component, Ctx, WidgetError};
use crate::proto::Value;
use crate::text::{display_width, theme};

/// Vertical container; children are laid out by the view with `gap` blank lines between them.
#[derive(Debug, Default)]
pub struct Stack {
    pub title: Option<String>,
    pub gap: usize,
}

impl Component for Stack {
    fn kind(&self) -> &'static str {
        "stack"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "title", &mut self.title);
        upd_usize(p, "gap", &mut self.gap, 0);
        Ok(())
    }

    fn lines(&self, _width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        match &self.title {
            Some(t) => vec![Line::from(Span::styled(t.clone(), theme::bold()))],
            None => Vec::new(),
        }
    }

    fn container(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A horizontal rule with an optional label.
#[derive(Debug, Default)]
pub struct Divider {
    label: Option<String>,
}

impl Component for Divider {
    fn kind(&self) -> &'static str {
        "divider"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "label", &mut self.label);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let text = match &self.label {
            Some(l) if display_width(l) + 6 <= width => {
                let rest = width - display_width(l) - 4;
                format!("── {l} {}", "─".repeat(rest))
            }
            _ => "─".repeat(width),
        };
        vec![Line::from(Span::styled(text, theme::muted()))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
