//! Stage: proto. Ops and events against their value maps and the msgpack bytes below them.

use rmpv::Value;

use super::value::{get, map, merge, num};
use super::{Event, Level, Mode, Op, ProtoError};

fn all_ops() -> Vec<Op> {
    vec![
        Op::Create {
            id: 42,
            kind: "markdown".into(),
            props: map(vec![("streaming", true.into())]),
            parent: 1,
            index: Some(3),
        },
        Op::Update { id: 42, props: map(vec![("title", "x".into())]) },
        Op::Append { id: 42, data: "chunk".into() },
        Op::Remove { id: 42 },
        Op::Move { id: 42, parent: 2, index: None },
        Op::Seal { id: 42 },
        Op::Focus { id: 7 },
        Op::SetMode { mode: Mode::Fullscreen },
        Op::Bind { chord: "ctrl+k".into(), action: "palette".into() },
        Op::Unbind { chord: "ctrl+k".into() },
        Op::Notify { text: "saved".into(), level: Level::Success, ttl_ms: 1500 },
        Op::Bell,
        Op::SetTheme {
            scheme: "tokyo-night".into(),
            overrides: vec![("accent".into(), "#ff8800".into())],
            truecolor: Some(false),
        },
    ]
}

fn msgpack_round_trip(v: &Value) -> Value {
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, v).unwrap();
    rmpv::decode::read_value(&mut bytes.as_slice()).unwrap()
}

#[test]
fn every_op_survives_value_and_msgpack_round_trip() {
    for op in all_ops() {
        let back = Op::from_value(&msgpack_round_trip(&op.to_value())).unwrap();
        assert_eq!(back, op);
    }
}

#[test]
fn missing_and_mistyped_fields_are_named() {
    let no_id = map(vec![("op", "seal".into())]);
    assert_eq!(Op::from_value(&no_id), Err(ProtoError::Missing("id")));
    let str_id = map(vec![("op", "seal".into()), ("id", "seven".into())]);
    assert_eq!(Op::from_value(&str_id), Err(ProtoError::Type("id")));
    let no_op = map(vec![("id", 1u64.into())]);
    assert_eq!(Op::from_value(&no_op), Err(ProtoError::Missing("op")));
    assert_eq!(Op::from_value(&Value::from(3)), Err(ProtoError::NotMap));
}

#[test]
fn unknown_op_and_bad_enum_values_are_rejected() {
    let unknown = map(vec![("op", "explode".into())]);
    assert_eq!(Op::from_value(&unknown), Err(ProtoError::UnknownOp("explode".into())));
    let bad_mode = map(vec![("op", "set_mode".into()), ("mode", "tiny".into())]);
    assert!(matches!(
        Op::from_value(&bad_mode),
        Err(ProtoError::Invalid { field: "mode", .. })
    ));
}

#[test]
fn create_without_props_gets_an_empty_map_and_hint_reads_the_id() {
    let v = map(vec![
        ("op", "create".into()),
        ("id", 20u64.into()),
        ("kind", "text".into()),
        ("parent", 1u64.into()),
    ]);
    match Op::from_value(&v).unwrap() {
        Op::Create { props, index, .. } => {
            assert_eq!(props, Value::Map(vec![]));
            assert_eq!(index, None);
        }
        other => panic!("decoded {other:?}"),
    }
    assert_eq!(Op::id_hint(&v), Some(20));
}

#[test]
fn nan_and_infinity_survive_msgpack() {
    let v = Value::Array(vec![Value::F64(f64::NAN), Value::F64(f64::INFINITY)]);
    let back = msgpack_round_trip(&v);
    let items = back.as_array().unwrap();
    assert!(num(&items[0]).unwrap().is_nan());
    assert_eq!(num(&items[1]), Some(f64::INFINITY));
}

#[test]
fn events_encode_their_kind_and_fields() {
    let ev = Event::Select { id: 9, index: 2, value: "gpt".into() };
    let v = ev.to_value();
    assert_eq!(get(&v, "ev").and_then(Value::as_str), Some("select"));
    assert_eq!(get(&v, "id").and_then(Value::as_u64), Some(9));
    assert_eq!(get(&v, "index").and_then(Value::as_u64), Some(2));
    let err = Event::Error { id: None, message: "bad".into() }.to_value();
    assert_eq!(get(&err, "id"), Some(&Value::Nil));
}

#[test]
fn merge_overwrites_adds_and_nil_removes() {
    let mut base = map(vec![("a", 1.into()), ("b", 2.into())]);
    merge(&mut base, &map(vec![("a", 10.into()), ("b", Value::Nil), ("c", 3.into())]));
    assert_eq!(get(&base, "a"), Some(&Value::from(10)));
    assert_eq!(get(&base, "b"), None);
    assert_eq!(get(&base, "c"), Some(&Value::from(3)));
}
