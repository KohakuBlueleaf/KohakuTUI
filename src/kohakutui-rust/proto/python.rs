//! Conversion between Python objects and protocol values (design §4.5).

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use rmpv::Value;

/// Deepest nesting accepted; a self-containing list would otherwise overflow the stack.
const MAX_DEPTH: u32 = 64;

/// A Python object as a protocol value: None, bool, int, float (NaN kept), str, bytes,
/// list/tuple, str-keyed dict, and array-likes or scalars with `tolist` / `__index__` /
/// `__float__` (numpy, torch).
pub(crate) fn to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    to_value_at(obj, 0)
}

fn to_value_at(obj: &Bound<'_, PyAny>, depth: u32) -> PyResult<Value> {
    if depth > MAX_DEPTH {
        return Err(PyValueError::new_err("value nested too deeply for the UI"));
    }
    if obj.is_none() {
        return Ok(Value::Nil);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Boolean(b.is_true()));
    }
    if let Ok(i) = obj.cast::<PyInt>() {
        if let Ok(v) = i.extract::<i64>() {
            return Ok(Value::from(v));
        }
        return Ok(Value::from(i.extract::<u64>()?));
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return Ok(Value::F64(f.value()));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Value::from(s.to_cow()?.as_ref()));
    }
    if let Ok(b) = obj.cast::<PyBytes>() {
        return Ok(Value::Binary(b.as_bytes().to_vec()));
    }
    if let Ok(d) = obj.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(d.len());
        for (k, v) in d.iter() {
            let key = k
                .cast::<PyString>()
                .map_err(|_| PyTypeError::new_err("UI maps need str keys"))?
                .to_cow()?
                .into_owned();
            entries.push((Value::from(key), to_value_at(&v, depth + 1)?));
        }
        return Ok(Value::Map(entries));
    }
    if let Ok(l) = obj.cast::<PyList>() {
        let items = l.iter().map(|v| to_value_at(&v, depth + 1));
        return Ok(Value::Array(items.collect::<PyResult<_>>()?));
    }
    if let Ok(t) = obj.cast::<PyTuple>() {
        let items = t.iter().map(|v| to_value_at(&v, depth + 1));
        return Ok(Value::Array(items.collect::<PyResult<_>>()?));
    }
    for method in ["tolist", "__index__", "__float__"] {
        if obj.hasattr(method)? {
            return to_value_at(&obj.call_method0(method)?, depth + 1);
        }
    }
    let name = obj.get_type().name()?;
    Err(PyTypeError::new_err(format!("cannot send {name} to the UI")))
}

/// A protocol value as a Python object.
pub(crate) fn to_py(py: Python<'_>, v: &Value) -> PyResult<Py<PyAny>> {
    Ok(match v {
        Value::Nil => py.None(),
        Value::Boolean(b) => PyBool::new(py, *b).to_owned().into_any().unbind(),
        Value::Integer(i) => match (i.as_i64(), i.as_u64()) {
            (Some(n), _) => n.into_pyobject(py)?.into_any().unbind(),
            (None, Some(n)) => n.into_pyobject(py)?.into_any().unbind(),
            (None, None) => py.None(),
        },
        Value::F32(f) => PyFloat::new(py, f64::from(*f)).into_any().unbind(),
        Value::F64(f) => PyFloat::new(py, *f).into_any().unbind(),
        Value::String(s) => PyString::new(py, &String::from_utf8_lossy(s.as_bytes()))
            .into_any()
            .unbind(),
        Value::Binary(b) | Value::Ext(_, b) => PyBytes::new(py, b).into_any().unbind(),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
        Value::Map(entries) => {
            let dict = PyDict::new(py);
            for (k, val) in entries {
                dict.set_item(to_py(py, k)?, to_py(py, val)?)?;
            }
            dict.into_any().unbind()
        }
    })
}
