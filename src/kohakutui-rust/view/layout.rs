//! Frame composition for both modes (design §6.3, §6.5).
//!
//! Inline: the unflushed transcript tail, live region, toasts, the top overlay and the bottom
//! region, capped below the screen height so history insertion keeps a scroll region. Full
//! screen: a scrollable transcript (with panels beside it when any exist), live region, bottom
//! region, and the top overlay as a centered modal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

use super::panes;
use super::render::{cell_parts, foldable, node_lines, separator};
use super::ViewState;
use super::{Anchor, Mode};
use crate::model::{FlushState, Model, BOTTOM, HEAD, LIVE, OVERLAYS, PANELS};
use crate::proto::{Level, NodeId};
use crate::text::{clip, display_width, theme};
use crate::widgets::{Cell, Ctx, Pane, Role};

/// The inline viewport: its lines, the caret within them, and where the top overlay's content
/// is: (overlay, row, col, width, height), the row possibly above the viewport when cut.
#[derive(Debug, Default)]
pub struct Screen {
    pub lines: Vec<Line<'static>>,
    pub cursor: Option<(u16, u16)>,
    pub overlay: Option<(NodeId, i32, u16, u16, u16)>,
}

/// The node that receives keys: the top overlay (unless the user typed away from it), else a
/// valid focus, else the first input.
pub fn input_target(m: &Model, vs: &ViewState) -> Option<NodeId> {
    let top = m.top_overlay();
    if let Some(top) = top.filter(|t| vs.overlay_blur != Some(*t)) {
        return Some(top);
    }
    page_input(m, vs).or(top)
}

/// The input outside the overlays that keys go to: a valid focus (a control in an open
/// transcript cell counts once focused), else the first input outside the transcript.
pub(crate) fn page_input(m: &Model, vs: &ViewState) -> Option<NodeId> {
    let full = vs.mode == Mode::Fullscreen;
    let open = |id: NodeId| {
        let region = m.region_of(id);
        m.node(id).is_some_and(|n| !n.sealed && n.comp.focusable())
            && region != Some(OVERLAYS)
            && (full || !matches!(region, Some(HEAD | PANELS)))
            && m.doc_of(id).is_none_or(|d| d == m.doc())
            && !m.ancestors_sealed(id)
            && !m.hidden(id)
    };
    if let Some(f) = vs.focus.filter(|f| open(*f)) {
        return Some(f);
    }
    [BOTTOM, LIVE, PANELS]
        .into_iter()
        .flat_map(|r| m.subtree(r))
        .find(|id| open(*id))
}

fn toast_lines(vs: &ViewState, width: u16) -> Vec<Line<'static>> {
    vs.toasts
        .iter()
        .map(|t| {
            let (icon, style) = match t.level {
                Level::Info => ("ℹ", theme::accent()),
                Level::Success => ("✓", theme::ok()),
                Level::Warn => ("!", theme::warn()),
                Level::Error => ("✗", theme::danger()),
            };
            let line = Line::from(vec![
                Span::styled(format!("{icon} "), style),
                Span::styled(t.text.clone(), style),
            ]);
            clip(line, width as usize)
        })
        .collect()
}

fn region(
    m: &Model,
    id: NodeId,
    width: u16,
    ctx: &Ctx,
    target: Option<NodeId>,
) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let mut cursor = None;
    let lines = node_lines(m, id, width, ctx, target, &mut cursor);
    (lines, cursor)
}

/// Shown in a dialog the user typed away from.
const BLURRED_HINT: &str = "tab returns to this dialog";

