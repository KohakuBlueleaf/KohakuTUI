//! `diff`: a file change as coloured lines with line numbers.
//!
//! Either `text`, a unified diff (`@@` hunks, `+` / `-` / space lines; `---` / `+++` headers
//! are skipped), or `old` and `new`, diffed here line by line with `context` unchanged lines
//! around each change (default 3). `path` titles it with the added / removed counts; `max_lines`
//! cuts a long diff with a "… N more lines" row. `start` numbers `old` / `new` from that line
//! (a snippet of a file).

use std::any::Any;

use ratatui::text::{Line, Span};

use super::props::{upd_opt_num, upd_opt_str, upd_str, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, WidgetError};
use crate::proto::Value;
use crate::text::{display_width, theme};

/// Above this many cells in the comparison table the middle is shown as replaced whole.
const MAX_TABLE: usize = 4_000_000;

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Hunk(String),
    Same(usize, usize, String),
    Del(usize, String),
    Add(usize, String),
}

#[derive(Debug, Default)]
pub struct Diff {
    text: String,
    old: Option<String>,
    new: Option<String>,
    path: Option<String>,
    /// Unchanged lines around a change, for `old` / `new` (unset: 3).
    context: Option<f64>,
    max_lines: usize,
    /// With `old` / `new`: the file line their first line is (a snippet of a larger file).
    start: usize,
    rows: Vec<Row>,
}

impl Diff {
    fn rebuild(&mut self) {
        self.rows = match (&self.old, &self.new) {
            (Some(old), Some(new)) => {
                let context = self.context.map_or(3, |c| c.max(0.0) as usize);
                let mut rows = compare(old, new, context);
                shift(&mut rows, self.start.saturating_sub(1));
                rows
            }
            _ => parse_unified(&self.text),
        };
    }

    fn counts(&self) -> (usize, usize) {
        self.rows.iter().fold((0, 0), |(a, d), r| match r {
            Row::Add(..) => (a + 1, d),
            Row::Del(..) => (a, d + 1),
            _ => (a, d),
        })
    }
}

/// Parses a unified diff into rows, numbering lines from the hunk headers.
fn parse_unified(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (1usize, 1usize);
    for line in text.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("diff ") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@") {
            // `-a,b +c,d`: the starts a and c (the counts may be absent)
            let start = |sign: char| {
                rest.split_whitespace()
                    .find_map(|w| w.strip_prefix(sign))
                    .and_then(|w| w.split(',').next()?.parse().ok())
                    .unwrap_or(1)
            };
            old = start('-');
            new = start('+');
            rows.push(Row::Hunk(line.to_owned()));
            continue;
        }
        if let Some(t) = line.strip_prefix('+') {
            rows.push(Row::Add(new, t.to_owned()));
            new += 1;
        } else if let Some(t) = line.strip_prefix('-') {
            rows.push(Row::Del(old, t.to_owned()));
            old += 1;
        } else if !line.starts_with('\\') {
            let t = line.strip_prefix(' ').unwrap_or(line);
            rows.push(Row::Same(old, new, t.to_owned()));
            old += 1;
            new += 1;
        }
    }
    rows
}

/// Line diff of `old` and `new` (common ends trimmed, the middle by longest common
/// subsequence), shown as hunks with `context` lines around each change.
fn compare(old: &str, new: &str, context: usize) -> Vec<Row> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);

    // every line as (old index, new index): Some on both sides when unchanged
    let mut all: Vec<(Option<usize>, Option<usize>)> =
        (0..pre).map(|i| (Some(i), Some(i))).collect();
    for (x, y) in middle(am, bm) {
        all.push((x.map(|i| i + pre), y.map(|j| j + pre)));
    }
    for k in 0..suf {
        all.push((Some(a.len() - suf + k), Some(b.len() - suf + k)));
    }

    let changed: Vec<bool> = all
        .iter()
        .map(|(x, y)| x.is_none() || y.is_none())
        .collect();
    let near = |i: usize| {
        let lo = i.saturating_sub(context);
        let hi = (i + context + 1).min(all.len());
        changed[lo..hi].iter().any(|c| *c)
    };
    let mut rows = Vec::new();
    let mut open = false;
    for (i, &(x, y)) in all.iter().enumerate() {
        if !near(i) {
            open = false;
            continue;
        }
        if !open {
            let o = x
                .or_else(|| all[..i].iter().rev().find_map(|p| p.0))
                .map_or(1, |v| v + 1);
            let n = y
                .or_else(|| all[..i].iter().rev().find_map(|p| p.1))
                .map_or(1, |v| v + 1);
            rows.push(Row::Hunk(format!("@@ -{o} +{n} @@")));
            open = true;
        }
        rows.push(match (x, y) {
            (Some(i), Some(j)) => Row::Same(i + 1, j + 1, a[i].to_owned()),
            (Some(i), None) => Row::Del(i + 1, a[i].to_owned()),
            (None, Some(j)) => Row::Add(j + 1, b[j].to_owned()),
            (None, None) => continue,
        });
    }
    rows
}

