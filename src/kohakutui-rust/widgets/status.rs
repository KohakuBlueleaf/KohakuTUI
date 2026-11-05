//! Live indicators: `status` (spinner + elapsed), `progress` (bar, rate, ETA) and `sparkline`.

use std::any::Any;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::props::{upd_bool, upd_num, upd_opt_num, upd_str, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{get, num};
use crate::proto::Value;
use crate::text::{display_width, theme};

/// A one-line activity indicator with elapsed time.
#[derive(Debug)]
pub struct Status {
    label: String,
    spinner: bool,
    tone: String,
    started: Instant,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            label: String::new(),
            spinner: true,
            tone: String::new(),
            started: Instant::now(),
        }
    }
}

impl Component for Status {
    fn kind(&self) -> &'static str {
        "status"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "label", &mut self.label);
        upd_bool(p, "spinner", &mut self.spinner, true);
        upd_str(p, "tone", &mut self.tone);
        if get(p, "restart").and_then(Value::as_bool) == Some(true) {
            self.started = Instant::now();
        }
        Ok(())
    }

    fn lines(&self, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
        let mut spans = Vec::new();
        if self.spinner {
            spans.push(Span::styled(theme::spinner(ctx.ms()), theme::accent()));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(self.label.clone(), theme::tone(&self.tone)));
        let secs = ctx.now.saturating_duration_since(self.started).as_secs();
        if secs >= 1 {
            spans.push(Span::styled(format!("  ({})", clock(secs)), theme::muted()));
        }
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        if display_width(&text) > width as usize {
            return vec![Line::from(truncate(&text, width as usize))];
        }
        vec![Line::from(spans)]
    }

    fn animating(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `h:mm:ss` or `m:ss` or `Ns`.
fn clock(secs: u64) -> String {
    match secs {
        s if s >= 3600 => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        s if s >= 60 => format!("{}:{:02}", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

/// A progress bar with value/total, percentage, rate and ETA.
#[derive(Debug, Default)]
pub struct Progress {
    value: f64,
    total: Option<f64>,
    label: String,
    unit: String,
    samples: VecDeque<(Instant, f64)>,
}

impl Progress {
    fn record(&mut self) {
        let now = Instant::now();
        self.samples.push_back((now, self.value));
        while self.samples.len() > 32
            || self
                .samples
                .front()
                .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(10))
        {
            self.samples.pop_front();
        }
    }

    /// Units per second over the recent samples.
    pub fn rate(&self) -> Option<f64> {
        let (t0, v0) = self.samples.front()?;
        let (t1, v1) = self.samples.back()?;
        let dt = t1.duration_since(*t0).as_secs_f64();
        (dt > 0.05).then(|| (v1 - v0) / dt)
    }
}

impl Component for Progress {
    fn kind(&self) -> &'static str {
        "progress"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_opt_num(p, "total", &mut self.total);
        upd_str(p, "label", &mut self.label);
        upd_str(p, "unit", &mut self.unit);
        if get(p, "value").is_some() {
            upd_num(p, "value", &mut self.value, 0.0);
            self.record();
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        self.value = num(data).ok_or(WidgetError::BadProp {
            prop: "data",
            reason: "progress append takes a number".into(),
        })?;
        self.record();
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let unit = if self.unit.is_empty() {
            "it"
        } else {
            &self.unit
        };
        let mut stats = match self.total {
            Some(t) if t > 0.0 => {
                let pct = (self.value / t * 100.0).clamp(0.0, 100.0);
                format!(" {}/{} {pct:>3.0}%", short(self.value), short(t))
            }
            _ => format!(" {}", short(self.value)),
        };
        if let Some(rate) = self.rate().filter(|r| *r > 0.0) {
            stats.push_str(&format!(" · {} {unit}/s", short(rate)));
            if let Some(t) = self.total {
                let left = ((t - self.value) / rate).max(0.0);
                stats.push_str(&format!(" · eta {}", clock(left.round() as u64)));
            }
        }
        let label = if self.label.is_empty() {
            String::new()
        } else {
            format!("{} ", self.label)
        };
        let bar_w = width.saturating_sub(display_width(&label) + display_width(&stats));
        let mut spans = vec![Span::styled(label, theme::bold())];
        if bar_w >= 4 {
            let frac = match self.total {
                Some(t) if t > 0.0 => (self.value / t).clamp(0.0, 1.0),
                _ => 0.0,
            };
            let filled = (frac * bar_w as f64).round() as usize;
            spans.push(Span::styled("━".repeat(filled), theme::accent()));
            spans.push(Span::styled("─".repeat(bar_w - filled), theme::muted()));
        }
        spans.push(Span::styled(stats, theme::muted()));
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        if display_width(&text) > width {
            return vec![Line::from(truncate(&text, width))];
        }
        vec![Line::from(spans)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Compact number: integers as-is, large values with k/M, small with 3 significant digits.
fn short(v: f64) -> String {
    let a = v.abs();
    if !v.is_finite() {
        format!("{v}")
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.1}k", v / 1e3)
    } else if v.fract() == 0.0 || a >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

/// A one-line chart of recent values; NaN shows as a marked gap.
#[derive(Debug)]
pub struct Sparkline {
    label: String,
    max_points: usize,
    digits: usize,
    values: VecDeque<f64>,
}

impl Default for Sparkline {
    fn default() -> Self {
        Sparkline { label: String::new(), max_points: 200, digits: 4, values: VecDeque::new() }
    }
}

impl Sparkline {
    fn push(&mut self, v: f64) {
        self.values.push_back(v);
        while self.values.len() > self.max_points.max(1) {
            self.values.pop_front();
        }
    }
}

impl Component for Sparkline {
    fn kind(&self) -> &'static str {
        "sparkline"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "label", &mut self.label);
        upd_usize(p, "max_points", &mut self.max_points, 200);
        upd_usize(p, "digits", &mut self.digits, 4);
        if let Some(vals) = get(p, "values").and_then(Value::as_array) {
            self.values.clear();
            for v in vals.iter().filter_map(num) {
                self.push(v);
            }
        }
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        let bad = || WidgetError::BadProp {
            prop: "data",
            reason: "sparkline append takes a number or a list of numbers".into(),
        };
        match data {
            Value::Array(items) => {
                for v in items {
                    self.push(num(v).ok_or_else(bad)?);
                }
            }
            v => self.push(num(v).ok_or_else(bad)?),
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let width = width as usize;
        let last = self.values.back().copied();
        let stat = match last {
            Some(v) if v.is_finite() => format!(" {v:.prec$}", prec = self.digits),
            Some(v) => format!(" {v}"),
            None => String::new(),
        };
        let label = if self.label.is_empty() {
            String::new()
        } else {
            format!("{} ", self.label)
        };
        let room = width.saturating_sub(display_width(&label) + display_width(&stat));
        let shown: Vec<f64> = self.values.iter().rev().take(room).rev().copied().collect();
        let finite = shown.iter().copied().filter(|v| v.is_finite());
        let (lo, hi) =
            finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let mut spans = vec![Span::styled(label, theme::bold())];
        for v in shown {
            if !v.is_finite() {
                spans.push(Span::styled("·", theme::danger()));
                continue;
            }
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            let i = (t * 7.0).round().clamp(0.0, 7.0) as usize;
            spans.push(Span::styled(BARS[i], theme::accent()));
        }
        let stat_style = match last {
            Some(v) if !v.is_finite() => theme::danger(),
            _ => Style::default(),
        };
        spans.push(Span::styled(stat, stat_style));
        vec![Line::from(spans)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
