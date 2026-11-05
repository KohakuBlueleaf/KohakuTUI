//! `chart`: a multi-row braille line chart that keeps a whole run, not a sliding window.
//!
//! Each series keeps every value until `max_points`, then halves itself by averaging pairs, so
//! memory stays bounded while the curve still spans step 1 to now. Rendering buckets the values
//! into two dot columns per cell and joins neighbouring columns with vertical strokes.

use std::any::Any;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::props::{upd_opt_num, upd_str, upd_usize};
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{display, get, num};
use crate::proto::Value;
use crate::text::{display_width, theme};

const PALETTE: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
];

/// Braille dot bits for (column 0..2, row 0..4) inside one cell.
const DOT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Debug, Default)]
struct Series {
    name: String,
    values: Vec<f64>,
    /// How many appended values each kept value stands for.
    stride: u64,
    count: u64,
    last: Option<f64>,
    nans: u64,
}

impl Series {
    fn push(&mut self, v: f64, cap: usize) {
        self.count += 1;
        self.last = Some(v);
        if v.is_nan() {
            self.nans += 1;
        }
        self.values.push(v);
        if self.values.len() > cap.max(16) {
            self.values = self.values.chunks(2).map(mean).collect();
            self.stride = self.stride.max(1) * 2;
        }
    }
}

/// Mean of the finite values, NaN when there are none.
fn mean(vals: &[f64]) -> f64 {
    let finite: Vec<f64> = vals.iter().copied().filter(|v| v.is_finite()).collect();
    match finite.len() {
        0 => f64::NAN,
        n => finite.iter().sum::<f64>() / n as f64,
    }
}

#[derive(Debug)]
pub struct Chart {
    label: String,
    unit: String,
    height: usize,
    digits: usize,
    max_points: usize,
    y_min: Option<f64>,
    y_max: Option<f64>,
    series: Vec<Series>,
}

impl Default for Chart {
    fn default() -> Self {
        Chart {
            label: String::new(),
            unit: String::new(),
            height: 8,
            digits: 4,
            max_points: 20_000,
            y_min: None,
            y_max: None,
            series: Vec::new(),
        }
    }
}

impl Chart {
    fn series_mut(&mut self, name: &str) -> &mut Series {
        let i = match self.series.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => {
                self.series
                    .push(Series { name: name.to_owned(), stride: 1, ..Series::default() });
                self.series.len() - 1
            }
        };
        &mut self.series[i]
    }

    fn push(&mut self, name: &str, data: &Value) -> Result<(), WidgetError> {
        let bad = || WidgetError::BadProp {
            prop: "data",
            reason: "chart append takes numbers, lists of numbers or {series: numbers}".into(),
        };
        let cap = self.max_points;
        match data {
            Value::Array(items) => {
                for v in items {
                    let v = num(v).ok_or_else(bad)?;
                    self.series_mut(name).push(v, cap);
                }
            }
            v => {
                let v = num(v).ok_or_else(bad)?;
                self.series_mut(name).push(v, cap);
            }
        }
        Ok(())
    }

    fn fmt(&self, v: f64) -> String {
        if !v.is_finite() {
            return format!("{v}");
        }
        let a = v.abs();
        if a != 0.0 && !(1e-3..1e6).contains(&a) {
            format!("{v:.2e}")
        } else {
            format!("{v:.prec$}", prec = self.digits)
        }
    }

    /// The y range over every finite value, widened when flat; props override either end.
    fn range(&self) -> Option<(f64, f64)> {
        let finite = self.series.iter().flat_map(|s| s.values.iter().copied());
        let (lo, hi) = finite
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        if !lo.is_finite() {
            return None;
        }
        let (mut lo, mut hi) = (self.y_min.unwrap_or(lo), self.y_max.unwrap_or(hi));
        if hi <= lo {
            let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
            lo -= pad;
            hi += pad;
        }
        Some((lo, hi))
    }

    fn header(&self, width: usize) -> Line<'static> {
        let mut spans = vec![Span::styled(self.label.clone(), theme::bold())];
        for (i, s) in self.series.iter().enumerate() {
            let color = Style::default().fg(PALETTE[i % PALETTE.len()]);
            let value = s.last.map(|v| self.fmt(v)).unwrap_or_default();
            spans.push(Span::raw("  "));
            spans.push(Span::styled("■ ", color));
            let value_style = match s.last {
                Some(v) if !v.is_finite() => theme::danger(),
                _ => Style::default(),
            };
            spans.push(Span::styled(format!("{} ", s.name), theme::muted()));
            spans.push(Span::styled(value, value_style));
            if s.nans > 0 {
                spans.push(Span::styled(format!(" ({} nan)", s.nans), theme::danger()));
            }
        }
        crate::text::clip(Line::from(spans), width)
    }

    /// Per dot column, the series value there (None for no data or all-NaN).
    fn columns(values: &[f64], dots: usize) -> Vec<Option<f64>> {
        let n = values.len();
        let mut out = vec![None; dots];
        if n == 0 || dots == 0 {
            return out;
        }
        if n <= dots {
            // fewer points than columns: each lands on its column and neighbours are joined by
            // a straight line, so a short series reads as a curve, not scattered dots
            let x_of = |i: usize| if n == 1 { 0 } else { i * (dots - 1) / (n - 1) };
            for (i, pair) in values.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                if !(a.is_finite() && b.is_finite()) {
                    continue;
                }
                let (x0, x1) = (x_of(i), x_of(i + 1));
                for (x, slot) in out.iter_mut().enumerate().take(x1 + 1).skip(x0) {
                    let t = if x1 == x0 {
                        0.0
                    } else {
                        (x - x0) as f64 / (x1 - x0) as f64
                    };
                    *slot = Some(a + (b - a) * t);
                }
            }
            for (i, v) in values.iter().enumerate() {
                if v.is_finite() {
                    out[x_of(i)] = Some(*v);
                }
            }
            return out;
        }
        for (x, slot) in out.iter_mut().enumerate() {
            let start = x * n / dots;
            let end = ((x + 1) * n / dots).max(start + 1);
            let m = mean(&values[start..end]);
            *slot = m.is_finite().then_some(m);
        }
        out
    }
}

