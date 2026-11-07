//! View stage: turns the model into lines and frames, and turns keys into semantic events
//! (design §6.3, §6.5, §7). View state (mode, focus, scroll, toasts, which folded cells are
//! open) never reaches Python.

mod chord;
mod flush;
mod input;
mod layout;
mod panes;
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
pub use layout::{fullscreen, hover_link, inline, input_target, link_hits, Screen};

/// A transient notice shown above the bottom region.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: Level,
    pub until: Instant,
}

/// The top line of a scrolled-up full-screen view: `offset` lines into top-level `node`, while
/// the scroll is `scroll` and the document has `cells` top-level nodes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Anchor {
    pub node: NodeId,
    pub offset: usize,
    pub scroll: usize,
    pub cells: usize,
}

impl ViewState {
    /// Moves the per-document view state to the model's shown document, when it changed: the
    /// old one keeps its scroll for later, the new one gets its own back; navigation,
    /// selection, follow and a focus left in the old document end.
    pub(crate) fn switch_doc(&mut self, m: &crate::model::Model) {
        let now = m.doc();
        if now == self.shown {
            return;
        }
        if m.contains(self.shown) {
            self.doc_scroll.insert(self.shown, self.scroll);
        }
        self.doc_scroll.retain(|id, _| m.contains(*id));
        self.scroll = self.doc_scroll.remove(&now).unwrap_or(0);
        self.shown = now;
        self.nav = None;
        self.selection = None;
        self.anchor = None;
        self.unseen = 0;
        if self
            .focus
            .is_some_and(|f| m.doc_of(f).is_some_and(|d| d != now))
        {
            self.focus = None;
        }
    }
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
    /// Full screen: the scroll of documents not shown now, restored when shown again.
    pub(crate) doc_scroll: std::collections::HashMap<NodeId, usize>,
    /// Full screen: panes as laid out on the last frame, and each pane's scroll (rows from top).
    pub(crate) pane_hits: Vec<panes::PaneHit>,
    pub(crate) pane_scroll: std::collections::HashMap<NodeId, usize>,
    /// A divider being dragged, and the last divider press (for double-clicks).
    pub(crate) drag: Option<panes::Drag>,
    pub(crate) divider_click: Option<(NodeId, std::time::Instant)>,
    /// The head row's children as drawn: (row, rows, node, first row shown), and its left edge.
    pub(crate) head_rows: Vec<(u16, u16, NodeId, u16)>,
    pub(crate) head_area: ratatui::layout::Rect,
    /// The bottom region as drawn in full screen (clicks focus what is under them).
    pub(crate) bottom_rows: Vec<(u16, u16, NodeId, u16)>,
    pub(crate) bottom_area: ratatui::layout::Rect,
    /// The transcript's rect on the last full-screen frame, and the rows of its top-level
    /// nodes that are not cells (pages: trees, tables), which take clicks like pane content.
    pub(crate) main_area: ratatui::layout::Rect,
    pub(crate) doc_rows: Vec<(u16, u16, NodeId, u16)>,
    /// Input controls inside open transcript cells as drawn: (row, rows, node, first column).
    pub(crate) control_rows: Vec<(u16, u16, NodeId, u16)>,
    /// The document drawn on the last frame: a change (shown, or the shown one removed) resets
    /// the per-document view state.
    pub(crate) shown: NodeId,
    /// Follow: where the top of the view was on the last frame (scrolled up), and messages
    /// that arrived below the view since the user scrolled up.
    pub(crate) anchor: Option<Anchor>,
    pub(crate) unseen: usize,
    /// The screen width on the last full-screen frame (caps pane widths).
    pub(crate) screen_width: u16,
    /// Every foldable header of the transcript in order, for keyboard navigation.
    pub(crate) heads: Vec<NodeId>,
    /// The header under the mouse pointer (full screen).
    pub(crate) hover: Option<NodeId>,
    /// The link under the pointer (full screen), drawn highlighted.
    pub(crate) link_hover: Option<u32>,
    /// The header focused in transcript navigation (ctrl+↑ in full screen); keys go to it.
    pub(crate) nav: Option<NodeId>,
    /// Full-screen text selection: drag to select, copied on release.
    pub(crate) selection: Option<Selection>,
    /// Selected text waiting for the engine to put it on the clipboard.
    pub(crate) copied: Option<String>,
    /// Rendered sealed cells for the full-screen transcript.
    pub(crate) cache: HashMap<NodeId, CachedCell>,
    /// Full screen: find in the shown document (ctrl+f).
    pub(crate) find: Option<Find>,
    /// Where the find bar's caret was drawn on the last frame.
    pub(crate) find_caret: Option<(u16, u16)>,
}

/// Find in the transcript: the query, whether keys go to it, and the match being shown,
/// counted from the bottom (0 = the newest), moved to on the next frame when `jump` is set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Find {
    pub query: String,
    pub editing: bool,
    pub current: usize,
    pub jump: bool,
    /// Matches on the last frame (shown as `current + 1 / total`).
    pub total: usize,
}

/// A drag selection on the full-screen frame, in screen cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
    /// The button was released: the text is copied once, the highlight stays until next press.
    pub done: bool,
    pub copied: bool,
    /// The area the press was in (the transcript, a pane, the input, a dialog): the selection
    /// stays inside it, so borders and neighbouring columns are never part of it.
    pub bounds: ratatui::layout::Rect,
}

impl Selection {
    /// A selection starting at the press `(col, row)`, kept inside `bounds`.
    pub fn new(at: (u16, u16), bounds: ratatui::layout::Rect) -> Self {
        Selection { anchor: at, head: at, done: false, copied: false, bounds }
    }

    /// A point moved inside the bounds (a drag past the edge selects up to it).
    pub fn clamp(&self, (col, row): (u16, u16)) -> (u16, u16) {
        let b = self.bounds;
        (
            col.clamp(b.x, b.right().saturating_sub(1).max(b.x)),
            row.clamp(b.y, b.bottom().saturating_sub(1).max(b.y)),
        )
    }

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
        let b = self.bounds;
        if col < b.x || col >= b.right() || row < b.y || row >= b.bottom() {
            return false;
        }
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
            doc_scroll: std::collections::HashMap::new(),
            pane_hits: Vec::new(),
            pane_scroll: std::collections::HashMap::new(),
            drag: None,
            divider_click: None,
            head_rows: Vec::new(),
            head_area: ratatui::layout::Rect::default(),
            bottom_rows: Vec::new(),
            bottom_area: ratatui::layout::Rect::default(),
            main_area: ratatui::layout::Rect::default(),
            doc_rows: Vec::new(),
            control_rows: Vec::new(),
            shown: crate::model::TRANSCRIPT,
            anchor: None,
            unseen: 0,
            screen_width: 80,
            heads: Vec::new(),
            hover: None,
            link_hover: None,
            nav: None,
            selection: None,
            copied: None,
            cache: HashMap::new(),
            find: None,
            find_caret: None,
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