/// The overlay in a rounded box like the boxed input: ╭─╮ │ … │ ╰─╯, content padded to the
/// inner width. Without the keys it is drawn muted, with a hint, and has no caret.
fn framed(
    m: &Model,
    id: NodeId,
    width: u16,
    ctx: &Ctx,
    focused: bool,
) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let w = width.max(5) as usize;
    let inner = w - 4;
    let (mut lines, cursor) = region(m, id, inner as u16, ctx, focused.then_some(id));
    if !focused {
        lines.push(Line::from(Span::styled(BLURRED_HINT, theme::muted())));
    }
    let edge = if focused {
        theme::accent()
    } else {
        theme::muted()
    };
    let rule =
        |l: &str, r: &str| Line::from(Span::styled(format!("{l}{}{r}", "─".repeat(w - 2)), edge));
    let mut out = vec![rule("╭", "╮")];
    for l in lines {
        let body = clip(l, inner);
        let used: usize = body.spans.iter().map(|s| display_width(&s.content)).sum();
        let mut spans = vec![Span::styled("│ ", edge)];
        spans.extend(body.spans);
        spans.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
        spans.push(Span::styled(" │", edge));
        out.push(Line::from(spans));
    }
    out.push(rule("╰", "╯"));
    (out, cursor.map(|(r, c)| (r + 1, c + 2)))
}

/// Unflushed transcript lines: whole cells, or the unflushed tail of the head cell.
fn transcript_tail(m: &Model, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for &id in m.children(m.doc()) {
        match m.flush_state(id) {
            Some(FlushState::Full) => {}
            Some(FlushState::Partial { src_off }) => {
                let parts = cell_parts(m, id, width, ctx);
                let (stable, pending, _) = parts.body.unwrap_or_default();
                out.extend(
                    stable
                        .into_iter()
                        .filter(|l| l.src >= src_off)
                        .map(|l| l.line),
                );
                out.extend(pending);
            }
            None => {
                out.extend(separator(m, id));
                let mut sink = None;
                out.extend(node_lines(m, id, width, ctx, None, &mut sink));
            }
        }
    }
    out
}

/// Composes the inline viewport, at most `rows - 2` lines.
pub fn inline(m: &Model, vs: &ViewState, width: u16, rows: u16, ctx: &Ctx) -> Screen {
    let max_h = rows.saturating_sub(2).max(1) as usize;
    let target = input_target(m, vs);

    // live content keeps one blank line from what is above it, scrollback included
    let mut upper = transcript_tail(m, width, ctx);
    let (live, _) = region(m, LIVE, width, ctx, None);
    if !live.is_empty() {
        upper.push(Line::default());
    }
    upper.extend(live);
    let toasts = toast_lines(vs, width);
    if !toasts.is_empty() {
        upper.push(Line::default());
    }
    upper.extend(toasts);

    // the overlay sits between the transcript and the bottom region and outranks the transcript;
    // a docked one takes the bottom region's place instead
    let docked = docked_overlay(m);
    let (mut overlay, mut overlay_cursor) = match m.top_overlay() {
        Some(top) if docked.is_none() => framed(m, top, width, ctx, target == Some(top)),
        _ => (Vec::new(), None),
    };
    let (mut bottom, mut bottom_cursor) = match docked {
        Some(top) => framed(m, top, width, ctx, target == Some(top)),
        None => region(m, BOTTOM, width, ctx, target),
    };
    if overlay_cursor.is_some() {
        bottom_cursor = None;
    }
    keep_gap_above(&mut bottom, &mut bottom_cursor);

    let mut bottom_cut = 0;
    if bottom.len() > max_h {
        let cut = bottom.len() - max_h;
        bottom_cut = cut;
        bottom.drain(..cut);
        bottom_cursor = bottom_cursor.and_then(|(r, c)| r.checked_sub(cut as u16).map(|r| (r, c)));
    }
    let room = max_h - bottom.len();
    let overlay_rows = overlay.len().saturating_sub(2) as u16;
    let mut overlay_cut = 0;
    if overlay.len() > room {
        let cut = overlay.len() - room;
        overlay_cut = cut;
        overlay.drain(..cut);
        overlay_cursor =
            overlay_cursor.and_then(|(r, c)| r.checked_sub(cut as u16).map(|r| (r, c)));
    }
    let room = room - overlay.len();
    if upper.len() > room {
        let cut = upper.len() - room + 1;
        upper.drain(..cut.min(upper.len()));
        if room > 0 {
            let marker = format!("⋯ {cut} more lines above");
            upper.insert(0, Line::from(Span::styled(marker, theme::muted())));
        }
    }

    let overlay_at = upper.len() as u16;
    let bottom_at = overlay_at + overlay.len() as u16;
    let cursor = overlay_cursor
        .map(|(r, c)| (overlay_at + r, c))
        .or(bottom_cursor.map(|(r, c)| (bottom_at + r, c)));

    let overlay_hit = match docked {
        // after the gap line and the top rule
        Some(top) => {
            let rows = bottom.len().saturating_sub(3) as u16;
            let row = bottom_at as i32 + 2 - bottom_cut as i32;
            Some((top, row, 2, width.saturating_sub(4).max(1), rows))
        }
        None => m.top_overlay().map(|top| {
            let row = overlay_at as i32 + 1 - overlay_cut as i32;
            (top, row, 2, width.saturating_sub(4).max(1), overlay_rows)
        }),
    };

    let mut lines = upper;
    lines.extend(overlay);
    lines.extend(bottom);
    Screen { lines, cursor, overlay: overlay_hit }
}

