//! Runtime: the engine thread that owns the model and the terminal, the input thread, and the
//! `Session` handle Python talks to (design §6.2, §8).
//!
//! Ops arrive on a channel and are applied in order; frames are coalesced to `max_fps`; events go
//! out on one queue. No engine thread ever touches Python.

mod engine;
mod input;
#[cfg(feature = "python")]
pub(crate) mod python;
mod session;
#[cfg(test)]
mod tests;

use std::time::Duration;

use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use thiserror::Error;

use crate::proto::{Event, Mode, Value};
use crate::term::{Backend, TermError};
use crate::widgets::Input;

pub use session::Session;

/// What ctrl+c does (design §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// Only an `Interrupt` event; Python decides.
    Event,
    /// A real SIGINT to the process group (Unix), plus the event.
    Signal,
    /// Restore the terminal and exit with status 130.
    Exit,
}

/// Everything a session needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    pub backend: Backend,
    pub mode: Mode,
    pub interrupt: Interrupt,
    pub max_fps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            backend: Backend::Auto,
            mode: Mode::Inline,
            interrupt: Interrupt::Event,
            max_fps: 60,
        }
    }
}

/// Input for the engine thread, from the input thread or injected by tests.
#[derive(Debug, Clone)]
pub enum InputEv {
    /// A real key or paste, stamped when it was read.
    Input(Input),
    /// A synthetic key `gap` after the previous synthetic one (typing vs. paste-burst pace).
    Typed {
        code: KeyCode,
        mods: KeyModifiers,
        gap: Duration,
    },
    Resize(u16, u16),
    /// A click or wheel step (full screen only).
    Mouse {
        kind: MouseEventKind,
        col: u16,
        row: u16,
    },
}

/// Messages to the engine thread.
pub(crate) enum Inbound {
    /// Raw op values; each is decoded on its own.
    Ops(Vec<Value>),
    /// Draw now, then signal.
    Sync(Sender<()>),
    Inject(Vec<InputEv>),
    Stop,
}

/// Messages from the engine thread.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outbound {
    Event(Event),
    /// Unblocks a waiting poll without an event.
    Wake,
}

/// A session that cannot start or is gone.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Term(#[from] TermError),
    #[error("the UI is not running")]
    NotRunning,
    #[error("cannot start the engine thread: {0}")]
    Spawn(#[from] std::io::Error),
}
