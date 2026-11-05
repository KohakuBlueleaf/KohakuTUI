//! Full-screen panes and the head row: laying side columns out around the transcript, drawing
//! them, and the hit boxes that route clicks, drags and the wheel back to them.
//!
//! A `pane` under the panels region is a column on its `side`, `width` columns wide plus a
//! one-column divider the user drags (or double-clicks to collapse). A collapsed pane is a
//! one-column strip (▸ / ◂) that a click opens. When the transcript would get narrower than
//! [`MIN_MAIN`], panes collapse for this frame (right ones first, then left) without changing
//! what the app set.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::input::to_event;
use super::render::node_lines;
use super::ViewState;
use crate::model::{Model, PANELS};
use crate::proto::value::map;
use crate::proto::{Event, NodeId, Value};
use crate::text::theme;
use crate::widgets::{Ctx, Mouse, MouseAct, Pane};

/// The transcript keeps at least this many columns.
pub const MIN_MAIN: u16 = 30;
/// Two divider presses this close are a double-click.
pub const DOUBLE_CLICK_MS: u128 = 400;

/// A pane as laid out on the last frame.
#[derive(Debug, Clone)]
pub(crate) struct PaneHit {
    pub id: NodeId,
    pub left: bool,
    /// Drawn as a strip (collapsed by the app, or for lack of room).
    pub strip: bool,
    /// The whole column, divider included.
    pub area: Rect,
    /// The divider column (the strip itself when collapsed).
    pub divider: u16,
    /// Children as drawn: (first row on screen, rows, node, first row of the node shown).
    pub rows: Vec<(u16, u16, NodeId, u16)>,
    /// The screen column where children's content starts.
    pub content_x: u16,
}

/// An in-progress divider drag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    pub id: NodeId,
    pub left: bool,
    /// The screen column of the pane's outer edge.
    pub edge: u16,
    /// The width when the drag started: a release without a change reports nothing.
    pub from: usize,
}

/// The panes under the panels region (other children are not panes).
pub(crate) fn panes(m: &Model) -> Vec<(NodeId, &Pane)> {
    m.children(PANELS)
        .iter()
        .filter_map(|&id| m.comp_as::<Pane>(id).map(|p| (id, p)))
        .collect()
}

/// Splits `area` into the transcript's rect and each pane's column.
pub(crate) fn split(m: &Model, area: Rect) -> (Rect, Vec<PaneHit>) {
    let list = panes(m);
    let cap = area.width / 2;
    // (id, left, width when open, strip)
    let mut cols: Vec<(NodeId, bool, u16, bool)> = list
        .iter()
        .map(|(id, p)| {
            let w = (p.width as u16).clamp(p.min as u16, cap.max(p.min as u16)) + 1;
            (*id, p.side_left, w, p.collapsed)
        })
        .collect();
    // each side with a pane adds a column of air beside the chat column
    let used = |cols: &[(NodeId, bool, u16, bool)]| -> u16 {
        let pad = u16::from(cols.iter().any(|c| c.1)) + u16::from(cols.iter().any(|c| !c.1));
        cols.iter().map(|c| if c.3 { 1 } else { c.2 }).sum::<u16>() + pad
    };
    // too narrow: collapse right panes (innermost last) first, then left ones
    while area.width.saturating_sub(used(&cols)) < MIN_MAIN {
        let next = cols
            .iter()
            .rposition(|c| !c.3 && !c.1)
            .or_else(|| cols.iter().rposition(|c| !c.3));
        match next {
            Some(i) => cols[i].3 = true,
            None => break,
        }
    }
    // not even room for the strips: no panes this frame
    if used(&cols) + MIN_MAIN.min(area.width) > area.width {
        return (area, Vec::new());
    }

    let mut hits = Vec::new();
    let mut x = area.x;
    for &(id, _, w, strip) in cols.iter().filter(|c| c.1) {
        let w = if strip { 1 } else { w };
        let divider = x + w - 1;
        hits.push(PaneHit {
            id,
            left: true,
            strip,
            area: Rect::new(x, area.y, w, area.height),
            divider,
            rows: Vec::new(),
            content_x: x + 1,
        });
        x += w;
    }
    let mut right = area.x + area.width;
    // right panes in reverse so the first one sits next to the transcript
    let mut rights = Vec::new();
    for &(id, _, w, strip) in cols.iter().filter(|c| !c.1).rev() {
        let w = if strip { 1 } else { w };
        right -= w;
        rights.push(PaneHit {
            id,
            left: false,
            strip,
            area: Rect::new(right, area.y, w, area.height),
            divider: right,
            rows: Vec::new(),
            content_x: right + 2,
        });
    }
    hits.extend(rights.into_iter().rev());
    // a column of air between each pane's divider and the chat column
    let pad = u16::from(hits.iter().any(|h| h.left));
    let pad_right = u16::from(hits.iter().any(|h| !h.left));
    let w = right.saturating_sub(x + pad + pad_right);
    let main = Rect::new(x + pad, area.y, w, area.height);
    (main, hits)
}

