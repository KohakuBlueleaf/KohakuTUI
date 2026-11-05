//! pyo3 bindings: `kohakutui._rs._Session`, wrapped by `kohakutui.App` (design §8).
//!
//! The class is frozen and keeps its session behind a lock it never holds while waiting, so a
//! `poll` blocked on the pump thread never conflicts with `stop` on the main thread.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::{Config, InputEv, Interrupt, RuntimeError, Session};
use crate::proto::python::{to_py, to_value};
use crate::proto::Mode;
use crate::term::{reset_terminal, Backend, TestScreen};
use crate::view::Chord;
use crate::widgets::Input;

impl From<RuntimeError> for PyErr {
    fn from(e: RuntimeError) -> PyErr {
        PyRuntimeError::new_err(e.to_string())
    }
}

/// One engine: start it, send it op dicts, poll its event dicts, stop it.
#[pyclass(module = "kohakutui._rs", name = "_Session", frozen)]
pub struct PySession {
    cfg: Mutex<Option<Config>>,
    session: Mutex<Option<Arc<Session>>>,
    screen: Option<TestScreen>,
}

impl PySession {
    fn running(&self) -> PyResult<Arc<Session>> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("the UI is not running"))
    }

    fn test_screen(&self) -> PyResult<&TestScreen> {
        self.screen
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("only a backend=\"test\" session has a screen"))
    }
}

#[pymethods]
impl PySession {
    #[new]
    #[pyo3(signature = (backend="auto", mode="inline", size=None, interrupt="event", max_fps=60, links="auto"))]
    fn new(
        backend: &str,
        mode: &str,
        size: Option<(u16, u16)>,
        interrupt: &str,
        max_fps: u32,
        links: &str,
    ) -> PyResult<Self> {
        let links = match links {
            "auto" | "osc8" => true,
            "text" => false,
            other => return Err(PyValueError::new_err(format!("unknown links {other:?}"))),
        };
        let screen = (backend == "test").then(|| {
            let (cols, rows) = size.unwrap_or((80, 24));
            TestScreen::new(cols, rows)
        });
        let backend = match (backend, &screen) {
            ("auto", _) => Backend::Auto,
            ("tty", _) => Backend::Tty,
            ("plain", _) => Backend::Plain,
            ("test", Some(s)) => Backend::Test(s.clone()),
            (other, _) => return Err(PyValueError::new_err(format!("unknown backend {other:?}"))),
        };
        let mode = Mode::parse(mode).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let interrupt = match interrupt {
            "event" | "raise" => Interrupt::Event,
            "signal" => Interrupt::Signal,
            "exit" => Interrupt::Exit,
            other => return Err(PyValueError::new_err(format!("unknown interrupt {other:?}"))),
        };
        let cfg = Config { backend, mode, interrupt, max_fps, links };
        Ok(PySession { cfg: Mutex::new(Some(cfg)), session: Mutex::new(None), screen })
    }

