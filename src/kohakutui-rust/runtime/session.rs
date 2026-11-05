//! `Session`: the handle that starts the engine thread, sends it ops and reads its events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};

use super::{engine, Config, Inbound, InputEv, Outbound, RuntimeError};
use crate::proto::{Event, Value};
use crate::term::{Backend, TermError, TestScreen};

/// A running engine.
pub struct Session {
    tx: Sender<Inbound>,
    rx: Receiver<Outbound>,
    wake: Sender<Outbound>,
    join: Mutex<Option<JoinHandle<()>>>,
    /// Set once the engine thread has ended; a poll then never blocks.
    done: AtomicBool,
    screen: Option<TestScreen>,
}

impl Session {
    /// Starts the engine thread; fails when the terminal cannot be opened.
    pub fn start(cfg: Config) -> Result<Session, RuntimeError> {
        let (tx, in_rx) = unbounded();
        let (out_tx, rx) = unbounded();
        let (opened_tx, opened_rx) = bounded(1);
        let screen = match &cfg.backend {
            Backend::Test(s) => Some(s.clone()),
            _ => None,
        };
        let wake = out_tx.clone();

        let join = thread::Builder::new()
            .name("kohakutui-engine".into())
            .spawn(move || engine::run(cfg, in_rx, out_tx, opened_tx))?;
        match opened_rx.recv() {
            Ok(Ok(())) => Ok(Session {
                tx,
                rx,
                wake,
                join: Mutex::new(Some(join)),
                done: AtomicBool::new(false),
                screen,
            }),
            Ok(Err(msg)) => {
                let _ = join.join();
                Err(TermError::NoTty(msg).into())
            }
            Err(_) => Err(RuntimeError::NotRunning),
        }
    }

    /// Queues raw op values; each is decoded and applied in order.
    pub fn apply(&self, ops: Vec<Value>) -> Result<(), RuntimeError> {
        self.tx
            .send(Inbound::Ops(ops))
            .map_err(|_| RuntimeError::NotRunning)
    }

    /// Waits up to `timeout` (None: forever) for events; returns at most `max`, or none when
    /// woken by `wake`.
    pub fn poll(&self, timeout: Option<Duration>, max: usize) -> Vec<Event> {
        let timeout = match self.done.load(Ordering::SeqCst) {
            true => Some(Duration::ZERO),
            false => timeout,
        };
        drain(&self.rx, timeout, max)
    }

    /// Unblocks one waiting `poll`.
    pub fn wake(&self) {
        let _ = self.wake.send(Outbound::Wake);
    }

    /// Waits until everything sent so far is applied and drawn.
    pub fn sync(&self, timeout: Duration) -> bool {
        let (done, wait) = bounded(1);
        if self.tx.send(Inbound::Sync(done)).is_err() {
            return false;
        }
        wait.recv_timeout(timeout).is_ok()
    }

    /// Feeds synthetic input to the engine.
    pub fn inject(&self, evs: Vec<InputEv>) -> Result<(), RuntimeError> {
        self.tx
            .send(Inbound::Inject(evs))
            .map_err(|_| RuntimeError::NotRunning)
    }

    /// Stops the engine: remaining transcript lines are flushed and the terminal restored.
    pub fn stop(&self) {
        let _ = self.tx.send(Inbound::Stop);
        let handle = self.join.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(h) = handle {
            let _ = h.join();
        }

        // a poll already blocked when the engine ended is released here
        if !self.done.swap(true, Ordering::SeqCst) {
            self.wake();
        }
    }

    pub fn is_running(&self) -> bool {
        self.join
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// The emulated screen of a test session.
    pub fn screen(&self) -> Option<&TestScreen> {
        self.screen.as_ref()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Waits up to `timeout` (None: forever) for an event, then takes what else is queued, up to
/// `max`. A wake returns early with whatever was collected.
fn drain(rx: &Receiver<Outbound>, timeout: Option<Duration>, max: usize) -> Vec<Event> {
    let mut out = Vec::new();
    let first = match timeout {
        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        Some(t) => rx.recv_deadline(Instant::now() + t),
    };
    match first {
        Ok(Outbound::Event(e)) => out.push(e),
        Ok(Outbound::Wake) | Err(_) => return out,
    }
    while out.len() < max.max(1) {
        match rx.try_recv() {
            Ok(Outbound::Event(e)) => out.push(e),
            Ok(Outbound::Wake) | Err(_) => break,
        }
    }
    out
}
