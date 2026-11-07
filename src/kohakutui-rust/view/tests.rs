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
    // the user line carries its band padding; compare the text
    let trimmed = |lines: &[ratatui::text::Line<'static>]| -> Vec<String> {
        texts(lines)
            .iter()
            .map(|l| l.trim_end().to_owned())
            .collect()
    };
    let mut flushed = take_flushable(&mut m, 40, &c);
    assert_eq!(trimmed(&flushed), vec!["› hi there"], "the open cell has nothing stable yet");

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
    expected.extend(trimmed(&cell_parts(&m, 22, 40, &c).all()));
    assert_eq!(trimmed(&flushed), expected);
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
    assert_eq!(folded, vec!["◆ grep  ▸", "  ⎿  3 matches  (ctrl+o to expand)"]);
    let open = texts(&cell_parts(&m, 20, 60, &c.expanded(true)).all());
    assert!(open.iter().any(|l| l.contains("rg pattern")), "{open:?}");

    let flushed = texts(&take_flushable(&mut m, 60, &c));
    assert_eq!(flushed, folded, "scrollback gets the folded form");
}

#[test]
fn user_messages_sit_on_a_full_width_band_and_others_do_not() {
    let mut m = Model::new();
    create(&mut m, 20, "cell", TRANSCRIPT, map(vec![("role", "user".into())]));
    create(&mut m, 21, "text", 20, map(vec![("value", "fix the flaky test".into())]));
    create(&mut m, 30, "cell", TRANSCRIPT, map(vec![("role", "assistant".into())]));
    create(&mut m, 31, "text", 30, map(vec![("value", "on it".into())]));
    let c = ctx();

    let user = cell_parts(&m, 20, 40, &c).prefix;
    assert_eq!(plain(&user[0]).trim_end(), "› fix the flaky test");
    for line in &user {
        let width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        assert_eq!(width, 40, "padded to the full width");
        assert!(line.spans.iter().all(|s| s.style.bg.is_some()), "every span carries the band");
    }
    let other = cell_parts(&m, 30, 40, &c).prefix;
    assert!(other
        .iter()
        .flat_map(|l| &l.spans)
        .all(|s| s.style.bg.is_none()));
}

#[test]
fn a_hidden_node_is_not_drawn_or_focused_until_shown_again() {
    let c = ctx();
    let mut m = chat();
    m.apply(Op::Seal { id: 22 }).unwrap();
    create(&mut m, 40, "composer", BOTTOM, map(vec![("placeholder", "say hi".into())]));
    create(&mut m, 41, "footer", BOTTOM, map(vec![("left", "model x".into())]));
    take_flushable(&mut m, 60, &c);
    let mut vs = ViewState::new(Mode::Inline);
    let shown = |m: &Model, vs: &ViewState| texts(&inline(m, vs, 60, 20, &c).lines).join("\n");
    assert!(shown(&m, &vs).contains("say hi"));

    let hide = |hidden: Value| Op::Update { id: 40, props: map(vec![("hidden", hidden)]) };
    m.apply(hide(true.into())).unwrap();
    let rows = shown(&m, &vs);
    assert!(!rows.contains("say hi") && rows.contains("model x"), "{rows}");
    // its keys go nowhere: typing does not reach it
    let mut events = Vec::new();
    handle_input(&mut m, &mut vs, key(KeyCode::Char('a'), KeyModifiers::NONE), &mut events);
    assert!(events.is_empty(), "{events:?}");

    // null is the default: shown again
    m.apply(hide(Value::Nil)).unwrap();
    assert!(shown(&m, &vs).contains("say hi"));
}

