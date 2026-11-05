//! Stage: view. A model against the scrollback lines it flushes, the inline viewport it composes
//! and the events its keys raise.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::render::cell_parts;
use super::{
    fullscreen, handle_input, handle_mouse, inline, take_flushable, Chord, Outcome, ViewState,
};
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
    assert_eq!(texts(&screen.lines), vec!["  partial", "", "› "], "a gap above the input");
    assert_eq!(screen.cursor, Some((2, 2)));

    // live content keeps a blank line from whatever is above it, scrollback included
    create(&mut m, 41, "text", LIVE, map(vec![("value", "working".into())]));
    m.apply(Op::Seal { id: 22 }).unwrap();
    take_flushable(&mut m, 60, &c);
    let screen = inline(&m, &vs, 60, 12, &c);
    assert_eq!(texts(&screen.lines), vec!["", "working", "", "› "]);

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
    assert_eq!(plain(&screen.lines[8]), "");
    assert_eq!(plain(&screen.lines[9]), "› ");
}

#[test]
fn a_notice_keeps_a_blank_line_from_what_is_above_it() {
    let c = ctx();
    let mut m = chat();
    m.apply(Op::Append { id: 23, data: "done".into() }).unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let mut vs = ViewState::new(Mode::Inline);
    let until = Instant::now() + Duration::from_secs(5);
    vs.toasts
        .push(super::Toast { text: "saved".into(), level: crate::proto::Level::Info, until });
    take_flushable(&mut m, 60, &c);

    let screen = inline(&m, &vs, 60, 12, &c);
    assert_eq!(texts(&screen.lines), vec!["", "● done", "", "ℹ saved", "", "› "]);
}

/// A sealed, folded tool cell with a code child and a summary.
fn folded_tool(m: &mut Model, id: NodeId) {
    let props = map(vec![
        ("role", "tool".into()),
        ("title", "grep".into()),
        ("collapsed", true.into()),
        ("summary", "3 matches".into()),
    ]);
    create(m, id, "cell", TRANSCRIPT, props);
    create(m, id + 1, "code", id, map(vec![("value", "rg pattern".into())]));
    m.apply(Op::Seal { id }).unwrap();
}

#[test]
fn a_folded_cell_shows_one_summary_line_until_opened() {
    let mut m = Model::new();
    folded_tool(&mut m, 20);
    let c = ctx();

    let folded = texts(&cell_parts(&m, 20, 60, &c).all());
    assert_eq!(folded, vec!["◆ grep", "  ⎿  3 matches  (ctrl+o to expand)"]);
    let open = texts(&cell_parts(&m, 20, 60, &c.expanded(true)).all());
    assert!(open.iter().any(|l| l.contains("rg pattern")), "{open:?}");

    let flushed = texts(&take_flushable(&mut m, 60, &c));
    assert_eq!(flushed, folded, "scrollback gets the folded form");
}

