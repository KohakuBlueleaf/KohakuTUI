//! Op decoding from a msgpack value map, one op at a time.

use rmpv::Value;

use super::value::{get, num};
use super::{Level, Mode, NodeId, Op, ProtoError};

impl Op {
    /// Decodes one op; the error names the field that is wrong.
    pub fn from_value(v: &Value) -> Result<Op, ProtoError> {
        if v.as_map().is_none() {
            return Err(ProtoError::NotMap);
        }
        let op = req_str(v, "op")?;
        Ok(match op {
            "create" => Op::Create {
                id: req_id(v, "id")?,
                kind: req_str(v, "kind")?.to_owned(),
                props: props(v, "props"),
                parent: req_id(v, "parent")?,
                index: opt_index(v, "index")?,
            },
            "update" => Op::Update { id: req_id(v, "id")?, props: props(v, "props") },
            "append" => Op::Append {
                id: req_id(v, "id")?,
                data: get(v, "data").cloned().unwrap_or(Value::Nil),
            },
            "remove" => Op::Remove { id: req_id(v, "id")? },
            "move" => Op::Move {
                id: req_id(v, "id")?,
                parent: req_id(v, "parent")?,
                index: opt_index(v, "index")?,
            },
            "seal" => Op::Seal { id: req_id(v, "id")? },
            "focus" => Op::Focus { id: req_id(v, "id")? },
            "set_mode" => Op::SetMode { mode: Mode::parse(req_str(v, "mode")?)? },
            "bind" => Op::Bind {
                chord: req_str(v, "chord")?.to_owned(),
                action: req_str(v, "action")?.to_owned(),
            },
            "unbind" => Op::Unbind { chord: req_str(v, "chord")?.to_owned() },
            "notify" => Op::Notify {
                text: req_str(v, "text")?.to_owned(),
                level: match get(v, "level").and_then(Value::as_str) {
                    Some(level) => Level::parse(level)?,
                    None => Level::Info,
                },
                ttl_ms: match get(v, "ttl_ms") {
                    None | Some(Value::Nil) => 3000,
                    Some(t) => num(t).ok_or(ProtoError::Type("ttl_ms"))?.clamp(0.0, 3.6e6) as u32,
                },
            },
            "bell" => Op::Bell,
            "copy" => Op::Copy { text: req_str(v, "text")?.to_owned() },
            "set_links" => {
                Op::SetLinks { osc8: get(v, "osc8").and_then(Value::as_bool).unwrap_or(true) }
            }
            "set_theme" => Op::SetTheme {
                scheme: req_str(v, "scheme")?.to_owned(),
                overrides: match get(v, "overrides") {
                    None | Some(Value::Nil) => Vec::new(),
                    Some(Value::Map(entries)) => entries
                        .iter()
                        .map(|(k, val)| match (k.as_str(), val.as_str()) {
                            (Some(k), Some(val)) => Ok((k.to_owned(), val.to_owned())),
                            _ => Err(ProtoError::Type("overrides")),
                        })
                        .collect::<Result<_, _>>()?,
                    Some(_) => return Err(ProtoError::Type("overrides")),
                },
                truecolor: match get(v, "truecolor") {
                    None | Some(Value::Nil) => None,
                    Some(b) => Some(b.as_bool().ok_or(ProtoError::Type("truecolor"))?),
                },
            },
            other => return Err(ProtoError::UnknownOp(other.to_owned())),
        })
    }

    /// The node an op targets, for error reports on ops that fail to decode.
    pub fn id_hint(v: &Value) -> Option<NodeId> {
        get(v, "id")?.as_u64()
    }
}

impl Mode {
    pub fn parse(s: &str) -> Result<Mode, ProtoError> {
        match s {
            "inline" => Ok(Mode::Inline),
            "fullscreen" => Ok(Mode::Fullscreen),
            other => Err(ProtoError::Invalid { field: "mode", value: other.to_owned() }),
        }
    }
}

impl Level {
    pub fn parse(s: &str) -> Result<Level, ProtoError> {
        match s {
            "info" => Ok(Level::Info),
            "success" => Ok(Level::Success),
            "warn" | "warning" => Ok(Level::Warn),
            "error" => Ok(Level::Error),
            other => Err(ProtoError::Invalid { field: "level", value: other.to_owned() }),
        }
    }
}

fn req_str<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, ProtoError> {
    get(v, key)
        .ok_or(ProtoError::Missing(key))?
        .as_str()
        .ok_or(ProtoError::Type(key))
}

fn req_id(v: &Value, key: &'static str) -> Result<NodeId, ProtoError> {
    get(v, key)
        .ok_or(ProtoError::Missing(key))?
        .as_u64()
        .ok_or(ProtoError::Type(key))
}

fn opt_index(v: &Value, key: &'static str) -> Result<Option<usize>, ProtoError> {
    match get(v, key) {
        None | Some(Value::Nil) => Ok(None),
        Some(i) => i
            .as_u64()
            .map(|i| Some(i as usize))
            .ok_or(ProtoError::Type(key)),
    }
}

fn props(v: &Value, key: &str) -> Value {
    match get(v, key) {
        Some(p) if p.as_map().is_some() => p.clone(),
        _ => Value::Map(Vec::new()),
    }
}
