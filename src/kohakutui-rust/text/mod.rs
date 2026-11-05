//! Text stage: width-aware wrapping, Markdown to styled lines, and the streaming collector that
//! decides which source prefix renders the same from now on (design §6.4, §6.6).

mod markdown;
mod stream;
#[cfg(test)]
mod tests;
pub mod theme;
mod wrap;

pub use markdown::render as render_markdown;
pub use stream::MdStream;
pub use wrap::{clip, display_width, plain, wrap_spans, wrap_text};

use ratatui::text::Line;

/// One rendered visual line and the byte offset where its source line starts.
///
/// Every visual line comes from exactly one source line (soft breaks render as hard breaks), so
/// scrollback progress can be tracked as a source offset that no width change can shift.
#[derive(Debug, Clone, PartialEq)]
pub struct MdLine {
    pub src: usize,
    pub line: Line<'static>,
}
