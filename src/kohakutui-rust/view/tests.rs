//! Stage: view. A model against the scrollback lines it flushes, the inline viewport it composes
//! and the events its keys raise.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};

use super::render::cell_parts;
use super::{handle_input, inline, take_flushable, Chord, Outcome, ViewState};
use crate::model::{FlushState, Model, BOTTOM, LIVE, OVERLAYS, TRANSCRIPT};
use crate::proto::value::map;
use crate::proto::{Event, Mode, NodeId, Op, Value};
use crate::text::plain;
use crate::widgets::{Ctx, Input};

fn ctx() -> Ctx {
    let now = Instant::now();
    Ctx::new(now, now)
}

fn create(m: &mut Model, id: NodeId, kind: &str, parent: NodeId, props: Value) {
    m.apply(Op::Create { id, kind: kind.into(), props, parent, index: None })
        .unwrap();
}

fn texts(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

/// A sealed user cell, then an open assistant cell (stream_flush) with a streaming body.
fn chat() -> Model {
    let mut m = Model::new();
    create(&mut m, 20, "cell", TRANSCRIPT, map(vec![("role", "user".into())]));
    create(&mut m, 21, "text", 20, map(vec![("value", "hi there".into())]));
    m.apply(Op::Seal { id: 20 }).unwrap();

    let assistant = map(vec![("role", "assistant".into()), ("stream_flush", true.into())]);
    create(&mut m, 22, "cell", TRANSCRIPT, assistant);
    create(&mut m, 23, "markdown", 22, map(vec![("streaming", true.into())]));
    m
}

thread_local! {
    static CLOCK: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// A key at typing pace: each one 60 ms after the last, so none reads as a paste burst.
fn key(code: KeyCode, mods: KeyModifiers) -> Input {
    let at = CLOCK.with(|c| {
        let at = c.get().unwrap_or_else(Instant::now) + Duration::from_millis(60);
        c.set(Some(at));
        at
    });
    Input::Key { code, mods, at }
}

#[test]
fn a_streaming_cell_reaches_scrollback_line_by_line_and_exactly_once() {
    let mut m = chat();
    let c = ctx();
    let mut flushed = take_flushable(&mut m, 40, &c);
    assert_eq!(texts(&flushed), vec!["› hi there"], "the open cell has nothing stable yet");

    let reply = "First line\nsecond line\n\n- item one\n- item two\nlast par";
    for chunk in reply.as_bytes().chunks(5) {
        let data = std::str::from_utf8(chunk).unwrap().into();
        m.apply(Op::Append { id: 23, data }).unwrap();
        flushed.extend(take_flushable(&mut m, 40, &c));
    }
    assert!(matches!(m.flush_state(22), Some(FlushState::Partial { .. })));
    assert!(!texts(&flushed).iter().any(|l| l.contains("last par")), "the open line waits");

    m.apply(Op::Seal { id: 22 }).unwrap();
    flushed.extend(take_flushable(&mut m, 40, &c));
    assert_eq!(m.flush_state(22), Some(FlushState::Full));

    let mut expected = vec!["› hi there".to_owned(), String::new()];
    expected.extend(texts(&cell_parts(&m, 22, 40, &c).all()));
    assert_eq!(texts(&flushed), expected);
    assert_eq!(expected[2], "● First line");
}

#[test]
fn an_open_cell_holds_back_every_cell_after_it() {
    let mut m = Model::new();
    create(&mut m, 20, "cell", TRANSCRIPT, map(vec![("title", "tool".into())]));
    create(&mut m, 21, "cell", TRANSCRIPT, map(vec![("title", "after".into())]));
    m.apply(Op::Seal { id: 21 }).unwrap();
    assert!(take_flushable(&mut m, 40, &ctx()).is_empty());

    m.apply(Op::Move { id: 20, parent: LIVE, index: None })
        .unwrap();
    let flushed = texts(&take_flushable(&mut m, 40, &ctx()));
    assert_eq!(flushed, vec!["• after"], "moving the open cell out unblocks the flush");
}

#[test]
fn the_inline_viewport_shows_only_the_unflushed_tail_and_stays_capped() {
    let c = ctx();
    let vs = ViewState::new(Mode::Inline);
    let body = (0..30).map(|i| format!("line {i}\n")).collect::<String>() + "partial";

    let mut m = chat();
    m.apply(Op::Append { id: 23, data: body.as_str().into() })
        .unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    take_flushable(&mut m, 60, &c);
    let screen = inline(&m, &vs, 60, 12, &c);
    assert_eq!(texts(&screen.lines), vec!["  partial", "› "]);
    assert_eq!(screen.cursor, Some((1, 2)));

    // without stream_flush the whole open cell stays in the viewport, tail first
    let mut m = chat();
    let no_stream = map(vec![("stream_flush", false.into())]);
    m.apply(Op::Update { id: 22, props: no_stream }).unwrap();
    m.apply(Op::Append { id: 23, data: body.as_str().into() })
        .unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let screen = inline(&m, &vs, 60, 12, &c);
    assert_eq!(screen.lines.len(), 10, "rows - 2");
    assert!(plain(&screen.lines[0]).starts_with("⋯ "), "{:?}", texts(&screen.lines));
    assert_eq!(plain(&screen.lines[9]), "› ");
}

#[test]
fn keys_go_to_the_composer_until_an_overlay_opens_and_esc_closes_it() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();

    for c in "ok".chars() {
        handle_input(&mut m, &mut vs, key(KeyCode::Char(c), KeyModifiers::NONE), &mut events);
    }
    handle_input(&mut m, &mut vs, key(KeyCode::Enter, KeyModifiers::NONE), &mut events);
    assert_eq!(events, vec![Event::Submit { id: 40, value: "ok".into() }]);

    events.clear();
    let items = Value::Array(vec!["a".into(), "b".into()]);
    create(&mut m, 50, "picker", OVERLAYS, map(vec![("items", items)]));
    for k in [KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
        handle_input(&mut m, &mut vs, key(k, KeyModifiers::NONE), &mut events);
    }
    let picked = Event::Select { id: 50, index: 1, value: "b".into() };
    let closed = Event::Close { id: 50, reason: "escape".into() };
    assert_eq!(events, vec![picked, closed]);
}

#[test]
fn bound_chords_become_actions_and_ctrl_keys_have_builtin_meanings() {
    let mut m = Model::new();
    let mut vs = ViewState::new(Mode::Inline);
    vs.bindings
        .push((Chord::parse("ctrl+k").unwrap(), "palette".into()));
    let mut events = Vec::new();
    let ctrl = KeyModifiers::CONTROL;

    let out = handle_input(&mut m, &mut vs, key(KeyCode::Char('K'), ctrl), &mut events);
    assert_eq!(out, Outcome::Redraw);
    assert_eq!(events, vec![Event::Action { name: "palette".into(), focus: None }]);

    let t = handle_input(&mut m, &mut vs, key(KeyCode::Char('t'), ctrl), &mut events);
    assert_eq!(t, Outcome::ToggleMode);
    let c = handle_input(&mut m, &mut vs, key(KeyCode::Char('c'), ctrl), &mut events);
    assert_eq!(c, Outcome::Interrupt);
    assert!(Chord::parse("hyper+x").is_none());
}
