//! Component trait, registry and every component (design §4.4, §12).
//!
//! A component renders itself as lines at a width and handles the keys sent to it while focused.
//! Containers (`stack`, `cell`) render only their own header; the view lays out their children.

mod cell;
mod composer;
mod data;
mod dialogs;
mod editor;
mod layout;
mod markdown;
mod picker;
mod props;
mod registry;
mod status;
#[cfg(test)]
mod tests;
mod text;

use std::any::Any;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::Line;
use thiserror::Error;

use crate::proto::Value;

pub use cell::{Cell, Role};
pub use composer::Composer;
pub use data::{Footer, Kv, Table};
pub use dialogs::{Card, Confirm, Prompt};
pub use editor::TextEditor;
pub use layout::{Divider, Stack};
pub use markdown::Markdown;
pub use picker::Picker;
pub use registry::{build, KINDS};
pub use status::{Progress, Sparkline, Status};
pub use text::{Code, Log, Text};

/// What a component sees while rendering.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub now: Instant,
    /// When the engine started; animation frames count from here.
    pub epoch: Instant,
    pub focused: bool,
}

impl Ctx {
    pub fn new(now: Instant, epoch: Instant) -> Self {
        Ctx { now, epoch, focused: false }
    }

    pub fn focused(self, focused: bool) -> Self {
        Ctx { focused, ..self }
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
    /// The caret as (row, col) within `lines(width)`, for the terminal cursor (IME).
    fn cursor(&self, _width: u16) -> Option<(u16, u16)> {
        None
    }
    /// Whether the component changes with time (spinners, elapsed time).
    fn animating(&self) -> bool {
        false
    }
    fn as_any(&self) -> &dyn Any;
}
