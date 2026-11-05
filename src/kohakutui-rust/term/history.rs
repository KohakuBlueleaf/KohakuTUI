//! Writing finished lines into scrollback above the inline viewport.
//!
//! Region strategy (after Codex `insert_history.rs`): lines first fill the free rows the
//! viewport would move down into; the rest scroll in through a scroll region spanning the rows
//! above the viewport, so its top rows leave into scrollback. The region needs at least two rows;
//! with fewer, or with `Reprint`, the viewport is cleared, the lines printed in its place and the
//! viewport redrawn below them. Every insertion is followed by a full viewport redraw.

use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use ratatui::backend::IntoCrossterm;
use ratatui::style::Modifier;
use ratatui::text::Line;

use super::{History, Out, Term};
use crate::text::plain;

impl Term {
    /// Writes `lines` (each at most one row wide) into history above the viewport.
    pub fn insert_history(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        if let Out::Plain(stdout) = &mut self.out {
            for line in lines {
                writeln!(stdout, "{}", plain(line).trim_end())?;
            }
            return stdout.flush();
        }

        match self.history {
            History::Region => self.insert_region(lines)?,
            History::Reprint => self.insert_reprint(lines)?,
        }
        self.prev = None;
        Ok(())
    }

    fn insert_region(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        let free = self.rows.saturating_sub(self.vt + self.vh);
        let k = (lines.len() as u16).min(free);

        // the viewport moves down k rows; its old rows take the first k lines
        for (j, line) in lines[..k as usize].iter().enumerate() {
            queue!(self.out, MoveTo(0, self.vt + j as u16), Clear(ClearType::UntilNewLine))?;
            write_line(&mut self.out, line)?;
        }
        self.vt += k;

        let rest = &lines[k as usize..];
        if rest.is_empty() {
            return Ok(());
        }
        if self.vt < 2 {
            return self.insert_reprint(rest);
        }

        // each newline at the region's bottom scrolls its top row into scrollback
        write!(self.out, "\x1b[1;{}r", self.vt)?;
        queue!(self.out, MoveTo(0, self.vt - 1))?;
        for line in rest {
            write!(self.out, "\r\n")?;
            queue!(self.out, Clear(ClearType::UntilNewLine))?;
            write_line(&mut self.out, line)?;
        }
        write!(self.out, "\x1b[r")?;
        Ok(())
    }

    fn insert_reprint(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        queue!(self.out, MoveTo(0, self.vt), Clear(ClearType::FromCursorDown))?;
        let mut row = self.vt;
        for line in lines {
            write_line(&mut self.out, line)?;
            write!(self.out, "\r\n")?;
            row = (row + 1).min(self.rows.saturating_sub(1));
        }
        self.vt = row;
        self.vh = 0;
        Ok(())
    }
}

/// Writes one styled line at the cursor, then resets the style.
pub(crate) fn write_line(w: &mut impl Write, line: &Line<'static>) -> io::Result<()> {
    for span in &line.spans {
        let style = line.style.patch(span.style);
        queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
        if let Some(fg) = style.fg {
            queue!(w, SetForegroundColor(fg.into_crossterm()))?;
        }
        if let Some(bg) = style.bg {
            queue!(w, SetBackgroundColor(bg.into_crossterm()))?;
        }
        let attrs = [
            (Modifier::BOLD, Attribute::Bold),
            (Modifier::DIM, Attribute::Dim),
            (Modifier::ITALIC, Attribute::Italic),
            (Modifier::UNDERLINED, Attribute::Underlined),
            (Modifier::REVERSED, Attribute::Reverse),
            (Modifier::CROSSED_OUT, Attribute::CrossedOut),
        ];
        for (m, a) in attrs {
            if style.add_modifier.contains(m) {
                queue!(w, SetAttribute(a))?;
            }
        }
        queue!(w, Print(span.content.as_ref()))?;
    }
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)
}