#[test]
fn full_screen_clicks_open_one_folded_cell_and_ctrl_o_opens_all() {
    let mut m = Model::new();
    folded_tool(&mut m, 20);
    folded_tool(&mut m, 30);
    let mut vs = ViewState::new(Mode::Fullscreen);
    let c = ctx();
    let draw = |m: &Model, vs: &mut ViewState| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 12));
        fullscreen(m, vs, &mut buf, &c);
        (0..12)
            .map(|y| {
                (0..50)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };

    let rows = draw(&m, &mut vs);
    let second = rows.iter().rposition(|r| r == "◆ grep").unwrap() as u16;
    assert_eq!(rows.iter().filter(|r| r.contains("rg pattern")).count(), 0);

    let click = MouseEventKind::Down(MouseButton::Left);
    let mut events = Vec::new();
    assert!(handle_mouse(&mut m, &mut vs, click, 2, second, &mut events));
    let rows = draw(&m, &mut vs);
    assert_eq!(rows.iter().filter(|r| r.contains("rg pattern")).count(), 1, "{rows:#?}");
    let off = handle_mouse(&mut m, &mut vs, click, 2, 11, &mut events);
    assert!(!off, "a click off any header does nothing");

    vs.expand_all = true;
    let rows = draw(&m, &mut vs);
    assert_eq!(
        rows.iter().filter(|r| r.contains("rg pattern")).count(),
        1,
        "a clicked cell stays the opposite of expand_all: {rows:#?}"
    );

    let mut inline_vs = ViewState::new(Mode::Inline);
    let inline_click = handle_mouse(&mut m, &mut inline_vs, click, 2, second, &mut events);
    assert!(!inline_click, "inline mode has no transcript mouse");
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
fn typing_past_an_open_dialog_reaches_the_input_and_returns_after_submit() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let actions = Value::Array(vec!["yes".into(), "no".into()]);
    create(
        &mut m,
        50,
        "card",
        OVERLAYS,
        map(vec![("title", "Run?".into()), ("actions", actions)]),
    );
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();
    let press = |m: &mut Model, vs: &mut ViewState, ev: &mut Vec<Event>, k: KeyCode| {
        handle_input(m, vs, key(k, KeyModifiers::NONE), ev)
    };

    press(&mut m, &mut vs, &mut events, KeyCode::Right);
    assert_eq!(super::layout::input_target(&m, &vs), Some(50), "the dialog has the keys");
    for c in "/tasks".chars() {
        press(&mut m, &mut vs, &mut events, KeyCode::Char(c));
    }
    assert!(events.is_empty(), "typing answers nothing: {events:?}");
    let screen = inline(&m, &vs, 40, 20, &ctx());
    let shown = texts(&screen.lines);
    assert!(shown.contains(&"› /tasks".to_owned()), "{shown:#?}");
    assert!(
        shown
            .iter()
            .any(|l| l.contains("tab returns to this dialog")),
        "{shown:#?}"
    );
    assert_eq!(screen.cursor, Some((shown.len() as u16 - 1, 8)), "the caret is in the input");

    press(&mut m, &mut vs, &mut events, KeyCode::Enter);
    assert_eq!(events, vec![Event::Submit { id: 40, value: "/tasks".into() }]);
    events.clear();
    press(&mut m, &mut vs, &mut events, KeyCode::Enter);
    let chose = Event::Select { id: 50, index: 1, value: "no".into() };
    assert_eq!(events, vec![chose], "after the submit, keys are the dialog's again");

    events.clear();
    press(&mut m, &mut vs, &mut events, KeyCode::Char('x'));
    press(&mut m, &mut vs, &mut events, KeyCode::Esc);
    assert!(events.is_empty(), "esc in the input returns to the dialog, closing nothing");
    press(&mut m, &mut vs, &mut events, KeyCode::Esc);
    assert_eq!(events, vec![Event::Close { id: 50, reason: "escape".into() }]);
}

#[test]
fn down_steps_from_the_input_to_a_button_and_typing_steps_back() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 41, "button", BOTTOM, map(vec![("label", "tasks".into())]));
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();
    let mut press = |m: &mut Model, vs: &mut ViewState, k: KeyCode| {
        handle_input(m, vs, key(k, KeyModifiers::NONE), &mut events)
    };

    press(&mut m, &mut vs, KeyCode::Down);
    assert_eq!(super::layout::input_target(&m, &vs), Some(41));
    press(&mut m, &mut vs, KeyCode::Up);
    assert_eq!(super::layout::input_target(&m, &vs), Some(40));
    press(&mut m, &mut vs, KeyCode::Down);
    press(&mut m, &mut vs, KeyCode::Char('h'));
    assert_eq!(super::layout::input_target(&m, &vs), Some(40), "typing returns to the input");
    press(&mut m, &mut vs, KeyCode::Down);
    press(&mut m, &mut vs, KeyCode::Enter);
    assert_eq!(events, vec![Event::Select { id: 41, index: 0, value: "tasks".into() }]);
}

