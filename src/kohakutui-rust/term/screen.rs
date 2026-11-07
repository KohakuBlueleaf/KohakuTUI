//! Opening the terminal, sizing the inline viewport, drawing frames and switching modes.

use std::io::{self, IsTerminal, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::terminal::{self, Clear, ClearType};
use ratatui::backend::{Backend as _, CrosstermBackend};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::guard::{mark_active, mark_fullscreen, restore_bytes};
use super::out::TtyOut;
use super::{Backend, History, Out, Term, TermError};
use crate::text::link;

/// Prints changed cells; each run of cells sharing a link sits between OSC 8 open and close,
/// and the mark (`underline_color`, see `text::link`) is stripped before the cell is printed.
pub(crate) fn draw_cells(out: &mut impl Write, cells: Vec<(u16, u16, &Cell)>) -> io::Result<()> {
    let mut open: Option<u32> = None;
    let mut run: Vec<(u16, u16, Cell)> = Vec::new();
    let flush = |out: &mut dyn Write, run: &mut Vec<(u16, u16, Cell)>| -> io::Result<()> {
        if !run.is_empty() {
            let mut backend = CrosstermBackend::new(out);
            backend.draw(run.iter().map(|(x, y, c)| (*x, *y, c)))?;
            run.clear();
        }
        Ok(())
    };
    for (x, y, cell) in cells {
        let id = link::id_of(Some(cell.underline_color));
        if id != open {
            flush(out, &mut run)?;
            if open.is_some() {
                out.write_all(link::CLOSE.as_bytes())?;
            }
            if let Some(id) = id {
                out.write_all(link::open(id).as_bytes())?;
            }
            open = id;
        }
        let mut plain = cell.clone();
        plain.underline_color = Color::Reset;
        run.push((x, y, plain));
    }
    flush(out, &mut run)?;
    if open.is_some() {
        out.write_all(link::CLOSE.as_bytes())?;
    }
    Ok(())
}

/// SGR-encoded button reports: clicks and the wheel, no motion.
#[cfg(not(windows))]
const MOUSE_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h";
#[cfg(not(windows))]
const MOUSE_OFF: &[u8] = b"\x1b[?1000l\x1b[?1006l";

impl Term {
    /// Opens the terminal for `backend`, takes raw mode and anchors the viewport at the cursor.
    pub fn open(backend: &Backend) -> Result<Term, TermError> {
        match backend {
            Backend::Test(screen) => {
                let (cols, rows) = screen.size();
                let (row, col) = screen.cursor();
                let mut term = Term::with(Out::Test(screen.clone()), cols, rows, History::detect());
                term.anchor(row, col)?;
                Ok(term)
            }
            Backend::Plain => Ok(Term::plain()),
            Backend::Auto => Term::tty().or_else(|_| Ok(Term::plain())),
            Backend::Tty => Term::tty(),
        }
    }

    fn with(out: Out, cols: u16, rows: u16, history: History) -> Term {
        Term {
            out,
            cols,
            rows,
            vt: 0,
            vh: 0,
            prev: None,
            fullscreen: false,
            saved_vt: 0,
            history,
            raw: false,
            mouse: false,
            #[cfg(windows)]
            saved_in_mode: None,
        }
    }

    fn plain() -> Term {
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or(100);
        Term::with(Out::Plain(io::stdout()), cols, 24, History::Reprint)
    }

    fn tty() -> Result<Term, TermError> {
        let out = TtyOut::open().map_err(|e| TermError::NoTty(e.to_string()))?;
        let (cols, rows) = terminal::size().map_err(|e| TermError::NoTty(e.to_string()))?;
        terminal::enable_raw_mode().map_err(|e| TermError::NoTty(e.to_string()))?;

        let mut term = Term::with(Out::Tty(out), cols, rows, History::detect());
        term.raw = true;
        mark_active(true);
        if cfg!(unix) {
            // bracketed paste and focus reports
            write!(term.out, "\x1b[?2004h\x1b[?1004h")?;
        }

        // crossterm asks the cursor row through stdout on Unix: only safe when stdout is the tty
        let pos = if cfg!(windows) || io::stdout().is_terminal() {
            crossterm::cursor::position().ok()
        } else {
            None
        };
        let (col, row) = pos.unwrap_or((0, rows.saturating_sub(1)));
        term.anchor(row, col)?;
        Ok(term)
    }

    /// Starts the viewport at the cursor row, on a fresh line if the cursor is mid-line.
    fn anchor(&mut self, row: u16, col: u16) -> io::Result<()> {
        self.vt = row.min(self.rows.saturating_sub(1));
        if col > 0 {
            write!(self.out, "\r\n")?;
            self.vt = (self.vt + 1).min(self.rows.saturating_sub(1));
        }
        Ok(())
    }

    pub fn is_plain(&self) -> bool {
        matches!(self.out, Out::Plain(_))
    }

    pub fn is_tty(&self) -> bool {
        matches!(self.out, Out::Tty(_))
    }

    /// (cols, rows).
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// The screen rows the frame occupies.
    pub fn area(&self) -> Rect {
        Rect::new(0, self.vt, self.cols, self.vh)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        if self.is_plain() {
            self.cols = cols;
            return Ok(());
        }
        self.cols = cols.max(8);
        self.rows = rows.max(3);
        self.vt = self.vt.min(self.rows.saturating_sub(1));
        if self.fullscreen {
            self.vt = 0;
            self.vh = self.rows;
            queue!(self.out, MoveTo(0, 0), Clear(ClearType::All))?;
        } else {
            queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown))?;
            self.vh = 0;
        }
        self.prev = None;
        Ok(())
    }

    /// Starts a frame: the terminal shows nothing until `end_frame` (synchronized output).
    pub fn begin_frame(&mut self) -> io::Result<()> {
        if !self.is_plain() {
            write!(self.out, "\x1b[?2026h")?;
        }
        Ok(())
    }

    pub fn end_frame(&mut self) -> io::Result<()> {
        if !self.is_plain() {
            write!(self.out, "\x1b[?2026l")?;
        }
        self.out.flush()
    }

    /// Sizes the inline viewport to `h` rows, scrolling the screen up when it would not fit.
    pub fn set_viewport(&mut self, h: u16) -> io::Result<()> {
        if self.is_plain() {
            return Ok(());
        }
        if self.fullscreen {
            if self.vh != self.rows || self.vt != 0 {
                self.vt = 0;
                self.vh = self.rows;
                self.prev = None;
            }
            return Ok(());
        }

        let h = h.min(self.rows);
        let mut moved = false;
        if self.vt + h > self.rows {
            let over = self.vt + h - self.rows;
            queue!(self.out, MoveTo(0, self.rows - 1))?;
            for _ in 0..over {
                self.out.write_all(b"\n")?;
            }
            self.vt -= over;
            moved = true;
        }
        if moved || h != self.vh || self.prev.is_none() {
            queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown))?;
            self.prev = None;
        }
        self.vh = h;
        Ok(())
    }

    /// Draws `buf` (whose area is `self.area()`) by diffing against the last frame.
    pub fn draw(&mut self, buf: &Buffer, cursor: Option<(u16, u16)>) -> io::Result<()> {
        if self.is_plain() {
            return Ok(());
        }
        let area = self.area();
        let prev = match self.prev.take() {
            Some(p) if p.area == area => p,
            _ => Buffer::empty(area),
        };

        // diffed with the link marks in place, so a cell whose link changed is redrawn
        draw_cells(&mut self.out, prev.diff(buf))?;
        match cursor {
            Some((x, y)) => queue!(self.out, MoveTo(x, y), Show)?,
            None => queue!(self.out, Hide)?,
        }
        self.prev = Some(buf.clone());
        Ok(())
    }

    pub fn enter_fullscreen(&mut self) -> io::Result<()> {
        if self.fullscreen || self.is_plain() {
            return Ok(());
        }
        self.saved_vt = self.vt;
        write!(self.out, "\x1b[?1049h")?;
        queue!(self.out, MoveTo(0, 0), Clear(ClearType::All))?;
        self.fullscreen = true;
        mark_fullscreen(true);
        self.vt = 0;
        self.vh = self.rows;
        self.prev = None;
        Ok(())
    }

    /// Back to the main screen; the old viewport position is not trusted (it may have reflowed),
    /// so the viewport is cleared from there down and redrawn.
    pub fn leave_fullscreen(&mut self) -> io::Result<()> {
        if !self.fullscreen {
            return Ok(());
        }
        write!(self.out, "\x1b[?1049l")?;
        self.fullscreen = false;
        mark_fullscreen(false);
        self.vt = self.saved_vt.min(self.rows.saturating_sub(1));
        self.vh = 0;
        queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown))?;
        self.prev = None;
        Ok(())
    }

    /// Turns click and wheel reports (no motion) on or off, on a real terminal only: the
    /// Turns click and wheel reports (no motion) on or off, on a real terminal only: the
    /// emulated test screen must not change the console mode of the process running the tests.
    /// On Windows only the mouse bits change; every other input flag is kept and the exact
    /// previous mode comes back when capture ends.
    pub fn set_mouse(&mut self, on: bool) -> io::Result<()> {
        if !self.raw || on == self.mouse {
            return Ok(());
        }
        #[cfg(windows)]
        {
            use crossterm_winapi::{ConsoleMode, Handle};
            const MOUSE_INPUT: u32 = 0x0010;
            const QUICK_EDIT: u32 = 0x0040;
            const EXTENDED_FLAGS: u32 = 0x0080;

            let console = ConsoleMode::from(Handle::current_in_handle()?);
            if on {
                let mode = console.mode()?;
                console.set_mode((mode | MOUSE_INPUT | EXTENDED_FLAGS) & !QUICK_EDIT)?;
                self.saved_in_mode = Some(mode);
            } else if let Some(mode) = self.saved_in_mode.take() {
                console.set_mode(mode)?;
            }
        }
        #[cfg(not(windows))]
        {
            let seq = if on { MOUSE_ON } else { MOUSE_OFF };
            self.out.write_all(seq)?;
        }
        self.mouse = on;
        Ok(())
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Asks the terminal to put `text` on the system clipboard (OSC 52); only on a real terminal.
    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        if self.is_tty() {
            write!(self.out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// The window title (OSC 0), on a real terminal only.
    pub fn title(&mut self, text: &str) -> io::Result<()> {
        if self.is_tty() {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            write!(self.out, "\x1b]0;{clean}\x07")?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// A desktop notification: OSC 9 (iTerm2, WezTerm, Windows Terminal's toast-less builds
    /// ignore it) and OSC 777 (kitty, foot, rxvt); terminals without either drop both.
    pub fn alert(&mut self, title: &str, body: &str) -> io::Result<()> {
        if self.is_tty() {
            let clean = |s: &str| -> String {
                s.chars().filter(|c| !c.is_control() && *c != ';').collect()
            };
            let (title, body) = (clean(title), clean(body));
            write!(self.out, "\x1b]9;{body}\x07\x1b]777;notify;{title};{body}\x07")?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// Hands the terminal to another program: no mouse, main screen, cooked mode, caret shown.
    /// Returns whether full screen was on, for `resume`.
    pub fn suspend(&mut self) -> io::Result<bool> {
        let was_full = self.fullscreen;
        if self.is_plain() {
            return Ok(was_full);
        }
        let mouse = self.mouse;
        self.set_mouse(false)?;
        self.mouse = mouse;
        if was_full {
            write!(self.out, "\x1b[?1049l")?;
        } else {
            // the live rows go; the program starts on a clean line where they were
            queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown))?;
        }
        if cfg!(unix) {
            write!(self.out, "\x1b[?2004l\x1b[?1004l")?;
        }
        queue!(self.out, Show)?;
        self.out.flush()?;
        if self.raw {
            terminal::disable_raw_mode()?;
        }
        Ok(was_full)
    }

    /// Takes the terminal back after `suspend` and forgets the last frame, so all is redrawn.
    pub fn resume(&mut self, was_full: bool) -> io::Result<()> {
        if self.is_plain() {
            return Ok(());
        }
        if self.raw {
            terminal::enable_raw_mode()?;
        }
        if cfg!(unix) {
            write!(self.out, "\x1b[?2004h\x1b[?1004h")?;
        }
        if was_full {
            write!(self.out, "\x1b[?1049h")?;
            queue!(self.out, MoveTo(0, 0), Clear(ClearType::All))?;
        } else if self.is_tty() {
            // the program may have written below the old viewport: start again at the cursor
            let pos = if cfg!(windows) || io::stdout().is_terminal() {
                crossterm::cursor::position().ok()
            } else {
                None
            };
            if let Some((col, row)) = pos {
                self.anchor(row, col)?;
            }
        }
        if self.mouse {
            self.mouse = false;
            self.set_mouse(true)?;
        }
        self.prev = None;
        self.out.flush()
    }

    pub fn bell(&mut self) -> io::Result<()> {
        if !self.is_plain() {
            write!(self.out, "\x07")?;
        }
        Ok(())
    }

    /// Clears the viewport, leaves the cursor where it started and restores the terminal.
    pub fn close(&mut self) -> io::Result<()> {
        if self.is_plain() {
            return self.out.flush();
        }
        self.set_mouse(false)?;
        self.leave_fullscreen()?;

        // reset first: resetting the scroll region homes the cursor
        self.out.write_all(restore_bytes())?;
        queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown), Show)?;
        self.out.flush()?;
        if self.raw {
            terminal::disable_raw_mode()?;
            self.raw = false;
            mark_active(false);
        }
        Ok(())
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        if self.raw {
            let _ = self.close();
        }
    }
}

/// Standard base64 with padding (for OSC 52).
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod base64_tests {
    #[test]
    fn encodes_like_rfc_4648() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(super::base64(raw.as_bytes()), enc);
        }
    }
}
