//! Terminal restore of last resort: a panic hook, and `reset_terminal` for a terminal left raw.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use super::out::TtyOut;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static HOOK: Once = Once::new();

/// End synchronized output, reset the scroll region, show the cursor, stop bracketed paste and
/// mouse reports, and reset colors. Resetting the scroll region homes the cursor: position it
/// afterwards.
pub(crate) fn restore_bytes() -> &'static [u8] {
    b"\x1b[?2026l\x1b[r\x1b[?25h\x1b[?2004l\x1b[?1004l\x1b[?1000l\x1b[?1006l\x1b[0m"
}

/// Leaves the alternate screen; only sent when it is (or may be) active, because it also
/// restores a saved cursor position.
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";

/// Records whether a raw-mode terminal is open, so the panic hook knows to restore it.
pub(crate) fn mark_active(active: bool) {
    ACTIVE.store(active, Ordering::SeqCst);
}

/// Records whether the alternate screen is in use.
pub(crate) fn mark_fullscreen(on: bool) {
    FULLSCREEN.store(on, Ordering::SeqCst);
}

fn restore(leave_alt: bool) {
    let _ = crossterm::terminal::disable_raw_mode();
    if let Ok(mut tty) = TtyOut::open() {
        if leave_alt {
            let _ = tty.write_all(LEAVE_ALT);
        }
        let _ = tty.write_all(restore_bytes());
        let _ = tty.write_all(b"\r\n");
        let _ = tty.flush();
    }
    ACTIVE.store(false, Ordering::SeqCst);
    FULLSCREEN.store(false, Ordering::SeqCst);
}

/// Restores a terminal left raw or on the alternate screen; for `python -m kohakutui reset`.
pub fn reset_terminal() {
    restore(true);
}

/// Restores an open terminal before any panic message is printed.
pub fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if ACTIVE.load(Ordering::SeqCst) {
                restore(FULLSCREEN.load(Ordering::SeqCst));
            }
            previous(info);
        }));
    });
}
