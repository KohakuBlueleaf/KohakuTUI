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

use std::collections::HashMap;
use std::time::Instant;

use ratatui::text::Line;

use crate::proto::{Level, Mode, NodeId};

pub use chord::Chord;
pub use flush::{flush_all, take_flushable};
pub use input::{handle_input, handle_mouse, Outcome};
pub use layout::{fullscreen, inline, input_target, link_hits, Screen};

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
    /// The open dialog the user typed away from: keys go to the input until a submit, tab or
    /// esc there hands them back.
    pub overlay_blur: Option<NodeId>,
    /// Where the top overlay's content was last drawn: (overlay, row, col, width, height).
    pub(crate) overlay_hit: Option<(NodeId, i32, u16, u16, u16)>,
    /// The top overlay when last seen; a change clears `overlay_blur`.
    seen_top: Option<NodeId>,
    /// Foldable headers on the last full-screen frame: (row, width, node).
    pub(crate) hits: Vec<(u16, u16, NodeId)>,
    /// Scroll boxes on the last full-screen frame: (first row, height, node).
    pub(crate) scroll_hits: Vec<(u16, u16, NodeId)>,
    /// Full screen: where linked cells were drawn, as (row, first col, end col, link id).
    pub(crate) link_hits: Vec<(u16, u16, u16, u32)>,
    /// Every foldable header of the transcript in order, for keyboard navigation.
    pub(crate) heads: Vec<NodeId>,
    /// The header under the mouse pointer (full screen).
    pub(crate) hover: Option<NodeId>,
    /// The header focused in transcript navigation (ctrl+↑ in full screen); keys go to it.
    pub(crate) nav: Option<NodeId>,
    /// Full-screen text selection: drag to select, copied on release.
    pub(crate) selection: Option<Selection>,
    /// Selected text waiting for the engine to put it on the clipboard.
    pub(crate) copied: Option<String>,
    /// Rendered sealed cells for the full-screen transcript.
    pub(crate) cache: HashMap<NodeId, CachedCell>,
}

/// A drag selection on the full-screen frame, in screen cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
    /// The button was released: the text is copied once, the highlight stays until next press.
    pub done: bool,
    pub copied: bool,
}

impl Selection {
    /// Start and end (col, row), in reading order.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let key = |p: (u16, u16)| (p.1, p.0);
        if key(self.anchor) <= key(self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn contains(&self, col: u16, row: u16) -> bool {
        let ((c0, r0), (c1, r1)) = self.ordered();
        if row < r0 || row > r1 {
            return false;
        }
        (row > r0 || col >= c0) && (row < r1 || col <= c1)
    }
}

/// One sealed cell rendered for full screen: width, version and expand-all it was drawn at,
/// its lines, and the headers and scroll boxes inside them.
pub(crate) type CachedCell = (
    u16,
    u64,
    bool,
    Vec<Line<'static>>,
    Vec<(usize, NodeId)>,
    Vec<(usize, usize, NodeId)>,
);

impl ViewState {
    pub fn new(mode: Mode) -> Self {
        ViewState {
            mode,
            focus: None,
            scroll: 0,
            bindings: Vec::new(),
            toasts: Vec::new(),
            expand_all: false,
            overlay_blur: None,
            overlay_hit: None,
            seen_top: None,
            hits: Vec::new(),
            scroll_hits: Vec::new(),
            link_hits: Vec::new(),
            heads: Vec::new(),
            hover: None,
            nav: None,
            selection: None,
            copied: None,
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
