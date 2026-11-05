//! Terminal restore of last resort: a panic hook, and `reset_terminal` for a terminal left raw.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use super::out::TtyOut;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static HOOK: Once = Once::new();

/// Leave the alternate screen, reset the scroll region, show the cursor, stop bracketed paste
/// and reset colors.
pub(crate) fn restore_bytes() -> &'static [u8] {
    b"\x1b[?2026l\x1b[?1049l\x1b[r\x1b[?25h\x1b[?2004l\x1b[0m"
}

/// Records whether a raw-mode terminal is open, so the panic hook knows to restore it.
pub(crate) fn mark_active(active: bool) {
    ACTIVE.store(active, Ordering::SeqCst);
}

/// Restores the terminal (raw mode off, escape state reset); safe to call at any time.
pub fn reset_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    if let Ok(mut tty) = TtyOut::open() {
        let _ = tty.write_all(restore_bytes());
        let _ = tty.write_all(b"\r\n");
        let _ = tty.flush();
    }
    ACTIVE.store(false, Ordering::SeqCst);
}

/// Restores an open terminal before any panic message is printed.
pub fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if ACTIVE.load(Ordering::SeqCst) {
                reset_terminal();
            }
            previous(info);
        }));
    });
}
