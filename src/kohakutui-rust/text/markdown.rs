//! Markdown source to styled, wrapped lines, each tagged with the start of its source line.
//!
//! Lists render compact whatever their looseness, soft breaks become line breaks and a block
//! separator belongs to the block after it, so rendering a prefix of the source gives a prefix of
//! the lines rendering the whole source gives.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme;
use super::wrap::{display_width, wrap_spans};
use super::MdLine;

/// Renders `src` at `width` columns.
pub fn render(src: &str, width: usize) -> Vec<MdLine> {
    let mut r = Renderer::new(src, width.max(8));
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        r.event(event, range.start);
    }
    r.flush_inline();
    r.out
}

enum Container {
    Quote,
    Item { marker: String, used: bool },
}

struct Table {
    src: usize,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

struct Renderer<'a> {
    src: &'a str,
    line_starts: Vec<usize>,
    width: usize,
    out: Vec<MdLine>,
    spans: Vec<Span<'static>>,
    span_src: Option<usize>,
    styles: Vec<Style>,
    containers: Vec<Container>,
    lists: Vec<Option<u64>>,
    need_blank: bool,
    code: Option<usize>,
    links: Vec<String>,
    table: Option<Table>,
    heading: bool,
}

impl<'a> Renderer<'a> {
    fn new(src: &'a str, width: usize) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Renderer {
            src,
            line_starts,
            width,
            out: Vec::new(),
            spans: Vec::new(),
            span_src: None,
            styles: vec![Style::default()],
            containers: Vec::new(),
            lists: Vec::new(),
            need_blank: false,
            code: None,
            links: Vec::new(),
            table: None,
            heading: false,
        }
    }

    fn line_start(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => self.line_starts[i],
            Err(i) => self.line_starts[i.saturating_sub(1)],
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&Style::default())
    }

    fn push_style(&mut self, patch: Style) {
        let s = self.style().patch(patch);
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn text(&mut self, text: &str, at: usize, style: Style) {
        if let Some(t) = self.table.as_mut() {
            t.cell.push_str(text);
            return;
        }
        if self.span_src.is_none() {
            self.span_src = Some(self.line_start(at));
        }
        self.spans.push(Span::styled(text.to_owned(), style));
    }

    /// Prefix spans for the next visual line, and the width they take.
    fn prefix(&mut self, first: bool) -> (Vec<Span<'static>>, usize) {
        let mut spans = Vec::new();
        let mut w = 0;
        let last_item = self
            .containers
            .iter()
            .rposition(|c| matches!(c, Container::Item { .. }));
        for (i, c) in self.containers.iter_mut().enumerate() {
            match c {
                Container::Quote => {
                    spans.push(Span::styled("▌ ", theme::muted()));
                    w += 2;
                }
                Container::Item { marker, used } => {
                    let mw = display_width(marker);
                    if first && !*used && Some(i) == last_item {
                        spans.push(Span::styled(marker.clone(), theme::accent()));
                        *used = true;
                    } else {
                        spans.push(Span::raw(" ".repeat(mw)));
                    }
                    w += mw;
                }
            }
        }
        (spans, w)
    }

    fn separate(&mut self, src: usize) {
        if self.need_blank {
            self.need_blank = false;
            let (spans, _) = self.prefix(false);
            let spans = spans
                .into_iter()
                .filter(|s| !s.content.trim().is_empty())
                .collect::<Vec<_>>();
            self.out.push(MdLine { src, line: Line::from(spans) });
        }
    }

    /// Emits one source line's spans, wrapped under the container prefixes.
    fn emit(&mut self, spans: Vec<Span<'static>>, src: usize) {
        self.separate(src);
        let (first_prefix, pw) = self.prefix(true);
        let (rest_prefix, _) = self.prefix(false);
        let avail = self.width.saturating_sub(pw).max(4);
        for (i, line) in wrap_spans(&spans, avail).into_iter().enumerate() {
            let mut all = if i == 0 {
                first_prefix.clone()
            } else {
                rest_prefix.clone()
            };
            all.extend(line.spans);
            self.out.push(MdLine { src, line: Line::from(all) });
        }
    }

    fn flush_inline(&mut self) {
        if let Some(src) = self.span_src.take() {
            let spans = std::mem::take(&mut self.spans);
            self.emit(spans, src);
        }
    }

    fn block_end(&mut self) {
        self.flush_inline();
        if self.lists.is_empty() {
            self.need_blank = true;
        }
    }

    fn event(&mut self, event: Event<'_>, at: usize) {
        match event {
            Event::Start(tag) => self.start(tag, at),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match self.code {
                Some(_) => self.code_text(&t, at),
                None => self.text(&t, at, self.style()),
            },
            Event::Code(t) => self.text(&t, at, self.style().patch(theme::code())),
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                self.text(&t, at, self.style().patch(theme::code()))
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                for (i, part) in t.split('\n').enumerate() {
                    if i > 0 {
                        self.flush_inline();
                    }
                    if !part.is_empty() {
                        self.text(part, at, theme::muted());
                    }
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(t) = self.table.as_mut() {
                    t.cell.push(' ');
                } else {
                    self.flush_inline();
                }
            }
            Event::Rule => {
                self.flush_inline();
                let line = vec![Span::styled("─".repeat(self.width.min(40)), theme::muted())];
                let src = self.line_start(at);
                self.emit(line, src);
                self.block_end();
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "[x] " } else { "[ ] " };
                self.text(mark, at, theme::accent());
            }
            Event::FootnoteReference(name) => {
                self.text(&format!("[{name}]"), at, theme::muted());
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, at: usize) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.heading = true;
                let style = match level {
                    HeadingLevel::H1 => {
                        theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    }
                    HeadingLevel::H2 => theme::accent().add_modifier(Modifier::BOLD),
                    _ => theme::bold(),
                };
                self.push_style(style);
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.separate(self.line_start(at));
                self.containers.push(Container::Quote);
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                self.code = Some(match kind {
                    CodeBlockKind::Fenced(_) => self.line_start(at),
                    CodeBlockKind::Indented => self.line_start(at),
                });
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.separate(self.line_start(at));
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_inline();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_owned(),
                };
                self.containers
                    .push(Container::Item { marker, used: false });
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT))
            }
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.into_string());
                self.push_style(theme::accent().add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.into_string());
                self.text("[image: ", at, theme::muted());
                self.push_style(theme::muted());
            }
            Tag::Table(_) => {
                self.flush_inline();
                self.table = Some(Table {
                    src: self.line_start(at),
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: String::new(),
                });
            }
            Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.block_end(),
            TagEnd::Heading(_) => {
                self.pop_style();
                self.heading = false;
                self.block_end();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.pop_style();
                self.containers.pop();
                if self.lists.is_empty() {
                    self.need_blank = true;
                }
            }
            TagEnd::CodeBlock => {
                self.code = None;
                self.block_end();
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.need_blank = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                self.containers.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.links.pop() {
                    let shown: String = self.spans.iter().map(|s| s.content.as_ref()).collect();
                    if !url.is_empty() && !shown.ends_with(url.as_str()) {
                        let at = self.span_src.unwrap_or(0);
                        self.text(&format!(" ({url})"), at, theme::muted());
                    }
                }
            }
            TagEnd::Image => {
                self.pop_style();
                self.links.pop();
                let at = self.span_src.unwrap_or(0);
                self.text("]", at, theme::muted());
            }
            TagEnd::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    let cell = std::mem::take(&mut t.cell);
                    t.row.push(cell.trim().to_owned());
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table_lines(t);
                    self.block_end();
                }
            }
            _ => {}
        }
    }

    fn code_text(&mut self, text: &str, at: usize) {
        let mut offset = at;
        for part in text.split_inclusive('\n') {
            let line = part.strip_suffix('\n').unwrap_or(part);
            let src = self.line_start(offset.min(self.src.len()));
            let spans = vec![
                Span::styled("  ", Style::default()),
                Span::styled(line.to_owned(), theme::code()),
            ];
            self.emit(spans, src);
            offset += part.len();
        }
    }

    fn table_lines(&mut self, t: Table) {
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let mut widths = vec![1usize; cols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(display_width(cell));
            }
        }
        let budget = self.width.saturating_sub(3 * (cols - 1)).max(cols);
        while widths.iter().sum::<usize>() > budget {
            let (i, _) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if widths[i] <= 3 {
                break;
            }
            widths[i] -= 1;
        }
        for (r, row) in t.rows.iter().enumerate() {
            let mut spans = Vec::new();
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", theme::muted()));
                }
                let cell = row.get(i).map(String::as_str).unwrap_or("");
                let style = if r == 0 {
                    theme::bold()
                } else {
                    Style::default()
                };
                spans.push(Span::styled(fit(cell, *w), style));
            }
            self.emit_unwrapped(spans, t.src);
            if r == 0 {
                let sep = widths
                    .iter()
                    .map(|w| "─".repeat(*w))
                    .collect::<Vec<_>>()
                    .join("─┼─");
                self.emit_unwrapped(vec![Span::styled(sep, theme::muted())], t.src);
            }
        }
    }

    fn emit_unwrapped(&mut self, spans: Vec<Span<'static>>, src: usize) {
        self.separate(src);
        let (mut all, _) = self.prefix(false);
        all.extend(spans);
        self.out.push(MdLine { src, line: Line::from(all) });
    }
}

/// Pads or truncates `s` to exactly `w` columns.
fn fit(s: &str, w: usize) -> String {
    let sw = display_width(s);
    if sw <= w {
        return format!("{s}{}", " ".repeat(w - sw));
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = display_width(c.encode_utf8(&mut [0; 4]));
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    used += 1;
    out.push_str(&" ".repeat(w.saturating_sub(used)));
    out
}