#[test]
fn a_docked_dialog_takes_the_input_box_place_and_keeps_every_key() {
    let c = ctx();
    let mut m = chat();
    m.apply(Op::Seal { id: 22 }).unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 41, "footer", BOTTOM, map(vec![("left", "model x".into())]));
    let items = Value::Array(vec!["alpha".into(), "beta".into()]);
    let picker = map(vec![
        ("items", items),
        ("dock", "input".into()),
        ("title", "Pick".into()),
    ]);
    create(&mut m, 50, "picker", OVERLAYS, picker);
    take_flushable(&mut m, 60, &c);

    // inline: the picker is drawn where the input and footer were, nothing above them
    let mut vs = ViewState::new(Mode::Inline);
    let screen = inline(&m, &vs, 60, 20, &c);
    let rows = texts(&screen.lines);
    assert!(!rows.iter().any(|r| r.contains("model x")), "the bottom region is replaced");
    let title = rows.iter().position(|r| r.contains("Pick")).unwrap();
    assert!(rows[title - 1].starts_with('╭'), "boxed like the input: {rows:#?}");
    assert!(rows[title].ends_with(" │"), "closed on the right: {rows:#?}");
    assert!(rows.last().unwrap().starts_with('╰'), "and it is the last thing on screen");

    // keys stay in the picker even the ones it ignores; the composer never sees them
    let mut events = Vec::new();
    for ch in "bet".chars() {
        handle_input(&mut m, &mut vs, key(KeyCode::Char(ch), KeyModifiers::NONE), &mut events);
    }
    handle_input(&mut m, &mut vs, key(KeyCode::Down, KeyModifiers::NONE), &mut events);
    handle_input(&mut m, &mut vs, key(KeyCode::Enter, KeyModifiers::NONE), &mut events);
    assert_eq!(
        events,
        vec![Event::Select { id: 50, index: 1, value: "beta".into() }],
        "the filter took the letters and ↓ did not leave the dialog"
    );

    // a docked dialog that ignores letters and ↓ (a card) still keeps them from the hidden input
    m.apply(Op::Remove { id: 50 }).unwrap();
    let card = map(vec![("title", "Job".into()), ("dock", "input".into())]);
    create(&mut m, 52, "card", OVERLAYS, card);
    for code in [KeyCode::Char('x'), KeyCode::Down, KeyCode::Char('y')] {
        handle_input(&mut m, &mut vs, key(code, KeyModifiers::NONE), &mut events);
    }
    assert_eq!(super::layout::input_target(&m, &vs), Some(52), "still the card's keys");
    m.apply(Op::Remove { id: 52 }).unwrap();

    // closing it gives the input box back, untouched
    let rows = texts(&inline(&m, &vs, 60, 20, &c).lines);
    assert!(rows.iter().any(|r| r.contains("model x")));
    assert!(!rows.iter().any(|r| r.contains('x') && r.starts_with('›')), "{rows:#?}");
    assert!(!rows.iter().any(|r| r.contains("› xy") || r.contains("› x")), "{rows:#?}");

    // full screen: at the bottom, not a centred box
    let picker = map(vec![
        ("items", Value::Array(vec!["alpha".into()])),
        ("dock", "input".into()),
        ("title", "Pick".into()),
    ]);
    create(&mut m, 51, "picker", OVERLAYS, picker);
    let mut vs = ViewState::new(Mode::Fullscreen);
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 20));
    fullscreen(&m, &mut vs, &mut buf, &c);
    let rows: Vec<String> = (0..20)
        .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect();
    let at = rows.iter().position(|r| r.contains("Pick")).unwrap();
    assert!(at > 12, "drawn in the bottom area: {rows:#?}");
    assert!(!rows.iter().any(|r| r.starts_with('┌') || r.contains("┐")), "no modal box");
}

/// A batch cell titled "tool calls" with `n` calls (grep ×2, read, then edits), the second failed.
fn batch(m: &mut Model, id: NodeId, n: usize) {
    let props = map(vec![
        ("role", "tool".into()),
        ("batch", 3.into()),
        ("title", "tool calls".into()),
    ]);
    create(m, id, "cell", TRANSCRIPT, props);
    let names = ["grep", "read", "grep", "edit", "edit", "ls"];
    for (i, name) in names.iter().enumerate().take(n) {
        let kid = id + 1 + i as NodeId * 2;
        let status = if i == 1 { "error" } else { "ok" };
        let props = map(vec![
            ("role", "tool".into()),
            ("title", (*name).into()),
            ("status", status.into()),
            ("collapsed", true.into()),
            ("summary", format!("result {i}").as_str().into()),
        ]);
        create(m, kid, "cell", id, props);
        create(
            m,
            kid + 1,
            "text",
            kid,
            map(vec![("value", format!("output {i}").as_str().into())]),
        );
    }
}

