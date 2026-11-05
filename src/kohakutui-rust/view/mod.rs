//! View stage: turns the model into lines and frames, and turns keys into semantic events
//! (design §6.3, §6.5, §7). View state (mode, focus, scroll, toasts) never reaches Python.

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
pub use input::{handle_input, Outcome};
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
    /// Rendered sealed cells for the full-screen transcript: id → (width, version, lines).
    pub(crate) cache: HashMap<NodeId, (u16, u64, Vec<Line<'static>>)>,
}

impl ViewState {
    pub fn new(mode: Mode) -> Self {
        ViewState {
            mode,
            focus: None,
            scroll: 0,
            bindings: Vec::new(),
            toasts: Vec::new(),
            cache: HashMap::new(),
        }
    }

    /// Drops expired toasts; returns whether any were dropped.
    pub fn expire_toasts(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.until > now);
        self.toasts.len() != before
    }
}