/// The top overlay when it asks to sit where the input box is.
pub(crate) fn docked_overlay(m: &Model) -> Option<NodeId> {
    m.top_overlay()
        .filter(|t| m.node(*t).is_some_and(|n| n.comp.docked()))
}

/// The bottom region always keeps one blank line from whatever is above it.
fn keep_gap_above(bottom: &mut Vec<Line<'static>>, cursor: &mut Option<(u16, u16)>) {
    if !bottom.is_empty() {
        bottom.insert(0, Line::default());
        *cursor = cursor.map(|(r, c)| (r + 1, c));
    }
}

/// Every transcript line for full screen, with the foldable headers (line, node) and scroll
/// boxes (first line, height, node) at every depth. Sealed cells are cached by width, version
/// and expand-all; opening a nested header bumps its ancestors' versions.
struct Transcript {
    lines: Vec<Line<'static>>,
    /// Foldable headers: (line, node).
    heads: Vec<(usize, NodeId)>,
    /// Scroll boxes: (first line, lines, node).
    scrolls: Vec<(usize, usize, NodeId)>,
    /// The first line of each user message (the sticky prompt when scrolled past it).
    prompts: Vec<usize>,
    /// Each top-level node's first line (its separator included), in order.
    bases: Vec<(NodeId, usize)>,
    /// Top-level nodes that are not cells and take input (a page's tree): (first line after
    /// the separator, lines, node).
    plain: Vec<(usize, usize, NodeId)>,
}

