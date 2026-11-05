//! The engine thread: applies ops, routes input, flushes scrollback and draws coalesced frames.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{never, select, Receiver, Sender};
use ratatui::buffer::Buffer;

use super::{input, Config, Inbound, InputEv, Interrupt, Outbound};
use crate::model::{Model, OVERLAYS};
use crate::proto::{Event, Mode, Op, Value};
use crate::term::{install_panic_hook, Term};
use crate::text::clip;
use crate::view::{self, Chord, Outcome, Toast, ViewState};
use crate::widgets::{Ctx, Input};

/// How often animated components (spinners, elapsed time) redraw.
const TICK: Duration = Duration::from_millis(80);
/// Presses this close together count as one ctrl+c sequence.
const INTERRUPT_WINDOW: Duration = Duration::from_secs(1);
/// After the third ctrl+c, how long the app gets to stop before the engine exits the process.
const HARD_EXIT_GRACE: Duration = Duration::from_secs(3);

/// Runs the engine until `Stop` or until every sender is gone; reports startup on `opened`.
pub(crate) fn run(
    cfg: Config,
    rx: Receiver<Inbound>,
    tx: Sender<Outbound>,
    opened: Sender<Result<(), String>>,
) {
    let term = match Term::open(&cfg.backend) {
        Ok(t) => t,
        Err(e) => {
            let _ = opened.send(Err(e.to_string()));
            return;
        }
    };
    install_panic_hook();

    let stop = Arc::new(AtomicBool::new(false));
    let (input_rx, input_thread) = if term.is_tty() {
        input::spawn(stop.clone())
    } else {
        (never(), None)
    };

    let mut eng = Engine::new(cfg, term, tx.clone());
    let _ = opened.send(Ok(()));

    // a panic still restores the terminal (Term's Drop) and still ends with Stopped
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        eng.start();
        eng.run(&rx, input_rx);
        eng.shutdown();
    }));
    drop(eng);

    stop.store(true, Ordering::SeqCst);
    if let Some(t) = input_thread {
        let _ = t.join();
    }
    if outcome.is_err() {
        let message = "the UI engine crashed; the terminal was restored".to_owned();
        let _ = tx.send(Outbound::Event(Event::Error { id: None, message }));
    }
    let _ = tx.send(Outbound::Event(Event::Stopped));
}

struct Engine {
    cfg: Config,
    term: Term,
    model: Model,
    vs: ViewState,
    tx: Sender<Outbound>,
    epoch: Instant,
    frame: Duration,
    dirty: bool,
    next_draw: Instant,
    next_tick: Instant,
    syncs: Vec<Sender<()>>,
    interrupts: u8,
    last_interrupt: Option<Instant>,
    /// Full screen was entered with ctrl+o from inline: the next ctrl+o goes back.
    expand_return: bool,
    hard_exit_at: Option<Instant>,
    /// Clock for injected keys, so their spacing is exact whatever the test's real timing.
    virt: Instant,
}

impl Engine {
    fn new(cfg: Config, term: Term, tx: Sender<Outbound>) -> Engine {
        let now = Instant::now();
        let fps = cfg.max_fps.clamp(1, 240);
        Engine {
            vs: ViewState::new(Mode::Inline),
            frame: Duration::from_secs(1) / fps,
            cfg,
            term,
            model: Model::new(),
            tx,
            epoch: now,
            dirty: true,
            next_draw: now,
            next_tick: now + TICK,
            syncs: Vec::new(),
            interrupts: 0,
            last_interrupt: None,
            expand_return: false,
            hard_exit_at: None,
            virt: now,
        }
    }

    fn emit(&self, ev: Event) {
        let _ = self.tx.send(Outbound::Event(ev));
    }

    fn report(&self, id: Option<u64>, err: impl std::fmt::Display) {
        self.emit(Event::Error { id, message: err.to_string() });
    }

    fn start(&mut self) {
        if self.cfg.mode == Mode::Fullscreen {
            self.set_mode(Mode::Fullscreen);
        }
        let (cols, rows) = self.term.size();
        self.emit(Event::Ready { cols, rows, tty: !self.term.is_plain() });
    }

