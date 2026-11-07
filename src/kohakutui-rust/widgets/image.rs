//! `image`: a picture drawn with half blocks, two pixels per cell (top in the foreground,
//! bottom in the background), so it shows on any colour terminal.
//!
//! `pixels` is a list of rows, each a list of `#rrggbb` (or `""` / nil for transparent); the
//! SDK's `Image.from_file` makes them from a picture file. Wider than the room, it is scaled
//! down (nearest pixel). `alt` is written instead on a terminal without colour, and under the
//! picture as a caption when `caption` is set.

use std::any::Any;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_bool, upd_str};
use super::text::truncate;
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{display, get};
use crate::proto::Value;
use crate::text::theme;

#[derive(Debug, Default)]
pub struct Image {
    /// Rows of pixels; None is transparent.
    pixels: Vec<Vec<Option<Color>>>,
    alt: String,
    caption: bool,
}

impl Image {
    fn width(&self) -> usize {
        self.pixels.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The pixel at (x, y) of the picture scaled to `w` × `h`.
    fn sample(&self, x: usize, y: usize, w: usize, h: usize) -> Option<Color> {
        let (sw, sh) = (self.width(), self.pixels.len());
        let sx = x * sw / w.max(1);
        let sy = y * sh / h.max(1);
        self.pixels.get(sy)?.get(sx).copied().flatten()
    }
}

fn pixel(v: &Value) -> Option<Color> {
    match v {
        Value::Nil => None,
        other => {
            let s = display(other);
            if s.is_empty() {
                None
            } else {
                theme::current().color(&s)
            }
        }
    }
}

impl Component for Image {
    fn kind(&self) -> &'static str {
        "image"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "alt", &mut self.alt);
        upd_bool(p, "caption", &mut self.caption, false);
        if let Some(rows) = get(p, "pixels") {
            let Some(rows) = rows.as_array() else {
                return Err(WidgetError::BadProp {
                    prop: "pixels",
                    reason: "a list of rows".into(),
                });
            };
            self.pixels = rows
                .iter()
                .map(|r| {
                    r.as_array()
                        .map(|c| c.iter().map(pixel).collect())
                        .unwrap_or_default()
                })
                .collect();
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let (sw, sh) = (self.width(), self.pixels.len());
        let alt = || {
            Line::from(Span::styled(
                truncate(&format!("[image: {}]", self.alt), width),
                theme::muted(),
            ))
        };
        if sw == 0 || sh == 0 || width == 0 {
            return vec![alt()];
        }
        let w = sw.min(width);
        let h = (sh * w / sw).max(1);
        let mut out = Vec::new();
        for row in (0..h).step_by(2) {
            let spans: Vec<Span<'static>> = (0..w)
                .map(|x| {
                    let top = self.sample(x, row, w, h);
                    let bottom = if row + 1 < h {
                        self.sample(x, row + 1, w, h)
                    } else {
                        None
                    };
                    match (top, bottom) {
                        (Some(t), Some(b)) => Span::styled("▀", Style::default().fg(t).bg(b)),
                        (Some(t), None) => Span::styled("▀", Style::default().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::default().fg(b)),
                        (None, None) => Span::raw(" "),
                    }
                })
                .collect();
            out.push(Line::from(spans));
        }
        if self.caption && !self.alt.is_empty() {
            out.push(Line::from(Span::styled(truncate(&self.alt, width), theme::muted())));
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