fn transcript_all(m: &Model, vs: &mut ViewState, width: u16, ctx: &Ctx) -> Transcript {
    let ctx = ctx.expanded(vs.expand_all);
    let mut out = Vec::new();
    let mut heads = Vec::new();
    let mut scrolls = Vec::new();
    let mut prompts = Vec::new();
    let mut bases = Vec::new();
    let mut plain = Vec::new();
    for &id in m.children(m.doc()) {
        bases.push((id, out.len()));
        out.extend(separator(m, id));
        let Some(node) = m.node(id) else { continue };
        let base = out.len();
        if m.comp_as::<Cell>(id).is_some_and(|c| c.role == Role::User) {
            prompts.push(base);
        }

        let cached = vs
            .cache
            .get(&id)
            .filter(|c| node.sealed && c.0 == width && c.1 == node.version && c.2 == vs.expand_all);
        let (lines, h, sc) = match cached {
            Some(c) => (c.3.clone(), c.4.clone(), c.5.clone()),
            None => {
                let rendered = if m.comp_as::<Cell>(id).is_some() {
                    let mut parts = cell_parts(m, id, width, &ctx);
                    let mut h = std::mem::take(&mut parts.heads);
                    let sc = std::mem::take(&mut parts.scrolls);
                    if foldable(m, id) {
                        h.insert(0, (0, id));
                    }
                    (parts.all(), h, sc)
                } else {
                    let mut sink = None;
                    (node_lines(m, id, width, &ctx, None, &mut sink), Vec::new(), Vec::new())
                };
                if node.sealed {
                    let (l, h, sc) = &rendered;
                    let entry =
                        (width, node.version, vs.expand_all, l.clone(), h.clone(), sc.clone());
                    vs.cache.insert(id, entry);
                }
                rendered
            }
        };
        // (only ones that take input: text and links keep the transcript's own handling)
        if m.comp_as::<Cell>(id).is_none() && node.comp.focusable() && !node.sealed {
            plain.push((base, lines.len(), id));
        }
        heads.extend(h.into_iter().map(|(i, n)| (base + i, n)));
        scrolls.extend(sc.into_iter().map(|(i, len, n)| (base + i, len, n)));
        out.extend(lines);
    }
    vs.cache.retain(|id, _| m.contains(*id));
    Transcript { lines: out, heads, scrolls, prompts, bases, plain }
}

