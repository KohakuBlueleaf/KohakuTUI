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
use crate::model::{FlushState, Model, BOTTOM, LIVE, PANELS, TRANSCRIPT};
use crate::proto::{Level, NodeId};
use crate::text::{clip, theme};
use crate::widgets::{Cell, Ctx};

/// The inline viewport: its lines and the caret within them.
#[derive(Debug, Default)]
pub struct Screen {
    pub lines: Vec<Line<'static>>,
    pub cursor: Option<(u16, u16)>,
}

/// The node that receives keys: the top overlay, else a valid focus, else the first input.
pub(crate) fn input_target(m: &Model, vs: &ViewState) -> Option<NodeId> {
    if let Some(top) = m.top_overlay() {
        return Some(top);
    }
    let usable = |id: NodeId| {
        m.node(id)
            .is_some_and(|n| !n.sealed && n.comp.focusable() && m.region_of(id) != Some(TRANSCRIPT))
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

/// The overlay framed for inline mode: a rule above, a gutter, a rule below.
fn framed(
    m: &Model,
    id: NodeId,
    width: u16,
    ctx: &Ctx,
) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let inner = width.saturating_sub(2).max(1);
    let (lines, cursor) = region(m, id, inner, ctx, Some(id));
    let rule = Line::from(Span::styled("─".repeat(width as usize), theme::accent()));
    let mut out = vec![rule.clone()];
    for l in lines {
        let mut spans = vec![Span::styled("│ ", theme::accent())];
        spans.extend(l.spans);
        out.push(Line::from(spans));
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

    let mut upper = transcript_tail(m, width, ctx);
    let (live, _) = region(m, LIVE, width, ctx, None);
    if !upper.is_empty() && !live.is_empty() {
        upper.push(Line::default());
    }
    upper.extend(live);
    upper.extend(toast_lines(vs, width));

    // the overlay sits between the transcript and the bottom region and outranks the transcript
    let (mut overlay, mut overlay_cursor) = match m.top_overlay() {
        Some(top) => framed(m, top, width, ctx),
        None => (Vec::new(), None),
    };
    let (mut bottom, mut bottom_cursor) = region(m, BOTTOM, width, ctx, target);
    if overlay_cursor.is_some() {
        bottom_cursor = None;
    }

    if bottom.len() > max_h {
        let cut = bottom.len() - max_h;
        bottom.drain(..cut);
        bottom_cursor = bottom_cursor.and_then(|(r, c)| r.checked_sub(cut as u16).map(|r| (r, c)));
    }
    let room = max_h - bottom.len();
    if overlay.len() > room {
        let cut = overlay.len() - room;
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

    let mut lines = upper;
    lines.extend(overlay);
    lines.extend(bottom);
    Screen { lines, cursor }
}

/// Every transcript line for full screen; sealed cells are cached by width and version.
fn transcript_all(m: &Model, vs: &mut ViewState, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for &id in m.children(TRANSCRIPT) {
        out.extend(separator(m, id));
        let Some(node) = m.node(id) else { continue };
        if node.sealed {
            if let Some((w, v, lines)) = vs.cache.get(&id) {
                if *w == width && *v == node.version {
                    out.extend(lines.iter().cloned());
                    continue;
                }
            }
        }
        let lines = if m.comp_as::<Cell>(id).is_some() {
            cell_parts(m, id, width, ctx).all()
        } else {
            let mut sink = None;
            node_lines(m, id, width, ctx, None, &mut sink)
        };
        if node.sealed {
            vs.cache.insert(id, (width, node.version, lines.clone()));
        }
        out.extend(lines);
    }
    vs.cache.retain(|id, _| m.contains(*id));
    out
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

    let (mut bottom, bottom_cursor) = region(m, BOTTOM, width, ctx, target);
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

    let lines = transcript_all(m, vs, main.width, ctx);
    let max_scroll = lines.len().saturating_sub(main_h as usize);
    vs.scroll = vs.scroll.min(max_scroll);
    let end = lines.len() - vs.scroll;
    let start = end.saturating_sub(main_h as usize);
    draw_lines(buf, main, &lines[start..end]);
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
    if let Some(top) = m.top_overlay() {
        cursor = modal(m, top, buf, ctx);
    }
    cursor.filter(|(x, y)| *x < area.right() && *y < area.bottom())
}

/// Draws an overlay as a centered bordered box; returns its caret in screen coordinates.
fn modal(m: &Model, id: NodeId, buf: &mut Buffer, ctx: &Ctx) -> Option<(u16, u16)> {
    let area = buf.area;
    let w = area.width.saturating_sub(4).clamp(10, 78).min(area.width);
    let inner_w = w.saturating_sub(4).max(1);
    let (lines, cursor) = region(m, id, inner_w, ctx, Some(id));

    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    Clear.render(rect, buf);
    Block::bordered()
        .border_style(theme::accent().add_modifier(Modifier::BOLD))
        .render(rect, buf);

    let inner = Rect::new(rect.x + 2, rect.y + 1, inner_w, h.saturating_sub(2));
    draw_lines(buf, inner, &lines);
    cursor
        .filter(|(r, _)| *r < inner.height)
        .map(|(r, c)| (inner.x + c, inner.y + r))
}
