//! Stage: runtime. A session driven by op values and injected keys, read on the emulated
//! terminal it draws to and the events it sends back.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use super::{Config, InputEv, Session};
use crate::proto::value::map;
use crate::proto::{Event, Mode, Op, Value};
use crate::term::{Backend, TestScreen};

const WAIT: Duration = Duration::from_secs(2);

fn session(cols: u16, rows: u16) -> Session {
    let screen = TestScreen::new(cols, rows);
    let cfg = Config { backend: Backend::Test(screen), ..Config::default() };
    Session::start(cfg).unwrap()
}

fn create(id: u64, kind: &str, parent: u64, props: Value) -> Value {
    let op = Op::Create { id, kind: kind.into(), props, parent, index: None };
    op.to_value()
}

/// Every event queued within a short wait.
fn events(s: &Session) -> Vec<Event> {
    s.poll(Some(Duration::from_millis(200)), 1000)
}

fn key(code: KeyCode, mods: KeyModifiers, gap_ms: u64) -> InputEv {
    InputEv::Typed { code, mods, gap: Duration::from_millis(gap_ms) }
}

/// Keys for `text` (`\n` is Enter), `gap_ms` apart.
fn typed(text: &str, gap_ms: u64) -> Vec<InputEv> {
    text.chars()
        .map(|c| {
            let code = if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            };
            key(code, KeyModifiers::NONE, gap_ms)
        })
        .collect()
}

#[test]
fn a_chat_turn_streams_into_scrollback_and_the_composer_stays_at_the_bottom() {
    let s = session(40, 10);
    let ready = events(&s);
    assert!(matches!(ready[..], [Event::Ready { cols: 40, rows: 10, tty: true }]));

    let composer = create(40, "composer", 3, map(vec![("placeholder", "say".into())]));
    s.apply(vec![composer]).unwrap();
    s.inject(typed("hello\n", 60)).unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::Submit { id: 40, value: "hello".into() }]);

    let cell = map(vec![("role", "assistant".into()), ("stream_flush", true.into())]);
    let body = map(vec![("streaming", true.into())]);
    s.apply(vec![
        create(20, "cell", 1, cell),
        create(21, "markdown", 20, body),
    ])
    .unwrap();
    for i in 0..12 {
        let chunk = Op::Append { id: 21, data: format!("line {i}\n").into() };
        s.apply(vec![chunk.to_value()]).unwrap();
        assert!(s.sync(WAIT));
    }
    s.apply(vec![Op::Seal { id: 20 }.to_value()]).unwrap();
    assert!(s.sync(WAIT));

    let history = s.screen().unwrap().history();
    let body: Vec<&String> = history.iter().filter(|l| l.contains("line ")).collect();
    assert_eq!(body.len(), 12, "each line once: {history:#?}");
    assert_eq!(body[0], "● line 0");
    assert_eq!(history.last().unwrap(), "› say", "{history:#?}");
}

#[test]
fn bad_ops_report_errors_and_the_rest_of_the_batch_still_applies() {
    let s = session(30, 8);
    events(&s);

    s.apply(vec![
        map(vec![("op", "explode".into())]),
        create(40, "text", 2, map(vec![("value", "still here".into())])),
        create(41, "text", 999, Value::Map(vec![])),
    ])
    .unwrap();
    assert!(s.sync(WAIT));

    let evs = events(&s);
    assert_eq!(evs.len(), 2, "{evs:?}");
    assert!(evs.iter().all(|e| matches!(e, Event::Error { .. })));
    assert!(s
        .screen()
        .unwrap()
        .screen()
        .iter()
        .any(|l| l == "still here"));
}

#[test]
fn a_dialog_answers_by_select_and_ctrl_t_toggles_full_screen() {
    let s = session(40, 12);
    events(&s);

    let items = Value::Array(vec!["alpha".into(), "beta".into()]);
    let picker = map(vec![("items", items), ("title", "Pick".into())]);
    s.apply(vec![create(50, "picker", 5, picker)]).unwrap();
    s.inject(typed("bet\n", 60)).unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::Select { id: 50, index: 1, value: "beta".into() }]);

    s.inject(vec![key(KeyCode::Char('t'), KeyModifiers::CONTROL, 60)])
        .unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::ModeChanged { mode: Mode::Fullscreen }]);
    let screen = s.screen().unwrap().screen();
    assert!(screen.iter().any(|l| l.contains("Pick")), "modal over full screen: {screen:#?}");
}