#[test]
fn a_batch_is_invisible_under_its_threshold_and_folds_like_the_web_from_it() {
    let c = ctx();
    let mut m = Model::new();
    batch(&mut m, 20, 2);
    let small = texts(&cell_parts(&m, 20, 60, &c).all());
    assert_eq!(
        small,
        vec![
            "✓ grep  ▸",
            "  ⎿  result 0  (ctrl+o to expand)",
            "",
            "✗ read  ▸",
            "  ⎿  result 1  (ctrl+o to expand)"
        ]
    );

    let mut m = Model::new();
    batch(&mut m, 20, 6);
    let parts = cell_parts(&m, 20, 80, &c);
    let rows = texts(&parts.all());
    assert_eq!(rows.len(), 1, "folded to its header: {rows:?}");
    assert!(
        rows[0].starts_with("✗ 6 tool calls  grep ×2, edit ×2, read, ls  1 ✗  ▸"),
        "{rows:?}"
    );
    // the title is only the noun: an untitled group counts "items"
    let mut plain = Model::new();
    batch(&mut plain, 20, 6);
    let untitled = map(vec![("title", Value::Nil)]);
    plain.apply(Op::Update { id: 20, props: untitled }).unwrap();
    let rows = texts(&cell_parts(&plain, 20, 80, &c).all());
    assert!(rows[0].starts_with("✗ 6 items  grep ×2"), "{rows:?}");

    // opened: every call under it, each folded on its own and listed as a header
    m.toggle_open(20);
    let parts = cell_parts(&m, 20, 80, &c);
    assert_eq!(parts.heads.len(), 6, "one header per nested call");
    let rows = texts(&parts.all());
    assert!(rows[0].ends_with("▾"), "{rows:?}");
    assert_eq!(rows[1], "  ✓ grep  ▸");
    assert!(!rows.iter().any(|r| r.contains("output 0")));
    m.toggle_open(21);
    let rows = texts(&cell_parts(&m, 20, 80, &c).all());
    assert!(rows.iter().any(|r| r.contains("output 0")), "a nested call opens on its own");
}

#[test]
fn full_screen_hover_tints_navigation_opens_nested_calls_and_the_wheel_scrolls_boxes() {
    let c = ctx();
    let mut m = Model::new();
    batch(&mut m, 20, 3);
    let tool = map(vec![
        ("role", "tool".into()),
        ("title", "bash".into()),
        ("status", "ok".into()),
    ]);
    create(&mut m, 40, "cell", TRANSCRIPT, tool);
    create(&mut m, 41, "scroll", 40, map(vec![("height", 3.into())]));
    let log = (0..10)
        .map(|i| format!("log {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    create(&mut m, 42, "text", 41, map(vec![("value", log.as_str().into())]));
    for id in [20, 40] {
        m.apply(Op::Seal { id }).unwrap();
    }
    let mut vs = ViewState::new(Mode::Fullscreen);
    let draw = |m: &Model, vs: &mut ViewState| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 16));
        fullscreen(m, vs, &mut buf, &c);
        buf
    };
    let row_text = |buf: &Buffer, y: u16| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>();
    let mut events = Vec::new();

    // hover: the header under the pointer is tinted, and only a change redraws
    let buf = draw(&m, &mut vs);
    let (y, _, id) = vs.hits[0];
    assert_eq!(id, 20);
    assert!(handle_mouse(&mut m, &mut vs, MouseEventKind::Moved, 3, y, &mut events));
    assert!(!handle_mouse(&mut m, &mut vs, MouseEventKind::Moved, 5, y, &mut events));
    let buf2 = draw(&m, &mut vs);
    assert_ne!(buf[(2, y)].bg, buf2[(2, y)].bg, "tinted");

    // keyboard: ctrl+↑ focuses the last header (the batch), enter opens it, ↓ reaches a call
    let ctrl = KeyModifiers::CONTROL;
    let none = KeyModifiers::NONE;
    handle_input(&mut m, &mut vs, key(KeyCode::Up, ctrl), &mut events);
    assert_eq!(vs.nav, Some(20));
    handle_input(&mut m, &mut vs, key(KeyCode::Enter, none), &mut events);
    draw(&m, &mut vs);
    assert_eq!(vs.heads, vec![20, 21, 23, 25], "the opened batch lists its calls");
    handle_input(&mut m, &mut vs, key(KeyCode::Down, none), &mut events);
    handle_input(&mut m, &mut vs, key(KeyCode::Char(' '), none), &mut events);
    let buf = draw(&m, &mut vs);
    assert!(
        (0..16).any(|y| row_text(&buf, y).contains("output 0")),
        "the nested call opened"
    );
    assert!(events.is_empty(), "navigation keys never reached an input");
    handle_input(&mut m, &mut vs, key(KeyCode::Esc, none), &mut events);
    assert_eq!(vs.nav, None);

    // the wheel over the scroll box scrolls the box, not the transcript
    let buf = draw(&m, &mut vs);
    let (top, h, sid) = vs.scroll_hits[0];
    assert_eq!((sid, h), (41, 4), "three rows and the hint");
    assert!((0..16).any(|y| row_text(&buf, y).contains("log 9")));
    let before = vs.scroll;
    assert!(handle_mouse(&mut m, &mut vs, MouseEventKind::ScrollUp, 4, top, &mut events));
    let buf = draw(&m, &mut vs);
    assert_eq!(vs.scroll, before);
    assert!((0..16).any(|y| row_text(&buf, y).contains("log 6")));
    assert!(!(0..16).any(|y| row_text(&buf, y).contains("log 9")), "the box moved up three");
    assert!((0..16).any(|y| row_text(&buf, y).contains("wheel scrolls in full screen")));

    // a box at its top cannot move: the wheel goes on to the transcript
    for _ in 0..3 {
        handle_mouse(&mut m, &mut vs, MouseEventKind::ScrollUp, 4, top, &mut events);
    }
    assert!(vs.scroll > before, "the transcript scrolled once the box could not");

    // a navigated header that goes away ends navigation; toggling a removed id is a no-op
    vs.scroll = 0;
    handle_input(&mut m, &mut vs, key(KeyCode::Up, ctrl), &mut events);
    let nav = vs.nav.unwrap();
    m.apply(Op::Remove { id: nav }).unwrap();
    draw(&m, &mut vs);
    assert_eq!(vs.nav, None);
    m.toggle_open(nav);
    assert!(!m.is_open(nav));
}

