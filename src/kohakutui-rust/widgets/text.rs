//! Plain-text components: `text`, `code` and `log`.

use std::any::Any;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_bool, upd_opt_str, upd_str, upd_usize};
use super::{Component, Ctx, WidgetError};
use crate::proto::value::display;
use crate::proto::Value;
use crate::text::{display_width, link, theme, wrap_text};

fn text_data(kind: &'static str, data: &Value) -> Result<String, WidgetError> {
    match data {
        Value::String(_) => Ok(display(data)),
        _ => Err(WidgetError::BadProp {
            prop: "data",
            reason: format!("{kind} append takes a string"),
        }),
    }
}

/// Wrapped text in one style; with `link`, the whole text is a link to that URL.
#[derive(Debug, Default)]
pub struct Text {
    value: String,
    tone: String,
    bold: bool,
    dim: bool,
    italic: bool,
    link: Option<String>,
}

impl Text {
    fn style(&self) -> Style {
        let mut s = theme::tone(&self.tone);
        if self.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.dim {
            s = s.add_modifier(Modifier::DIM);
        }
        if self.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        match &self.link {
            // a tone keeps its colour; otherwise links take the theme's link colour
            Some(url) if self.tone.is_empty() => s.patch(link::style(url)),
            Some(url) => s.add_modifier(Modifier::UNDERLINED).patch(link::mark(url)),
            None => s,
        }
    }

    /// The text as shown: with links written as text, the URL follows (unless it is the text).
    fn shown(&self) -> String {
        match &self.link {
            Some(url) if !link::osc8() && !url.is_empty() && *url != self.value => {
                format!("{} ({url})", self.value)
            }
            _ => self.value.clone(),
        }
    }
}

impl Component for Text {
    fn kind(&self) -> &'static str {
        "text"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "value", &mut self.value);
        upd_str(p, "tone", &mut self.tone);
        upd_bool(p, "bold", &mut self.bold, false);
        upd_bool(p, "dim", &mut self.dim, false);
        upd_bool(p, "italic", &mut self.italic, false);
        upd_opt_str(p, "link", &mut self.link);
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        self.value.push_str(&text_data("text", data)?);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        wrap_text(&self.shown(), self.style(), width as usize)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Monospace text behind a gutter; `max_lines` keeps the tail.
#[derive(Debug, Default)]
pub struct Code {
    value: String,
    lang: Option<String>,
    max_lines: usize,
}

impl Component for Code {
    fn kind(&self) -> &'static str {
        "code"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "value", &mut self.value);
        upd_opt_str(p, "lang", &mut self.lang);
        upd_usize(p, "max_lines", &mut self.max_lines, 0);
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        self.value.push_str(&text_data("code", data)?);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let avail = (width as usize).saturating_sub(2).max(1);
        let src: Vec<&str> = self.value.trim_end_matches('\n').split('\n').collect();
        let skip = match self.max_lines {
            0 => 0,
            n => src.len().saturating_sub(n),
        };
        let mut out = Vec::new();
        if skip > 0 {
            out.push(Line::from(Span::styled(format!("│ … {skip} more lines"), theme::muted())));
        }
        for line in &src[skip..] {
            for wrapped in wrap_text(line, theme::code(), avail) {
                let mut spans = vec![Span::styled("│ ", theme::muted())];
                spans.extend(wrapped.spans);
                out.push(Line::from(spans));
            }
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A scrolling line log: `\n` ends a line, `\r` rewrites the current one (progress bars).
#[derive(Debug)]
pub struct Log {
    title: Option<String>,
    max_lines: usize,
    height: usize,
    lines: Vec<String>,
    current: String,
}

impl Default for Log {
    fn default() -> Self {
        Log {
            title: None,
            max_lines: 1000,
            height: 10,
            lines: Vec::new(),
            current: String::new(),
        }
    }
}

impl Log {
    fn push_text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\n' => self.lines.push(std::mem::take(&mut self.current)),
                '\r' => self.current.clear(),
                c => self.current.push(c),
            }
        }
        let excess = self.lines.len().saturating_sub(self.max_lines);
        self.lines.drain(..excess);
    }
}

impl Component for Log {
    fn kind(&self) -> &'static str {
        "log"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "title", &mut self.title);
        upd_usize(p, "max_lines", &mut self.max_lines, 1000);
        upd_usize(p, "height", &mut self.height, 10);
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        let text = text_data("log", data)?;
        self.push_text(&text);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let mut all: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        if !self.current.is_empty() {
            all.push(&self.current);
        }
        let shown = &all[all.len().saturating_sub(self.height.max(1))..];
        let mut out = Vec::new();
        if let Some(t) = &self.title {
            out.push(Line::from(Span::styled(
                format!("{t} ({})", all.len()),
                theme::muted().add_modifier(Modifier::BOLD),
            )));
        }
        for l in shown {
            out.push(Line::from(Span::raw(truncate(l, width))));
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `s` cut to `width` columns with an ellipsis when it is longer.
pub(crate) fn truncate(s: &str, width: usize) -> String {
    if display_width(s) <= width {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = display_width(c.encode_utf8(&mut [0; 4]));
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}
