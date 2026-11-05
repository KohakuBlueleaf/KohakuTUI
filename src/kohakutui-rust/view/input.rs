//! Key routing (design §7.1): ctrl+c and bound chords first, then the top overlay (esc closes
//! it), then the focused input, then the built-in keys (mode toggle, scrolling, focus cycling).

use crossterm::event::{KeyCode, KeyModifiers};

use super::layout::input_target;
use super::ViewState;
use crate::model::{Model, BOTTOM, LIVE, PANELS};
use crate::proto::{Event, Mode, NodeId};
use crate::widgets::{Emit, Input};

/// What the engine must do after a key, beyond the events already queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    ToggleMode,
    Interrupt,
}

/// Routes one input; semantic events go to `events`.
pub fn handle_input(
    m: &mut Model,
    vs: &mut ViewState,
    input: Input,
    events: &mut Vec<Event>,
) -> Outcome {
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
        if code == KeyCode::Esc {
            if let Some(top) = m.top_overlay() {
                events.push(Event::Close { id: top, reason: "escape".into() });
                return Outcome::Redraw;
            }
        }
    }

    if let Some(t) = target {
        let mut emits = Vec::new();
        let handled = match m.node_mut(t) {
            Some(node) if !node.sealed => node.comp.on_input(&input, &mut emits),
            _ => false,
        };
        events.extend(emits.into_iter().map(|e| to_event(t, e)));
        if handled {
            m.bump(t);
            return Outcome::Redraw;
        }
    }

    let Input::Key { code, mods, .. } = input else {
        return Outcome::Nothing;
    };
    builtin(m, vs, code, mods)
}

fn builtin(m: &Model, vs: &mut ViewState, code: KeyCode, mods: KeyModifiers) -> Outcome {
    const PAGE: usize = 10;
    match (code, vs.mode) {
        (KeyCode::PageUp, Mode::Fullscreen) => vs.scroll += PAGE,
        (KeyCode::PageDown, Mode::Fullscreen) => vs.scroll = vs.scroll.saturating_sub(PAGE),
        (KeyCode::End, Mode::Fullscreen) if mods.contains(KeyModifiers::CONTROL) => vs.scroll = 0,
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