/// Draws one pane into `buf` and records its children's rows.
pub(crate) fn draw(
    m: &Model,
    vs: &mut ViewState,
    hit: &mut PaneHit,
    buf: &mut Buffer,
    ctx: &Ctx,
    target: Option<NodeId>,
) {
    let a = hit.area;
    if a.height == 0 {
        return;
    }
    let focused = target.is_some_and(|t| m.is_within(t, hit.id));
    let edge = if focused {
        theme::accent()
    } else {
        theme::border()
    };
    for y in a.y..a.y + a.height {
        buf.set_string(hit.divider, y, "│", edge);
    }
    let pane = m.comp_as::<Pane>(hit.id);
    if hit.strip {
        let mark = if hit.left { "▸" } else { "◂" };
        buf.set_string(hit.divider, a.y, mark, theme::accent());
        return;
    }
    let inner_x = if hit.left { a.x } else { a.x + 1 };
    let inner = Rect::new(inner_x, a.y, a.width - 1, a.height);
    if inner.width < 3 {
        return;
    }
    let mut y = inner.y;
    if let Some(title) = pane.and_then(|p| p.title.clone()) {
        let style = if focused {
            theme::focus_area()
        } else {
            theme::bold()
        };
        buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
        let line = Line::from(Span::styled(format!(" {title}"), theme::bold()));
        buf.set_line(inner.x, y, &line, inner.width);
        y += 1;
    }
    // one column of padding on the left, one on the right for the scroll marks
    let content_w = inner.width.saturating_sub(2);
    hit.content_x = inner.x + 1;
    let room = (inner.y + inner.height).saturating_sub(y) as usize;
    if room == 0 {
        return;
    }
    let (lines, rows) = stacked(m, hit.id, content_w, ctx, target);
    let max = lines.len().saturating_sub(room);
    let top = vs.pane_scroll.get(&hit.id).copied().unwrap_or(0).min(max);
    vs.pane_scroll.insert(hit.id, top);
    for (i, line) in lines.iter().skip(top).take(room).enumerate() {
        buf.set_line(inner.x + 1, y + i as u16, line, content_w);
    }
    hit.rows = visible_rows(&rows, top, room, y);
    if top > 0 {
        buf.set_string(inner.x + inner.width - 1, y, "↑", theme::faint());
    }
    if top < max {
        let last = y + room as u16 - 1;
        buf.set_string(inner.x + inner.width - 1, last, "↓", theme::faint());
    }
}

/// Children of `parent` one under another, with each child's (first line, lines, node).
pub(crate) fn stacked(
    m: &Model,
    parent: NodeId,
    width: u16,
    ctx: &Ctx,
    target: Option<NodeId>,
) -> (Vec<Line<'static>>, Vec<(usize, usize, NodeId)>) {
    let mut lines = Vec::new();
    let mut rows = Vec::new();
    for &child in m.children(parent) {
        if m.hidden(child) {
            continue;
        }
        let mut sink = None;
        let got = node_lines(m, child, width, ctx, target, &mut sink);
        rows.push((lines.len(), got.len(), child));
        lines.extend(got);
    }
    (lines, rows)
}

