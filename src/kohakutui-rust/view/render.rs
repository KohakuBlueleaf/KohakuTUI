//! Nodes to lines: generic containers stack their children; cells indent theirs under a header.
//!
//! A cell splits into a prefix (header and leading children), an optional streaming body (its last
//! `markdown` child when `stream_flush` is set) and a trailer, so the flusher can write the body
//! into scrollback line by line before the cell seals.

use ratatui::text::{Line, Span};

use crate::model::{Model, TRANSCRIPT};
use crate::proto::NodeId;
use crate::text::{clip, MdLine};
use crate::widgets::{Cell, Ctx, Markdown, Stack};

/// A cell's lines, split for incremental flushing.
pub(crate) struct CellParts {
    pub prefix: Vec<Line<'static>>,
    /// Stable body lines with their source offsets, the unstable tail, and the stable end.
    pub body: Option<(Vec<MdLine>, Vec<Line<'static>>, usize)>,
    pub trailer: Vec<Line<'static>>,
}

impl CellParts {
    /// Every line, in order.
    pub fn all(self) -> Vec<Line<'static>> {
        let mut out = self.prefix;
        if let Some((stable, pending, _)) = self.body {
            out.extend(stable.into_iter().map(|l| l.line));
            out.extend(pending);
        }
        out.extend(self.trailer);
        out
    }
}

/// A node's lines at `width`, children included; `cursor` gets the caret of `target`.
pub(crate) fn node_lines(
    m: &Model,
    id: NodeId,
    width: u16,
    ctx: &Ctx,
    target: Option<NodeId>,
    cursor: &mut Option<(u16, u16)>,
) -> Vec<Line<'static>> {
    let Some(node) = m.node(id) else {
        return Vec::new();
    };
    if m.comp_as::<Cell>(id).is_some() {
        return cell_parts(m, id, width, ctx).all();
    }

    let ctx = ctx.focused(target == Some(id));
    let mut out: Vec<Line<'static>> = node
        .comp
        .lines(width, &ctx)
        .into_iter()
        .map(|l| clip(l, width as usize))
        .collect();
    if target == Some(id) {
        *cursor = node.comp.cursor(width);
    }
    if !node.comp.container() {
        return out;
    }

    let gap = m.comp_as::<Stack>(id).map_or(0, |s| s.gap);
    for (i, child) in node.children.iter().enumerate() {
        if i > 0 {
            out.extend((0..gap).map(|_| Line::default()));
        }
        let base = out.len() as u16;
        let mut child_cursor = None;
        let child_ctx = ctx.focused(false);
        out.extend(node_lines(m, *child, width, &child_ctx, target, &mut child_cursor));
        if let Some((r, c)) = child_cursor {
            *cursor = Some((base + r, c));
        }
    }
    out
}

/// A cell split into prefix, streaming body and trailer.
pub(crate) fn cell_parts(m: &Model, id: NodeId, width: u16, ctx: &Ctx) -> CellParts {
    let (Some(cell), Some(node)) = (m.comp_as::<Cell>(id), m.node(id)) else {
        return CellParts { prefix: Vec::new(), body: None, trailer: Vec::new() };
    };
    let indent = cell.indent();
    let inner = width.saturating_sub(indent as u16).max(1);

    // the last child streams into scrollback only when the cell opts in and it is markdown
    let body_id = node
        .children
        .last()
        .copied()
        .filter(|c| cell.stream_flush && m.comp_as::<Markdown>(*c).is_some());

    let mut prefix = cell.header(width, ctx);
    let mut sink = None;
    for child in node.children.iter().filter(|c| Some(**c) != body_id) {
        let lines = node_lines(m, *child, inner, ctx, None, &mut sink);
        prefix.extend(indented(lines, indent, cell.body_style()));
    }

    let body = body_id.and_then(|b| m.comp_as::<Markdown>(b)).map(|md| {
        let stable = md
            .stable(inner)
            .into_iter()
            .map(|l| MdLine { src: l.src, line: indent_line(l.line, indent) })
            .collect();
        let pending = indented(md.pending(inner), indent, Default::default());
        (stable, pending, md.stable_end())
    });

    let mut parts = CellParts {
        prefix,
        body,
        trailer: if node.sealed {
            cell.trailer_lines(width)
        } else {
            Vec::new()
        },
    };

    // without a title row, the icon replaces the indent of the cell's first line
    if cell.title.is_none() && indent >= 2 {
        if let Some(icon) = cell.icon_span(ctx) {
            let first = match &mut parts.body {
                _ if !parts.prefix.is_empty() => parts.prefix.first_mut(),
                Some((stable, pending, _)) => match stable.first_mut() {
                    Some(l) => Some(&mut l.line),
                    None => pending.first_mut(),
                },
                None => None,
            };
            if let Some(line) = first {
                if let Some(lead) = line.spans.first_mut() {
                    *lead = Span::raw(" ".repeat(indent.saturating_sub(2)));
                }
                line.spans.insert(1, icon);
                line.spans.insert(2, Span::raw(" "));
            }
        }
    }
    parts
}

/// The blank line between transcript cells: every cell but the first gets one above it.
pub(crate) fn separator(m: &Model, id: NodeId) -> Vec<Line<'static>> {
    match m.children(TRANSCRIPT).first() {
        Some(first) if *first != id => vec![Line::default()],
        _ => Vec::new(),
    }
}

fn indent_line(line: Line<'static>, indent: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    spans.extend(line.spans);
    Line::from(spans).style(line.style)
}

fn indented(
    lines: Vec<Line<'static>>,
    indent: usize,
    style: ratatui::style::Style,
) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|l| indent_line(l, indent).patch_style(style))
        .collect()
}