/// Moves every line number (and the hunk headers) down by `by` lines.
fn shift(rows: &mut [Row], by: usize) {
    if by == 0 {
        return;
    }
    for row in rows.iter_mut() {
        match row {
            Row::Same(o, n, _) => {
                *o += by;
                *n += by;
            }
            Row::Del(o, _) => *o += by,
            Row::Add(n, _) => *n += by,
            Row::Hunk(h) => {
                let (mut o, mut n) = (0, 0);
                for w in h.split_whitespace() {
                    if let Some(v) = w.strip_prefix('-').and_then(|v| v.parse::<usize>().ok()) {
                        o = v;
                    }
                    if let Some(v) = w.strip_prefix('+').and_then(|v| v.parse::<usize>().ok()) {
                        n = v;
                    }
                }
                *h = format!("@@ -{} +{} @@", o + by, n + by);
            }
        }
    }
}

/// The changed middle as aligned pairs: deletions before additions within each gap.
fn middle(a: &[&str], b: &[&str]) -> Vec<(Option<usize>, Option<usize>)> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_TABLE {
        let mut out: Vec<_> = (0..n).map(|i| (Some(i), None)).collect();
        out.extend((0..m).map(|j| (None, Some(j))));
        return out;
    }
    // lcs[i][j]: the longest common subsequence of a[i..] and b[j..]
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push((Some(i), Some(j)));
            i += 1;
            j += 1;
        } else if j >= m || (i < n && lcs[at(i + 1, j)] >= lcs[at(i, j + 1)]) {
            out.push((Some(i), None));
            i += 1;
        } else {
            out.push((None, Some(j)));
            j += 1;
        }
    }
    out
}

impl Component for Diff {
    fn kind(&self) -> &'static str {
        "diff"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "text", &mut self.text);
        upd_opt_str(p, "old", &mut self.old);
        upd_opt_str(p, "new", &mut self.new);
        upd_opt_str(p, "path", &mut self.path);
        upd_opt_num(p, "context", &mut self.context);
        upd_usize(p, "max_lines", &mut self.max_lines, 0);
        upd_usize(p, "start", &mut self.start, 1);
        self.rebuild();
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let mut out = Vec::new();
        let (added, removed) = self.counts();
        if let Some(path) = &self.path {
            let counts = format!("  +{added} -{removed}");
            let room = width.saturating_sub(display_width(&counts));
            out.push(Line::from(vec![
                Span::styled(truncate(path, room), theme::bold()),
                Span::styled(format!("  +{added}"), theme::ok()),
                Span::styled(format!(" -{removed}"), theme::danger()),
            ]));
        }
        if self.rows.is_empty() {
            out.push(Line::from(Span::styled("(no changes)", theme::muted())));
            return out;
        }
        let digits = self
            .rows
            .iter()
            .map(|r| match r {
                Row::Same(o, n, _) => (*o).max(*n),
                Row::Del(o, _) => *o,
                Row::Add(n, _) => *n,
                Row::Hunk(_) => 0,
            })
            .max()
            .unwrap_or(0)
            .to_string()
            .len();
        let limit = if self.max_lines == 0 {
            usize::MAX
        } else {
            self.max_lines
        };
        for row in self.rows.iter().take(limit) {
            let (num, mark, text, style) = match row {
                Row::Hunk(h) => {
                    out.push(Line::from(Span::styled(truncate(h, width), theme::muted())));
                    continue;
                }
                Row::Same(_, n, t) => (*n, ' ', t, ratatui::style::Style::default()),
                Row::Del(o, t) => (*o, '-', t, theme::danger()),
                Row::Add(n, t) => (*n, '+', t, theme::ok()),
            };
            let gutter = format!("{num:>digits$} {mark} ");
            let room = width.saturating_sub(display_width(&gutter));
            let text = text.replace('\t', "    ");
            out.push(Line::from(vec![
                Span::styled(gutter, theme::muted().patch(style)),
                Span::styled(truncate(&text, room), style),
            ]));
        }
        if self.rows.len() > limit {
            let more = format!("… {} more lines", self.rows.len() - limit);
            out.push(Line::from(Span::styled(more, theme::muted())));
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