/// Like [`stacked`], with the caret of `target` (row, column) when it is a descendant.
#[allow(clippy::type_complexity)]
pub(crate) fn stacked_with_cursor(
    m: &Model,
    parent: NodeId,
    width: u16,
    ctx: &Ctx,
    target: Option<NodeId>,
) -> (Vec<Line<'static>>, Option<(u16, u16)>, Vec<(usize, usize, NodeId)>) {
    let mut lines = Vec::new();
    let mut rows = Vec::new();
    let mut cursor = None;
    for &child in m.children(parent) {
        if m.hidden(child) {
            continue;
        }
        let mut sink = None;
        let got = node_lines(m, child, width, ctx, target, &mut sink);
        if let Some((r, c)) = sink {
            cursor = Some((lines.len() as u16 + r, c));
        }
        rows.push((lines.len(), got.len(), child));
        lines.extend(got);
    }
    (lines, cursor, rows)
}

/// The part of each child's rows inside the window `[top, top + room)`, placed from screen
/// row `y`: (screen row, rows, node, the node's first row shown).
pub(crate) fn visible_rows(
    rows: &[(usize, usize, NodeId)],
    top: usize,
    room: usize,
    y: u16,
) -> Vec<(u16, u16, NodeId, u16)> {
    rows.iter()
        .filter(|(at, len, _)| at + len > top && *at < top + room)
        .map(|&(at, len, id)| {
            let first = at.max(top);
            let last = (at + len).min(top + room);
            (y + (first - top) as u16, (last - first) as u16, id, (first - at) as u16)
        })
        .collect()
}

/// The node drawn at screen `(col, row)` in a set of recorded rows, with the row and column
/// in the node's own coordinates.
pub(crate) fn node_at(
    rows: &[(u16, u16, NodeId, u16)],
    x0: u16,
    col: u16,
    row: u16,
) -> Option<(NodeId, u16, u16)> {
    rows.iter()
        .find(|(y, h, _, _)| row >= *y && row < y + h)
        .map(|&(y, _, id, skip)| (id, row - y + skip, col.saturating_sub(x0)))
}

/// Whether two divider presses on `id` make a double-click; remembers this one.
pub(crate) fn double_click(vs: &mut ViewState, id: NodeId) -> bool {
    let now = Instant::now();
    let double = vs.divider_click.is_some_and(|(prev, at)| {
        prev == id && now.duration_since(at).as_millis() <= DOUBLE_CLICK_MS
    });
    vs.divider_click = if double { None } else { Some((id, now)) };
    double
}

/// Sets a pane's width (content columns) or collapsed flag and reports it as a `pane` event.
fn set_pane(
    m: &mut Model,
    id: NodeId,
    width: Option<usize>,
    collapsed: Option<bool>,
    events: &mut Vec<Event>,
) {
    let mut props = Vec::new();
    if let Some(w) = width {
        props.push(("width", Value::from(w as u64)));
    }
    if let Some(c) = collapsed {
        props.push(("collapsed", Value::from(c)));
    }
    if let Some(n) = m.node_mut(id) {
        let _ = n.comp.update(&map(props));
    }
    m.bump(id);
    if let Some(p) = m.comp_as::<Pane>(id) {
        events.push(Event::Pane { id, width: p.width as u64, collapsed: p.collapsed });
    }
}

/// Gives a click (or middle click) to the node under it and focuses it when it takes keys.
fn click(
    m: &mut Model,
    vs: &mut ViewState,
    at: Option<(NodeId, u16, u16)>,
    act: MouseAct,
    width: u16,
    events: &mut Vec<Event>,
) -> bool {
    let Some((id, row, col)) = at else {
        return false;
    };
    let mut emits = Vec::new();
    let Some(node) = m.node_mut(id).filter(|n| !n.sealed) else {
        return false;
    };
    let used = node
        .comp
        .on_mouse(&Mouse { act, row, col, width }, &mut emits);
    if node.comp.focusable() {
        vs.focus = Some(id);
    }
    m.bump(id);
    events.extend(emits.into_iter().map(|e| to_event(id, e)));
    used
}

