//! Scrollback flushing: which transcript lines are final and may be written above the viewport.
//!
//! Cells flush strictly in order. The head cell may flush partially when it streams: its prefix
//! once, then each stable body line by source offset, then the trailer when it seals (§5.1).

use ratatui::text::Line;

use super::render::{cell_parts, node_lines, separator};
use crate::model::{FlushState, Model, TRANSCRIPT};
use crate::proto::NodeId;
use crate::widgets::{Cell, Ctx};

/// Takes every line that became final since the last call, advancing flush state.
pub fn take_flushable(m: &mut Model, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let cells: Vec<NodeId> = m.children(TRANSCRIPT).to_vec();

    for id in cells {
        let state = m.flush_state(id);
        if state == Some(FlushState::Full) {
            continue;
        }
        let sealed = m.node(id).is_some_and(|n| n.sealed);
        let is_cell = m.comp_as::<Cell>(id).is_some();

        // a plain node in the transcript flushes whole, once sealed
        if !is_cell {
            if !sealed {
                break;
            }
            out.extend(separator(m, id));
            let mut sink = None;
            out.extend(node_lines(m, id, width, ctx, None, &mut sink));
            m.set_flush(id, FlushState::Full);
            continue;
        }

        let parts = cell_parts(m, id, width, ctx);
        match (state, sealed) {
            (None, true) => {
                out.extend(separator(m, id));
                out.extend(parts.all());
                m.set_flush(id, FlushState::Full);
            }
            (None, false) => {
                let Some((stable, _, end)) = parts.body else {
                    break;
                };
                if stable.is_empty() {
                    break;
                }
                out.extend(separator(m, id));
                out.extend(parts.prefix);
                out.extend(stable.into_iter().map(|l| l.line));
                m.set_flush(id, FlushState::Partial { src_off: end });
                break;
            }
            (Some(FlushState::Partial { src_off }), _) => {
                let (stable, pending, end) = parts.body.unwrap_or_default();
                let end = end.max(src_off);
                out.extend(
                    stable
                        .into_iter()
                        .filter(|l| l.src >= src_off)
                        .map(|l| l.line),
                );
                if sealed {
                    out.extend(pending);
                    out.extend(parts.trailer);
                    m.set_flush(id, FlushState::Full);
                } else {
                    m.set_flush(id, FlushState::Partial { src_off: end });
                    break;
                }
            }
            (Some(FlushState::Full), _) => unreachable!("full cells are skipped above"),
        }
    }
    out
}

/// Takes every remaining transcript line as if each cell had sealed, for shutdown.
pub fn flush_all(m: &mut Model, width: u16, ctx: &Ctx) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let cells: Vec<NodeId> = m.children(TRANSCRIPT).to_vec();

    for id in cells {
        match m.flush_state(id) {
            Some(FlushState::Full) => continue,
            Some(FlushState::Partial { src_off }) => {
                let parts = cell_parts(m, id, width, ctx);
                let (stable, pending, _) = parts.body.unwrap_or_default();
                out.extend(
                    stable
                        .into_iter()
                        .filter(|l| l.src >= src_off)
                        .map(|l| l.line),
                );
                out.extend(pending);
                out.extend(parts.trailer);
            }
            None => {
                out.extend(separator(m, id));
                let mut sink = None;
                out.extend(node_lines(m, id, width, ctx, None, &mut sink));
            }
        }
        m.set_flush(id, FlushState::Full);
    }
    out
}
