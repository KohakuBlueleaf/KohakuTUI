//! Op and Event encoding to msgpack value maps, the shapes Python and peers receive.

use rmpv::Value;

use super::value::map;
use super::{Event, Level, Mode, Op};

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Inline => "inline",
            Mode::Fullscreen => "fullscreen",
        }
    }
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Success => "success",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

fn index(i: Option<usize>) -> Value {
    i.map_or(Value::Nil, |i| Value::from(i as u64))
}

impl Op {
    /// The value form `from_value` reads back.
    pub fn to_value(&self) -> Value {
        match self {
            Op::Create { id, kind, props, parent, index: i } => map(vec![
                ("op", "create".into()),
                ("id", (*id).into()),
                ("kind", kind.as_str().into()),
                ("props", props.clone()),
                ("parent", (*parent).into()),
                ("index", index(*i)),
            ]),
            Op::Update { id, props } => map(vec![
                ("op", "update".into()),
                ("id", (*id).into()),
                ("props", props.clone()),
            ]),
            Op::Append { id, data } => map(vec![
                ("op", "append".into()),
                ("id", (*id).into()),
                ("data", data.clone()),
            ]),
            Op::Remove { id } => map(vec![("op", "remove".into()), ("id", (*id).into())]),
            Op::Move { id, parent, index: i } => map(vec![
                ("op", "move".into()),
                ("id", (*id).into()),
                ("parent", (*parent).into()),
                ("index", index(*i)),
            ]),
            Op::Seal { id } => map(vec![("op", "seal".into()), ("id", (*id).into())]),
            Op::Focus { id } => map(vec![("op", "focus".into()), ("id", (*id).into())]),
            Op::SetMode { mode } => {
                map(vec![("op", "set_mode".into()), ("mode", mode.as_str().into())])
            }
            Op::Bind { chord, action } => map(vec![
                ("op", "bind".into()),
                ("chord", chord.as_str().into()),
                ("action", action.as_str().into()),
            ]),
            Op::Unbind { chord } => {
                map(vec![("op", "unbind".into()), ("chord", chord.as_str().into())])
            }
            Op::Notify { text, level, ttl_ms } => map(vec![
                ("op", "notify".into()),
                ("text", text.as_str().into()),
                ("level", level.as_str().into()),
                ("ttl_ms", (*ttl_ms).into()),
            ]),
            Op::Bell => map(vec![("op", "bell".into())]),
            Op::Copy { text } => map(vec![("op", "copy".into()), ("text", text.as_str().into())]),
            Op::SetLinks { osc8 } => {
                map(vec![("op", "set_links".into()), ("osc8", (*osc8).into())])
            }
            Op::SetTheme { scheme, overrides, truecolor } => map(vec![
                ("op", "set_theme".into()),
                ("scheme", scheme.as_str().into()),
                (
                    "overrides",
                    Value::Map(
                        overrides
                            .iter()
                            .map(|(k, v)| (k.as_str().into(), v.as_str().into()))
                            .collect(),
                    ),
                ),
                ("truecolor", truecolor.map_or(Value::Nil, Value::Boolean)),
            ]),
        }
    }
}

impl Event {
    /// The map Python receives: `{"ev": <kind>, ...fields}`.
    pub fn to_value(&self) -> Value {
        match self {
            Event::Ready { cols, rows, tty } => map(vec![
                ("ev", "ready".into()),
                ("cols", (*cols).into()),
                ("rows", (*rows).into()),
                ("tty", (*tty).into()),
            ]),
            Event::Resize { cols, rows } => map(vec![
                ("ev", "resize".into()),
                ("cols", (*cols).into()),
                ("rows", (*rows).into()),
            ]),
            Event::Submit { id, value } => map(vec![
                ("ev", "submit".into()),
                ("id", (*id).into()),
                ("value", value.clone()),
            ]),
            Event::Change { id, value } => map(vec![
                ("ev", "change".into()),
                ("id", (*id).into()),
                ("value", value.clone()),
            ]),
            Event::Select { id, index, value } => map(vec![
                ("ev", "select".into()),
                ("id", (*id).into()),
                ("index", (*index).into()),
                ("value", value.clone()),
            ]),
            Event::Close { id, reason } => map(vec![
                ("ev", "close".into()),
                ("id", (*id).into()),
                ("reason", reason.as_str().into()),
            ]),
            Event::Action { name, focus } => map(vec![
                ("ev", "action".into()),
                ("name", name.as_str().into()),
                ("focus", focus.map_or(Value::Nil, Value::from)),
            ]),
            Event::Interrupt { count } => {
                map(vec![("ev", "interrupt".into()), ("count", (*count).into())])
            }
            Event::ModeChanged { mode } => map(vec![
                ("ev", "mode_changed".into()),
                ("mode", mode.as_str().into()),
            ]),
            Event::Copy { text } => {
                map(vec![("ev", "copy".into()), ("text", text.as_str().into())])
            }
            Event::Link { url } => map(vec![("ev", "link".into()), ("url", url.as_str().into())]),
            Event::Error { id, message } => map(vec![
                ("ev", "error".into()),
                ("id", id.map_or(Value::Nil, Value::from)),
                ("message", message.as_str().into()),
            ]),
            Event::Stopped => map(vec![("ev", "stopped".into())]),
        }
    }
}
