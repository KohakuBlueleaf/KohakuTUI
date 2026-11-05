//! Terminal layer: owns the TTY and turns lines and buffers into bytes (design §6.1).
//!
//! Inline mode keeps a viewport at the bottom of the screen and writes finished lines into the
//! real scrollback above it; full-screen mode draws on the alternate screen. Output goes to the
//! terminal device itself (`/dev/tty`, `CONOUT$`), never to fd 1, so redirected or captured
//! stdout cannot touch the UI.

mod guard;
mod history;
mod out;
mod screen;
#[cfg(test)]
mod tests;

use ratatui::buffer::Buffer;
use thiserror::Error;

pub use guard::{install_panic_hook, reset_terminal};
pub use out::{Out, TestScreen};

/// Which bytes go where.
#[derive(Debug, Clone)]
pub enum Backend {
    /// The real terminal; falls back to `Plain` when there is none.
    Auto,
    /// The real terminal, or an error.
    Tty,
    /// No terminal: finished lines are printed to stdout as plain text.
    Plain,
    /// An in-memory terminal emulator, for tests.
    Test(TestScreen),
}

/// How finished lines are written above the inline viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    /// Scroll a region above the viewport (DECSTBM); the viewport is not reprinted.
    Region,
    /// Clear the viewport, print the lines, redraw the viewport below them.
    Reprint,
}

impl History {
    /// The strategy for this terminal: `KOHAKUTUI_HISTORY` wins, multiplexers that mishandle
    /// partial scroll regions get `Reprint`.
    pub fn detect() -> History {
        match std::env::var("KOHAKUTUI_HISTORY").as_deref() {
            Ok("region") => return History::Region,
            Ok("reprint") => return History::Reprint,
            _ => {}
        }
        if std::env::var_os("ZELLIJ").is_some() {
            History::Reprint
        } else {
            History::Region
        }
    }
}

/// The terminal and the geometry of what is drawn on it.
pub struct Term {
    out: Out,
    cols: u16,
    rows: u16,
    /// Inline viewport: top row and height.
    vt: u16,
    vh: u16,
    /// What the screen shows inside the viewport, for diffing; None forces a full redraw.
    prev: Option<Buffer>,
    fullscreen: bool,
    saved_vt: u16,
    history: History,
    raw: bool,
    /// Whether click and wheel reports are on.
    mouse: bool,
    /// The console input mode before mouse capture, restored exactly when it ends.
    #[cfg(windows)]
    saved_in_mode: Option<u32>,
}

/// The terminal cannot be used.
#[derive(Debug, Error)]
pub enum TermError {
    #[error("no terminal: {0}")]
    NoTty(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
