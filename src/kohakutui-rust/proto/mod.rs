//! Wire protocol: Ops (Python to engine), Events (engine to Python) and the dynamic value type
//! they carry (design §4). Decoding is per op, so one bad op never rejects its batch.

mod decode;
mod encode;
#[cfg(feature = "python")]
pub(crate) mod python;
#[cfg(test)]
mod tests;
pub mod value;

pub use rmpv::Value;
use thiserror::Error;

/// Protocol version as (major, minor). Peers must agree on major; minor only adds.
pub const PROTOCOL: (u16, u16) = (1, 0);

/// Node identity, allocated by the producer (design §4.1).
pub type NodeId = u64;

/// How the engine presents the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Finished output goes to the terminal's scrollback; a small live viewport at the bottom.
    Inline,
    /// Alternate screen with a scrollable transcript and panels.
    Fullscreen,
}

/// Severity of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warn,
    Error,
}

/// One instruction from Python to the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Create {
        id: NodeId,
        kind: String,
        props: Value,
        parent: NodeId,
        index: Option<usize>,
    },
    Update {
        id: NodeId,
        props: Value,
    },
    Append {
        id: NodeId,
        data: Value,
    },
    Remove {
        id: NodeId,
    },
    Move {
        id: NodeId,
        parent: NodeId,
        index: Option<usize>,
    },
    Seal {
        id: NodeId,
    },
    Focus {
        id: NodeId,
    },
    SetMode {
        mode: Mode,
    },
    Bind {
        chord: String,
        action: String,
    },
    Unbind {
        chord: String,
    },
    Notify {
        text: String,
        level: Level,
        ttl_ms: u32,
    },
    Bell,
    /// Puts text on the system clipboard (OSC 52) with a "copied" notice.
    Copy {
        text: String,
    },
    /// Shows document `id` (the transcript, or a `document` node under the docs region) where
    /// the transcript is: drawn, flushed to scrollback inline, taking input.
    ShowDoc {
        id: NodeId,
        /// Inline: a line written to scrollback before the document's own (a "── name ──"
        /// divider), so it belongs to the switch and to no document.
        note: Option<String>,
        /// Full screen: the first time it is shown, start at its top (a page) rather than
        /// following its end (a conversation).
        top: bool,
    },
    /// Links as OSC 8 hyperlinks (true, the default on a terminal) or written as "text (url)".
    /// Send it before drawing: text already laid out keeps its form.
    SetLinks {
        osc8: bool,
    },
    /// Switches the theme: a built-in scheme, entries overriding it (role or palette name →
    /// `#rrggbb` or another name), and forced truecolor on/off (detected when unset).
    SetTheme {
        scheme: String,
        overrides: Vec<(String, String)>,
        truecolor: Option<bool>,
    },
}

/// One semantic event from the engine to Python. Raw keys never cross unless bound.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Ready {
        cols: u16,
        rows: u16,
        tty: bool,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Submit {
        id: NodeId,
        value: Value,
    },
    Change {
        id: NodeId,
        value: Value,
    },
    Select {
        id: NodeId,
        index: u32,
        value: Value,
    },
    Close {
        id: NodeId,
        reason: String,
    },
    Action {
        name: String,
        focus: Option<NodeId>,
    },
    Interrupt {
        count: u8,
    },
    ModeChanged {
        mode: Mode,
    },
    /// Text the user selected in full screen, copied to the clipboard.
    Copy {
        text: String,
    },
    /// A link clicked in full screen (inline, the terminal handles links itself).
    Link {
        url: String,
    },
    /// The user resized or collapsed a pane (its new width and state, for the app to keep).
    Pane {
        id: NodeId,
        width: u64,
        collapsed: bool,
    },
    Error {
        id: Option<NodeId>,
        message: String,
    },
    Stopped,
}

/// A value that is not a well-formed op.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtoError {
    #[error("op is not a map")]
    NotMap,
    #[error("missing field `{0}`")]
    Missing(&'static str),
    #[error("field `{0}` has the wrong type")]
    Type(&'static str),
    #[error("unknown op `{0}`")]
    UnknownOp(String),
    #[error("invalid {field}: `{value}`")]
    Invalid { field: &'static str, value: String },
}
