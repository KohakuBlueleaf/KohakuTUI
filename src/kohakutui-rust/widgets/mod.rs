//! Component trait, registry and every component (design §4.4, §12).
//!
//! A component renders itself as lines at a width and handles the keys sent to it while focused.
//! Containers (`stack`, `cell`) render only their own header; the view lays out their children.

mod bars;
mod button;
mod cell;
mod chart;
mod composer;
mod data;
mod dialogs;
mod editor;
mod field;
mod layout;
mod markdown;
mod pane;
mod picker;
mod props;
mod registry;
mod scroll;
mod status;
#[cfg(test)]
mod tests;
mod text;
mod tree;

use std::any::Any;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::Line;
use thiserror::Error;

use crate::proto::Value;

pub use bars::Bars;
pub use button::Button;
pub use cell::{fold_marker, Cell, Role};
pub use chart::Chart;
pub use composer::Composer;
pub use data::{Footer, Kv, Table};
pub use dialogs::{Card, Confirm, Prompt};
pub use editor::TextEditor;
pub use field::Field;
pub use layout::{Columns, Divider, Stack, Tabs};
pub use markdown::Markdown;
pub use pane::{Pane, TabStrip};
pub use picker::Picker;
pub use registry::{build, KINDS};
pub use scroll::Scroll;
pub use status::{Progress, Sparkline, Status};
pub use text::{Code, Log, Text};
pub use tree::Tree;

/// What a component sees while rendering.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    /// The node that has the keys, so a control deep in a transcript cell can draw as focused.
    pub focus: Option<crate::proto::NodeId>,
    pub now: Instant,
    /// When the engine started; animation frames count from here.
    pub epoch: Instant,
    pub focused: bool,
    /// Folded (collapsed) cells render in full: the full-screen transcript opened them.
    pub expand_all: bool,
}

impl Ctx {
    pub fn new(now: Instant, epoch: Instant) -> Self {
        Ctx { focus: None, now, epoch, focused: false, expand_all: false }
    }

    pub fn focused(self, focused: bool) -> Self {
        Ctx { focused, ..self }
    }

    pub fn with_focus(self, focus: Option<crate::proto::NodeId>) -> Self {
        Ctx { focus, ..self }
    }

    pub fn expanded(self, expand_all: bool) -> Self {
        Ctx { expand_all, ..self }
    }

    /// Milliseconds since the engine started.
    pub fn ms(&self) -> u128 {
        self.now.saturating_duration_since(self.epoch).as_millis()
    }
}

/// Input delivered to the focused component; `at` is when the key was read.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Key {
        code: KeyCode,
        mods: KeyModifiers,
        at: Instant,
    },
    Paste(String),
}

/// What a mouse did to a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAct {
    Click,
    /// A middle-button press (closes a tab).
    Middle,
    ScrollUp,
    ScrollDown,
}

/// A mouse event in a component's own line coordinates, at the width it was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mouse {
    pub act: MouseAct,
    pub row: u16,
    pub col: u16,
    pub width: u16,
}

/// A semantic event a component raises; the view adds the node id.
#[derive(Debug, Clone, PartialEq)]
pub enum Emit {
    Submit(Value),
    Change(Value),
    Select { index: u32, value: Value },
    Close(String),
}

/// Props or append data a component cannot take.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WidgetError {
    #[error("unknown component kind `{0}`")]
    UnknownKind(String),
    #[error("`{0}` does not take append")]
    NoAppend(&'static str),
    #[error("bad `{prop}`: {reason}")]
    BadProp { prop: &'static str, reason: String },
}

/// One kind of node.
pub trait Component: Send + Any {
    /// The kind name used in `create`.
    fn kind(&self) -> &'static str;
    /// Applies a props patch: present keys change, nil resets to the default.
    fn update(&mut self, props: &Value) -> Result<(), WidgetError>;
    /// Adds streamed data (text, numbers, rows) without resending props.
    fn append(&mut self, _data: &Value) -> Result<(), WidgetError> {
        Err(WidgetError::NoAppend(self.kind()))
    }
    /// Freezes the component: streams finish.
    fn seal(&mut self) {}
    /// The component's own lines at `width` (a container's header only).
    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>>;
    /// Whether children are laid out under this node.
    fn container(&self) -> bool {
        false
    }
    fn focusable(&self) -> bool {
        false
    }
    /// Handles input while focused; returns whether it consumed it.
    fn on_input(&mut self, _input: &Input, _out: &mut Vec<Emit>) -> bool {
        false
    }
    /// Handles a click or wheel step on the component; returns whether it used it.
    fn on_mouse(&mut self, _mouse: &Mouse, _out: &mut Vec<Emit>) -> bool {
        false
    }
    /// The caret as (row, col) within `lines(width)`, for the terminal cursor (IME).
    fn cursor(&self, _width: u16) -> Option<(u16, u16)> {
        None
    }
    /// Whether the component changes with time (spinners, elapsed time).
    fn animating(&self) -> bool {
        false
    }
    /// As an overlay: drawn where the input box is (`dock: "input"`), not over the content.
    fn docked(&self) -> bool {
        false
    }
    /// The rows `frame_children` keeps out of `total` child lines: (start, end).
    fn window(&self, total: usize) -> (usize, usize) {
        (0, total)
    }
    /// A container whose children sit side by side: (children per row, gap columns).
    fn side_by_side(&self) -> Option<(usize, usize)> {
        None
    }
    /// Whether the child at `index` is drawn (`tabs` shows one pane).
    fn shows_child(&self, _index: usize) -> bool {
        true
    }
    /// A container's chance to window or decorate its children's lines (`scroll` clips them).
    fn frame_children(&self, children: Vec<Line<'static>>, _width: u16) -> Vec<Line<'static>> {
        children
    }
    fn as_any(&self) -> &dyn Any;
}

/// Reads the `dock` prop into `flag`: `"input"` docks, anything else (or nil) does not.
pub(crate) fn upd_dock(p: &Value, flag: &mut bool) {
    if let Some(v) = crate::proto::value::get(p, "dock") {
        *flag = v.as_str() == Some("input");
    }
}