/// Mouse on a pane, its divider or the head row (full screen, no dialog). None: not theirs.
pub(crate) fn mouse(
    m: &mut Model,
    vs: &mut ViewState,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    events: &mut Vec<Event>,
) -> Option<bool> {
    // a divider drag owns the mouse until the release (a press means the release was lost)
    if matches!(kind, MouseEventKind::Down(_)) {
        vs.drag = None;
    }
    if let Some(d) = vs.drag {
        match kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                let w = if d.left {
                    col.saturating_sub(d.edge)
                } else {
                    d.edge.saturating_sub(col + 1)
                };
                let cap = vs.screen_width / 2;
                if let Some(n) = m.node_mut(d.id) {
                    let _ = n
                        .comp
                        .update(&map(vec![("width", Value::from(w.min(cap) as u64))]));
                }
                m.bump(d.id);
                return Some(true);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                vs.drag = None;
                if m.comp_as::<Pane>(d.id).is_some_and(|p| p.width != d.from) {
                    // a drag is not the first half of a double-click
                    vs.divider_click = None;
                    set_pane(m, d.id, None, None, events);
                }
                return Some(true);
            }
            _ => {}
        }
    }
    let inside = |r: Rect| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height;
    if let Some(hit) = vs.pane_hits.iter().find(|h| inside(h.area)).cloned() {
        let content_w = hit.area.width.saturating_sub(3);
        let at = node_at(&hit.rows, hit.content_x, col, row);
        return Some(match kind {
            MouseEventKind::Down(MouseButton::Left) if hit.strip => {
                set_pane(m, hit.id, None, Some(false), events);
                true
            }
            MouseEventKind::Down(MouseButton::Left) if col == hit.divider => {
                if double_click(vs, hit.id) {
                    set_pane(m, hit.id, None, Some(true), events);
                } else {
                    let edge = if hit.left {
                        hit.area.x
                    } else {
                        hit.area.x + hit.area.width
                    };
                    let from = m.comp_as::<Pane>(hit.id).map_or(0, |p| p.width);
                    vs.drag = Some(Drag { id: hit.id, left: hit.left, edge, from });
                }
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                vs.selection = None;
                click(m, vs, at, MouseAct::Click, content_w, events)
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                click(m, vs, at, MouseAct::Middle, content_w, events)
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = kind == MouseEventKind::ScrollUp;
                let top = vs.pane_scroll.entry(hit.id).or_insert(0);
                *top = if up { top.saturating_sub(3) } else { *top + 3 };
                true
            }
            // a press inside a pane started no selection: its drag and release are no-ops
            _ => false,
        });
    }
    // a click on the input (or a row under it) gives it the keys
    if inside(vs.bottom_area) {
        if kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        vs.selection = None;
        let at = node_at(&vs.bottom_rows, vs.bottom_area.x, col, row);
        let width = vs.bottom_area.width;
        return Some(click(m, vs, at, MouseAct::Click, width, events) || at.is_some());
    }
    let head = vs.head_area;
    if inside(head) {
        let act = match kind {
            MouseEventKind::Down(MouseButton::Left) => MouseAct::Click,
            MouseEventKind::Down(MouseButton::Middle) => MouseAct::Middle,
            _ => return Some(false),
        };
        // the width the head row was drawn at, so a scrolled tab strip maps the click right
        let at = node_at(&vs.head_rows, head.x, col, row);
        return Some(click(m, vs, at, act, head.width, events));
    }
    None
}

/// alt+←/→ on a focused pane moves its divider (two columns a press).
pub(crate) fn resize_key(
    m: &mut Model,
    vs: &ViewState,
    target: Option<NodeId>,
    code: KeyCode,
    mods: KeyModifiers,
    events: &mut Vec<Event>,
) -> bool {
    if !mods.contains(KeyModifiers::ALT) || !matches!(code, KeyCode::Left | KeyCode::Right) {
        return false;
    }
    let Some(t) = target else { return false };
    let Some(hit) = vs
        .pane_hits
        .iter()
        .find(|h| !h.strip && m.is_within(t, h.id))
    else {
        return false;
    };
    let Some(p) = m.comp_as::<Pane>(hit.id) else {
        return false;
    };
    // the divider follows the arrow: right widens a left pane and narrows a right one
    let wider = (code == KeyCode::Right) == hit.left;
    let w = if wider {
        p.width + 2
    } else {
        p.width.saturating_sub(2).max(p.min)
    };
    let w = w.min(vs.screen_width as usize / 2);
    set_pane(m, hit.id, Some(w), None, events);
    true
}
