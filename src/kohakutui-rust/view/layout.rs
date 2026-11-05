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
use ratatui::widgets::{Block, Clear, Widget};

use super::render::{cell_parts, node_lines, separator};
use super::ViewState;
use crate::model::{FlushState, Model, BOTTOM, LIVE, OVERLAYS, PANELS, TRANSCRIPT};
use crate::proto::{Level, NodeId};
use crate::text::{clip, theme};
use crate::widgets::{Cell, Ctx};

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
pub(crate) fn input_target(m: &Model, vs: &ViewState) -> Option<NodeId> {
    let top = m.top_overlay();
    if let Some(top) = top.filter(|t| vs.overlay_blur != Some(*t)) {
        return Some(top);
    }
    page_input(m, vs).or(top)
}

/// The input outside the overlays that keys go to: a valid focus, else the first input.
pub(crate) fn page_input(m: &Model, vs: &ViewState) -> Option<NodeId> {
    let usable = |id: NodeId| {
        m.node(id).is_some_and(|n| {
            !n.sealed
                && n.comp.focusable()
                && !matches!(m.region_of(id), Some(TRANSCRIPT) | Some(OVERLAYS))
        })
    };
    if let Some(f) = vs.focus.filter(|f| usable(*f)) {
        return Some(f);
    }
    [BOTTOM, LIVE, PANELS]
        .into_iter()
        .flat_map(|r| m.subtree(r))
        .find(|id| usable(*id))
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

/// The overlay framed for inline mode: a rule above, a gutter, a rule below. Without the keys
/// it is drawn muted, with a hint, and has no caret.
fn framed(
    m: &Model,
    id: NodeId,
    width: u16,
    ctx: &Ctx,
    focused: bool,
) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let inner = width.saturating_sub(2).max(1);
    let (mut lines, cursor) = region(m, id, inner, ctx, focused.then_some(id));
    if !focused {
        lines.push(Line::from(Span::styled(BLURRED_HINT, theme::muted())));
    }
    let edge = if focused {
        theme::accent()
    } else {
        theme::muted()
    };
    let rule = Line::from(Span::styled("─".repeat(width as usize), edge));
    let mut out = vec![rule.clone()];
    for l in lines {
        let mut spans = vec![Span::styled("│ ", edge)];
        spans.extend(l.spans);
        out.push(clip(Line::from(spans), width as usize));
    }
    out.push(rule);
    (out, cursor.map(|(r, c)| (r + 1, c + 2)))
}