#[test]
fn the_caret_of_an_input_in_a_scrolled_box_follows_the_window() {
    let mut m = Model::new();
    create(&mut m, 60, "scroll", OVERLAYS, map(vec![("height", 3.into())]));
    let log = (0..6)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    create(&mut m, 61, "text", 60, map(vec![("value", log.as_str().into())]));
    create(&mut m, 62, "prompt", 60, map(vec![("question", "Q".into())]));
    let mut cursor = None;
    let lines = super::render::node_lines(&m, 60, 40, &ctx(), Some(62), &mut cursor);
    let rows = texts(&lines);
    // tail-following: line 5, the question, the input row, then the hint
    let (r, _) = cursor.expect("the caret is inside the window");
    assert!(rows[r as usize].starts_with('›'), "{rows:#?} caret row {r}");
}

#[test]
fn ctrl_home_and_ctrl_end_jump_the_full_screen_transcript_past_a_focused_input() {
    let mut m = chat();
    let body = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
    m.apply(Op::Append { id: 23, data: body.as_str().into() })
        .unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let mut vs = ViewState::new(Mode::Fullscreen);
    vs.focus = Some(40);
    let c = ctx();
    let draw = |m: &Model, vs: &mut ViewState| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 10));
        fullscreen(m, vs, &mut buf, &c);
        (0..10)
            .map(|y| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let ctrl = KeyModifiers::CONTROL;
    let mut events = Vec::new();

    handle_input(&mut m, &mut vs, key(KeyCode::Home, ctrl), &mut events);
    let top = draw(&m, &mut vs);
    assert!(top.contains("line 0") && top.contains("ctrl+end"), "{top}");
    assert!(vs.scroll > 0);

    handle_input(&mut m, &mut vs, key(KeyCode::End, ctrl), &mut events);
    let bottom = draw(&m, &mut vs);
    assert_eq!(vs.scroll, 0);
    assert!(bottom.contains("line 39") && !bottom.contains("line 0 "), "{bottom}");
    assert!(events.is_empty(), "the composer never saw the keys");
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
    let second = rows.iter().rposition(|r| r == "◆ grep  ▸").unwrap() as u16;
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
    let beta = rows.iter().position(|r| r.contains("  beta ")).unwrap() as u16;

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
    events.clear();
    handle_input(&mut m, &mut vs, key(KeyCode::Esc, KeyModifiers::NONE), &mut events);
    assert_eq!(events, vec![Event::Action { name: "escape".into(), focus: None }]);
    let o = handle_input(&mut m, &mut vs, key(KeyCode::Char('o'), ctrl), &mut events);
    assert_eq!(o, Outcome::ExpandAll);
    let c = handle_input(&mut m, &mut vs, key(KeyCode::Char('c'), ctrl), &mut events);
    assert_eq!(c, Outcome::Interrupt);
    assert!(Chord::parse("hyper+x").is_none());
}

