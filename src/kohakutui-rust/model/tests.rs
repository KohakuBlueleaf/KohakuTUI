//! Stage: model. Op sequences against the resulting tree and the seal and flush rules.

use super::{FlushState, Model, ModelError, LIVE, TRANSCRIPT};
use crate::proto::value::map;
use crate::proto::{NodeId, Op, Value};

fn create(id: NodeId, kind: &str, parent: NodeId, index: Option<usize>) -> Op {
    Op::Create { id, kind: kind.into(), props: Value::Map(vec![]), parent, index }
}

/// A transcript with cells 20, 21, 22, each holding a markdown child (30, 31, 32).
fn three_cells() -> Model {
    let mut m = Model::new();
    for i in 0..3 {
        m.apply(create(20 + i, "cell", TRANSCRIPT, None)).unwrap();
        m.apply(create(30 + i, "markdown", 20 + i, None)).unwrap();
    }
    m
}

#[test]
fn create_places_children_in_order_and_at_an_index() {
    let m = three_cells();
    assert_eq!(m.children(TRANSCRIPT), &[20, 21, 22]);

    let mut m = m;
    m.apply(create(23, "cell", TRANSCRIPT, Some(1))).unwrap();
    assert_eq!(m.children(TRANSCRIPT), &[20, 23, 21, 22]);
    assert_eq!(m.region_of(30), Some(TRANSCRIPT));
}

#[test]
fn create_rejects_reserved_duplicate_unknown_and_non_container_parents() {
    let mut m = three_cells();
    assert_eq!(m.apply(create(3, "text", LIVE, None)), Err(ModelError::Reserved(3)));
    assert_eq!(m.apply(create(20, "text", LIVE, None)), Err(ModelError::Duplicate(20)));
    assert_eq!(m.apply(create(40, "text", 99, None)), Err(ModelError::Unknown(99)));
    assert_eq!(m.apply(create(40, "text", 30, None)), Err(ModelError::NotContainer(30)));
    assert!(matches!(m.apply(create(40, "nope", LIVE, None)), Err(ModelError::Widget(_))));
    assert!(!m.contains(40), "a failed create leaves nothing behind");
}

#[test]
fn sealed_nodes_reject_changes_but_can_still_move_until_flushed() {
    let mut m = three_cells();
    m.apply(Op::Seal { id: 21 }).unwrap();
    assert!(m.node(31).unwrap().sealed, "seal reaches descendants");

    let append = Op::Append { id: 31, data: "x".into() };
    assert_eq!(m.apply(append), Err(ModelError::Sealed(31)));
    assert_eq!(m.apply(create(40, "text", 21, None)), Err(ModelError::Sealed(21)));
    let update = Op::Update { id: 21, props: map(vec![("title", "t".into())]) };
    assert_eq!(m.apply(update), Err(ModelError::Sealed(21)));

    m.apply(Op::Move { id: 21, parent: LIVE, index: None })
        .unwrap();
    assert_eq!(m.children(LIVE), &[21]);
    m.apply(Op::Remove { id: 21 }).unwrap();
    assert!(!m.contains(31), "remove drops the subtree");
}

#[test]
fn flushed_cells_freeze_themselves_and_their_positions() {
    let mut m = three_cells();
    m.set_flush(20, FlushState::Full);
    m.set_flush(21, FlushState::Partial { src_off: 10 });
    assert_eq!(m.frozen_len(), 2);

    assert_eq!(m.apply(Op::Remove { id: 21 }), Err(ModelError::Frozen(21)));
    assert_eq!(m.apply(Op::Remove { id: 31 }), Err(ModelError::Frozen(31)));
    let mv = Op::Move { id: 20, parent: LIVE, index: None };
    assert_eq!(m.apply(mv), Err(ModelError::Frozen(20)));
    for i in 0..=1 {
        let op = create(40, "cell", TRANSCRIPT, Some(i));
        assert_eq!(m.apply(op), Err(ModelError::Frozen(TRANSCRIPT)));
    }
    assert_eq!(
        m.apply(create(40, "text", 21, None)),
        Err(ModelError::Frozen(21)),
        "no new children inside a cell already in scrollback"
    );

    m.apply(create(40, "cell", TRANSCRIPT, Some(2))).unwrap();
    m.apply(Op::Move { id: 22, parent: LIVE, index: None })
        .unwrap();
    assert_eq!(m.children(TRANSCRIPT), &[20, 21, 40]);
}

#[test]
fn nodes_already_in_scrollback_reject_updates_but_take_appends() {
    let mut m = three_cells();
    m.set_flush(20, FlushState::Partial { src_off: 4 });

    let retitle = Op::Update { id: 20, props: map(vec![("title", "new".into())]) };
    assert_eq!(m.apply(retitle), Err(ModelError::Frozen(20)));
    let rewrite = Op::Update { id: 30, props: map(vec![("source", "replaced".into())]) };
    assert_eq!(m.apply(rewrite), Err(ModelError::Frozen(30)));

    let more = Op::Append { id: 30, data: "more".into() };
    assert!(m.apply(more).is_ok(), "streaming on continues");
}

#[test]
fn move_rejects_cycles_and_regions() {
    let mut m = three_cells();
    m.apply(create(40, "stack", LIVE, None)).unwrap();
    m.apply(create(41, "stack", 40, None)).unwrap();
    let cyc = Op::Move { id: 40, parent: 41, index: None };
    assert_eq!(m.apply(cyc), Err(ModelError::Cycle(40)));
    assert_eq!(m.apply(Op::Remove { id: LIVE }), Err(ModelError::Reserved(LIVE)));
}

#[test]
fn changes_bump_versions_up_to_the_region() {
    let mut m = three_cells();
    let before = (m.node(20).unwrap().version, m.node(TRANSCRIPT).unwrap().version);
    m.apply(Op::Append { id: 30, data: "hi".into() }).unwrap();
    assert!(m.node(20).unwrap().version > before.0);
    assert!(m.node(TRANSCRIPT).unwrap().version > before.1);
}
