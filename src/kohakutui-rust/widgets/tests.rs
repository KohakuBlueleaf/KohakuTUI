//! Stage: widgets. Components against their rendered lines and the events their keys raise.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};

use super::{build, Component, Ctx, Emit, Input, Mouse, MouseAct, TextEditor, KINDS};
use crate::proto::value::map;
use crate::proto::Value;
use crate::text::{display_width, plain};

fn ctx() -> Ctx {
    let now = Instant::now();
    Ctx::new(now, now)
}

fn texts(c: &dyn Component, width: u16) -> Vec<String> {
    c.lines(width, &ctx()).iter().map(plain).collect()
}

thread_local! {
    /// One clock per test thread, so key timestamps only move forward across calls.
    static CLOCK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Feeds keys `gap` apart on the thread's clock, like a person typing; returns every emit.
fn keys(c: &mut dyn Component, keys: &[(KeyCode, KeyModifiers)], gap: Duration) -> Vec<Emit> {
    let mut out = Vec::new();
    let mut at = CLOCK.with(|c| c.get()).unwrap_or_else(Instant::now);

    for (code, mods) in keys {
        at += gap;
        c.on_input(&Input::Key { code: *code, mods: *mods, at }, &mut out);
    }

    // a fresh call starts after a pause, so the first key never continues a burst
    CLOCK.with(|c| c.set(Some(at + Duration::from_secs(1))));
    out
}

fn chars(s: &str) -> Vec<(KeyCode, KeyModifiers)> {
    s.chars()
        .map(|c| (KeyCode::Char(c), KeyModifiers::NONE))
        .collect()
}

const TYPED: Duration = Duration::from_millis(60);
const PASTED: Duration = Duration::from_micros(300);

#[test]
fn every_registered_kind_builds_and_renders_in_a_narrow_width() {
    for kind in KINDS {
        let c = build(kind, &Value::Map(vec![])).unwrap();
        for line in c.lines(12, &ctx()) {
            assert!(display_width(&plain(&line)) <= 12, "{kind}: {:?}", plain(&line));
        }
    }
    assert!(build("nope", &Value::Map(vec![])).is_err());
}

#[test]
fn editor_moves_by_grapheme_and_keeps_columns_across_lines() {
    let mut ed = TextEditor::default();
    ed.insert("中文ab\nxyz");
    ed.home();
    ed.right();
    assert!(ed.up());
    assert_eq!(&ed.text()[ed.cursor()..], "文ab\nxyz");
    ed.backspace();
    assert_eq!(ed.text(), "文ab\nxyz");
    assert!(!ed.up());
}

#[test]
fn editor_render_places_the_caret_on_wide_characters() {
    let mut ed = TextEditor::default();
    ed.insert("你好世界");
    let (lines, caret) = ed.render(5, Default::default());
    assert_eq!(lines.iter().map(plain).collect::<Vec<_>>(), vec!["你好", "世界"]);
    assert_eq!(caret, (1, 4));
    ed.home();
    ed.right();
    assert_eq!(ed.render(5, Default::default()).1, (0, 2));
}

#[test]
fn composer_submits_typed_text_and_clears() {
    let mut c = build("composer", &Value::Map(vec![])).unwrap();
    let mut seq = chars("hello");
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    let out = keys(c.as_mut(), &seq, TYPED);
    assert_eq!(out, vec![Emit::Submit("hello".into())]);
    assert_eq!(texts(c.as_ref(), 40), vec!["› "]);
}

#[test]
fn composer_treats_enter_inside_a_key_burst_as_a_newline() {
    let mut c = build("composer", &Value::Map(vec![])).unwrap();
    let mut seq = chars("line one");
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    seq.extend(chars("line two"));
    let out = keys(c.as_mut(), &seq, PASTED);
    assert!(out.is_empty(), "a pasted newline must not submit: {out:?}");
    assert_eq!(texts(c.as_ref(), 40), vec!["› line one", "  line two"]);
}

#[test]
fn composer_shift_enter_inserts_a_newline_and_history_walks_back() {
    let mut c = build("composer", &Value::Map(vec![])).unwrap();
    let mut seq = chars("a");
    seq.push((KeyCode::Enter, KeyModifiers::SHIFT));
    seq.extend(chars("b"));
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    let out = keys(c.as_mut(), &seq, TYPED);
    assert_eq!(out, vec![Emit::Submit("a\nb".into())]);
    keys(c.as_mut(), &[(KeyCode::Up, KeyModifiers::NONE)], TYPED);
    assert_eq!(texts(c.as_ref(), 40), vec!["› a", "  b"]);
}

#[test]
fn composer_burst_needs_keys_close_together_not_just_a_fast_enter() {
    let mut c = build("composer", &Value::Map(vec![])).unwrap();
    keys(c.as_mut(), &chars("hi"), PASTED);
    let out = keys(c.as_mut(), &[(KeyCode::Enter, KeyModifiers::NONE)], PASTED);
    assert_eq!(out, vec![Emit::Submit("hi".into())]);
}

fn click(row: u16, col: u16, width: u16) -> Mouse {
    Mouse { act: MouseAct::Click, row, col, width }
}

#[test]
fn a_picker_selects_the_clicked_row_and_scrolls_with_the_wheel() {
    let items = Value::Array(vec!["alpha".into(), "beta".into(), "gamma".into()]);
    let props = map(vec![("title", "Pick".into()), ("items", items)]);
    let mut p = build("picker", &props).unwrap();
    let mut out = Vec::new();

    // rows: title, filter, alpha, beta, gamma, hint
    assert!(!p.on_mouse(&click(1, 3, 30), &mut out), "the filter row selects nothing");
    assert!(!p.on_mouse(&click(5, 3, 30), &mut out), "neither does the hint");
    assert!(p.on_mouse(&click(3, 3, 30), &mut out));
    assert_eq!(out, vec![Emit::Select { index: 1, value: "beta".into() }]);

    let wheel = Mouse { act: MouseAct::ScrollDown, row: 0, col: 0, width: 30 };
    p.on_mouse(&wheel, &mut out);
    assert!(texts(p.as_ref(), 30).contains(&"› gamma".to_owned()));
}

#[test]
fn card_and_confirm_buttons_answer_a_click() {
    let actions = Value::Array(vec!["Back".into(), "Kill".into()]);
    let props = map(vec![("title", "Task".into()), ("actions", actions)]);
    let mut card = build("card", &props).unwrap();
    let mut out = Vec::new();
    let shown = texts(card.as_ref(), 30);
    let kill_col = shown[1].find("Kill").unwrap() as u16;
    assert!(!card.on_mouse(&click(0, kill_col, 30), &mut out), "the title is not a button");
    assert!(card.on_mouse(&click(1, kill_col, 30), &mut out));
    assert_eq!(out, vec![Emit::Select { index: 1, value: "Kill".into() }]);

    let mut confirm = build("confirm", &map(vec![("question", "Sure?".into())])).unwrap();
    out.clear();
    assert!(confirm.on_mouse(&click(1, 1, 30), &mut out));
    assert_eq!(out, vec![Emit::Submit(Value::Boolean(true))]);
}

#[test]
fn back_makes_left_close_a_picker_or_card_with_reason_back() {
    let left = [(KeyCode::Left, KeyModifiers::NONE)];
    let back = vec![Emit::Close("back".into())];
    let items = Value::Array(vec!["a".into()]);
    let mut p = build("picker", &map(vec![("items", items), ("back", true.into())])).unwrap();
    assert_eq!(keys(p.as_mut(), &left, TYPED), back);
    keys(p.as_mut(), &chars("ab"), TYPED);
    assert!(keys(p.as_mut(), &left, TYPED).is_empty(), "inside the filter ← moves the caret");

    let actions = Value::Array(vec!["x".into(), "y".into()]);
    let card_props = map(vec![("actions", actions), ("back", true.into())]);
    let mut card = build("card", &card_props).unwrap();
    assert_eq!(keys(card.as_mut(), &left, TYPED), back);
}

#[test]
fn a_button_highlights_when_focused_and_selects_on_enter() {
    let props = map(vec![
        ("label", "2 tasks".into()),
        ("hint", "↓ to view".into()),
        ("focused_hint", "enter to view".into()),
        ("value", "tasks".into()),
    ]);
    let mut b = build("button", &props).unwrap();
    assert_eq!(texts(b.as_ref(), 40), vec!["2 tasks · ↓ to view"]);
    let lines = b.lines(40, &ctx().focused(true));
    assert_eq!(plain(&lines[0]), " 2 tasks  · enter to view");

    let enter = [(KeyCode::Enter, KeyModifiers::NONE)];
    let selected = vec![Emit::Select { index: 0, value: "tasks".into() }];
    assert_eq!(keys(b.as_mut(), &enter, TYPED), selected);
    let typed = keys(b.as_mut(), &chars("x"), TYPED);
    assert!(typed.is_empty(), "a button takes no text");
}

#[test]
fn composer_lets_down_go_when_it_has_nothing_below() {
    let mut c = build("composer", &Value::Map(vec![])).unwrap();
    let mut out = Vec::new();
    let down = Input::Key { code: KeyCode::Down, mods: KeyModifiers::NONE, at: Instant::now() };
    assert!(!c.on_input(&down, &mut out), "an empty input with no history lets ↓ through");
}

#[test]
fn dialogs_leave_unused_letters_and_digits_for_the_input() {
    let mut confirm = build("confirm", &map(vec![("question", "Sure?".into())])).unwrap();
    let mut out = Vec::new();
    let at = Instant::now();
    let key = |c: char| Input::Key { code: KeyCode::Char(c), mods: KeyModifiers::NONE, at };
    assert!(!confirm.on_input(&key('h'), &mut out), "only y and n are confirm's letters");
    assert!(confirm.on_input(&key('y'), &mut out));

    let actions = Value::Array(vec!["a".into(), "b".into()]);
    let mut card = build("card", &map(vec![("actions", actions)])).unwrap();
    assert!(!card.on_input(&key('7'), &mut out), "a digit with no button is not the card's");
    assert!(card.on_input(&key('2'), &mut out));
}

#[test]
fn a_boxed_composer_draws_its_border_and_keeps_the_caret_inside() {
    let props = map(vec![("boxed", true.into()), ("placeholder", "ask".into())]);
    let mut c = build("composer", &props).unwrap();
    assert_eq!(texts(c.as_ref(), 12), vec!["╭──────────╮", "│ > ask    │", "╰──────────╯"]);
    assert_eq!(c.cursor(12), Some((1, 4)));

    keys(c.as_mut(), &chars("abcdefghij"), TYPED);
    let out = texts(c.as_ref(), 12);
    assert_eq!(out.len(), 4, "a long line wraps inside the box: {out:#?}");
    assert!(out.iter().all(|l| display_width(l) == 12), "{out:#?}");
    assert_eq!(texts(c.as_ref(), 6), vec!["› abcd", "  efgh", "  ij"], "too narrow to box");
}

#[test]
fn composer_submits_a_command_typed_in_full() {
    let cmds = Value::Array(vec![map(vec![("name", "model".into())]), "mode".into()]);
    let mut c = build("composer", &map(vec![("commands", cmds)])).unwrap();
    let mut seq = chars("/model");
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    let out = keys(c.as_mut(), &seq, TYPED);
    assert_eq!(out, vec![Emit::Submit("/model".into())]);
}

#[test]
fn composer_completes_a_slash_command_before_submitting() {
    let cmds = Value::Array(vec![
        map(vec![("name", "model".into()), ("desc", "switch model".into())]),
        map(vec![("name", "clear".into())]),
    ]);
    let mut c = build("composer", &map(vec![("commands", cmds)])).unwrap();
    keys(c.as_mut(), &chars("/mo"), TYPED);
    assert!(texts(c.as_ref(), 40).iter().any(|l| l.contains("/model")));
    let out = keys(c.as_mut(), &[(KeyCode::Enter, KeyModifiers::NONE)], TYPED);
    assert!(out.is_empty());
    assert_eq!(texts(c.as_ref(), 40)[0], "› /model ");
    let out = keys(c.as_mut(), &[(KeyCode::Enter, KeyModifiers::NONE)], TYPED);
    assert_eq!(out, vec![Emit::Submit("/model ".into())]);
}

#[test]
fn picker_filters_fuzzily_and_selects_the_original_index() {
    let items = Value::Array(vec!["gpt-5".into(), "claude-opus".into(), "gemini-pro".into()]);
    let mut p = build("picker", &map(vec![("items", items)])).unwrap();
    keys(p.as_mut(), &chars("cop"), TYPED);
    let shown = texts(p.as_ref(), 40);
    assert!(shown.iter().any(|l| l.contains("claude-opus")), "{shown:?}");
    assert!(!shown.iter().any(|l| l.contains("gpt-5")), "{shown:?}");
    let out = keys(p.as_mut(), &[(KeyCode::Enter, KeyModifiers::NONE)], TYPED);
    assert_eq!(out, vec![Emit::Select { index: 1, value: "claude-opus".into() }]);
}

#[test]
fn confirm_and_card_emit_their_choice() {
    let mut c = build("confirm", &map(vec![("question", "Go?".into())])).unwrap();
    assert_eq!(keys(c.as_mut(), &chars("y"), TYPED), vec![Emit::Submit(Value::Boolean(true))]);
    let actions = Value::Array(vec![
        map(vec![("id", "ok".into()), ("label", "OK".into())]),
        map(vec![("id", "no".into()), ("label", "Cancel".into())]),
    ]);
    let mut card = build("card", &map(vec![("actions", actions)])).unwrap();
    let out = keys(
        card.as_mut(),
        &[
            (KeyCode::Right, KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
        ],
        TYPED,
    );
    assert_eq!(out, vec![Emit::Select { index: 1, value: "no".into() }]);
}

#[test]
fn log_rewrites_the_line_on_carriage_return_and_keeps_the_tail() {
    let mut log = build("log", &map(vec![("height", 2.into())])).unwrap();
    log.append(&"a\nb\nprogress 10%\rprogress 90%\n".into())
        .unwrap();
    assert_eq!(texts(log.as_ref(), 40), vec!["b", "progress 90%"]);
    assert!(log.append(&Value::from(3)).is_err());
}

#[test]
fn sparkline_marks_nan_as_a_gap() {
    let mut s = build("sparkline", &map(vec![("label", "loss".into())])).unwrap();
    s.append(&Value::Array(vec![Value::F64(1.0), Value::F64(f64::NAN), Value::F64(0.5)]))
        .unwrap();
    let line = &texts(s.as_ref(), 40)[0];
    assert!(line.starts_with("loss █·▁"), "{line:?}");
}

#[test]
fn chart_keeps_the_whole_run_and_draws_it_across_the_width() {
    let mut c = build("chart", &map(vec![("label", "loss".into()), ("height", 4.into())])).unwrap();
    let falling: Vec<Value> = (0..1000)
        .map(|i| Value::F64(10.0 - i as f64 / 100.0))
        .collect();
    c.append(&map(vec![("loss", Value::Array(falling))]))
        .unwrap();
    c.append(&map(vec![("ema", 5.into())])).unwrap();

    let out = texts(c.as_ref(), 40);
    assert_eq!(out.len(), 1 + 4 + 1, "header, 4 plot rows, x axis: {out:#?}");
    assert!(out[0].starts_with("loss  ■ loss 0.0100  ■ ema 5.0000"), "{out:#?}");
    assert!(out.iter().all(|l| display_width(l) <= 40), "{out:#?}");
    assert!(out[5].ends_with("1000"), "the x axis spans the whole run: {out:#?}");

    let top = out[1].chars().skip_while(|c| *c != '┤').nth(1);
    assert!(top.is_some_and(|c| c != ' '), "the curve starts at the top left: {out:#?}");
    let bottom_last = out[4].trim_end().chars().last();
    assert!(bottom_last.is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c)), "{out:#?}");
}

