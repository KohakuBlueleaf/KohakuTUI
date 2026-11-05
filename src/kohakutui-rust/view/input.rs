//! Key routing (design §7.1): ctrl+c and bound chords first, then the top overlay (esc closes
//! it), then the focused input, then the built-in keys (mode toggle, scrolling, focus cycling).
//! A character or paste the dialog does not take goes to the page's input instead, which keeps
//! the keys until a submit, tab or esc there hands them back to the dialog.
//! Mouse events exist only in full screen: the wheel scrolls, a click opens a folded cell.

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use super::layout::{input_target, page_input};
use super::{Selection, ViewState};
use crate::model::{Model, BOTTOM, LIVE, PANELS, TRANSCRIPT};
use crate::proto::{Event, Mode, NodeId};
use crate::widgets::{Emit, Input, Mouse, MouseAct};

/// What the engine must do after a key, beyond the events already queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    ToggleMode,
    /// ctrl+o: open every folded cell (switching to full screen from inline).
    ExpandAll,
    Interrupt,
}

/// Routes one input; semantic events go to `events`.
pub fn handle_input(
    m: &mut Model,
    vs: &mut ViewState,
    input: Input,
    events: &mut Vec<Event>,
) -> Outcome {
    let top = m.top_overlay();
    vs.track_overlay(top);
    let target = input_target(m, vs);

    if let Input::Key { code, mods, .. } = &input {
        let (code, mods) = (*code, *mods);
        if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
            return Outcome::Interrupt;
        }
        if let Some((_, action)) = vs.bindings.iter().find(|(c, _)| c.matches(code, mods)) {
            events.push(Event::Action { name: action.clone(), focus: target });
            return Outcome::Redraw;
        }
        if code == KeyCode::Char('t') && mods == KeyModifiers::CONTROL {
            return Outcome::ToggleMode;
        }
        if code == KeyCode::Char('o') && mods == KeyModifiers::CONTROL {
            return Outcome::ExpandAll;
        }
        if let Some(outcome) = navigate(m, vs, code, mods) {
            return outcome;
        }
        // ctrl+end / ctrl+home jump the full-screen transcript even while an input has focus
        if vs.mode == Mode::Fullscreen && mods.contains(KeyModifiers::CONTROL) {
            match code {
                KeyCode::End => {
                    vs.scroll = 0;
                    return Outcome::Redraw;
                }
                KeyCode::Home => {
                    // the layout clamps this to the top of the transcript
                    vs.scroll = usize::MAX / 2;
                    return Outcome::Redraw;
                }
                _ => {}
            }
        }
        if code == KeyCode::Esc {
            if let Some(top) = top.filter(|t| target == Some(*t)) {
                events.push(Event::Close { id: top, reason: "escape".into() });
                return Outcome::Redraw;
            }
        }
    }

    if let Some(t) = target {
        let (handled, submitted) = deliver(m, t, &input, events);
        if handled {
            if submitted && top.is_some_and(|o| o != t) {
                vs.overlay_blur = None;
            }
            return Outcome::Redraw;
        }
        // a docked dialog has replaced the input box: its keys never leave it
        let docked = super::layout::docked_overlay(m).is_some();
        if let Some(top) = top.filter(|o| *o == t && is_text(&input) && !docked) {
            if let Some(page) = page_input(m, vs) {
                vs.overlay_blur = Some(top);
                deliver(m, page, &input, events);
                return Outcome::Redraw;
            }
        }

        // ↓ that the dialog does not use leaves it for the input below
        let down = matches!(input, Input::Key { code: KeyCode::Down, .. });
        if top == Some(t) && down && !docked && page_input(m, vs).is_some() {
            vs.overlay_blur = Some(t);
            return Outcome::Redraw;
        }

        // typing on a node that takes no text (a focused button) goes to the first input that
        // does, the bottom region's first
        if top.is_none_or(|o| o != t) && is_text(&input) {
            let inputs = [BOTTOM, LIVE, PANELS]
                .into_iter()
                .flat_map(|r| m.subtree(r))
                .filter(|id| {
                    *id != t && m.node(*id).is_some_and(|n| !n.sealed && n.comp.focusable())
                })
                .collect::<Vec<_>>();
            for other in inputs {
                if deliver(m, other, &input, events).0 {
                    vs.focus = Some(other);
                    return Outcome::Redraw;
                }
            }
        }
    }

    let Input::Key { code, .. } = input else {
        return Outcome::Nothing;
    };

    // tab or esc the input did not use hands the keys back to the dialog it left
    if top.is_some_and(|o| vs.overlay_blur == Some(o))
        && matches!(code, KeyCode::Tab | KeyCode::Esc)
    {
        vs.overlay_blur = None;
        return Outcome::Redraw;
    }

    // esc on a control in the transcript (a form in a reply) gives the keys back to the input
    if code == KeyCode::Esc && top.is_none() {
        if let Some(t) = target.filter(|t| m.region_of(*t) == Some(TRANSCRIPT)) {
            let back = m.subtree(BOTTOM).into_iter().find(|id| {
                *id != t && m.node(*id).is_some_and(|n| !n.sealed && n.comp.focusable())
            });
            vs.focus = back;
            return Outcome::Redraw;
        }
    }

    // a bare esc with no dialog is the application's (e.g. interrupt the running turn)
    if code == KeyCode::Esc && top.is_none() {
        events.push(Event::Action { name: "escape".into(), focus: target });
        return Outcome::Redraw;
    }
    builtin(m, vs, code)
}

