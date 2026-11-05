//! View stage: turns the model into lines and frames, and turns keys into semantic events
//! (design §6.3, §6.5, §7). View state (mode, focus, scroll, toasts, which folded cells are
//! open) never reaches Python.

mod chord;
mod flush;
mod input;
mod layout;
mod render;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::text::Line;

use crate::proto::{Level, Mode, NodeId};

pub use chord::Chord;
pub use flush::{flush_all, take_flushable};
pub use input::{handle_input, handle_mouse, Outcome};
pub use layout::{fullscreen, inline, Screen};

/// A transient notice shown above the bottom region.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: Level,
    pub until: Instant,
}

/// Engine-local presentation state.
#[derive(Debug)]
pub struct ViewState {
    pub mode: Mode,
    pub focus: Option<NodeId>,
    /// Full-screen transcript scroll, in lines up from the bottom.
    pub scroll: usize,
    pub bindings: Vec<(Chord, String)>,
    pub toasts: Vec<Toast>,
    /// Full screen opens every folded cell (ctrl+o).
    pub expand_all: bool,
    /// Transcript cells clicked open (or shut) against `expand_all`.
    pub toggled: HashSet<NodeId>,
    /// The open dialog the user typed away from: keys go to the input until a submit, tab or
    /// esc there hands them back.
    pub overlay_blur: Option<NodeId>,
    /// Where the top overlay's content was last drawn: (overlay, row, col, width, height).
    pub(crate) overlay_hit: Option<(NodeId, i32, u16, u16, u16)>,
    /// The top overlay when last seen; a change clears `overlay_blur`.
    seen_top: Option<NodeId>,
    /// Folded-cell headers on the last full-screen frame: (row, width, cell).
    pub(crate) hits: Vec<(u16, u16, NodeId)>,
    /// Rendered sealed cells for the full-screen transcript: id → (width, version, open, lines).
    pub(crate) cache: HashMap<NodeId, (u16, u64, bool, Vec<Line<'static>>)>,
}

impl ViewState {
    pub fn new(mode: Mode) -> Self {
        ViewState {
            mode,
            focus: None,
            scroll: 0,
            bindings: Vec::new(),
            toasts: Vec::new(),
            expand_all: false,
            toggled: HashSet::new(),
            overlay_blur: None,
            overlay_hit: None,
            seen_top: None,
            hits: Vec::new(),
            cache: HashMap::new(),
        }
    }

    /// Notes the top overlay: when it changed (opened, closed, or a child closed over it), the
    /// new top has the keys.
    pub fn track_overlay(&mut self, top: Option<NodeId>) {
        if top != self.seen_top {
            self.seen_top = top;
            self.overlay_blur = None;
        }
    }

    /// Drops expired toasts; returns whether any were dropped.
    pub fn expire_toasts(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.until > now);
        self.toasts.len() != before
    }
}
