//! Stage: term. Lines and frames against what a terminal emulator (vt100) shows afterwards:
//! its scrollback plus its screen.

use ratatui::buffer::Buffer;
use ratatui::text::Line;

use super::{Backend, History, Term, TestScreen};

/// A terminal of `cols`×`rows` with the cursor at `start_row`, using `history`.
fn term(cols: u16, rows: u16, start_row: u16, history: History) -> (Term, TestScreen) {
    let screen = TestScreen::new(cols, rows);
    let mut s = screen.clone();
    for _ in 0..start_row {
        std::io::Write::write_all(&mut s, b"shell\r\n").unwrap();
    }
    let mut t = Term::open(&Backend::Test(screen.clone())).unwrap();
    t.history = history;
    (t, screen)
}

fn frame(t: &mut Term, lines: &[&str]) {
    t.begin_frame().unwrap();
    t.set_viewport(lines.len() as u16).unwrap();
    let area = t.area();
    let mut buf = Buffer::empty(area);
    for (i, l) in lines.iter().enumerate() {
        buf.set_line(0, area.y + i as u16, &Line::from(*l), area.width);
    }
    t.draw(&buf, None).unwrap();
    t.end_frame().unwrap();
}

fn insert(t: &mut Term, lines: &[String]) {
    let lines: Vec<Line<'static>> = lines.iter().map(|l| Line::from(l.clone())).collect();
    t.begin_frame().unwrap();
    t.insert_history(&lines).unwrap();
    t.end_frame().unwrap();
}

/// Streams 25 history lines in uneven batches under a 2-row viewport, redrawing between.
fn stream_history(history: History, start_row: u16) -> (Vec<String>, TestScreen) {
    let (mut t, screen) = term(30, 8, start_row, history);
    frame(&mut t, &["› typing", "footer"]);

    let all: Vec<String> = (0..25).map(|i| format!("history {i:02}")).collect();
    let mut i = 0;
    for batch in [1, 3, 1, 7, 2, 11] {
        insert(&mut t, &all[i..i + batch]);
        frame(&mut t, &["› typing", "footer"]);
        i += batch;
    }
    (all, screen)
}

fn assert_history_then_viewport(all: &[String], screen: &TestScreen, shell_rows: usize) {
    let shown = screen.history();
    let body: Vec<&String> = shown.iter().filter(|l| l.starts_with("history")).collect();
    assert_eq!(body.len(), all.len(), "every line exactly once: {shown:#?}");
    assert!(body.iter().zip(all).all(|(a, b)| *a == b), "in order: {shown:#?}");
    assert_eq!(&shown[shown.len() - 2..], ["› typing", "footer"], "{shown:#?}");
    assert_eq!(shown.iter().filter(|l| *l == "shell").count(), shell_rows);
}

#[test]
fn region_history_lands_in_scrollback_once_and_in_order() {
    let (all, screen) = stream_history(History::Region, 0);
    assert_history_then_viewport(&all, &screen, 0);
    let visible = screen.screen();
    assert_eq!(visible[6..], ["› typing", "footer"], "viewport pinned to the bottom");
}

#[test]
fn region_history_keeps_the_shell_output_above_it() {
    let (all, screen) = stream_history(History::Region, 3);
    assert_history_then_viewport(&all, &screen, 3);
}

#[test]
fn reprint_history_gives_the_same_scrollback() {
    let (all, screen) = stream_history(History::Reprint, 2);
    assert_history_then_viewport(&all, &screen, 2);
}

#[test]
fn a_viewport_taller_than_the_free_rows_scrolls_the_screen_up() {
    let (mut t, screen) = term(30, 8, 6, History::Region);
    frame(&mut t, &["a", "b", "c", "d"]);
    let visible = screen.screen();
    assert_eq!(visible[4..], ["a", "b", "c", "d"]);
    assert_eq!(screen.history().iter().filter(|l| *l == "shell").count(), 6);
}

#[test]
fn fullscreen_round_trip_restores_the_inline_screen() {
    let (mut t, screen) = term(30, 8, 0, History::Region);
    frame(&mut t, &["› inline"]);
    insert(&mut t, &["kept".to_owned()]);
    frame(&mut t, &["› inline"]);

    t.enter_fullscreen().unwrap();
    frame(&mut t, &["FULL SCREEN"; 8]);
    assert!(screen.screen().iter().all(|l| l == "FULL SCREEN"));

    t.leave_fullscreen().unwrap();
    frame(&mut t, &["› inline"]);
    let shown = screen.history();
    assert_eq!(shown, vec!["kept", "› inline"]);
    assert!(
        !shown.iter().any(|l| l.contains("FULL")),
        "the alternate screen leaves no trace"
    );
}

#[test]
fn close_clears_the_viewport_and_keeps_history() {
    let (mut t, screen) = term(30, 8, 0, History::Region);
    insert(&mut t, &["done".to_owned()]);
    frame(&mut t, &["› composer", "footer"]);
    t.close().unwrap();
    assert_eq!(screen.history(), vec!["done"]);
    assert_eq!(screen.cursor(), (1, 0), "the shell continues right under the output");
}

#[test]
fn linked_cells_and_lines_are_wrapped_in_osc8_and_printed_without_the_mark() {
    use crate::text::link::{self, CLOSE};
    use ratatui::text::Span;
    let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 12, 1));
    let line = Line::from(vec![
        Span::raw("go "),
        Span::styled("docs", link::style("https://a.io/d")),
    ]);
    buf.set_line(0, 0, &line, 12);
    let empty = Buffer::empty(buf.area);
    let mut out = Vec::new();
    super::screen::draw_cells(&mut out, empty.diff(&buf)).unwrap();
    let out = String::from_utf8(out).unwrap();
    let id = link::id_of(link::mark("https://a.io/d").underline_color).unwrap();
    let open = format!("\x1b]8;id=kt{id};https://a.io/d\x1b\\");
    // one open before the link's first cell, one close after its last, nothing else linked
    assert_eq!(out.matches(&open).count(), 1, "{out:?}");
    assert_eq!(out.matches(CLOSE).count(), 1, "{out:?}");
    let (before, rest) = out.split_once(&open).unwrap();
    assert!(before.contains("go") && !before.contains('d'), "{out:?}");
    let (inside, _) = rest.split_once(CLOSE).unwrap();
    assert!(inside.contains('d') && inside.contains('s'), "{out:?}");
    // the mark is never printed as an underline colour (SGR 58)
    assert!(!out.contains("[58"), "{out:?}");

    let mut out = Vec::new();
    super::history::write_line(&mut out, &line).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("{open}docs{CLOSE}")), "{out:?}");
}