/// Gives `input` to node `id`; returns (handled, it submitted).
fn deliver(m: &mut Model, id: NodeId, input: &Input, events: &mut Vec<Event>) -> (bool, bool) {
    let mut emits = Vec::new();
    let handled = match m.node_mut(id) {
        Some(node) if !node.sealed => node.comp.on_input(input, &mut emits),
        _ => false,
    };
    if !handled {
        return (false, false);
    }
    let submitted = emits.iter().any(|e| matches!(e, Emit::Submit(_)));
    events.extend(emits.into_iter().map(|e| to_event(id, e)));
    m.bump(id);
    (true, submitted)
}

/// Typing: a paste or a character without ctrl.
fn is_text(input: &Input) -> bool {
    match input {
        Input::Paste(_) => true,
        Input::Key { code: KeyCode::Char(_), mods, .. } => !mods.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}

/// One mouse event; returns whether the frame changed. With a dialog open it goes to the dialog
/// (a click there also gives it the keys back); otherwise, in full screen, the wheel scrolls the
/// transcript and a click on a folded cell's header opens or shuts it.
pub fn handle_mouse(
    m: &mut Model,
    vs: &mut ViewState,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    events: &mut Vec<Event>,
) -> bool {
    const WHEEL: usize = 3;
    vs.track_overlay(m.top_overlay());
    if let Some(top) = m.top_overlay() {
        return overlay_mouse(m, vs, top, kind, col, row, events);
    }
    if vs.mode != Mode::Fullscreen {
        return false;
    }
    let header_at = |vs: &ViewState| {
        vs.hits
            .iter()
            .find(|(y, w, _)| *y == row && col < *w)
            .map(|&(_, _, id)| id)
    };
    match kind {
        // the pointer tints the header it is over; redraw only when that changes
        MouseEventKind::Moved => {
            let hover = header_at(vs);
            if hover == vs.hover {
                return false;
            }
            vs.hover = hover;
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = kind == MouseEventKind::ScrollUp;
            // over a scroll box the wheel scrolls the box, elsewhere the transcript
            let boxed = vs
                .scroll_hits
                .iter()
                .find(|(y, h, _)| row >= *y && row < y + h)
                .map(|&(y, _, id)| (y, id));
            if let Some((y, id)) = boxed {
                let act = if up {
                    MouseAct::ScrollUp
                } else {
                    MouseAct::ScrollDown
                };
                let mouse = Mouse { act, row: row - y, col, width: 0 };
                let mut emits = Vec::new();
                let moved = m
                    .node_mut(id)
                    .is_some_and(|n| n.comp.on_mouse(&mouse, &mut emits));
                if moved {
                    m.bump(id);
                    return true;
                }
            }
            if up {
                vs.scroll += WHEEL;
            } else {
                vs.scroll = vs.scroll.saturating_sub(WHEEL);
            }
        }
        // a press starts a possible selection (and toggles a header); a drag selects; the
        // release copies what was selected
        MouseEventKind::Down(MouseButton::Left) => {
            // only a visible selection needs a redraw when the press clears it
            let had = vs.selection.take().is_some_and(|s| s.anchor != s.head);
            vs.selection = Some(Selection {
                anchor: (col, row),
                head: (col, row),
                done: false,
                copied: false,
            });
            match header_at(vs) {
                Some(id) => m.toggle_open(id),
                None => return had,
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let Some(sel) = vs.selection.as_mut().filter(|s| !s.done) else {
                return false;
            };
            if sel.head == (col, row) {
                return false;
            }
            sel.head = (col, row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let Some(sel) = vs.selection.as_mut() else {
                return false;
            };
            if sel.anchor == sel.head {
                // a plain click: nothing selected; on a link it reports the URL
                vs.selection = None;
                let hit = vs
                    .link_hits
                    .iter()
                    .find(|(y, x0, x1, _)| *y == row && (*x0..*x1).contains(&col));
                if let Some(url) = hit.and_then(|h| crate::text::link::url(h.3)) {
                    events.push(Event::Link { url });
                }
                return false;
            }
            sel.head = (col, row);
            sel.done = true;
        }
        _ => return false,
    }
    true
}

/// Transcript navigation in full screen: ctrl+↑ enters it on the last header; ↑↓ (j/k) move
/// between foldable headers, enter or space toggles one, esc or any other key leaves it (the
/// key then goes on as usual). None when the key is not navigation's.
fn navigate(
    m: &mut Model,
    vs: &mut ViewState,
    code: KeyCode,
    mods: KeyModifiers,
) -> Option<Outcome> {
    if vs.mode != Mode::Fullscreen || m.top_overlay().is_some() {
        vs.nav = None;
        return None;
    }
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let at = vs.nav.and_then(|n| vs.heads.iter().position(|h| *h == n));
    match (vs.nav, code) {
        (None, KeyCode::Up) if ctrl => vs.nav = vs.heads.last().copied(),
        (None, _) => return None,
        (Some(_), KeyCode::Up | KeyCode::Char('k')) => {
            let i = at.map_or(vs.heads.len().saturating_sub(1), |i| i.saturating_sub(1));
            vs.nav = vs.heads.get(i).copied();
        }
        (Some(_), KeyCode::Down | KeyCode::Char('j')) => {
            let i = at.map_or(0, |i| (i + 1).min(vs.heads.len().saturating_sub(1)));
            vs.nav = vs.heads.get(i).copied();
        }
        (Some(id), KeyCode::Enter | KeyCode::Char(' ')) => m.toggle_open(id),
        (Some(_), KeyCode::Esc) => vs.nav = None,
        (Some(_), _) => {
            vs.nav = None;
            return None;
        }
    }
    Some(Outcome::Redraw)
}

fn overlay_mouse(
    m: &mut Model,
    vs: &mut ViewState,
    top: NodeId,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    events: &mut Vec<Event>,
) -> bool {
    let Some((_, y, x, w, h)) = vs.overlay_hit.filter(|hit| hit.0 == top) else {
        return false;
    };
    let act = match kind {
        MouseEventKind::Down(MouseButton::Left) => MouseAct::Click,
        MouseEventKind::ScrollUp => MouseAct::ScrollUp,
        MouseEventKind::ScrollDown => MouseAct::ScrollDown,
        _ => return false,
    };
    // only the dialog's own content counts: a click on the input below keeps its keys there
    let r = i32::from(row) - y;
    if r < 0 || r >= i32::from(h) || col < x || col >= x.saturating_add(w) {
        return false;
    }

    vs.overlay_blur = None;
    let mouse = Mouse { act, row: r as u16, col: col - x, width: w };
    let mut emits = Vec::new();
    if let Some(node) = m.node_mut(top).filter(|n| !n.sealed) {
        if node.comp.on_mouse(&mouse, &mut emits) {
            m.bump(top);
        }
    }
    events.extend(emits.into_iter().map(|e| to_event(top, e)));
    true
}

fn builtin(m: &Model, vs: &mut ViewState, code: KeyCode) -> Outcome {
    const PAGE: usize = 10;
    match (code, vs.mode) {
        (KeyCode::PageUp, Mode::Fullscreen) => vs.scroll += PAGE,
        (KeyCode::PageDown, Mode::Fullscreen) => vs.scroll = vs.scroll.saturating_sub(PAGE),
        // ↑/↓ that the focused input did not use step between inputs in layout order
        (KeyCode::Up | KeyCode::Down, _) if m.top_overlay() == vs.overlay_blur => {
            let inputs = focusable(m);
            let cur = input_target(m, vs).and_then(|t| inputs.iter().position(|i| *i == t));
            let next = match (cur, code) {
                (Some(i), KeyCode::Down) if i + 1 < inputs.len() => i + 1,
                (Some(i), KeyCode::Up) if i > 0 => i - 1,
                _ => return Outcome::Nothing,
            };
            vs.focus = Some(inputs[next]);
        }
        (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
            let inputs = focusable(m);
            if inputs.is_empty() {
                return Outcome::Nothing;
            }
            let cur = vs.focus.and_then(|f| inputs.iter().position(|i| *i == f));
            let n = inputs.len();
            let next = match (cur, code) {
                (Some(i), KeyCode::BackTab) => (i + n - 1) % n,
                (Some(i), _) => (i + 1) % n,
                (None, _) => 0,
            };
            vs.focus = Some(inputs[next]);
        }
        _ => return Outcome::Nothing,
    }
    Outcome::Redraw
}

/// Every open focusable node in layout order: controls in open transcript cells (forms in a
/// reply that is still live), then the live, panel and bottom regions.
fn focusable(m: &Model) -> Vec<NodeId> {
    [TRANSCRIPT, LIVE, PANELS, BOTTOM]
        .into_iter()
        .flat_map(|r| m.subtree(r))
        .filter(|id| m.node(*id).is_some_and(|n| !n.sealed && n.comp.focusable()))
        .filter(|id| !m.ancestors_sealed(*id) && !m.hidden(*id))
        .collect()
}

fn to_event(id: NodeId, e: Emit) -> Event {
    match e {
        Emit::Submit(value) => Event::Submit { id, value },
        Emit::Change(value) => Event::Change { id, value },
        Emit::Select { index, value } => Event::Select { id, index, value },
        Emit::Close(reason) => Event::Close { id, reason },
    }
}