fn draw_lines(buf: &mut Buffer, area: Rect, lines: &[Line<'static>]) {
    // rows past the screen (more fixed rows than it is tall) are not drawn
    let area = area.intersection(buf.area);
    for (i, line) in lines.iter().take(area.height as usize).enumerate() {
        buf.set_line(area.x, area.y + i as u16, line, area.width);
    }
}

/// Draws the full-screen frame into `buf` (its area is the whole screen); returns the caret.
pub fn fullscreen(
    m: &Model,
    vs: &mut ViewState,
    buf: &mut Buffer,
    ctx: &Ctx,
) -> Option<(u16, u16)> {
    let area = buf.area;
    let target = input_target(m, vs);
    let width = area.width;
    vs.screen_width = width;

    // the head row (a tab strip of pages) across the top, over everything else
    let (head, head_rows) = panes::stacked(m, HEAD, width, ctx, target);
    let head_h = head.len().min(3).min(area.height as usize / 4) as u16;
    draw_lines(buf, Rect::new(area.x, area.y, width, head_h), &head);
    vs.head_rows = panes::visible_rows(&head_rows, 0, head_h as usize, area.y);
    vs.head_area = Rect::new(area.x, area.y, width, head_h);
    let body = Rect::new(area.x, area.y + head_h, width, area.height - head_h);

    // under it panes take the full height; the chat column between them holds the
    // transcript, the toasts, the live rows and the input
    let pane_list = !panes::panes(m).is_empty();
    let (mut column, mut pane_hits) = if pane_list {
        panes::split(m, body)
    } else {
        (body, Vec::new())
    };
    // without panes, other panel content takes the right third
    // (panel content that is not a pane; a hidden pane is not laid out at all)
    let loose: Vec<NodeId> = m
        .children(PANELS)
        .iter()
        .copied()
        .filter(|&id| m.comp_as::<Pane>(id).is_none())
        .collect();
    let has_panels = !pane_list && !loose.is_empty() && width >= 60;
    let panel_w = if has_panels {
        (width / 3).clamp(24, 60)
    } else {
        0
    };
    column.width -= panel_w;
    let cw = column.width;

    // a docked overlay replaces the bottom region, framed, and may take two thirds of the screen
    let docked = docked_overlay(m);
    let (mut bottom, mut bottom_cursor, bottom_rows) = match docked {
        Some(top) => {
            let (lines, cursor) = framed(m, top, cw, ctx, target == Some(top));
            (lines, cursor, Vec::new())
        }
        None => panes::stacked_with_cursor(m, BOTTOM, cw, ctx, target),
    };
    let (mut live, _) = region(m, LIVE, cw, ctx, None);
    live.truncate((area.height / 3) as usize);
    // the row above the input is kept for the live rows, so a spinner coming and going does
    // not move the transcript: with nothing live it is the blank gap above the input
    let gap = live.is_empty();
    if gap {
        keep_gap_above(&mut bottom, &mut bottom_cursor);
    }
    let cap = match docked {
        Some(_) => area.height as usize * 2 / 3,
        None => (area.height / 2) as usize,
    };
    bottom.truncate(cap);
    // a caret cut off with its rows is not shown
    bottom_cursor = bottom_cursor.filter(|(r, _)| (*r as usize) < bottom.len());
    // short of rows, the input keeps its own first, then the live rows, then the toasts
    let mut room = (column.height as usize).saturating_sub(bottom.len());
    live.truncate(room);
    room -= live.len();
    let mut toasts = toast_lines(vs, cw);
    toasts.truncate(room);
    let fixed = (bottom.len() + live.len() + toasts.len()) as u16;
    let main_h = column.height.saturating_sub(fixed);
    let main = Rect::new(column.x, column.y, cw, main_h);
    let below = column.y + main_h;

    let t = transcript_all(m, vs, main.width, ctx);
    let Transcript { lines, heads, scrolls, prompts, bases, plain } = t;
    let total = lines.len();
    let h = main_h as usize;
    // follow: scrolled up, the line at the top of the view stays there whatever changes below
    // or around it (new lines, a re-wrap, a fold), until the user scrolls; messages that arrive
    // meanwhile are counted
    let cells = m.children(m.doc()).len();
    match vs.anchor.filter(|a| vs.scroll > 0 && vs.scroll == a.scroll) {
        Some(a) => {
            if let Some(&(_, base)) = bases.iter().find(|(id, _)| *id == a.node) {
                let top = (base + a.offset).min(total);
                vs.scroll = total.saturating_sub(top + h).max(1);
            }
            vs.unseen += cells.saturating_sub(a.cells);
        }
        None if vs.scroll == 0 => vs.unseen = 0,
        None => {}
    }
    vs.anchor = None;
    // the header focused by keyboard navigation stays in view; a removed one ends navigation
    if vs
        .nav
        .is_some_and(|n| !heads.iter().any(|(_, id)| *id == n))
    {
        vs.nav = None;
    }
    if let Some(&(i, _)) = vs.nav.and_then(|n| heads.iter().find(|(_, id)| *id == n)) {
        let lowest = (total - i).saturating_sub(h);
        vs.scroll = vs
            .scroll
            .clamp(lowest, (total - i).saturating_sub(1).max(lowest));
    }
    let max_scroll = total.saturating_sub(h);
    vs.scroll = vs.scroll.min(max_scroll);
    let end = total - vs.scroll;
    let start = end.saturating_sub(h);
    draw_lines(buf, main, &lines[start..end]);
    vs.main_area = main;
    // top-level nodes that are not cells (a page's tree or table) take clicks
    vs.doc_rows = panes::visible_rows(&plain, start, end - start, main.y);
    if vs.scroll > 0 {
        // the node holding the top line, and how far into it, for the next frame
        if let Some(&(node, base)) = bases.iter().rev().find(|(_, b)| *b <= start) {
            let offset = start - base;
            vs.anchor = Some(Anchor { node, offset, scroll: vs.scroll, cells });
        }
    }
    vs.heads = heads.iter().map(|(_, id)| *id).collect();
    vs.hits = heads
        .into_iter()
        .filter(|(i, _)| (start..end).contains(i))
        .map(|(i, id)| (main.y + (i - start) as u16, main.width, id))
        .collect();
    vs.scroll_hits = scrolls
        .into_iter()
        .filter(|(i, len, _)| *i < end && i + len > start)
        .map(|(i, len, id)| {
            let top = i.max(start);
            let bottom = (i + len).min(end);
            (main.y + (top - start) as u16, (bottom - top) as u16, id)
        })
        .collect();
    // the header under the pointer is tinted, the navigated one highlighted
    for &(y, w, id) in &vs.hits {
        let style = if vs.nav == Some(id) {
            theme::selection()
        } else if vs.hover == Some(id) {
            theme::user_band()
        } else {
            continue;
        };
        buf.set_style(Rect::new(main.x, y, w, 1), style);
    }
    // scrolled past a prompt: the last one above the view stays on the top row
    if let Some(&at) = prompts.iter().rev().find(|&&at| at < start) {
        if main_h > 2 {
            buf.set_style(Rect::new(main.x, main.y, main.width, 1), theme::user_band());
            buf.set_line(main.x, main.y, &lines[at], main.width);
            vs.hits.retain(|&(y, _, _)| y != main.y);
            // a scroll box under the sticky row starts one row lower
            vs.scroll_hits = std::mem::take(&mut vs.scroll_hits)
                .into_iter()
                .filter_map(|(y, rows, id)| match y == main.y {
                    true => (rows > 1).then_some((y + 1, rows - 1, id)),
                    false => Some((y, rows, id)),
                })
                .collect();
        }
    }
    if vs.scroll > 0 && main_h > 0 {
        let text = if vs.unseen > 0 {
            format!(" ↓ {} new · ctrl+end ", vs.unseen)
        } else {
            format!(" ↓ {} more · ctrl+end ", vs.scroll)
        };
        let note = Line::from(Span::styled(text, theme::selection()));
        // on the top row instead when the bottom one holds the navigated header
        let bottom = main.y + main_h - 1;
        let covers = vs
            .hits
            .iter()
            .any(|&(y, _, id)| y == bottom && vs.nav == Some(id));
        let y = if covers { main.y } else { bottom };
        // the note's row is its own: what was drawn under it is cleared, not half covered
        for x in main.x..main.x + main.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
        buf.set_line(main.x, y, &note, main.width);
    }

    for hit in pane_hits.iter_mut() {
        panes::draw(m, vs, hit, buf, ctx, target);
    }
    vs.pane_hits = pane_hits;
    if has_panels {
        let pane = Rect::new(column.x + cw, body.y, panel_w, body.height);
        let inner = pane.width.saturating_sub(2);
        let mut sink = None;
        let mut panel_lines = Vec::new();
        for (i, &p) in loose.iter().enumerate() {
            if i > 0 {
                panel_lines.push(Line::default());
            }
            panel_lines.extend(node_lines(m, p, inner, ctx, None, &mut sink));
        }
        for y in 0..pane.height {
            buf.set_string(pane.x, pane.y + y, "│", theme::muted());
        }
        draw_lines(buf, Rect::new(pane.x + 2, pane.y, inner, pane.height), &panel_lines);
    }

    // under the transcript, in the chat column: toasts, the live rows, then the input
    let mut y = below;
    for group in [&toasts, &live] {
        draw_lines(buf, Rect::new(column.x, y, cw, group.len() as u16), group);
        y += group.len() as u16;
    }
    let bottom_y = y;
    draw_lines(buf, Rect::new(column.x, y, cw, bottom.len() as u16), &bottom);
    let skip = u16::from(gap && docked.is_none() && !bottom_rows.is_empty());
    let room = bottom.len().saturating_sub(skip as usize);
    vs.bottom_rows = panes::visible_rows(&bottom_rows, 0, room, bottom_y + skip);
    vs.bottom_area = Rect::new(column.x, bottom_y, cw, bottom.len() as u16);

    let mut cursor = bottom_cursor.map(|(r, c)| (column.x + c, bottom_y + r));
    vs.overlay_hit = None;
    if let Some(top) = docked {
        let rows = bottom.len().saturating_sub(3) as u16;
        vs.overlay_hit = Some((top, bottom_y as i32 + 2, column.x + 2, cw.saturating_sub(4), rows));
    } else if let Some(top) = m.top_overlay() {
        let focused = target == Some(top);
        let (modal_cursor, inner) = modal(m, top, buf, ctx, focused);
        vs.overlay_hit = Some((top, inner.y as i32, inner.x, inner.width, inner.height));
        if focused {
            cursor = modal_cursor;
        }
    }
    select(vs, buf, area);
    cursor.filter(|(x, y)| *x < area.right() && *y < area.bottom())
}

/// The drag selection: its text is taken once, on release, from what this frame drew; then it
/// is shown reversed until the next press.
fn select(vs: &mut ViewState, buf: &mut Buffer, area: Rect) {
    let Some(mut sel) = vs.selection else {
        return;
    };
    let ((c0, r0), (c1, r1)) = sel.ordered();
    let r1 = r1.min(area.bottom().saturating_sub(1));
    if sel.done && !sel.copied {
        let mut rows = Vec::new();
        for y in r0..=r1 {
            let from = if y == r0 { c0 } else { area.x };
            let to = if y == r1 {
                c1
            } else {
                area.right().saturating_sub(1)
            };
            let row: String = (from..=to.min(area.right().saturating_sub(1)))
                .map(|x| buf[(x, y)].symbol())
                .collect();
            rows.push(row.trim_end().to_owned());
        }
        let text = rows.join("\n");
        if !text.trim().is_empty() {
            vs.copied = Some(text);
        }
        sel.copied = true;
        vs.selection = Some(sel);
    }
    for y in r0..=r1 {
        for x in area.x..area.right() {
            if sel.contains(x, y) {
                let cell = &mut buf[(x, y)];
                cell.set_style(cell.style().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

/// Draws an overlay as a centered bordered box; returns its caret in screen coordinates and
/// the area its content was drawn in.
fn modal(
    m: &Model,
    id: NodeId,
    buf: &mut Buffer,
    ctx: &Ctx,
    focused: bool,
) -> (Option<(u16, u16)>, Rect) {
    let area = buf.area;
    let w = area.width.saturating_sub(4).clamp(10, 78).min(area.width);
    let inner_w = w.saturating_sub(4).max(1);
    let (mut lines, cursor) = region(m, id, inner_w, ctx, focused.then_some(id));
    if !focused {
        let hint = Line::from(Span::styled(BLURRED_HINT, theme::muted()));
        lines.push(clip(hint, inner_w as usize));
    }

    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    Clear.render(rect, buf);
    let edge = match focused {
        true => theme::accent().add_modifier(Modifier::BOLD),
        false => theme::muted(),
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(edge)
        .render(rect, buf);

    let inner = Rect::new(rect.x + 2, rect.y + 1, inner_w, h.saturating_sub(2));
    draw_lines(buf, inner, &lines);
    let cursor = cursor
        .filter(|(r, _)| *r < inner.height)
        .map(|(r, c)| (inner.x + c, inner.y + r));
    (cursor, inner)
}

/// Runs of linked cells in a drawn frame, as (row, first col, end col, link id), so a click in
/// full screen can find the URL under it.
pub fn link_hits(buf: &Buffer) -> Vec<(u16, u16, u16, u32)> {
    let area = buf.area;
    let mut hits = Vec::new();
    for y in area.top()..area.bottom() {
        let mut run: Option<(u16, u32)> = None;
        for x in area.left()..=area.right() {
            let id = (x < area.right())
                .then(|| crate::text::link::id_of(Some(buf[(x, y)].underline_color)))
                .flatten();
            if run.map(|(_, r)| Some(r)) != Some(id) {
                if let Some((start, r)) = run.take() {
                    hits.push((y, start, x, r));
                }
                run = id.map(|i| (x, i));
            }
        }
    }
    hits
}