#[test]
fn a_click_on_an_inline_dialog_reaches_the_row_it_landed_on() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let items = Value::Array(vec!["alpha".into(), "beta".into()]);
    create(&mut m, 50, "picker", OVERLAYS, map(vec![("items", items)]));
    let mut vs = ViewState::new(Mode::Inline);
    let screen = inline(&m, &vs, 40, 20, &ctx());
    let rows = texts(&screen.lines);
    let beta = rows.iter().position(|r| r.ends_with("beta")).unwrap() as u16;

    // the viewport starts at screen row 3
    vs.overlay_hit = screen.overlay.map(|(id, r, c, w, h)| (id, 3 + r, c, w, h));
    let click = MouseEventKind::Down(MouseButton::Left);
    let mut events = Vec::new();
    assert!(!handle_mouse(&mut m, &mut vs, click, 0, 3 + beta, &mut events), "the gutter");
    assert!(handle_mouse(&mut m, &mut vs, click, 4, 3 + beta, &mut events));
    assert_eq!(events, vec![Event::Select { id: 50, index: 1, value: "beta".into() }]);

    // typed away from the dialog: a click on the input below it leaves the keys in the input
    events.clear();
    handle_input(&mut m, &mut vs, key(KeyCode::Char('é'), KeyModifiers::NONE), &mut events);
    handle_input(&mut m, &mut vs, key(KeyCode::Esc, KeyModifiers::NONE), &mut events);
    vs.overlay_blur = Some(50);
    let input_row = 3 + rows.len() as u16 - 1;
    assert!(!handle_mouse(&mut m, &mut vs, click, 4, input_row, &mut events));
    assert_eq!(vs.overlay_blur, Some(50));
}

#[test]
fn down_from_a_dialog_that_does_not_use_it_moves_to_the_input() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 41, "button", BOTTOM, map(vec![("label", "tasks".into())]));
    let actions = Value::Array(vec!["yes".into(), "no".into()]);
    create(&mut m, 50, "card", OVERLAYS, map(vec![("actions", actions)]));
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();

    handle_input(&mut m, &mut vs, key(KeyCode::Down, KeyModifiers::NONE), &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(40), "↓ leaves the dialog");
    handle_input(&mut m, &mut vs, key(KeyCode::Down, KeyModifiers::NONE), &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(41), "and keeps stepping down");
    handle_input(&mut m, &mut vs, key(KeyCode::Tab, KeyModifiers::NONE), &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(50), "tab returns to the dialog");
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn a_dialog_under_a_closed_child_gets_its_keys_back() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let actions = Value::Array(vec!["ok".into()]);
    create(&mut m, 50, "card", OVERLAYS, map(vec![("actions", actions.clone())]));
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();
    handle_input(&mut m, &mut vs, key(KeyCode::Char('x'), KeyModifiers::NONE), &mut events);
    assert_eq!(vs.overlay_blur, Some(50), "typed away from the dialog");

    create(&mut m, 60, "card", OVERLAYS, map(vec![("actions", actions)]));
    handle_input(&mut m, &mut vs, key(KeyCode::Enter, KeyModifiers::NONE), &mut events);
    m.apply(Op::Remove { id: 60 }).unwrap();
    events.clear();
    handle_input(&mut m, &mut vs, key(KeyCode::Enter, KeyModifiers::NONE), &mut events);
    let answered = Event::Select { id: 50, index: 0, value: "ok".into() };
    assert_eq!(events, vec![answered], "the first key after the child closes is the parent's");
}

#[test]
fn typing_on_a_button_prefers_the_bottom_input_over_a_live_card() {
    let mut m = Model::new();
    let actions = Value::Array(vec!["a".into(), "b".into()]);
    create(&mut m, 30, "card", LIVE, map(vec![("actions", actions)]));
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 41, "button", BOTTOM, map(vec![("label", "tasks".into())]));
    let mut vs = ViewState::new(Mode::Inline);
    vs.focus = Some(41);
    let mut events = Vec::new();
    handle_input(&mut m, &mut vs, key(KeyCode::Char('1'), KeyModifiers::NONE), &mut events);
    assert!(events.is_empty(), "the live card's shortcut did not fire: {events:?}");
    assert_eq!(vs.focus, Some(40));
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
    let o = handle_input(&mut m, &mut vs, key(KeyCode::Char('o'), ctrl), &mut events);
    assert_eq!(o, Outcome::ExpandAll);
    let c = handle_input(&mut m, &mut vs, key(KeyCode::Char('c'), ctrl), &mut events);
    assert_eq!(c, Outcome::Interrupt);
    assert!(Chord::parse("hyper+x").is_none());
}