    fn run(&mut self, rx: &Receiver<Inbound>, mut input_rx: Receiver<InputEv>) {
        loop {
            let wait = self.wait_time();
            let mut running = true;
            select! {
                recv(rx) -> msg => match msg {
                    Ok(m) => running = self.inbound(m),
                    Err(_) => running = false,
                },
                recv(input_rx) -> ev => match ev {
                    Ok(ev) => self.input(ev),
                    Err(_) => input_rx = never(),
                },
                default(wait) => {}
            }

            // drain what queued meanwhile, so a burst of ops becomes one frame
            while running {
                match rx.try_recv() {
                    Ok(m) => running = self.inbound(m),
                    Err(_) => break,
                }
            }
            if !running {
                return;
            }
            self.housekeeping();
            if self.dirty && (Instant::now() >= self.next_draw || !self.syncs.is_empty()) {
                self.draw();
            }
        }
    }

    /// How long the loop may sleep: until the next frame, tick, toast expiry or hard exit.
    fn wait_time(&self) -> Duration {
        let now = Instant::now();
        let mut until = now + Duration::from_millis(500);
        if self.dirty {
            until = until.min(self.next_draw);
        }
        if self.animating() {
            until = until.min(self.next_tick);
        }
        if let Some(t) = self.vs.toasts.iter().map(|t| t.until).min() {
            until = until.min(t);
        }
        if let Some(t) = self.hard_exit_at {
            until = until.min(t);
        }
        until.saturating_duration_since(now)
    }

    fn animating(&self) -> bool {
        self.model
            .subtree(crate::model::LIVE)
            .into_iter()
            .chain(self.model.subtree(crate::model::TRANSCRIPT))
            .chain(self.model.subtree(crate::model::OVERLAYS))
            .chain(self.model.subtree(crate::model::BOTTOM))
            .any(|id| {
                self.model
                    .node(id)
                    .is_some_and(|n| !n.sealed && n.comp.animating())
            })
    }

    fn housekeeping(&mut self) {
        let now = Instant::now();
        if now >= self.next_tick {
            self.next_tick = now + TICK;
            if self.animating() {
                self.dirty = true;
            }
        }
        if self.vs.expire_toasts(now) {
            self.dirty = true;
        }
        if self.hard_exit_at.is_some_and(|t| now >= t) {
            let _ = self.term.close();
            std::process::exit(130);
        }
    }

    /// Handles one message; false means stop.
    fn inbound(&mut self, msg: Inbound) -> bool {
        match msg {
            Inbound::Ops(batch) => {
                for raw in batch {
                    self.raw_op(raw);
                }
            }
            Inbound::Sync(done) => {
                self.syncs.push(done);
                self.dirty = true;
            }
            Inbound::Inject(evs) => {
                for ev in evs {
                    self.input(ev);
                }
            }
            Inbound::Stop => return false,
        }
        true
    }

    fn raw_op(&mut self, raw: Value) {
        match Op::from_value(&raw) {
            Ok(op) => self.op(op),
            Err(e) => self.report(Op::id_hint(&raw), e),
        }
        self.dirty = true;
    }

