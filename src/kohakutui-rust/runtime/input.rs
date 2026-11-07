//! The input thread: reads terminal events and forwards keys, pastes, resizes, clicks and wheel
//! steps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use crossterm::event::{self, Event as TermEvent, KeyEventKind, MouseButton, MouseEventKind};

use super::InputEv;
use crate::widgets::Input;

/// Starts reading; the thread stops within one poll interval after `stop` is set.
/// While `paused` is set it reads nothing, so another program (an editor) gets the keys.
pub(crate) fn spawn(
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) -> (Receiver<InputEv>, Option<JoinHandle<()>>) {
    let (tx, rx) = unbounded();
    let handle = thread::Builder::new()
        .name("kohakutui-input".into())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if paused.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(30));
                    continue;
                }
                match event::poll(Duration::from_millis(50)) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(_) => break,
                }
                let ev = match event::read() {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                let msg = match ev {
                    // Windows reports releases too; only presses and repeats are input
                    TermEvent::Key(k) if k.kind != KeyEventKind::Release => {
                        InputEv::Input(Input::Key {
                            code: k.code,
                            mods: k.modifiers,
                            at: Instant::now(),
                        })
                    }
                    TermEvent::Paste(s) => InputEv::Input(Input::Paste(s)),
                    TermEvent::FocusGained => InputEv::Focus(true),
                    TermEvent::FocusLost => InputEv::Focus(false),
                    TermEvent::Resize(c, r) => InputEv::Resize(c, r),
                    TermEvent::Mouse(me)
                        if matches!(
                            me.kind,
                            MouseEventKind::Down(MouseButton::Left)
                                | MouseEventKind::Down(MouseButton::Middle)
                                | MouseEventKind::ScrollUp
                                | MouseEventKind::ScrollDown
                                | MouseEventKind::Moved
                                | MouseEventKind::Drag(MouseButton::Left)
                                | MouseEventKind::Up(MouseButton::Left)
                        ) =>
                    {
                        InputEv::Mouse { kind: me.kind, col: me.column, row: me.row }
                    }
                    _ => continue,
                };
                if tx.send(msg).is_err() {
                    break;
                }
            }
        })
        .ok();
    (rx, handle)
}