    /// Opens the terminal and starts the engine thread. A session starts once.
    fn start(&self, py: Python<'_>) -> PyResult<()> {
        let cfg = self
            .cfg
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("the session was already started"))?;
        let session = py.detach(move || Session::start(cfg))?;
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(session));
        Ok(())
    }

    /// Sends op dicts; each is decoded on the engine thread, a bad one becomes an error event.
    fn apply(&self, ops: &Bound<'_, PyList>) -> PyResult<()> {
        let values = ops
            .iter()
            .map(|o| to_value(&o))
            .collect::<PyResult<Vec<_>>>()?;
        self.running()?.apply(values)?;
        Ok(())
    }

    /// Waits for event dicts (None: forever), without holding the GIL.
    #[pyo3(signature = (timeout=None, max=256))]
    fn poll(&self, py: Python<'_>, timeout: Option<f64>, max: usize) -> PyResult<Vec<Py<PyAny>>> {
        let session = self.running()?;
        let timeout = timeout.map(|t| Duration::from_secs_f64(t.max(0.0)));
        let events = py.detach(move || session.poll(timeout, max));
        events.iter().map(|e| to_py(py, &e.to_value())).collect()
    }

    /// Makes a blocked `poll` return an empty list now.
    fn wake(&self) {
        if let Ok(s) = self.running() {
            s.wake();
        }
    }

    /// Waits until every op sent so far is applied and drawn; False on timeout.
    #[pyo3(signature = (timeout=2.0))]
    fn sync(&self, py: Python<'_>, timeout: f64) -> PyResult<bool> {
        let session = self.running()?;
        let timeout = Duration::from_secs_f64(timeout.max(0.0));
        Ok(py.detach(move || session.sync(timeout)))
    }

    /// Flushes the transcript, restores the terminal and ends the engine. Idempotent.
    fn stop(&self, py: Python<'_>) {
        let session = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(s) = session {
            py.detach(move || s.stop());
        }
    }

    #[getter(running)]
    fn is_running(&self) -> bool {
        self.running().is_ok_and(|s| s.is_running())
    }

    /// The test terminal's visible rows.
    fn screen(&self) -> PyResult<Vec<String>> {
        Ok(self.test_screen()?.screen())
    }

    /// The test terminal's visible rows as style runs: per row a list of
    /// `(text, fg, bg, flags)`, colours None (default), an index string or `#rrggbb`, flags a
    /// set of "bold", "dim", "italic", "underline", "inverse".
    #[allow(clippy::type_complexity)]
    fn screen_cells(
        &self,
    ) -> PyResult<Vec<Vec<(String, Option<String>, Option<String>, Vec<&'static str>)>>> {
        let rows = self.test_screen()?.cells();
        Ok(rows
            .into_iter()
            .map(|runs| {
                runs.into_iter()
                    .map(|run| {
                        let s = run.style;
                        let flags = [
                            (s.bold, "bold"),
                            (s.dim, "dim"),
                            (s.italic, "italic"),
                            (s.underline, "underline"),
                            (s.inverse, "inverse"),
                        ]
                        .into_iter()
                        .filter_map(|(on, name)| on.then_some(name))
                        .collect();
                        (run.text, s.fg, s.bg, flags)
                    })
                    .collect()
            })
            .collect())
    }

    /// The test terminal's scrollback followed by its visible rows.
    fn history(&self) -> PyResult<Vec<String>> {
        Ok(self.test_screen()?.history())
    }

    /// The test terminal's cursor as (row, col), or None while hidden.
    fn cursor(&self) -> PyResult<Option<(u16, u16)>> {
        let s = self.test_screen()?;
        Ok(s.cursor_visible().then(|| s.cursor()))
    }

    /// Feeds synthetic input to a test session: `{"text": s}` types at human pace,
    /// `{"burst": s}` at paste pace, `{"key": "ctrl+t"}`, `{"paste": s}`, `{"resize": [c, r]}`,
    /// `{"click": [col, row]}`, `{"scroll": n}` (wheel steps, positive is up).
    fn inject(&self, events: &Bound<'_, PyList>) -> PyResult<()> {
        self.test_screen()?;
        let mut out = Vec::new();
        for ev in events.iter() {
            let ev = ev
                .cast::<PyDict>()
                .map_err(|_| PyValueError::new_err("inject takes dicts"))?;
            out.extend(parse_inject(ev)?);
        }
        self.running()?.inject(out)?;
        Ok(())
    }
}

fn typed(text: &str, gap: Duration) -> Vec<InputEv> {
    text.chars()
        .map(|c| InputEv::Typed {
            code: if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            },
            mods: KeyModifiers::NONE,
            gap,
        })
        .collect()
}

fn parse_inject(ev: &Bound<'_, PyDict>) -> PyResult<Vec<InputEv>> {
    if let Some(t) = ev.get_item("text")? {
        return Ok(typed(&t.extract::<String>()?, Duration::from_millis(60)));
    }
    if let Some(t) = ev.get_item("burst")? {
        return Ok(typed(&t.extract::<String>()?, Duration::from_micros(300)));
    }
    if let Some(t) = ev.get_item("paste")? {
        return Ok(vec![InputEv::Input(Input::Paste(t.extract()?))]);
    }
    if let Some(r) = ev.get_item("resize")? {
        let (cols, rows): (u16, u16) = r.extract()?;
        return Ok(vec![InputEv::Resize(cols, rows)]);
    }
    if let Some(c) = ev.get_item("click")? {
        let (col, row): (u16, u16) = c.extract()?;
        // a whole click: press then release where it was pressed (links act on the release)
        let down = MouseEventKind::Down(MouseButton::Left);
        let up = MouseEventKind::Up(MouseButton::Left);
        return Ok(vec![
            InputEv::Mouse { kind: down, col, row },
            InputEv::Mouse { kind: up, col, row },
        ]);
    }
    if let Some(n) = ev.get_item("scroll")? {
        let n: i32 = n.extract()?;
        let kind = if n > 0 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        };
        let steps = n.unsigned_abs() as usize;
        return Ok(vec![InputEv::Mouse { kind, col: 0, row: 0 }; steps]);
    }
    if let Some(k) = ev.get_item("key")? {
        let name: String = k.extract()?;
        let chord = Chord::parse(&name)
            .ok_or_else(|| PyValueError::new_err(format!("unknown key {name:?}")))?;
        let (code, mods) = chord.parts();
        return Ok(vec![InputEv::Typed { code, mods, gap: Duration::from_millis(60) }]);
    }
    Err(PyValueError::new_err(
        "inject event needs text, burst, paste, resize, key, click or scroll",
    ))
}

/// Restores a terminal left in raw mode or on the alternate screen.
#[pyfunction(name = "reset_terminal")]
fn py_reset_terminal() {
    reset_terminal();
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySession>()?;
    m.add_function(wrap_pyfunction!(py_reset_terminal, m)?)?;
    Ok(())
}
