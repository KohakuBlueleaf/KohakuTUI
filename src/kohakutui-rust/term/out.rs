//! Output sinks: the terminal device, stdout for the plain backend, or an emulator for tests.

use std::io::{self, Write};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex, MutexGuard};

/// Where frames are written.
pub enum Out {
    Tty(TtyOut),
    Plain(io::Stdout),
    Test(TestScreen),
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Out::Tty(t) => t.write(buf),
            Out::Plain(s) => s.write(buf),
            Out::Test(t) => t.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Out::Tty(t) => t.flush(),
            Out::Plain(s) => s.flush(),
            Out::Test(t) => t.flush(),
        }
    }
}

/// The terminal device: `/dev/tty` on Unix.
#[cfg(unix)]
pub struct TtyOut(io::BufWriter<std::fs::File>);

#[cfg(unix)]
impl TtyOut {
    pub fn open() -> io::Result<TtyOut> {
        let file = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
        Ok(TtyOut(io::BufWriter::with_capacity(1 << 16, file)))
    }
}

#[cfg(unix)]
impl Write for TtyOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// The console screen buffer `CONOUT$` on Windows, written as UTF-16 with `WriteConsoleW` so
/// text survives whatever the console code page is.
#[cfg(windows)]
pub struct TtyOut {
    console: crossterm_winapi::Console,
    buf: Vec<u8>,
}

#[cfg(windows)]
impl TtyOut {
    pub fn open() -> io::Result<TtyOut> {
        use crossterm_winapi::{Console, ConsoleMode, Handle};
        const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

        let handle = Handle::current_out_handle()?;
        let mode = ConsoleMode::from(handle.clone());
        let current = mode.mode()?;
        mode.set_mode(current | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
        Ok(TtyOut { console: Console::from(handle), buf: Vec::with_capacity(1 << 16) })
    }
}

#[cfg(windows)]
impl Write for TtyOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }

        // keep an incomplete UTF-8 sequence for the next flush rather than garbling it
        let valid = match std::str::from_utf8(&self.buf) {
            Ok(_) => self.buf.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid > 0 {
            self.console.write_char_buffer(&self.buf[..valid])?;
        }
        self.buf.drain(..valid);
        Ok(())
    }
}

/// A cell colour: None is the terminal's default, else an index (0-255) or `#rrggbb`.
pub type CellColor = Option<String>;

fn color(c: vt100::Color) -> CellColor {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(i.to_string()),
        vt100::Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

/// The look shared by a run of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStyle {
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// Neighbouring cells of one row that look the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledRun {
    pub text: String,
    pub style: RunStyle,
}

/// A shared in-memory terminal (vt100 emulator with scrollback), the test backend's screen.
#[derive(Clone)]
pub struct TestScreen(Arc<Mutex<vt100::Parser>>);

impl std::fmt::Debug for TestScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (cols, rows) = self.size();
        write!(f, "TestScreen({cols}x{rows})")
    }
}

impl TestScreen {
    pub fn new(cols: u16, rows: u16) -> TestScreen {
        let parser = vt100::Parser::new(nz(rows.max(2)), nz(cols.max(8)), 100_000);
        TestScreen(Arc::new(Mutex::new(parser)))
    }

    fn lock(&self) -> MutexGuard<'_, vt100::Parser> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// (cols, rows).
    pub fn size(&self) -> (u16, u16) {
        let (rows, cols) = self.lock().screen().size();
        (cols.get(), rows.get())
    }

    pub fn set_size(&self, cols: u16, rows: u16) {
        self.lock()
            .screen_mut()
            .set_size(nz(rows.max(2)), nz(cols.max(8)));
    }

    /// (row, col) of the cursor.
    pub fn cursor(&self) -> (u16, u16) {
        self.lock().screen().cursor_position()
    }

    pub fn cursor_visible(&self) -> bool {
        !self.lock().screen().hide_cursor()
    }

    /// The visible rows, trailing spaces trimmed.
    pub fn screen(&self) -> Vec<String> {
        let p = self.lock();
        let cols = p.screen().size().1.get();
        p.screen()
            .rows(0, cols)
            .map(|r| r.trim_end().to_owned())
            .collect()
    }

    /// The visible rows as runs of one style each, for snapshots and screenshots.
    pub fn cells(&self) -> Vec<Vec<StyledRun>> {
        let p = self.lock();
        let screen = p.screen();
        let (rows, cols) = screen.size();
        let mut out = Vec::with_capacity(rows.get() as usize);
        for r in 0..rows.get() {
            let mut runs: Vec<StyledRun> = Vec::new();
            for c in 0..cols.get() {
                let Some(cell) = screen.cell(r, c) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let style = RunStyle {
                    fg: color(cell.fgcolor()),
                    bg: color(cell.bgcolor()),
                    bold: cell.bold(),
                    dim: cell.dim(),
                    italic: cell.italic(),
                    underline: cell.underline(),
                    inverse: cell.inverse(),
                };
                let text = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                match runs.last_mut() {
                    Some(run) if run.style == style => run.text.push_str(text),
                    _ => runs.push(StyledRun { text: text.to_owned(), style }),
                }
            }
            out.push(runs);
        }
        out
    }

    /// Scrollback then the visible rows, trailing blank rows dropped.
    pub fn history(&self) -> Vec<String> {
        let mut p = self.lock();
        let cols = p.screen().size().1.get();
        let mut out = Vec::new();

        // vt100 shows scrollback by offset: row 0 at offset k is the k-th line from the end
        p.screen_mut().set_scrollback(usize::MAX);
        let len = p.screen().scrollback();
        for offset in (1..=len).rev() {
            p.screen_mut().set_scrollback(offset);
            if let Some(row) = p.screen().rows(0, cols).next() {
                out.push(row.trim_end().to_owned());
            }
        }
        p.screen_mut().set_scrollback(0);

        out.extend(p.screen().rows(0, cols).map(|r| r.trim_end().to_owned()));
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out
    }
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v).unwrap_or(NonZeroU16::MIN)
}

impl Write for TestScreen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.lock().process(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