#[test]
fn tab_reaches_a_control_in_an_open_reply_and_esc_gives_the_keys_back() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 50, "cell", TRANSCRIPT, map(vec![("role", "assistant".into())]));
    let slider = map(vec![
        ("control", "slider".into()),
        ("value", 1.into()),
        ("max", 9.into()),
    ]);
    create(&mut m, 51, "field", 50, slider);
    let mut vs = ViewState::new(Mode::Inline);
    let mut events = Vec::new();
    let press = |m: &mut Model, vs: &mut ViewState, k: KeyCode, events: &mut Vec<Event>| {
        handle_input(m, vs, key(k, KeyModifiers::NONE), events)
    };

    press(&mut m, &mut vs, KeyCode::Tab, &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(51), "tab wraps to the form");
    press(&mut m, &mut vs, KeyCode::Right, &mut events);
    // the control draws as focused (its key hint) though it sits inside a cell
    let ctx = ctx().with_focus(super::layout::input_target(&m, &vs));
    let rows = texts(&cell_parts(&m, 50, 60, &ctx).all());
    assert!(rows[0].ends_with("2  ←/→"), "{rows:?}");
    press(&mut m, &mut vs, KeyCode::Esc, &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(40), "esc: back to the input");
    assert_eq!(events, vec![Event::Change { id: 51, value: 2.into() }], "no escape action");

    // once the reply is sealed its controls are static: tab no longer reaches them
    m.apply(Op::Seal { id: 50 }).unwrap();
    press(&mut m, &mut vs, KeyCode::Tab, &mut events);
    assert_eq!(super::layout::input_target(&m, &vs), Some(40));
}

#[test]
fn a_click_on_a_control_in_an_open_full_screen_reply_reaches_it() {
    let mut m = Model::new();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    create(&mut m, 50, "cell", TRANSCRIPT, map(vec![("role", "assistant".into())]));
    create(&mut m, 51, "markdown", 50, map(vec![("source", "pick one".into())]));
    let segments = Value::Array(vec![
        map(vec![("label", "Start".into()), ("value", "start".into())]),
        map(vec![("label", "Reset".into()), ("value", "reset".into())]),
    ]);
    create(&mut m, 52, "button", 50, map(vec![("segments", segments)]));
    let mut vs = ViewState::new(Mode::Fullscreen);
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 12));
    fullscreen(&m, &mut vs, &mut buf, &ctx());
    let row = (0..12u16)
        .find(|&y| {
            (0..60u16)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("Reset")
        })
        .expect("the button is drawn");
    let line: String = (0..60u16).map(|x| buf[(x, row)].symbol()).collect();
    let col = line.find("Reset").unwrap() as u16;

    let mut events = Vec::new();
    let press = MouseEventKind::Down(MouseButton::Left);
    handle_mouse(&mut m, &mut vs, press, col + 1, row, &mut events);
    assert!(
        events.iter().any(
            |e| matches!(e, Event::Select { id: 52, value, .. } if *value == Value::from("reset"))
        ),
        "{events:?}"
    );

    // a sealed reply's controls are static: the same click selects nothing
    m.apply(Op::Seal { id: 50 }).unwrap();
    fullscreen(&m, &mut vs, &mut buf, &ctx());
    events.clear();
    handle_mouse(&mut m, &mut vs, press, col + 1, row, &mut events);
    assert!(!events.iter().any(|e| matches!(e, Event::Select { .. })), "{events:?}");
}