#[test]
fn ctrl_o_is_a_round_trip_and_a_drag_in_full_screen_copies_what_it_covers() {
    let s = session(40, 12);
    events(&s);
    let cell = map(vec![("role", "assistant".into())]);
    let text = map(vec![("value", "copy me please".into())]);
    s.apply(vec![
        create(20, "cell", 1, cell),
        create(21, "text", 20, text),
        Op::Seal { id: 20 }.to_value(),
    ])
    .unwrap();
    assert!(s.sync(WAIT));

    // ctrl+o from inline: an expanded full-screen view; ctrl+o again: back to inline
    let ctrl_o = || vec![key(KeyCode::Char('o'), KeyModifiers::CONTROL, 60)];
    s.inject(ctrl_o()).unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::ModeChanged { mode: Mode::Fullscreen }]);
    s.inject(ctrl_o()).unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::ModeChanged { mode: Mode::Inline }]);

    // in full screen (ctrl+t), press, drag and release over "copy me": it is copied
    s.inject(vec![key(KeyCode::Char('t'), KeyModifiers::CONTROL, 60)])
        .unwrap();
    assert!(s.sync(WAIT));
    events(&s);
    let screen = s.screen().unwrap().screen();
    let row = screen.iter().position(|l| l.contains("copy me")).unwrap() as u16;
    // a column, not a byte offset: the line starts with a multi-byte icon
    let line = &screen[row as usize];
    let col = line[..line.find("copy").unwrap()].chars().count() as u16;
    let mouse = |kind, col| InputEv::Mouse { kind, col, row };
    s.inject(vec![
        mouse(MouseEventKind::Down(MouseButton::Left), col),
        mouse(MouseEventKind::Drag(MouseButton::Left), col + 4),
        mouse(MouseEventKind::Up(MouseButton::Left), col + 6),
    ])
    .unwrap();
    assert!(s.sync(WAIT));
    assert_eq!(events(&s), vec![Event::Copy { text: "copy me".into() }]);
    assert!(s.sync(WAIT), "the toast's frame");
    let screen = s.screen().unwrap().screen();
    assert!(screen.iter().any(|l| l.contains("copied 7 characters")), "{screen:#?}");
}

#[test]
fn stop_flushes_open_cells_and_clears_the_viewport() {
    let s = session(30, 8);
    events(&s);
    s.apply(vec![
        create(20, "cell", 1, map(vec![("title", "unfinished".into())])),
        create(40, "composer", 3, Value::Map(vec![])),
    ])
    .unwrap();
    assert!(s.sync(WAIT));
    let screen = s.screen().unwrap().clone();

    s.stop();
    assert_eq!(screen.history(), vec!["• unfinished"]);
    assert!(events(&s).contains(&Event::Stopped));
    assert!(s.apply(vec![]).is_err(), "a stopped session takes no ops");
}

#[test]
fn ctrl_c_counts_presses_and_bound_chords_send_actions() {
    let s = session(30, 8);
    events(&s);
    let bind = map(vec![
        ("op", "bind".into()),
        ("chord", "f2".into()),
        ("action", "help".into()),
    ]);
    s.apply(vec![bind]).unwrap();

    let ctrl_c = key(KeyCode::Char('c'), KeyModifiers::CONTROL, 0);
    let f2 = key(KeyCode::F(2), KeyModifiers::NONE, 0);
    s.inject(vec![ctrl_c.clone(), ctrl_c, f2]).unwrap();
    assert!(s.sync(WAIT));

    let evs = events(&s);
    let help = Event::Action { name: "help".into(), focus: None };
    assert_eq!(
        evs,
        vec![
            Event::Interrupt { count: 1 },
            Event::Interrupt { count: 2 },
            help
        ]
    );
}

#[test]
fn poll_after_stop_returns_at_once_with_stopped() {
    let s = session(30, 8);
    events(&s);
    s.stop();
    assert_eq!(s.poll(None, 10), vec![Event::Stopped]);

    let started = std::time::Instant::now();
    assert!(s.poll(None, 10).is_empty());
    assert!(started.elapsed() < Duration::from_secs(1), "a dead engine never blocks poll");
}

#[test]
fn wake_unblocks_a_waiting_poll() {
    let s = std::sync::Arc::new(session(30, 8));
    events(&s);
    let waiter = {
        let s = s.clone();
        std::thread::spawn(move || s.poll(None, 10))
    };
    std::thread::sleep(Duration::from_millis(50));
    s.wake();
    assert_eq!(waiter.join().unwrap(), vec![]);
}