#[test]
fn chart_halves_its_points_past_max_points_but_keeps_counting() {
    let props = map(vec![("label", "x".into()), ("max_points", 100.into())]);
    let mut c = build("chart", &props).unwrap();
    for i in 0..1000 {
        c.append(&Value::F64(f64::from(i))).unwrap();
    }
    c.append(&Value::F64(f64::NAN)).unwrap();
    let out = texts(c.as_ref(), 40);
    assert!(out[0].contains("(1 nan)"), "{out:#?}");
    assert!(out.last().unwrap().ends_with("1001"), "{out:#?}");
    assert!(c.append(&"text".into()).is_err());
}

#[test]
fn progress_shows_value_total_and_percentage() {
    let p = build(
        "progress",
        &map(vec![
            ("value", 25.into()),
            ("total", 100.into()),
            ("label", "epoch".into()),
        ]),
    )
    .unwrap();
    let line = &texts(p.as_ref(), 60)[0];
    assert!(line.starts_with("epoch ━"), "{line:?}");
    assert!(line.contains("25/100  25%"), "{line:?}");
    assert!(display_width(line) <= 60);
}

#[test]
fn table_reads_rows_as_arrays_or_maps() {
    let cols = Value::Array(vec![
        map(vec![("key", "n".into()), ("label", "Name".into())]),
        "v".into(),
    ]);
    let mut t = build("table", &map(vec![("columns", cols)])).unwrap();
    t.append(&map(vec![("n", "a".into()), ("v", 1.into())]))
        .unwrap();
    t.append(&Value::Array(vec!["bb".into(), 22.into()]))
        .unwrap();
    let out = texts(t.as_ref(), 40);
    assert_eq!(out[0], "Name │ v ");
    assert_eq!(out[2], "a    │ 1 ");
    assert_eq!(out[3], "bb   │ 22");
}
