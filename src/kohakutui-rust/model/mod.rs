//! Retained document: the five regions, the node tree, seal state and scrollback flush state
//! (design §5). The model enforces the invariants that keep scrollback append-only.

mod apply;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::HashMap;

use thiserror::Error;

use crate::proto::{NodeId, ProtoError};
use crate::widgets::{Component, WidgetError};

/// Finished output, written to scrollback in inline mode.
pub const TRANSCRIPT: NodeId = 1;
/// Spinners, progress and docked widgets under the open transcript cells.
pub const LIVE: NodeId = 2;
/// Composer and footer, always at the bottom.
pub const BOTTOM: NodeId = 3;
/// Side panes in full-screen mode.
pub const PANELS: NodeId = 4;
/// Modal stack; the last child takes input.
pub const OVERLAYS: NodeId = 5;
/// Region ids, in layout order.
pub const REGIONS: [NodeId; 5] = [TRANSCRIPT, LIVE, BOTTOM, PANELS, OVERLAYS];
/// Ids below this are reserved for the engine.
pub const FIRST_FREE: NodeId = 16;

/// One node of the document.
pub struct Node {
    pub id: NodeId,
    pub parent: NodeId,
    pub children: Vec<NodeId>,
    pub comp: Box<dyn Component>,
    pub sealed: bool,
    /// Bumped on every change to the node or a descendant.
    pub version: u64,
}

/// How much of a transcript cell is in scrollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushState {
    /// The header and the streaming body up to source byte `src_off` are in scrollback.
    Partial { src_off: usize },
    /// The whole cell is in scrollback.
    Full,
}

/// The node tree plus per-cell flush state.
pub struct Model {
    nodes: HashMap<NodeId, Node>,
    flush: HashMap<NodeId, FlushState>,
}

/// An op the model refuses; the engine reports it as `Event::Error`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    #[error("no node {0}")]
    Unknown(NodeId),
    #[error("node {0} already exists")]
    Duplicate(NodeId),
    #[error("node {0} is reserved")]
    Reserved(NodeId),
    #[error("node {0} is sealed")]
    Sealed(NodeId),
    #[error("node {0} cannot hold children")]
    NotContainer(NodeId),
    #[error("node {0} or its position is already in scrollback")]
    Frozen(NodeId),
    #[error("node {0} cannot move under itself")]
    Cycle(NodeId),
    #[error("{0} is not a tree op")]
    NotTreeOp(&'static str),
    #[error(transparent)]
    Widget(#[from] WidgetError),
    #[error(transparent)]
    Proto(#[from] ProtoError),
}
