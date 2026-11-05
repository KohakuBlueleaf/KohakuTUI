//! `scroll`: a container whose children show through a window of `height` rows (0: all of
//! them). ↑↓, pgup/pgdn, home/end and the wheel move the window; at the bottom it follows new
//! lines. `keys` maps single characters to actions: pressing one emits `select` with the action
//! as its value (`{"k": "stop"}`). As a dialog it takes `dock: "input"` like the others.

use std::any::Any;
use std::cell::Cell as Shared;

use crossterm::event::KeyCode;
use ratatui::text::{Line, Span};

use super::props::{upd_opt_str, upd_usize};
use super::text::truncate;
use super::{upd_dock, Component, Ctx, Emit, Input, Mouse, MouseAct, WidgetError};
use crate::proto::value::{display, get};
use crate::proto::Value;
use crate::text::theme;

const WHEEL: usize = 3;

#[derive(Debug)]
pub struct Scroll {
    title: Option<String>,
    height: usize,
    keys: Vec<(char, String)>,
    dock: bool,
    /// Lines between the bottom of the window and the end of the content; 0 follows the tail.
    from_bottom: usize,
    /// Content height and window height at the last render, to clamp key scrolling.
    seen: Shared<(usize, usize)>,
}

impl Default for Scroll {
    fn default() -> Self {
        Scroll {
            title: None,
            height: 12,
            keys: Vec::new(),
            dock: false,
            from_bottom: 0,
            seen: Shared::new((0, 0)),
        }
    }
}

impl Scroll {
    fn max_from_bottom(&self) -> usize {
        let (total, window) = self.seen.get();
        total.saturating_sub(window)
    }

    /// Moves the window; false when it could not move (already at that end, nothing to scroll).
    fn move_by(&mut self, up: isize) -> bool {
        let next = self.from_bottom as isize + up;
        let next = next.clamp(0, self.max_from_bottom() as isize) as usize;
        let moved = next != self.from_bottom;
        self.from_bottom = next;
        moved
    }

    fn hint(&self, start: usize, end: usize, total: usize) -> String {
        let mut parts = Vec::new();
        if total > end - start {
            parts.push(format!("{}–{} of {total}", start + 1, end));
            // in the transcript only the wheel reaches it, and only in full screen
            let how = if self.dock {
                "↑↓ scroll"
            } else {
                "wheel scrolls in full screen"
            };
            parts.push(how.to_owned());
        }
        for (key, action) in &self.keys {
            parts.push(format!("{key} {action}"));
        }
        if self.dock {
            parts.push("esc close".to_owned());
        }
        parts.join(" · ")
    }
}

impl Component for Scroll {
    fn kind(&self) -> &'static str {
        "scroll"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_str(p, "title", &mut self.title);
        upd_usize(p, "height", &mut self.height, 12);
        upd_dock(p, &mut self.dock);
        if let Some(keys) = get(p, "keys") {
            self.keys = match keys {
                Value::Map(entries) => entries
                    .iter()
                    .filter_map(|(k, v)| Some((display(k).chars().next()?, display(v))))
                    .collect(),
                _ => Vec::new(),
            };
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        match &self.title {
            Some(t) => vec![Line::from(Span::styled(
                truncate(t, width as usize),
                theme::bold(),
            ))],
            None => Vec::new(),
        }
    }

    fn container(&self) -> bool {
        true
    }

    fn window(&self, total: usize) -> (usize, usize) {
        let window = if self.height == 0 {
            total
        } else {
            self.height.min(total)
        };
        let end = total - self.from_bottom.min(total - window);
        (end - window, end)
    }

    fn frame_children(&self, children: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
        let total = children.len();
        let (start, end) = self.window(total);
        let window = end - start;
        self.seen.set((total, window));

        let mut out: Vec<Line<'static>> = children.into_iter().skip(start).take(window).collect();
        let hint = self.hint(start, end, total);
        if !hint.is_empty() {
            out.push(Line::from(Span::styled(
                truncate(&format!("  {hint}"), width as usize),
                theme::muted(),
            )));
        }
        out
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_input(&mut self, input: &Input, out: &mut Vec<Emit>) -> bool {
        let Input::Key { code, .. } = input else {
            return false;
        };
        let page = self.seen.get().1.max(1) as isize;
        match code {
            KeyCode::Up => _ = self.move_by(1),
            KeyCode::Down => _ = self.move_by(-1),
            KeyCode::PageUp => _ = self.move_by(page),
            KeyCode::PageDown => _ = self.move_by(-page),
            KeyCode::Home => self.from_bottom = self.max_from_bottom(),
            KeyCode::End => self.from_bottom = 0,
            KeyCode::Char(c) => {
                let Some((_, action)) = self.keys.iter().find(|(k, _)| k == c) else {
                    return false;
                };
                out.push(Emit::Select { index: 0, value: Value::from(action.as_str()) });
            }
            _ => return false,
        }
        true
    }

    fn on_mouse(&mut self, m: &Mouse, _out: &mut Vec<Emit>) -> bool {
        // a box that cannot move lets the wheel go on to whatever is around it
        match m.act {
            MouseAct::ScrollUp => self.move_by(WHEEL as isize),
            MouseAct::ScrollDown => self.move_by(-(WHEEL as isize)),
            MouseAct::Click | MouseAct::Middle => false,
        }
    }

    fn docked(&self) -> bool {
        self.dock
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