    fn op(&mut self, op: Op) {
        match op {
            Op::SetMode { mode } => self.set_mode(mode),
            Op::Bind { chord, action } => match Chord::parse(&chord) {
                Some(c) => {
                    self.vs.bindings.retain(|(b, _)| *b != c);
                    self.vs.bindings.push((c, action));
                }
                None => self.report(None, format!("unknown chord `{chord}`")),
            },
            Op::Unbind { chord } => {
                if let Some(c) = Chord::parse(&chord) {
                    self.vs.bindings.retain(|(b, _)| *b != c);
                }
            }
            Op::Notify { text, level, ttl_ms } => self.vs.toasts.push(Toast {
                text,
                level,
                until: Instant::now() + Duration::from_millis(ttl_ms.into()),
            }),
            Op::Bell => {
                let _ = self.term.bell();
            }
            Op::SetTheme { scheme, overrides, truecolor } => {
                let truecolor = truecolor.unwrap_or_else(crate::text::theme::detect_truecolor);
                match crate::text::theme::Theme::scheme(&scheme, truecolor) {
                    Some(t) => {
                        crate::text::theme::set_current(t.with_overrides(&overrides));
                        // cached full-screen lines still carry the old colours
                        self.vs.cache.clear();
                    }
                    None => self.report(None, format!("unknown theme `{scheme}`")),
                }
            }
            Op::Focus { id } => match self.model.node(id) {
                Some(n) if n.comp.focusable() && !n.sealed => self.vs.focus = Some(id),
                Some(_) => self.report(Some(id), format!("node {id} cannot take focus")),
                None => self.report(Some(id), format!("no node {id}")),
            },
            tree => {
                let creates_overlay = matches!(tree, Op::Create { parent: OVERLAYS, .. });
                match self.model.apply(tree) {
                    Ok(id) => {
                        // with no terminal, nobody can answer a dialog: close it at once
                        if creates_overlay && self.term.is_plain() {
                            self.emit(Event::Close { id, reason: "no_tty".into() });
                        }
                    }
                    Err(e) => self.report(None, e),
                }
            }
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        if mode == self.vs.mode || self.term.is_plain() {
            return;
        }
        let res = match mode {
            Mode::Fullscreen => self.term.enter_fullscreen(),
            Mode::Inline => self.term.leave_fullscreen(),
        };
        if let Err(e) = res {
            self.report(None, e);
            return;
        }
        self.vs.mode = mode;
        self.vs.scroll = 0;
        self.expand_return = false;
        if mode == Mode::Inline {
            self.vs.expand_all = false;
        }
        self.dirty = true;
        self.emit(Event::ModeChanged { mode });
    }

    fn input(&mut self, ev: InputEv) {
        let input = match ev {
            InputEv::Resize(cols, rows) => {
                if let crate::term::Backend::Test(screen) = &self.cfg.backend {
                    screen.set_size(cols, rows);
                }
                if let Err(e) = self.term.resize(cols, rows) {
                    self.report(None, e);
                }
                self.dirty = true;
                self.emit(Event::Resize { cols, rows });
                return;
            }
            InputEv::Typed { code, mods, gap } => {
                self.virt += gap;
                Input::Key { code, mods, at: self.virt }
            }
            InputEv::Input(i) => i,
            InputEv::Mouse { kind, col, row } => {
                let mut events = Vec::new();
                if view::handle_mouse(&mut self.model, &mut self.vs, kind, col, row, &mut events) {
                    self.dirty = true;
                }
                for ev in events {
                    self.emit(ev);
                }
                return;
            }
        };

        let mut events = Vec::new();
        let outcome = view::handle_input(&mut self.model, &mut self.vs, input, &mut events);
        for ev in events {
            self.emit(ev);
        }
        match outcome {
            Outcome::Nothing => {}
            Outcome::Redraw => self.dirty = true,
            Outcome::ToggleMode => {
                let next = match self.vs.mode {
                    Mode::Inline => Mode::Fullscreen,
                    Mode::Fullscreen => Mode::Inline,
                };
                self.set_mode(next);
            }
            Outcome::ExpandAll => {
                self.model.clear_open();
                // from inline, ctrl+o is a round trip: an expanded full-screen view, then back
                match self.vs.mode {
                    Mode::Inline => {
                        self.set_mode(Mode::Fullscreen);
                        if self.vs.mode == Mode::Fullscreen {
                            self.vs.expand_all = true;
                            self.expand_return = true;
                        }
                    }
                    Mode::Fullscreen if self.expand_return => self.set_mode(Mode::Inline),
                    Mode::Fullscreen => self.vs.expand_all = !self.vs.expand_all,
                }
                self.dirty = true;
            }
            Outcome::Interrupt => self.interrupt(),
        }
    }

    fn interrupt(&mut self) {
        let now = Instant::now();
        let recent = self
            .last_interrupt
            .is_some_and(|t| now.duration_since(t) < INTERRUPT_WINDOW);
        self.interrupts = if recent {
            self.interrupts.saturating_add(1)
        } else {
            1
        };
        self.last_interrupt = Some(now);

        match self.cfg.interrupt {
            Interrupt::Exit => {
                let _ = self.term.close();
                std::process::exit(130);
            }
            Interrupt::Signal => send_sigint(),
            Interrupt::Event => {}
        }
        self.emit(Event::Interrupt { count: self.interrupts });
        if self.interrupts >= 3 && self.hard_exit_at.is_none() {
            self.hard_exit_at = Some(now + HARD_EXIT_GRACE);
        }
    }

    fn draw(&mut self) {
        // cleared first: whatever this frame queues for the next one (a copy's toast) stays
        self.dirty = false;
        if let Err(e) = self.try_draw() {
            self.report(None, format!("draw failed: {e}"));
        }
        self.next_draw = Instant::now() + self.frame;
        for done in self.syncs.drain(..) {
            let _ = done.send(());
        }
    }

    fn try_draw(&mut self) -> std::io::Result<()> {
        let ctx = Ctx::new(Instant::now(), self.epoch);
        self.vs.track_overlay(self.model.top_overlay());
        let (cols, rows) = self.term.size();
        self.term.begin_frame()?;

        // scrollback first, inline only: in full screen flush state waits for the return
        if self.vs.mode == Mode::Inline {
            let lines = view::take_flushable(&mut self.model, cols, &ctx);
            let lines: Vec<_> = lines.into_iter().map(|l| clip(l, cols as usize)).collect();
            self.term.insert_history(&lines)?;
        }
        if self.term.is_plain() {
            return self.term.end_frame();
        }

        match self.vs.mode {
            Mode::Inline => {
                let screen = view::inline(&self.model, &self.vs, cols, rows, &ctx);
                self.term.set_viewport(screen.lines.len() as u16)?;
                let area = self.term.area();
                let mut buf = Buffer::empty(area);
                for (i, line) in screen.lines.iter().take(area.height as usize).enumerate() {
                    buf.set_line(area.x, area.y + i as u16, line, area.width);
                }
                let cursor = screen
                    .cursor
                    .filter(|(r, c)| *r < area.height && *c < area.width)
                    .map(|(r, c)| (area.x + c, area.y + r));
                self.vs.overlay_hit = screen
                    .overlay
                    .map(|(id, r, c, w, h)| (id, i32::from(area.y) + r, area.x + c, w, h));
                self.term.draw(&buf, cursor)?;
            }
            Mode::Fullscreen => {
                self.term.set_viewport(rows)?;
                let mut buf = Buffer::empty(self.term.area());
                let cursor = view::fullscreen(&self.model, &mut self.vs, &mut buf, &ctx);
                self.term.draw(&buf, cursor)?;
                if let Some(text) = self.vs.copied.take() {
                    self.copy(text)?;
                }
            }
        }

        // the mouse is ours only in full screen: inline, the terminal keeps its own wheel
        // scrolling and text selection at all times, dialog or not
        self.term.set_mouse(self.vs.mode == Mode::Fullscreen)?;
        self.term.end_frame()
    }

    /// Puts a full-screen selection on the clipboard (OSC 52), says so, and reports it.
    fn copy(&mut self, text: String) -> std::io::Result<()> {
        self.term.copy(&text)?;
        let chars = text.chars().count();
        self.vs.toasts.push(Toast {
            text: format!("copied {chars} characters · shift+drag uses the terminal's selection"),
            level: crate::proto::Level::Info,
            until: Instant::now() + Duration::from_millis(2500),
        });
        self.emit(Event::Copy { text });
        self.dirty = true;
        Ok(())
    }

    /// Writes every remaining transcript line to scrollback and restores the terminal.
    fn shutdown(&mut self) {
        let ctx = Ctx::new(Instant::now(), self.epoch);
        let (cols, _) = self.term.size();
        let _ = self.term.leave_fullscreen();
        let lines = view::flush_all(&mut self.model, cols, &ctx);
        let lines: Vec<_> = lines.into_iter().map(|l| clip(l, cols as usize)).collect();
        let _ = self.term.begin_frame();
        let _ = self.term.insert_history(&lines);
        let _ = self.term.end_frame();
        let _ = self.term.close();
        for done in self.syncs.drain(..) {
            let _ = done.send(());
        }
    }
}

#[cfg(unix)]
fn send_sigint() {
    // SAFETY: kill(0, SIGINT) only sends a signal to our own process group
    unsafe {
        libc::kill(0, libc::SIGINT);
    }
}

#[cfg(not(unix))]
fn send_sigint() {}
