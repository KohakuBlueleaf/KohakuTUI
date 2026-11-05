//! Data components: `kv` (aligned fields), `table` and `footer`.

use std::any::Any;

use ratatui::text::{Line, Span};

use super::props::{upd_str, upd_usize};
use super::text::truncate;
use super::{Component, Ctx, WidgetError};
use crate::proto::value::{display, get, str_of};
use crate::proto::Value;
use crate::text::{display_width, theme, wrap_spans};

/// Label/value rows with the labels aligned.
#[derive(Debug, Default)]
pub struct Kv {
    fields: Vec<(String, String)>,
}

impl Component for Kv {
    fn kind(&self) -> &'static str {
        "kv"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        match get(p, "fields") {
            Some(Value::Array(items)) => {
                self.fields = items
                    .iter()
                    .map(|f| {
                        let label = str_of(f, "label").unwrap_or_default().to_owned();
                        let value = get(f, "value").map(display).unwrap_or_default();
                        (label, value)
                    })
                    .collect();
            }
            Some(Value::Map(entries)) => {
                self.fields = entries
                    .iter()
                    .map(|(k, v)| (display(k), display(v)))
                    .collect();
            }
            Some(_) => self.fields.clear(),
            None => {}
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let lw = self
            .fields
            .iter()
            .map(|(l, _)| display_width(l))
            .max()
            .unwrap_or(0)
            .min(width as usize / 2);
        let mut out = Vec::new();
        for (label, value) in &self.fields {
            let pad = lw.saturating_sub(display_width(label));
            let head = format!("{}{}  ", truncate(label, lw), " ".repeat(pad));
            let avail = (width as usize).saturating_sub(lw + 2).max(1);
            for (i, line) in wrap_spans(&[Span::raw(value.clone())], avail)
                .into_iter()
                .enumerate()
            {
                let lead = if i == 0 {
                    Span::styled(head.clone(), theme::muted())
                } else {
                    Span::raw(" ".repeat(lw + 2))
                };
                let mut spans = vec![lead];
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Rows under a header; rows are arrays or maps keyed by column key.
#[derive(Debug, Default)]
pub struct Table {
    columns: Vec<(String, String)>,
    rows: Vec<Vec<String>>,
    max_rows: usize,
}

impl Table {
    fn row_of(&self, row: &Value) -> Vec<String> {
        match row {
            Value::Array(cells) => cells.iter().map(display).collect(),
            Value::Map(_) => self
                .columns
                .iter()
                .map(|(key, _)| get(row, key).map(display).unwrap_or_default())
                .collect(),
            other => vec![display(other)],
        }
    }
}

impl Component for Table {
    fn kind(&self) -> &'static str {
        "table"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        if let Some(cols) = get(p, "columns") {
            self.columns = cols
                .as_array()
                .map(|cols| {
                    cols.iter()
                        .map(|c| match c {
                            Value::Map(_) => {
                                let key = str_of(c, "key").unwrap_or_default().to_owned();
                                let label = str_of(c, "label").map_or(key.clone(), str::to_owned);
                                (key, label)
                            }
                            other => (display(other), display(other)),
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        if let Some(rows) = get(p, "rows") {
            let rows: Vec<Value> = rows.as_array().cloned().unwrap_or_default();
            self.rows = rows.iter().map(|r| self.row_of(r)).collect();
        }
        upd_usize(p, "max_rows", &mut self.max_rows, 0);
        Ok(())
    }

    fn append(&mut self, data: &Value) -> Result<(), WidgetError> {
        match data {
            Value::Array(items) if items.iter().all(|i| i.is_array() || i.is_map()) => {
                let rows: Vec<Vec<String>> = items.iter().map(|r| self.row_of(r)).collect();
                self.rows.extend(rows);
            }
            row => {
                let row = self.row_of(row);
                self.rows.push(row);
            }
        }
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let ncols = self
            .columns
            .len()
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return Vec::new();
        }
        let header: Vec<String> = (0..ncols)
            .map(|i| self.columns.get(i).map(|c| c.1.clone()).unwrap_or_default())
            .collect();
        let skip = match self.max_rows {
            0 => 0,
            n => self.rows.len().saturating_sub(n),
        };
        let rows = &self.rows[skip..];
        let mut widths: Vec<usize> = header.iter().map(|h| display_width(h).max(1)).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate().take(ncols) {
                widths[i] = widths[i].max(display_width(cell));
            }
        }
        let budget = (width as usize).saturating_sub(3 * (ncols - 1)).max(ncols);
        while widths.iter().sum::<usize>() > budget {
            let (i, _) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if widths[i] <= 3 {
                break;
            }
            widths[i] -= 1;
        }
        let render = |cells: &[String], style| {
            let mut spans = Vec::new();
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", theme::muted()));
                }
                let cell = truncate(cells.get(i).map(String::as_str).unwrap_or(""), *w);
                let pad = w.saturating_sub(display_width(&cell));
                spans.push(Span::styled(format!("{cell}{}", " ".repeat(pad)), style));
            }
            Line::from(spans)
        };
        let mut out = vec![render(&header, theme::bold())];
        let sep = widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("─┼─");
        out.push(Line::from(Span::styled(sep, theme::muted())));
        if skip > 0 {
            out.push(Line::from(Span::styled(format!("… {skip} earlier rows"), theme::muted())));
        }
        for row in rows {
            out.push(render(row, Default::default()));
        }
        out
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One status line: left text, right text aligned to the edge.
#[derive(Debug, Default)]
pub struct Footer {
    left: String,
    right: String,
}

impl Component for Footer {
    fn kind(&self) -> &'static str {
        "footer"
    }

    fn update(&mut self, p: &Value) -> Result<(), WidgetError> {
        upd_str(p, "left", &mut self.left);
        upd_str(p, "right", &mut self.right);
        Ok(())
    }

    fn lines(&self, width: u16, _ctx: &Ctx) -> Vec<Line<'static>> {
        let width = width as usize;
        let right = truncate(&self.right, width / 2);
        let rw = display_width(&right);
        let left = truncate(&self.left, width.saturating_sub(rw + 1));
        let gap = width.saturating_sub(display_width(&left) + rw);
        vec![Line::from(vec![
            Span::styled(left, theme::muted()),
            Span::raw(" ".repeat(gap)),
            Span::styled(right, theme::muted()),
        ])]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
