//! Patch readers: a present key overwrites the field, an explicit nil resets it to its default.

use crate::proto::value::{display, get, num};
use crate::proto::Value;

pub(crate) fn upd_str(p: &Value, key: &str, field: &mut String) {
    if let Some(v) = get(p, key) {
        *field = match v {
            Value::Nil => String::new(),
            other => display(other),
        };
    }
}

pub(crate) fn upd_opt_str(p: &Value, key: &str, field: &mut Option<String>) {
    if let Some(v) = get(p, key) {
        *field = match v {
            Value::Nil => None,
            other => Some(display(other)),
        };
    }
}

pub(crate) fn upd_bool(p: &Value, key: &str, field: &mut bool, default: bool) {
    if let Some(v) = get(p, key) {
        *field = v.as_bool().unwrap_or(default);
    }
}

pub(crate) fn upd_num(p: &Value, key: &str, field: &mut f64, default: f64) {
    if let Some(v) = get(p, key) {
        *field = num(v).unwrap_or(default);
    }
}

pub(crate) fn upd_opt_num(p: &Value, key: &str, field: &mut Option<f64>) {
    if let Some(v) = get(p, key) {
        *field = num(v);
    }
}

pub(crate) fn upd_usize(p: &Value, key: &str, field: &mut usize, default: usize) {
    if let Some(v) = get(p, key) {
        *field = num(v).filter(|n| *n >= 0.0).map_or(default, |n| n as usize);
    }
}

pub(crate) fn upd_strings(p: &Value, key: &str, field: &mut Vec<String>) {
    if let Some(v) = get(p, key) {
        *field = match v.as_array() {
            Some(items) => items.iter().map(display).collect(),
            None => Vec::new(),
        };
    }
}
