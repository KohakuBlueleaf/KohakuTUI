//! Tree storage and queries: lookup, children, ancestry, flush state and attach/detach.

use std::collections::{HashMap, HashSet};

use super::{FlushState, Model, Node, REGIONS, TRANSCRIPT};
use crate::proto::NodeId;
use crate::widgets::{Component, Stack};

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    /// A document holding only the five empty regions.
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        for id in REGIONS {
            nodes.insert(
                id,
                Node {
                    id,
                    parent: 0,
                    children: Vec::new(),
                    comp: Box::<Stack>::default(),
                    sealed: false,
                    version: 0,
                },
            );
        }
        Model { nodes, flush: HashMap::new(), open: HashSet::new() }
    }

    /// Whether the user toggled this foldable node (opened it, or closed it under expand-all).
    pub fn is_open(&self, id: NodeId) -> bool {
        self.open.contains(&id)
    }

    /// Drops every toggle (ctrl+o starts from expand-all alone).
    pub fn clear_open(&mut self) {
        for id in std::mem::take(&mut self.open) {
            self.bump(id);
        }
    }

    /// Flips the node's toggle; its version (and its ancestors') moves so caches redraw it.
    pub fn toggle_open(&mut self, id: NodeId) {
        if !self.nodes.contains_key(&id) {
            return;
        }
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
        self.bump(id);
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Children of `id`, empty for an unknown node.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes.get(&id).map_or(&[], |n| n.children.as_slice())
    }

    pub fn comp(&self, id: NodeId) -> Option<&dyn Component> {
        self.nodes.get(&id).map(|n| n.comp.as_ref())
    }

    /// Typed access to a node's component.
    pub fn comp_as<T: 'static>(&self, id: NodeId) -> Option<&T> {
        self.comp(id)?.as_any().downcast_ref::<T>()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() == REGIONS.len()
    }

    /// Whether `ancestor` is `id` or above it.
    pub fn is_within(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut cur = id;
        loop {
            if cur == ancestor {
                return true;
            }
            match self.nodes.get(&cur) {
                Some(n) if n.parent != 0 => cur = n.parent,
                _ => return false,
            }
        }
    }

    /// The region a node lives in, None for an unknown node.
    pub fn region_of(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = id;
        loop {
            let n = self.nodes.get(&cur)?;
            if n.parent == 0 {
                return Some(cur);
            }
            cur = n.parent;
        }
    }

    pub fn flush_state(&self, id: NodeId) -> Option<FlushState> {
        self.flush.get(&id).copied()
    }

    pub fn set_flush(&mut self, id: NodeId, state: FlushState) {
        self.flush.insert(id, state);
    }

    /// Whether `id` or an ancestor has any of it in scrollback.
    pub fn in_scrollback(&self, id: NodeId) -> bool {
        let mut cur = id;
        loop {
            if self.flush.contains_key(&cur) {
                return true;
            }
            match self.nodes.get(&cur) {
                Some(n) if n.parent != 0 => cur = n.parent,
                _ => return false,
            }
        }
    }

    /// Leading transcript cells with anything in scrollback: positions before this are frozen.
    pub fn frozen_len(&self) -> usize {
        self.children(TRANSCRIPT)
            .iter()
            .take_while(|id| self.flush.contains_key(id))
            .count()
    }

    /// Bumps the version of `id` and every ancestor, so render caches see the change.
    pub(crate) fn bump(&mut self, id: NodeId) {
        let mut cur = id;
        while let Some(n) = self.nodes.get_mut(&cur) {
            n.version += 1;
            if n.parent == 0 {
                break;
            }
            cur = n.parent;
        }
    }

    pub(crate) fn attach(&mut self, id: NodeId, parent: NodeId, index: Option<usize>) {
        if let Some(p) = self.nodes.get_mut(&parent) {
            let at = index.unwrap_or(p.children.len()).min(p.children.len());
            p.children.insert(at, id);
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.parent = parent;
        }
        self.bump(parent);
    }

    pub(crate) fn detach(&mut self, id: NodeId) {
        let Some(parent) = self.nodes.get(&id).map(|n| n.parent) else {
            return;
        };
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.retain(|c| *c != id);
        }
        self.bump(parent);
    }

    pub(crate) fn insert(&mut self, node: Node) {
        self.nodes.insert(node.id, node);
    }

    /// Drops a node and its whole subtree, flush state included.
    pub(crate) fn drop_subtree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            if let Some(n) = self.nodes.remove(&cur) {
                stack.extend(n.children);
            }
            self.flush.remove(&cur);
            self.open.remove(&cur);
        }
    }

    /// Every node under `id`, `id` first, in render order.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            out.push(cur);
            stack.extend(self.children(cur).iter().rev());
        }
        out
    }
}
