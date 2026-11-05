//! Key routing (design §7.1): ctrl+c and bound chords first, then the top overlay (esc closes
//! it), then the focused input, then the built-in keys (mode toggle, scrolling, focus cycling).
//! A character or paste the dialog does not take goes to the page's input instead, which keeps
//! the keys until a submit, tab or esc there hands them back to the dialog.
//! Mouse events exist only in full screen: the wheel scrolls, a click opens a folded cell.

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use super::layout::{input_target, page_input};
use super::ViewState;
use crate::model::{Model, BOTTOM, LIVE, PANELS};
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
        if let Some(top) = top.filter(|o| *o == t && is_text(&input)) {
            if let Some(page) = page_input(m, vs) {
                vs.overlay_blur = Some(top);
                deliver(m, page, &input, events);
                return Outcome::Redraw;
            }
        }

        // ↓ that the dialog does not use leaves it for the input below
        let down = matches!(input, Input::Key { code: KeyCode::Down, .. });
        if top == Some(t) && down && page_input(m, vs).is_some() {
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

    let Input::Key { code, mods, .. } = input else {
        return Outcome::Nothing;
    };

    // tab or esc the input did not use hands the keys back to the dialog it left
    if let Some(top) = top.filter(|o| vs.overlay_blur == Some(*o)) {
        if matches!(code, KeyCode::Tab | KeyCode::Esc) {
            let _ = top;
            vs.overlay_blur = None;
            return Outcome::Redraw;
        }
    }
    builtin(m, vs, code, mods)
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
    match kind {
        MouseEventKind::ScrollUp => vs.scroll += WHEEL,
        MouseEventKind::ScrollDown => vs.scroll = vs.scroll.saturating_sub(WHEEL),
        MouseEventKind::Down(MouseButton::Left) => {
            let hit = vs.hits.iter().find(|(y, w, _)| *y == row && col < *w);
            let Some(&(_, _, id)) = hit else {
                return false;
            };
            if !vs.toggled.remove(&id) {
                vs.toggled.insert(id);
            }
        }
        _ => return false,
    }
    true
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

fn builtin(m: &Model, vs: &mut ViewState, code: KeyCode, mods: KeyModifiers) -> Outcome {
    const PAGE: usize = 10;
    match (code, vs.mode) {
        (KeyCode::PageUp, Mode::Fullscreen) => vs.scroll += PAGE,
        (KeyCode::PageDown, Mode::Fullscreen) => vs.scroll = vs.scroll.saturating_sub(PAGE),
        (KeyCode::End, Mode::Fullscreen) if mods.contains(KeyModifiers::CONTROL) => vs.scroll = 0,
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

/// Every open focusable node outside the transcript, in layout order.
fn focusable(m: &Model) -> Vec<NodeId> {
    [LIVE, PANELS, BOTTOM]
        .into_iter()
        .flat_map(|r| m.subtree(r))
        .filter(|id| m.node(*id).is_some_and(|n| !n.sealed && n.comp.focusable()))
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