/// Unflushed transcript lines: whole cells, or the unflushed tail of the head cell.
fn transcript_tail(m: &Model, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for &id in m.children(TRANSCRIPT) {
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

    // the overlay sits between the transcript and the bottom region and outranks the transcript
    let (mut overlay, mut overlay_cursor) = match m.top_overlay() {
        Some(top) => framed(m, top, width, ctx, target == Some(top)),
        None => (Vec::new(), None),
    };
    let (mut bottom, mut bottom_cursor) = region(m, BOTTOM, width, ctx, target);
    if overlay_cursor.is_some() {
        bottom_cursor = None;
    }
    keep_gap_above(&mut bottom, &mut bottom_cursor);

    if bottom.len() > max_h {
        let cut = bottom.len() - max_h;
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

    let overlay_hit = m.top_overlay().map(|top| {
        let row = overlay_at as i32 + 1 - overlay_cut as i32;
        (top, row, 2, width.saturating_sub(2).max(1), overlay_rows)
    });

    let mut lines = upper;
    lines.extend(overlay);
    lines.extend(bottom);
    Screen { lines, cursor, overlay: overlay_hit }
}

/// The bottom region always keeps one blank line from whatever is above it.
fn keep_gap_above(bottom: &mut Vec<Line<'static>>, cursor: &mut Option<(u16, u16)>) {
    if !bottom.is_empty() {
        bottom.insert(0, Line::default());
        *cursor = cursor.map(|(r, c)| (r + 1, c));
    }
}

/// Every transcript line for full screen, plus the line index of each foldable cell's header.
/// Sealed cells are cached by width, version and whether they are open.
fn transcript_all(
    m: &Model,
    vs: &mut ViewState,
    width: u16,
    ctx: &Ctx,
) -> (Vec<Line<'static>>, Vec<(usize, NodeId)>) {
    let mut out = Vec::new();
    let mut heads = Vec::new();
    for &id in m.children(TRANSCRIPT) {
        out.extend(separator(m, id));
        let Some(node) = m.node(id) else { continue };
        let open = vs.expand_all ^ vs.toggled.contains(&id);
        let ctx = ctx.expanded(open);
        if m.comp_as::<Cell>(id).is_some_and(|c| c.foldable()) {
            heads.push((out.len(), id));
        }

        if node.sealed {
            if let Some((w, v, o, lines)) = vs.cache.get(&id) {
                if *w == width && *v == node.version && *o == open {
                    out.extend(lines.iter().cloned());
                    continue;
                }
            }
        }
        let lines = if m.comp_as::<Cell>(id).is_some() {
            cell_parts(m, id, width, &ctx).all()
        } else {
            let mut sink = None;
            node_lines(m, id, width, &ctx, None, &mut sink)
        };
        if node.sealed {
            vs.cache
                .insert(id, (width, node.version, open, lines.clone()));
        }
        out.extend(lines);
    }
    vs.cache.retain(|id, _| m.contains(*id));
    vs.toggled.retain(|id| m.contains(*id));
    (out, heads)
}

fn draw_lines(buf: &mut Buffer, area: Rect, lines: &[Line<'static>]) {
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

    let (mut bottom, mut bottom_cursor) = region(m, BOTTOM, width, ctx, target);
    keep_gap_above(&mut bottom, &mut bottom_cursor);
    bottom.truncate((area.height / 2) as usize);
    let (mut live, _) = region(m, LIVE, width, ctx, None);
    live.truncate((area.height / 3) as usize);
    let toasts = toast_lines(vs, width);
    let fixed = (bottom.len() + live.len() + toasts.len()) as u16;
    let main_h = area.height.saturating_sub(fixed);

    // panels take the right third when the screen is wide enough
    let has_panels = !m.children(PANELS).is_empty() && width >= 60;
    let panel_w = if has_panels {
        (width / 3).clamp(24, 60)
    } else {
        0
    };
    let main = Rect::new(area.x, area.y, width - panel_w, main_h);

    let (lines, heads) = transcript_all(m, vs, main.width, ctx);
    let max_scroll = lines.len().saturating_sub(main_h as usize);
    vs.scroll = vs.scroll.min(max_scroll);
    let end = lines.len() - vs.scroll;
    let start = end.saturating_sub(main_h as usize);
    draw_lines(buf, main, &lines[start..end]);
    vs.hits = heads
        .into_iter()
        .filter(|(i, _)| (start..end).contains(i))
        .map(|(i, id)| (main.y + (i - start) as u16, main.width, id))
        .collect();
    if vs.scroll > 0 && main_h > 0 {
        let note =
            Line::from(Span::styled(format!(" ↓ {} more · pgdn ", vs.scroll), theme::selection()));
        let y = main.y + main_h - 1;
        buf.set_line(main.x, y, &note, main.width);
    }

    if has_panels {
        let pane = Rect::new(main.x + main.width, area.y, panel_w, main_h);
        let inner = pane.width.saturating_sub(2);
        let mut sink = None;
        let mut panel_lines = Vec::new();
        for (i, &p) in m.children(PANELS).iter().enumerate() {
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

    let mut y = area.y + main_h;
    for group in [&live, &toasts] {
        draw_lines(buf, Rect::new(area.x, y, width, group.len() as u16), group);
        y += group.len() as u16;
    }
    let bottom_y = y;
    draw_lines(buf, Rect::new(area.x, y, width, bottom.len() as u16), &bottom);

    let mut cursor = bottom_cursor.map(|(r, c)| (area.x + c, bottom_y + r));
    vs.overlay_hit = None;
    if let Some(top) = m.top_overlay() {
        let focused = target == Some(top);
        let (modal_cursor, inner) = modal(m, top, buf, ctx, focused);
        vs.overlay_hit = Some((top, inner.y as i32, inner.x, inner.width, inner.height));
        if focused {
            cursor = modal_cursor;
        }
    }
    cursor.filter(|(x, y)| *x < area.right() && *y < area.bottom())
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
    Block::bordered().border_style(edge).render(rect, buf);

    let inner = Rect::new(rect.x + 2, rect.y + 1, inner_w, h.saturating_sub(2));
    draw_lines(buf, inner, &lines);
    let cursor = cursor
        .filter(|(r, _)| *r < inner.height)
        .map(|(r, c)| (inner.x + c, inner.y + r));
    (cursor, inner)
}
