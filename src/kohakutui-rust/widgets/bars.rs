//! `bars`: horizontal bar charts in eighth-block resolution.
//!
//! One row per label. With one series, or `stacked`, each row is one bar (stacked: the
//! series' segments in their colours); with several series side by side, each label gets one
//! row per series. `max` fixes the full length (a gauge), else the longest bar fills the room.
//! `percent` writes each value as its share of the row values' total (a pie as bars).

use std::any::Any;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_opt_num, upd_strings};
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{display, get, num};
use crate::proto::Value;
use crate::text::{clip, display_width, theme};

const PALETTE: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
];
/// One drawn row: label, segments as (series index, value), value text.
type Row = (String, Vec<(usize, f64)>, String);

const MAX_BAR: usize = 48;
const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

#[derive(Debug, Default)]
pub struct Bars {
    labels: Vec<String>,
    series: Vec<(String, Vec<f64>)>,
    stacked: bool,
    percent: bool,
    max: Option<f64>,
    /// Decimals; unset: none when every value is whole, else two.
    digits: Option<usize>,
}

/// `n` eighth-cells of bar as full blocks plus one partial block.
fn bar(eighths: usize) -> String {
    format!("{}{}", "█".repeat(eighths / 8), EIGHTHS[eighths % 8])
}

impl Bars {
    fn fmt(&self, v: f64) -> String {
        let digits = self.digits.unwrap_or_else(|| {
            let whole = self
                .series
                .iter()
                .flat_map(|(_, d)| d)
                .all(|v| v.fract() == 0.0);
            if whole {
                0
            } else {
                2
            }
        });
        format!("{v:.digits$}")
    }

    fn value(&self, series: usize, label: usize) -> f64 {
        self.series
            .get(series)
            .and_then(|(_, d)| d.get(label))
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// The rows: (label shown, segments of (series index, value), value text).
    fn rows(&self) -> Vec<Row> {
        let n = self.series.len();
        let total: f64 = (0..self.labels.len())
            .map(|l| (0..n).map(|s| self.value(s, l)).sum::<f64>())
            .sum();
        let shown = |v: f64| {
            if self.percent && total > 0.0 {
                format!("{}%", (v / total * 100.0).round())
            } else {
                self.fmt(v)
            }
        };
        let mut rows = Vec::new();
        for (l, label) in self.labels.iter().enumerate() {
            if self.stacked || n == 1 {
                let segs: Vec<(usize, f64)> = (0..n).map(|s| (s, self.value(s, l))).collect();
                let sum = segs.iter().map(|(_, v)| v).sum::<f64>();
                let text = match self.max {
                    Some(max) if !self.percent => format!("{} / {}", shown(sum), self.fmt(max)),
                    _ => shown(sum),
                };
                rows.push((label.clone(), segs, text));
            } else {
                for s in 0..n {
                    let v = self.value(s, l);
                    let name = if s == 0 { label.clone() } else { String::new() };
                    rows.push((name, vec![(s, v)], shown(v)));
                }
            }
        }
        rows
    }
}

impl Component for Bars {
    fn kind(&self) -> &'static str {
        "bars"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_strings(p, "labels", &mut self.labels);
        if let Some(v) = get(p, "series") {
            self.series = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| {
                    let name = get(s, "name").map(display).unwrap_or_default();
                    let data = get(s, "data")
                        .and_then(Value::as_array)
                        .map(|d| d.iter().map(|v| num(v).unwrap_or(f64::NAN)).collect())
                        .unwrap_or_default();
                    (name, data)
                })
                .collect();
        }
        if let Some(v) = get(p, "stacked") {
            self.stacked = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = get(p, "percent") {
            self.percent = v.as_bool().unwrap_or(false);
        }
        upd_opt_num(p, "max", &mut self.max);
        if let Some(v) = get(p, "digits") {
            self.digits = num(v).filter(|d| *d >= 0.0).map(|d| (d as usize).min(10));
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let w = width as usize;
        let mut out = Vec::new();
        let named = self.series.iter().filter(|(n, _)| !n.is_empty()).count();
        if self.series.len() > 1 && named > 0 {
            let mut spans = Vec::new();
            for (i, (name, _)) in self.series.iter().enumerate() {
                let color = Style::default().fg(PALETTE[i % PALETTE.len()]);
                spans.push(Span::styled("■ ", color));
                spans.push(Span::styled(format!("{name}  "), theme::muted()));
            }
            out.push(clip(Line::from(spans), w));
        }
        let rows = self.rows();
        if rows.is_empty() {
            out.push(Line::from(Span::styled("no data", theme::muted())));
            return out;
        }
        let label_w = rows
            .iter()
            .map(|(l, _, _)| display_width(l))
            .max()
            .unwrap_or(0)
            .min(w / 3);
        let value_w = rows
            .iter()
            .map(|(_, _, t)| display_width(t))
            .max()
            .unwrap_or(0);
        // bars past ~48 cells read worse, not better, on a wide terminal
        let room = w.saturating_sub(label_w + value_w + 4).clamp(4, MAX_BAR);
        let sums = rows
            .iter()
            .map(|(_, segs, _)| segs.iter().map(|(_, v)| v).sum::<f64>());
        // a share is drawn against the whole (50% fills half), anything else against the
        // longest bar (or a fixed `max`)
        let full = match self.max {
            Some(max) => max,
            None if self.percent => sums.sum(),
            None => sums.fold(0.0, f64::max),
        };
        for (label, segs, text) in rows {
            let pad = label_w.saturating_sub(display_width(&label));
            let mut spans = vec![
                Span::styled(
                    format!("{}{}", clip_text(&label, label_w), " ".repeat(pad)),
                    theme::muted(),
                ),
                Span::styled(" │", theme::faint()),
            ];
            let mut used = 0;
            let mut drawn = 0.0;
            for (s, v) in segs {
                if full <= 0.0 {
                    break;
                }
                // segments end where their running total ends, so stacked bars stay exact
                drawn += v;
                let end = ((drawn / full).min(1.0) * (room * 8) as f64).round() as usize;
                let len = end.saturating_sub(used);
                if len == 0 {
                    continue;
                }
                let color = PALETTE[s % PALETTE.len()];
                spans.push(Span::styled(bar(len), Style::default().fg(color)));
                used += len;
            }
            let cells = used.div_ceil(8);
            spans.push(Span::raw(" ".repeat(room.saturating_sub(cells) + 1)));
            spans.push(Span::styled(text, theme::bold()));
            out.push(clip(Line::from(spans), w));
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn clip_text(s: &str, w: usize) -> String {
    if display_width(s) <= w {
        return s.to_owned();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if display_width(&out) + 2 > w {
            break;
        }
        out.push(ch);
    }
    out + "…"
}
