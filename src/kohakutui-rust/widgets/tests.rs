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
    // the selected row is a full-width bar: compare its text
    let rows = texts(p.as_ref(), 30);
    assert!(rows.iter().any(|r| r.trim_end() == "› gamma"), "{rows:?}");
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
    // ← is never a caret key in a picker: with nothing to its left it goes back
    assert_eq!(keys(p.as_mut(), &left, TYPED), back);

    let actions = Value::Array(vec!["x".into(), "y".into()]);
    let card_props = map(vec![("actions", actions), ("back", true.into())]);
    let mut card = build("card", &card_props).unwrap();
    assert_eq!(keys(card.as_mut(), &left, TYPED), back);
}

#[test]
fn a_segmented_button_chooses_a_segment_with_left_and_right() {
    let none = KeyModifiers::NONE;
    let seg = |l: &str, v: &str| map(vec![("label", l.into()), ("value", v.into())]);
    let segments = Value::Array(vec![
        seg("● 2 creatures", "creatures"),
        seg("! 1 waiting", "approvals"),
    ]);
    let mut b = build("button", &map(vec![("segments", segments.clone())])).unwrap();
    assert_eq!(texts(b.as_ref(), 60), vec!["● 2 creatures  ·  ! 1 waiting"]);

    // focused: the chosen segment is the highlighted one; → moves it, ← at the start stays
    let focused = |b: &dyn Component| plain(&b.lines(60, &ctx().focused(true))[0]);
    assert!(focused(b.as_ref()).starts_with(" ● 2 creatures "));
    assert!(keys(b.as_mut(), &[(KeyCode::Right, none)], TYPED).is_empty());
    assert!(focused(b.as_ref()).ends_with(" ! 1 waiting "), "{}", focused(b.as_ref()));
    let out = keys(b.as_mut(), &[(KeyCode::Enter, none)], TYPED);
    assert_eq!(out, vec![Emit::Select { index: 1, value: "approvals".into() }]);

    // an update keeps the choice; a click picks the segment under it
    b.update(&map(vec![("segments", segments)])).unwrap();
    let mut out = Vec::new();
    b.on_mouse(&Mouse { act: MouseAct::Click, row: 0, col: 3, width: 60 }, &mut out);
    assert_eq!(out, vec![Emit::Select { index: 0, value: "creatures".into() }]);
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
fn backslash_enter_inserts_a_newline_instead_of_submitting() {
    let mut c = build("composer", &map(vec![])).unwrap();
    let mut seq = chars("one\\");
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    seq.extend(chars("two"));
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    let out = keys(c.as_mut(), &seq, TYPED);
    assert_eq!(out, vec![Emit::Submit("one\ntwo".into())]);
}

#[test]
fn rich_submit_tells_ctrl_enter_from_enter() {
    let mut c = build("composer", &map(vec![("rich_submit", true.into())])).unwrap();
    let mut seq = chars("hi");
    seq.push((KeyCode::Enter, KeyModifiers::CONTROL));
    seq.extend(chars("yo"));
    seq.push((KeyCode::Enter, KeyModifiers::NONE));
    // ctrl+s is the same "send now" where a terminal cannot tell ctrl+enter apart
    seq.extend(chars("go"));
    seq.push((KeyCode::Char('s'), KeyModifiers::CONTROL));
    let out = keys(c.as_mut(), &seq, TYPED);
    let sub = |t: &str, now: bool| {
        Emit::Submit(Value::Map(vec![("text".into(), t.into()), ("now".into(), now.into())]))
    };
    assert_eq!(out, vec![sub("hi", true), sub("yo", false), sub("go", true)]);
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
fn picker_rows_with_choices_group_open_on_current_and_cycle_options() {
    let none = KeyModifiers::NONE;
    let opts = |v: &[&str]| Value::Array(v.iter().map(|o| (*o).into()).collect());
    let model = |name: &str, group: &str, current: bool, choices: Value| {
        map(vec![
            ("label", name.into()),
            ("value", format!("{group}/{name}").as_str().into()),
            ("group", group.into()),
            ("current", current.into()),
            ("choices", choices),
            ("chosen", map(vec![("reasoning", "high".into())])),
        ])
    };
    let groups = Value::Array(vec![
        Value::Array(vec!["reasoning".into(), opts(&["low", "high", "max"])]),
        Value::Array(vec!["speed".into(), opts(&["normal", "fast"])]),
    ]);
    let items = Value::Array(vec![
        model("gpt-5.5", "openai", false, Value::Array(vec![])),
        model("gpt-6-astra", "codex", true, groups),
        model("gpt-5.4", "codex", false, Value::Array(vec![])),
    ]);
    let mut p = build("picker", &map(vec![("items", items)])).unwrap();

    let shown = texts(p.as_ref(), 80);
    assert!(
        shown.contains(&"openai".to_owned()) && shown.contains(&"codex".to_owned()),
        "{shown:?}"
    );
    let row = shown.iter().find(|l| l.contains("gpt-6-astra")).unwrap();
    assert!(row.starts_with("› gpt-6-astra  [reasoning=high]  [speed=normal]"), "{row}");
    let hint = shown.last().unwrap();
    assert!(hint.contains("↑↓ move") && hint.contains("→ options"), "{hint}");

    // → focuses the options; ↑↓ change the focused one, → moves to the next group
    keys(p.as_mut(), &[(KeyCode::Right, none)], TYPED);
    let hint = texts(p.as_ref(), 80).last().unwrap().clone();
    assert!(hint.contains("↑↓ change reasoning") && hint.contains("← list"), "{hint}");
    let seq = [
        (KeyCode::Down, none),
        (KeyCode::Right, none),
        (KeyCode::Up, none),
    ];
    assert!(keys(p.as_mut(), &seq, TYPED).is_empty());
    let out = keys(p.as_mut(), &[(KeyCode::Enter, none)], TYPED);
    let chosen = map(vec![("reasoning", "max".into()), ("speed", "fast".into())]);
    let value = Value::Map(vec![
        ("value".into(), "codex/gpt-6-astra".into()),
        ("choices".into(), chosen),
    ]);
    assert_eq!(out, vec![Emit::Select { index: 1, value }]);

    // the filter matches the group too; plain rows still select their plain value
    keys(p.as_mut(), &chars("open"), TYPED);
    let out = keys(p.as_mut(), &[(KeyCode::Enter, none)], TYPED);
    assert_eq!(out, vec![Emit::Select { index: 0, value: "openai/gpt-5.5".into() }]);
}

#[test]
fn a_multi_picker_ticks_with_space_and_answers_every_ticked_value_in_order() {
    let none = KeyModifiers::NONE;
    let item = |v: &str, checked: bool| {
        map(vec![
            ("label", v.into()),
            ("value", v.into()),
            ("checked", checked.into()),
        ])
    };
    let items = Value::Array(vec![item("red", false), item("green", true), item("blue", false)]);
    let mut p = build("picker", &map(vec![("items", items), ("multi", true.into())])).unwrap();
    let rows = texts(p.as_ref(), 40);
    assert_eq!(rows[1].trim_end(), "› ☐ red");
    assert_eq!(rows[2].trim_end(), "  ☑ green");
    assert!(rows.last().unwrap().contains("space tick"));

    // tick blue (after green in item order), untick nothing else; enter answers both
    let seq = [
        (KeyCode::Down, none),
        (KeyCode::Down, none),
        (KeyCode::Char(' '), none),
    ];
    assert!(keys(p.as_mut(), &seq, TYPED).is_empty(), "space ticks, it does not filter");
    let out = keys(p.as_mut(), &[(KeyCode::Enter, none)], TYPED);
    let picked = Value::Array(vec!["green".into(), "blue".into()]);
    assert_eq!(out, vec![Emit::Select { index: 2, value: picked }]);

    // nothing ticked: enter answers the highlighted row alone
    let items = Value::Array(vec![item("a", false), item("b", false)]);
    let mut p = build("picker", &map(vec![("items", items), ("multi", true.into())])).unwrap();
    let out = keys(p.as_mut(), &[(KeyCode::Down, none), (KeyCode::Enter, none)], TYPED);
    let only = Value::Array(vec!["b".into()]);
    assert_eq!(out, vec![Emit::Select { index: 1, value: only }]);
}

#[test]
fn the_focused_area_is_tinted_and_its_selected_row_is_a_selection_bar() {
    use crate::text::theme;
    let none = KeyModifiers::NONE;
    let item = |name: &str, group: &str| {
        map(vec![
            ("label", name.into()),
            ("value", name.into()),
            ("group", group.into()),
        ])
    };
    let items = Value::Array(vec![item("a1", "alpha"), item("a2", "alpha"), item("b1", "beta")]);
    let props = map(vec![
        ("items", items),
        ("sidebar", true.into()),
        ("max_visible", 3.into()),
    ]);
    let mut p = build("picker", &props).unwrap();
    let tint = theme::focus_area().bg.expect("the default theme tints");
    let bar = theme::selection_bg().bg.unwrap();
    assert_ne!(tint, bar, "the area and the selection must look different");

    // backgrounds of the sections column and of the list column on body row `r`
    // the column fits the longest section name ("alpha" + 8); then "│ " separates the list
    let side_w = 13;
    let bgs = |p: &dyn Component, r: usize| {
        let line = &p.lines(50, &ctx())[1 + r];
        let (mut col, mut side, mut list) = (0usize, Vec::new(), Vec::new());
        for span in &line.spans {
            let w = crate::text::display_width(&span.content);
            match col {
                c if c < side_w => side.push(span.style.bg),
                c if c >= side_w + 2 => list.push(span.style.bg),
                _ => {}
            }
            col += w;
        }
        (side, list)
    };

    // list focused: the list is tinted, its selected row is the bar, the sections are plain
    let (side, list) = bgs(p.as_ref(), 0);
    assert!(side.iter().all(|b| b.is_none() || *b == Some(bar)), "{side:?}");
    assert!(list.iter().skip(1).all(|b| *b == Some(bar)), "selected row: {list:?}");
    let (_, list) = bgs(p.as_ref(), 1);
    assert!(list.iter().skip(1).all(|b| *b == Some(tint)), "other rows: {list:?}");

    // sections focused: now the column is tinted and the current section is the bar
    keys(p.as_mut(), &[(KeyCode::Left, none)], TYPED);
    let (side, list) = bgs(p.as_ref(), 1);
    assert!(side.iter().all(|b| *b == Some(tint)), "{side:?}");
    assert!(list.iter().all(|b| b.is_none()), "the list is no longer tinted: {list:?}");
    let (side, _) = bgs(p.as_ref(), 0);
    assert!(side.contains(&Some(bar)), "current section: {side:?}");

    // rows past a short section keep the separator in its column, focused area or not
    for zone_key in [KeyCode::Right, KeyCode::Left] {
        keys(p.as_mut(), &[(zone_key, none)], TYPED);
        let rows = texts(p.as_ref(), 50);
        let cols: Vec<usize> = rows[1..4]
            .iter()
            .map(|r| r.chars().position(|c| c == '│').unwrap())
            .collect();
        assert!(cols.iter().all(|c| *c == side_w), "{rows:#?}");
    }
}

#[test]
fn a_sidebar_picker_lists_one_category_at_a_time_and_follows_the_filter() {
    let none = KeyModifiers::NONE;
    let opts = |v: &[&str]| Value::Array(v.iter().map(|o| (*o).into()).collect());
    let item = |name: &str, group: &str, current: bool| {
        let choices = Value::Array(vec![
            Value::Array(vec!["reasoning".into(), opts(&["low", "high"])]),
            Value::Array(vec!["speed".into(), opts(&["normal", "fast"])]),
        ]);
        map(vec![
            ("label", name.into()),
            ("value", format!("{group}/{name}").as_str().into()),
            ("group", group.into()),
            ("current", current.into()),
            (
                "choices",
                if group == "codex" {
                    choices
                } else {
                    Value::Array(vec![])
                },
            ),
        ])
    };
    let items = Value::Array(vec![
        item("gpt-5.5", "openai", false),
        item("gpt-6-astra", "codex", true),
        item("gpt-5.4", "codex", false),
        item("claude-opus", "anthropic", false),
    ]);
    let props = map(vec![
        ("items", items),
        ("sidebar", true.into()),
        ("max_visible", 4.into()),
    ]);
    let mut p = build("picker", &props).unwrap();

    // opens on the current item's category; the column counts items per category
    let shown = texts(p.as_ref(), 70);
    let rows = &shown[1..5];
    let first = rows[0].starts_with("  openai") && rows[0].contains("1 │ › gpt-6-astra");
    assert!(first, "{shown:#?}");
    let second = rows[1].starts_with("› codex") && rows[1].contains("2 │   gpt-5.4");
    assert!(second, "{shown:#?}");
    assert!(
        shown
            .iter()
            .all(|l| !l.contains("gpt-5.5") && !l.contains("claude")),
        "{shown:#?}"
    );
    assert_eq!(shown.len(), 6, "title-less: filter, 4 fixed rows, hint");
    assert!(shown[5].contains("← sections") && shown[5].contains("→ options"), "{shown:#?}");

    // ← focuses the sections: ↓ is the next section there, not the next item
    keys(p.as_mut(), &[(KeyCode::Left, none)], TYPED);
    let hint = texts(p.as_ref(), 70)[5].clone();
    assert!(hint.contains("↑↓ section") && hint.contains("→ list"), "{hint}");
    keys(p.as_mut(), &[(KeyCode::Down, none)], TYPED);
    assert!(texts(p.as_ref(), 70)
        .iter()
        .any(|l| l.contains("claude-opus")));
    keys(p.as_mut(), &[(KeyCode::Up, none)], TYPED);
    // → back to the list, → into the options, → to the speed group, ↓ to fast
    let seq = [
        (KeyCode::Right, none),
        (KeyCode::Right, none),
        (KeyCode::Right, none),
        (KeyCode::Down, none),
        (KeyCode::Enter, none),
    ];
    let out = keys(p.as_mut(), &seq, TYPED);
    let chosen = map(vec![("reasoning", "low".into()), ("speed", "fast".into())]);
    let value = Value::Map(vec![
        ("value".into(), "codex/gpt-6-astra".into()),
        ("choices".into(), chosen),
    ]);
    assert_eq!(out, vec![Emit::Select { index: 1, value }]);

    // typing always filters (and gives the keys to the list); with no match here the list
    // moves to a category that has one
    keys(p.as_mut(), &chars("opus"), TYPED);
    assert!(texts(p.as_ref(), 70).last().unwrap().contains("↑↓ move"));
    let shown = texts(p.as_ref(), 70);
    assert!(shown.iter().any(|l| l.starts_with("› anthropic")), "{shown:#?}");
    assert!(shown[1].contains("│ › claude-opus"), "{shown:#?}");
    assert!(
        shown
            .iter()
            .any(|l| l.starts_with("  codex") && l.contains(" 0 │")),
        "{shown:#?}"
    );

    // `section` opens on a named section
    let named = map(vec![
        (
            "items",
            Value::Array(vec![item("x", "openai", false), item("y", "anthropic", false)]),
        ),
        ("sidebar", true.into()),
        ("section", "anthropic".into()),
    ]);
    let q = build("picker", &named).unwrap();
    assert!(texts(q.as_ref(), 70).iter().any(|l| l.contains("› y")));

    // narrow: a tab row instead of the column
    let narrow = texts(p.as_ref(), 40);
    assert!(narrow[1].contains("openai · codex · anthropic"), "{narrow:#?}");
}

#[test]
fn a_scroll_box_windows_its_content_follows_the_tail_and_maps_action_keys() {
    use ratatui::text::Line;
    let none = KeyModifiers::NONE;
    let keys_map = map(vec![("k", "stop".into())]);
    let props = map(vec![
        ("height", 3.into()),
        ("keys", keys_map),
        ("dock", "input".into()),
    ]);
    let mut sc = build("scroll", &props).unwrap();
    let content = |n: usize| {
        (0..n)
            .map(|i| Line::from(format!("line {i}")))
            .collect::<Vec<_>>()
    };
    let shown = |sc: &dyn Component, n: usize| -> Vec<String> {
        sc.frame_children(content(n), 60)
            .iter()
            .map(crate::text::plain)
            .collect()
    };

    // follows the tail, with a hint naming the range, the keys and esc
    let rows = shown(sc.as_ref(), 10);
    assert_eq!(&rows[..3], ["line 7", "line 8", "line 9"]);
    assert!(
        rows[3].contains("8–10 of 10")
            && rows[3].contains("k stop")
            && rows[3].contains("esc close")
    );

    // ↑ scrolls back, home goes to the top, new content does not move a scrolled view
    keys(sc.as_mut(), &[(KeyCode::Up, none)], TYPED);
    assert_eq!(shown(sc.as_ref(), 10)[..3], ["line 6", "line 7", "line 8"]);
    keys(sc.as_mut(), &[(KeyCode::Home, none)], TYPED);
    assert_eq!(shown(sc.as_ref(), 10)[0], "line 0");
    keys(sc.as_mut(), &[(KeyCode::End, none)], TYPED);
    assert_eq!(
        shown(sc.as_ref(), 12)[..3],
        ["line 9", "line 10", "line 11"],
        "back to following"
    );

    // a mapped key selects its action; an unmapped one is not consumed
    let out = keys(sc.as_mut(), &chars("k"), TYPED);
    assert_eq!(out, vec![Emit::Select { index: 0, value: "stop".into() }]);
    assert!(!sc.on_input(
        &Input::Key { code: KeyCode::Char('z'), mods: none, at: Instant::now() },
        &mut Vec::new()
    ));

    // short content is shown whole, with no range in the hint
    let rows = shown(sc.as_ref(), 2);
    assert_eq!(&rows[..2], ["line 0", "line 1"]);
    assert!(!rows[2].contains(" of "));
}

#[test]
fn a_secret_prompt_draws_dots_and_answers_the_real_text() {
    let props = map(vec![("question", "API key".into()), ("secret", true.into())]);
    let mut p = build("prompt", &props).unwrap();
    keys(p.as_mut(), &chars("sk-abc"), TYPED);
    let shown = texts(p.as_ref(), 40);
    assert_eq!(shown[1], "› ••••••");
    assert!(!shown.iter().any(|l| l.contains("sk-abc")));
    assert_eq!(p.cursor(40), Some((1, 8)));
    let out = keys(p.as_mut(), &[(KeyCode::Enter, KeyModifiers::NONE)], TYPED);
    assert_eq!(out, vec![Emit::Submit("sk-abc".into())]);
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
fn cell_header_shows_title_detail_and_only_meaningful_status() {
    let props = map(vec![
        ("role", "tool".into()),
        ("title", "bash".into()),
        ("detail", "pytest -q".into()),
        ("status", "running".into()),
        ("tone", "tool".into()),
    ]);
    let mut c = build("cell", &props).unwrap();
    let header = texts(c.as_ref(), 40);
    assert_eq!(header.len(), 1);
    assert!(
        header[0].ends_with("bash  pytest -q"),
        "running is shown by the spinner: {header:?}"
    );
    c.update(&map(vec![("status", "waiting for approval".into())]))
        .unwrap();
    assert!(texts(c.as_ref(), 60)[0].starts_with("⚙ bash  pytest -q  waiting for approval"));
    c.update(&map(vec![("status", "interrupted".into())]))
        .unwrap();
    assert_eq!(texts(c.as_ref(), 60)[0], "○ bash  pytest -q");
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

#[test]
fn markdown_shows_a_new_source_of_the_same_length() {
    // the render cache is keyed by width and source length: drawn, then replaced by an
    // equally long source, it must draw the new text (a state value 3 -> 5 in a sentence)
    let mut md = build("markdown", &map(vec![("source", "will start 3 workers".into())])).unwrap();
    assert_eq!(texts(md.as_ref(), 40), vec!["will start 3 workers"]);
    md.update(&map(vec![("source", "will start 5 workers".into())]))
        .unwrap();
    assert_eq!(texts(md.as_ref(), 40), vec!["will start 5 workers"]);
}
