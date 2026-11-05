//! Nodes to lines: generic containers stack their children; cells indent theirs under a header.
//!
//! A cell splits into a prefix (header and leading children), an optional streaming body (its last
//! `markdown` child when `stream_flush` is set) and a trailer, so the flusher can write the body
//! into scrollback line by line before the cell seals.

use ratatui::text::{Line, Span};

use crate::model::{Model, TRANSCRIPT};
use crate::proto::NodeId;
use crate::text::{clip, display_width, theme, MdLine};
use crate::widgets::{Cell, Ctx, Markdown, Role, Stack};

/// A cell's lines, split for incremental flushing.
#[derive(Default)]
pub(crate) struct CellParts {
    pub prefix: Vec<Line<'static>>,
    /// Stable body lines with their source offsets, the unstable tail, and the stable end.
    pub body: Option<(Vec<MdLine>, Vec<Line<'static>>, usize)>,
    pub trailer: Vec<Line<'static>>,
    /// Foldable headers nested inside the prefix: (line, node).
    pub heads: Vec<(usize, NodeId)>,
    /// Scroll boxes inside the prefix: (first line, height, node).
    pub scrolls: Vec<(usize, usize, NodeId)>,
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
    let header = out.len() as u16;
    let mut children = Vec::new();
    for (i, child) in node.children.iter().enumerate() {
        if i > 0 {
            children.extend((0..gap).map(|_| Line::default()));
        }
        let base = header + children.len() as u16;
        let mut child_cursor = None;
        let child_ctx = ctx.focused(false);
        children.extend(node_lines(m, *child, width, &child_ctx, target, &mut child_cursor));
        if let Some((r, c)) = child_cursor {
            *cursor = Some((base + r, c));
        }
    }
    // the caret moves with the window, and is hidden when its line was cut off
    let (start, end) = node.comp.window(children.len());
    if let Some((r, c)) = *cursor {
        if let Some(r) = usize::from(r).checked_sub(usize::from(header)) {
            *cursor = (start..end)
                .contains(&r)
                .then(|| (header + (r - start) as u16, c));
        }
    }
    out.extend(node.comp.frame_children(children, width));
    out
}

/// A cell split into prefix, streaming body and trailer.
pub(crate) fn cell_parts(m: &Model, id: NodeId, width: u16, ctx: &Ctx) -> CellParts {
    let (Some(cell), Some(node)) = (m.comp_as::<Cell>(id), m.node(id)) else {
        return CellParts::default();
    };
    if cell.batch > 0 {
        return batch_parts(m, id, cell, width, ctx);
    }
    if cell.folded(ctx, m.is_open(id)) {
        let mut prefix = cell.header(width, ctx, Some(true));
        prefix.extend(cell.summary_lines(width, node.sealed));
        return CellParts { prefix, ..CellParts::default() };
    }

    let indent = cell.indent();
    let inner = width.saturating_sub(indent as u16).max(1);

    // the last child streams into scrollback only when the cell opts in and it is markdown
    let body_id = node
        .children
        .last()
        .copied()
        .filter(|c| cell.stream_flush && m.comp_as::<Markdown>(*c).is_some());

    let mut prefix = cell.header(width, ctx, cell.foldable().then_some(false));
    let mut heads = Vec::new();
    let mut scrolls = Vec::new();
    for child in node.children.iter().filter(|c| Some(**c) != body_id) {
        let base = prefix.len();
        let (lines, h, sc) = child_lines(m, *child, inner, ctx);
        heads.extend(h.into_iter().map(|(i, n)| (base + i, n)));
        scrolls.extend(sc.into_iter().map(|(i, len, n)| (base + i, len, n)));
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
        heads,
        scrolls,
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

    // the user's own messages sit on a full-width band so they stand out in the history
    if cell.role == Role::User {
        let band = theme::user_band();
        if band.bg.is_some() {
            for line in &mut parts.prefix {
                *line = banded(std::mem::take(line), width as usize, band);
            }
        }
    }
    parts
}

/// Whether a node folds: a collapsed titled cell, or a batch at its threshold.
pub(crate) fn foldable(m: &Model, id: NodeId) -> bool {
    match m.comp_as::<Cell>(id) {
        Some(c) if c.batch > 0 => m.children(id).len() >= c.batch,
        Some(c) => c.foldable(),
        None => false,
    }
}

/// A child's lines with the foldable headers and scroll boxes inside it (line offsets).
type ChildLines = (Vec<Line<'static>>, Vec<(usize, NodeId)>, Vec<(usize, usize, NodeId)>);

fn child_lines(m: &Model, child: NodeId, width: u16, ctx: &Ctx) -> ChildLines {
    if m.comp_as::<Cell>(child).is_some() {
        let mut parts = cell_parts(m, child, width, ctx);
        let mut heads = std::mem::take(&mut parts.heads);
        let scrolls = std::mem::take(&mut parts.scrolls);
        if foldable(m, child) {
            heads.insert(0, (0, child));
        }
        return (parts.all(), heads, scrolls);
    }
    let mut sink = None;
    let lines = node_lines(m, child, width, ctx, None, &mut sink);
    let scrolls = match m.comp(child).map(|c| c.kind()) {
        Some("scroll") => vec![(0, lines.len(), child)],
        _ => Vec::new(),
    };
    (lines, Vec::new(), scrolls)
}

/// A group of cells: transparent below its threshold, else one header naming the count with the
/// cell's title as the noun ("✓ 5 tool calls  read ×3, grep ×2  1 ✗ ▸", "5 items" untitled)
/// that opens to the members, each folding on its own.
fn batch_parts(m: &Model, id: NodeId, cell: &Cell, width: u16, ctx: &Ctx) -> CellParts {
    let kids = m.children(id).to_vec();
    let mut parts = CellParts::default();
    // folded by default, like the web's batch; expand-all and the user's toggle flip it
    let open = kids.len() >= cell.batch && (ctx.expand_all ^ m.is_open(id));
    if kids.len() >= cell.batch {
        let noun = cell.title.as_deref().unwrap_or("items");
        parts.prefix = batch_header(m, &kids, noun, width, ctx, !open);
        if !open {
            return parts;
        }
    }

    // transparent: calls separated like transcript entries; open: indented under the header
    let indent = if kids.len() >= cell.batch { 2 } else { 0 };
    let inner = width.saturating_sub(indent as u16).max(1);
    for (i, kid) in kids.iter().enumerate() {
        if i > 0 && indent == 0 {
            parts.prefix.push(Line::default());
        }
        let base = parts.prefix.len();
        let (lines, h, sc) = child_lines(m, *kid, inner, ctx);
        parts
            .heads
            .extend(h.into_iter().map(|(i, n)| (base + i, n)));
        parts
            .scrolls
            .extend(sc.into_iter().map(|(i, len, n)| (base + i, len, n)));
        parts
            .prefix
            .extend(indented(lines, indent, Default::default()));
    }
    parts
}

fn batch_header(
    m: &Model,
    kids: &[NodeId],
    noun: &str,
    width: u16,
    ctx: &Ctx,
    folded: bool,
) -> Vec<Line<'static>> {
    let (mut running, mut errors, mut interrupted) = (0, 0, 0);
    let mut names: Vec<(String, usize)> = Vec::new();
    for kid in kids {
        let Some(c) = m.comp_as::<Cell>(*kid) else {
            continue;
        };
        match c.status.as_str() {
            "running" => running += 1,
            s if s.starts_with("waiting") => running += 1,
            "error" | "failed" => errors += 1,
            "interrupted" | "away" => interrupted += 1,
            _ => {}
        }
        let name = c.title.clone().unwrap_or_else(|| "tool".to_owned());
        match names.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => names.push((name, 1)),
        }
    }
    // most used first, ties in order of appearance
    names.sort_by_key(|n| std::cmp::Reverse(n.1));
    let mut summary: Vec<String> = names
        .iter()
        .take(4)
        .map(|(n, c)| {
            if *c > 1 {
                format!("{n} ×{c}")
            } else {
                n.clone()
            }
        })
        .collect();
    if names.len() > 4 {
        summary.push(format!("+{} more", names.len() - 4));
    }

    let icon = if running > 0 {
        Span::styled(theme::spinner(ctx.ms()), theme::tone("running"))
    } else if errors > 0 {
        Span::styled("✗", theme::danger())
    } else if interrupted > 0 {
        Span::styled("○", theme::warn())
    } else {
        Span::styled("✓", theme::ok())
    };
    let mut spans = vec![
        icon,
        Span::raw(" "),
        Span::styled(
            format!("{} {noun}", kids.len()),
            theme::accent().add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(format!("  {}", summary.join(", ")), theme::muted()),
    ];
    for (n, mark, style) in [
        (running, "⚙", theme::warn()),
        (errors, "✗", theme::danger()),
        (interrupted, "○", theme::warn()),
    ] {
        if n > 0 {
            spans.push(Span::styled(format!("  {n} {mark}"), style));
        }
    }
    spans.push(Span::styled(crate::widgets::fold_marker(folded), theme::muted()));
    if folded {
        spans.push(Span::styled("  ctrl+o expands", theme::faint()));
    }
    crate::text::wrap_spans(&spans, width as usize)
}

/// `line` padded to `width` with `band` under every span (set per span so the background
/// survives being written to scrollback).
fn banded(line: Line<'static>, width: usize, band: ratatui::style::Style) -> Line<'static> {
    let used: usize = line.spans.iter().map(|s| display_width(&s.content)).sum();
    let mut spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|s| {
            let style = band.patch(s.style);
            Span::styled(s.content, style)
        })
        .collect();
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), band));
    }
    Line::from(spans).style(line.style)
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
