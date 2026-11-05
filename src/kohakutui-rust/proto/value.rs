//! Readers and builders for the msgpack value maps that props, append data and events use.

use rmpv::Value;

/// The entry `key` of a map value.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, val)| val)
}

/// A number of any msgpack width as f64 (NaN and infinities survive).
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => i.as_f64(),
        Value::F32(f) => Some(f64::from(*f)),
        Value::F64(f) => Some(*f),
        _ => None,
    }
}

/// The entry `key` as a string.
pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    get(v, key)?.as_str()
}

/// The entry `key` as a number.
pub fn num_of(v: &Value, key: &str) -> Option<f64> {
    get(v, key).and_then(num)
}

/// The entry `key` as a bool.
pub fn bool_of(v: &Value, key: &str) -> Option<bool> {
    get(v, key)?.as_bool()
}

/// Whether the map has `key`, including an explicit nil.
pub fn has(v: &Value, key: &str) -> bool {
    get(v, key).is_some()
}

/// A string-keyed map value.
pub fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (Value::from(k), v))
            .collect(),
    )
}

/// Shallow merge of `patch` into `base`; a key set to nil is removed.
pub fn merge(base: &mut Value, patch: &Value) {
    let Some(patch) = patch.as_map() else { return };
    if base.as_map().is_none() {
        *base = Value::Map(Vec::new());
    }
    let Value::Map(entries) = base else { return };
    for (k, v) in patch {
        let pos = entries.iter().position(|(bk, _)| bk == k);
        match (pos, v.is_nil()) {
            (Some(i), true) => {
                entries.remove(i);
            }
            (Some(i), false) => entries[i].1 = v.clone(),
            (None, true) => {}
            (None, false) => entries.push((k.clone(), v.clone())),
        }
    }
}

/// Plain text of a value, for labels built from arbitrary data.
pub fn display(v: &Value) -> String {
    match v {
        Value::Nil => String::new(),
        Value::String(s) => s.as_str().unwrap_or_default().to_owned(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::F32(f) => trim_float(f64::from(*f)),
        Value::F64(f) => trim_float(*f),
        Value::Array(items) => items.iter().map(display).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn trim_float(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}