#[test]
fn columns_sit_side_by_side_pack_or_stack_and_tabs_show_one_pane() {
    let mut m = Model::new();
    let text = |v: &str| map(vec![("value", v.into())]);
    create(&mut m, 60, "columns", TRANSCRIPT, map(vec![("count", 2.into())]));
    create(&mut m, 61, "text", 60, text("left"));
    create(&mut m, 62, "text", 60, text("right"));
    let mut sink = None;
    let rows = texts(&super::render::node_lines(&m, 60, 40, &ctx(), None, &mut sink));
    // two columns of (40 - 3) / 2 = 18: "left" padded to 18, the gap of 3, then "right"
    assert_eq!(rows, vec![format!("left{}right", " ".repeat(14 + 3))]);
    // too narrow for two 16-wide columns: they stack
    let rows = texts(&super::render::node_lines(&m, 60, 30, &ctx(), None, &mut sink));
    assert_eq!(rows, vec!["left", "right"]);

    // packed: each child at its own width, wrapping when the row is full
    create(
        &mut m,
        70,
        "columns",
        TRANSCRIPT,
        map(vec![("pack", true.into()), ("gap", 2.into())]),
    );
    for (i, word) in ["[live]", "[v2]", "[beta]"].iter().enumerate() {
        create(&mut m, 71 + i as NodeId, "text", 70, text(word));
    }
    let rows = texts(&super::render::node_lines(&m, 70, 40, &ctx(), None, &mut sink));
    assert_eq!(rows, vec!["[live]  [v2]  [beta]"]);
    let rows = texts(&super::render::node_lines(&m, 70, 14, &ctx(), None, &mut sink));
    assert_eq!(rows, vec!["[live]  [v2]", "[beta]"]);

    let labels = Value::Array(vec!["One".into(), "Two".into()]);
    create(
        &mut m,
        80,
        "tabs",
        TRANSCRIPT,
        map(vec![("labels", labels), ("active", 1.into())]),
    );
    create(&mut m, 81, "text", 80, text("pane one"));
    create(&mut m, 82, "text", 80, text("pane two"));
    let rows = texts(&super::render::node_lines(&m, 80, 40, &ctx(), None, &mut sink));
    assert_eq!(rows, vec![" One │ Two ", "pane two"], "only the active pane");
    // a control in a closed tab cannot take the keys
    let labels = Value::Array(vec!["A".into(), "B".into()]);
    create(
        &mut m,
        90,
        "tabs",
        TRANSCRIPT,
        map(vec![("labels", labels), ("active", 1.into())]),
    );
    create(&mut m, 91, "stack", 90, Value::Map(vec![]));
    create(&mut m, 92, "stack", 90, Value::Map(vec![]));
    create(&mut m, 83, "field", 91, map(vec![("control", "toggle".into())]));
    create(&mut m, 84, "field", 92, map(vec![("control", "toggle".into())]));
    assert!(m.hidden(83) && !m.hidden(84));
    let mut vs = ViewState::new(Mode::Inline);
    vs.focus = Some(83);
    assert_ne!(super::layout::input_target(&m, &vs), Some(83));
}

