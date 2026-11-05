//! Applying tree ops, with the seal and flush rules of design §5.1.
//!
//! A sealed node takes no new children, updates, appends or focus. A cell with any of it in
//! scrollback takes only appends (its stream goes on): it cannot be updated, moved or removed,
//! and nothing can be created or moved into the transcript at or before it, so scrollback stays
//! append-only and in order.

use super::{Model, ModelError, Node, FIRST_FREE, OVERLAYS, REGIONS, TRANSCRIPT};
use crate::proto::{NodeId, Op};
use crate::widgets::build;

impl Model {
    /// Applies one tree op. Control ops (mode, bindings, notify) are the engine's.
    pub fn apply(&mut self, op: Op) -> Result<NodeId, ModelError> {
        match op {
            Op::Create { id, kind, props, parent, index } => {
                if id < FIRST_FREE {
                    return Err(ModelError::Reserved(id));
                }
                if self.contains(id) {
                    return Err(ModelError::Duplicate(id));
                }
                self.check_target(parent, index)?;

                let comp = build(&kind, &props)?;
                self.insert(Node {
                    id,
                    parent,
                    children: Vec::new(),
                    comp,
                    sealed: false,
                    version: 0,
                });
                self.attach(id, parent, index);
                Ok(id)
            }
            Op::Update { id, props } => {
                // a prop change could rewrite lines already printed above the viewport
                if self.in_scrollback(id) {
                    return Err(ModelError::Frozen(id));
                }
                self.open_node(id)?.comp.update(&props)?;
                self.bump(id);
                Ok(id)
            }
            Op::Append { id, data } => {
                self.open_node(id)?.comp.append(&data)?;
                self.bump(id);
                Ok(id)
            }
            Op::Remove { id } => {
                self.check_movable(id)?;
                self.detach(id);
                self.drop_subtree(id);
                Ok(id)
            }
            Op::Move { id, parent, index } => {
                self.check_movable(id)?;
                if self.is_within(parent, id) {
                    return Err(ModelError::Cycle(id));
                }
                self.check_target(parent, index)?;
                self.detach(id);
                self.attach(id, parent, index);
                Ok(id)
            }
            Op::Seal { id } => {
                if !self.contains(id) {
                    return Err(ModelError::Unknown(id));
                }
                if REGIONS.contains(&id) {
                    return Err(ModelError::Reserved(id));
                }
                for n in self.subtree(id) {
                    if let Some(node) = self.node_mut(n) {
                        if !node.sealed {
                            node.sealed = true;
                            node.comp.seal();
                        }
                    }
                }
                self.bump(id);
                Ok(id)
            }
            Op::Focus { .. } => Err(ModelError::NotTreeOp("focus")),
            Op::SetMode { .. } => Err(ModelError::NotTreeOp("set_mode")),
            Op::Bind { .. } | Op::Unbind { .. } => Err(ModelError::NotTreeOp("bind")),
            Op::Notify { .. } => Err(ModelError::NotTreeOp("notify")),
            Op::Bell => Err(ModelError::NotTreeOp("bell")),
            Op::Copy { .. } => Err(ModelError::NotTreeOp("copy")),
            Op::SetLinks { .. } => Err(ModelError::NotTreeOp("set_links")),
            Op::SetTheme { .. } => Err(ModelError::NotTreeOp("set_theme")),
        }
    }

    /// The node, if it exists and is not sealed.
    fn open_node(&mut self, id: NodeId) -> Result<&mut Node, ModelError> {
        let node = self.node_mut(id).ok_or(ModelError::Unknown(id))?;
        if node.sealed {
            return Err(ModelError::Sealed(id));
        }
        Ok(node)
    }

    /// Whether a child may be placed under `parent` at `index`.
    fn check_target(&self, parent: NodeId, index: Option<usize>) -> Result<(), ModelError> {
        let p = self.node(parent).ok_or(ModelError::Unknown(parent))?;
        if !p.comp.container() {
            return Err(ModelError::NotContainer(parent));
        }
        if p.sealed {
            return Err(ModelError::Sealed(parent));
        }
        if self.in_scrollback(parent) {
            return Err(ModelError::Frozen(parent));
        }

        // a transcript insert must land after every cell already (partly) in scrollback
        if parent == TRANSCRIPT {
            if let Some(i) = index {
                if i < self.frozen_len() {
                    return Err(ModelError::Frozen(parent));
                }
            }
        }
        Ok(())
    }

    /// Whether `id` may be moved or removed.
    fn check_movable(&self, id: NodeId) -> Result<(), ModelError> {
        if REGIONS.contains(&id) {
            return Err(ModelError::Reserved(id));
        }
        if !self.contains(id) {
            return Err(ModelError::Unknown(id));
        }
        if self.in_scrollback(id) {
            return Err(ModelError::Frozen(id));
        }
        Ok(())
    }

    /// The overlay that takes input: the last child of the overlay region.
    pub fn top_overlay(&self) -> Option<NodeId> {
        self.children(OVERLAYS).last().copied()
    }
}