impl Component for Chart {
    fn kind(&self) -> &'static str {
        "chart"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "label", &mut self.label);
        upd_str(p, "unit", &mut self.unit);
        upd_usize(p, "height", &mut self.height, 8);
        upd_usize(p, "digits", &mut self.digits, 4);
        upd_usize(p, "max_points", &mut self.max_points, 20_000);
        upd_opt_num(p, "y_min", &mut self.y_min);
        upd_opt_num(p, "y_max", &mut self.y_max);
        if get(p, "clear").and_then(Value::as_bool) == Some(true) {
            self.series.clear();
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        match data {
            Value::Map(entries) => {
                for (k, v) in entries {
                    self.push(&display(k), v)?;
                }
                Ok(())
            }
            other => {
                let name = if self.label.is_empty() {
                    "value".to_owned()
                } else {
                    self.label.clone()
                };
                self.push(&name, other)
            }
        }
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let height = self.height.clamp(2, 60);
        let mut out = vec![self.header(width)];
        let Some((lo, hi)) = self.range() else {
            out.push(crate::text::clip(
                Line::from(Span::styled("  no data yet", theme::muted())),
                width,
            ));
            return out;
        };

        let labels = [self.fmt(hi), self.fmt((hi + lo) / 2.0), self.fmt(lo)];
        let axis_w = labels.iter().map(|l| display_width(l)).max().unwrap_or(1) + 1;
        let plot_w = width.saturating_sub(axis_w + 1).max(4);
        let (dots_x, dots_y) = (plot_w * 2, height * 4);

        // cells hold braille bits and the series that drew last
        let mut bits = vec![vec![0u8; plot_w]; height];
        let mut owner = vec![vec![usize::MAX; plot_w]; height];
        let to_dot = |v: f64| {
            let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            dots_y - 1 - (t * (dots_y - 1) as f64).round() as usize
        };
        for (si, s) in self.series.iter().enumerate() {
            let mut prev: Option<usize> = None;
            for (x, v) in Self::columns(&s.values, dots_x).into_iter().enumerate() {
                let Some(v) = v else {
                    prev = None;
                    continue;
                };
                let y = to_dot(v);
                let (a, b) = match prev {
                    Some(p) => (p.min(y), p.max(y)),
                    None => (y, y),
                };
                for yy in a..=b {
                    let (cx, cy) = (x / 2, yy / 4);
                    bits[cy][cx] |= DOT[x % 2][yy % 4];
                    owner[cy][cx] = si;
                }
                prev = Some(y);
            }
        }

        for row in 0..height {
            let label = match row {
                0 => labels[0].clone(),
                r if r == height - 1 => labels[2].clone(),
                r if r == height / 2 && height >= 5 => labels[1].clone(),
                _ => String::new(),
            };
            let pad = axis_w - 1 - display_width(&label);
            let tick = if label.is_empty() { "│" } else { "┤" };
            let mut spans = vec![Span::styled(
                format!("{}{label} {tick}", " ".repeat(pad)),
                theme::muted(),
            )];
            for cx in 0..plot_w {
                let b = bits[row][cx];
                if b == 0 {
                    spans.push(Span::raw(" "));
                    continue;
                }
                let glyph = char::from_u32(0x2800 + u32::from(b)).unwrap_or(' ');
                let color = PALETTE[owner[row][cx] % PALETTE.len()];
                spans.push(Span::styled(glyph.to_string(), Style::default().fg(color)));
            }
            out.push(Line::from(spans));
        }

        // x axis: first and last step under the plot
        let count = self.series.iter().map(|s| s.count).max().unwrap_or(0);
        let left = "1".to_owned();
        let right = match self.unit.as_str() {
            "" => count.to_string(),
            unit => format!("{count} {unit}"),
        };
        let gap = plot_w.saturating_sub(display_width(&left) + display_width(&right));
        let axis = format!("{}└{left}{}{right}", " ".repeat(axis_w), " ".repeat(gap));
        out.push(Line::from(Span::styled(axis, theme::muted())));
        out.into_iter()
            .map(|l| crate::text::clip(l, width))
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
