//! `markdown`: Markdown text that may stream in by `append`.

use std::any::Any;
use std::sync::Mutex;

use ratatui::text::Line;

use super::props::upd_bool;
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{display, get};
use crate::proto::Value;
use crate::text::{MdLine, MdStream};

/// Render cache key: (width, stable_end).
type Cache = Option<(usize, usize, Vec<MdLine>)>;

#[derive(Debug, Default)]
pub struct Markdown {
    stream: MdStream,
    streaming: bool,
    cache: Mutex<Cache>,
}

impl Markdown {
    /// The stable prefix's lines with their source offsets (cached per width).
    pub fn stable(&self, width: u16) -> Vec<MdLine> {
        let width = width as usize;
        let end = self.stream.stable_end();
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        match cache.as_ref() {
            Some((w, e, lines)) if *w == width && *e == end => lines.clone(),
            _ => {
                let lines = self.stream.stable_lines(width);
                *cache = Some((width, end, lines.clone()));
                lines
            }
        }
    }

    pub fn pending(&self, width: u16) -> Vec<Line<'static>> {
        self.stream.pending_lines(width as usize)
    }

    pub fn stable_end(&self) -> usize {
        self.stream.stable_end()
    }

    pub fn finished(&self) -> bool {
        self.stream.is_finished()
    }
}

impl Component for Markdown {
    fn kind(&self) -> &'static str {
        "markdown"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_bool(p, "streaming", &mut self.streaming, false);
        if let Some(src) = get(p, "source") {
            self.stream = MdStream::new();
            self.stream.push(&display(src));
        }
        if !self.streaming {
            self.stream.finish();
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        let text = data.as_str().ok_or(WidgetError::BadProp {
            prop: "data",
            reason: "markdown append takes a string".into(),
        })?;
        if self.stream.is_finished() {
            let mut next = MdStream::new();
            let src = self.stream.source().to_owned();
            next.push(&src);
            next.push(text);
            if !self.streaming {
                next.finish();
            }
            self.stream = next;
        } else {
            self.stream.push(text);
        }
        Ok(())
    }

    fn seal(&mut self) {
        self.stream.finish();
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = self.stable(width).into_iter().map(|l| l.line).collect();
        out.extend(self.pending(width));
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