#[test]
fn a_link_in_full_screen_highlights_under_the_pointer_and_a_click_reports_its_url() {
    let mut m = Model::new();
    create(
        &mut m,
        100,
        "markdown",
        TRANSCRIPT,
        map(vec![("source", "see [docs](https://a.io/d) now".into())]),
    );
    let mut vs = ViewState::new(Mode::Fullscreen);
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 6));
    fullscreen(&m, &mut vs, &mut buf, &ctx());
    vs.link_hits = super::link_hits(&buf);
    let &(row, x0, x1, _) = vs.link_hits.first().expect("a link was drawn");
    assert_eq!(x1 - x0, 4, "the four cells of `docs`");

    // the pointer over the link highlights it; moving off clears it
    let mut none = Vec::new();
    assert!(handle_mouse(&mut m, &mut vs, MouseEventKind::Moved, x0 + 1, row, &mut none));
    let id = vs.link_hover.expect("the link under the pointer");
    super::hover_link(&mut buf, &vs.link_hits, id);
    assert!(buf[(x0, row)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert!(!buf[(x1 + 1, row)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert!(handle_mouse(&mut m, &mut vs, MouseEventKind::Moved, x1 + 2, row, &mut none));
    assert_eq!(vs.link_hover, None);

    let mut events = Vec::new();
    let press = MouseEventKind::Down(MouseButton::Left);
    let release = MouseEventKind::Up(MouseButton::Left);
    handle_mouse(&mut m, &mut vs, press, x0 + 1, row, &mut events);
    handle_mouse(&mut m, &mut vs, release, x0 + 1, row, &mut events);
    assert_eq!(events, vec![Event::Link { url: "https://a.io/d".into() }]);
    // off the link, and a drag over it, report nothing
    events.clear();
    handle_mouse(&mut m, &mut vs, press, x1 + 2, row, &mut events);
    handle_mouse(&mut m, &mut vs, release, x1 + 2, row, &mut events);
    handle_mouse(&mut m, &mut vs, press, x0, row, &mut events);
    handle_mouse(&mut m, &mut vs, MouseEventKind::Drag(MouseButton::Left), x1, row, &mut events);
    handle_mouse(&mut m, &mut vs, release, x1, row, &mut events);
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn ctrl_f_finds_in_the_full_screen_transcript_and_steps_between_matches() {
    let mut m = chat();
    let body = (0..40)
        .map(|i| {
            if i % 10 == 3 {
                format!("needle {i}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect::<String>();
    m.apply(Op::Append { id: 23, data: body.as_str().into() })
        .unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let mut vs = ViewState::new(Mode::Fullscreen);
    vs.focus = Some(40);
    let c = ctx();
    let draw = |m: &Model, vs: &mut ViewState| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 12));
        let caret = fullscreen(m, vs, &mut buf, &c);
        let text = (0..12)
            .map(|y| (0..50).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        (text, caret)
    };
    let mut events = Vec::new();
    let none = KeyModifiers::NONE;
    handle_input(&mut m, &mut vs, key(KeyCode::Char('f'), KeyModifiers::CONTROL), &mut events);
    for ch in "needle".chars() {
        handle_input(&mut m, &mut vs, key(KeyCode::Char(ch), none), &mut events);
    }
    let (screen, caret) = draw(&m, &mut vs);
    assert!(screen.contains("find: needle  4/4"), "{screen}");
    assert!(screen.contains("needle 33"), "the newest match is in view: {screen}");
    let caret_row = caret.map(|(_, y)| y as usize);
    assert!(caret_row.is_some_and(|y| screen.lines().nth(y).unwrap().contains("find:")));
    // older matches
    for _ in 0..3 {
        handle_input(&mut m, &mut vs, key(KeyCode::Enter, none), &mut events);
    }
    let (screen, _) = draw(&m, &mut vs);
    assert!(screen.contains("1/4") && screen.contains("needle 3 "), "{screen}");
    handle_input(&mut m, &mut vs, key(KeyCode::Down, none), &mut events);
    let (screen, _) = draw(&m, &mut vs);
    assert!(screen.contains("2/4") && screen.contains("needle 13"), "{screen}");
    assert!(events.is_empty(), "the composer never saw the keys: {events:?}");
    handle_input(&mut m, &mut vs, key(KeyCode::Esc, none), &mut events);
    let (screen, _) = draw(&m, &mut vs);
    assert!(!screen.contains("find:"));
}

#[test]
fn a_cell_with_actions_is_navigable_and_its_keys_become_actions() {
    let mut m = Model::new();
    let actions = Value::Array(vec![map(vec![
        ("key", "c".into()),
        ("label", "copy".into()),
        ("value", "copy".into()),
    ])]);
    create(
        &mut m,
        20,
        "cell",
        TRANSCRIPT,
        map(vec![("role", "user".into()), ("actions", actions)]),
    );
    create(&mut m, 21, "text", 20, map(vec![("value", "hello".into())]));
    m.apply(Op::Seal { id: 20 }).unwrap();
    create(&mut m, 40, "composer", BOTTOM, Value::Map(vec![]));
    let mut vs = ViewState::new(Mode::Fullscreen);
    vs.focus = Some(40);
    let c = ctx();
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
    fullscreen(&m, &mut vs, &mut buf, &c);
    let mut events = Vec::new();
    handle_input(&mut m, &mut vs, key(KeyCode::Up, KeyModifiers::CONTROL), &mut events);
    assert_eq!(vs.nav, Some(20));
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
    fullscreen(&m, &mut vs, &mut buf, &c);
    let screen: String = (0..8)
        .map(|y| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
        .collect();
    assert!(screen.contains("c copy"), "{screen}");
    handle_input(&mut m, &mut vs, key(KeyCode::Char('c'), KeyModifiers::NONE), &mut events);
    assert_eq!(events, vec![Event::Action { name: "copy".into(), focus: Some(20) }]);
}
