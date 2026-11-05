//! Stage: text. Wrapping and Markdown against display width; streaming against the final render.

use proptest::prelude::*;
use ratatui::style::Style;
use ratatui::text::Span;

use super::{display_width, plain, render_markdown, wrap_spans, wrap_text, MdLine, MdStream};

const REPLY: &str = "# Plan\n\nHere is the plan for **the refactor**, step by step:\n\n\
1. Parse the `config` file\n2. Validate every field\n   - nested *item*\n3. Write it back\n\n\
```python\ndef main():\n    print(\"hi\")\n```\n\n\
| name | value |\n|------|-------|\n| a | 1 |\n| bb | 22 |\n\n\
> quoted line one\n> quoted line two\n\n\
混合中文與 English 的段落，用來測試寬字元的換行是否正確，而且不會切壞任何字。\n\n\
Final paragraph with a [link](https://example.com) and trailing text.\n";

fn texts(lines: &[MdLine]) -> Vec<String> {
    lines.iter().map(|l| plain(&l.line)).collect()
}

/// Feeds `src` in `chunks`, flushing the way the scrollback flusher does, and returns the
/// flushed lines: on every push, the stable lines whose source starts at or after the offset.
fn stream_flush(src: &str, cuts: &[usize], widths: &[usize]) -> (Vec<MdLine>, MdStream) {
    let mut s = MdStream::new();
    let mut flushed = Vec::new();
    let mut src_off = 0;
    let mut last = 0;
    let mut bounds: Vec<usize> = cuts.iter().map(|c| c % (src.len() + 1)).collect();
    bounds.push(src.len());
    bounds.sort_unstable();
    for (i, b) in bounds.into_iter().enumerate() {
        let b = (b..=src.len()).find(|b| src.is_char_boundary(*b)).unwrap();
        if b <= last {
            continue;
        }
        s.push(&src[last..b]);
        last = b;
        let width = widths[i % widths.len()];
        flushed.extend(
            s.stable_lines(width)
                .into_iter()
                .filter(|l| l.src >= src_off),
        );
        src_off = s.stable_end();
    }
    s.finish();
    let width = widths[0];
    flushed.extend(
        s.stable_lines(width)
            .into_iter()
            .filter(|l| l.src >= src_off),
    );
    (flushed, s)
}

#[test]
fn wrap_breaks_at_spaces_and_trims_the_break() {
    let lines = wrap_text("alpha beta gamma delta", Style::default(), 11);
    let out: Vec<_> = lines.iter().map(plain).collect();
    assert_eq!(out, vec!["alpha beta", "gamma delta"]);
}

#[test]
fn wrap_splits_cjk_anywhere_and_never_exceeds_width() {
    let text = "這是一段很長的中文句子需要換行";
    for width in [3, 4, 7, 10] {
        let lines = wrap_text(text, Style::default(), width);
        for l in &lines {
            assert!(display_width(&plain(l)) <= width, "{width}: {:?}", plain(l));
        }
        let joined: String = lines.iter().map(plain).collect();
        assert_eq!(joined, text);
    }
}

#[test]
fn wrap_hard_splits_a_word_longer_than_the_width() {
    let lines = wrap_text("abcdefghij xy", Style::default(), 4);
    let out: Vec<_> = lines.iter().map(plain).collect();
    assert_eq!(out, vec!["abcd", "efgh", "ij", "xy"]);
}

#[test]
fn wrap_keeps_styles_across_a_word() {
    let bold = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
    let spans = [Span::styled("he", bold), Span::raw("llo world")];
    let lines = wrap_spans(&spans, 6);
    assert_eq!(plain(&lines[0]), "hello");
    assert_eq!(lines[0].spans[0].style, bold);
    assert_eq!(plain(&lines[1]), "world");
}

#[test]
fn markdown_renders_blocks_with_markers_and_separators() {
    let out = texts(&render_markdown(REPLY, 80));
    assert_eq!(out[0], "Plan");
    assert_eq!(out[1], "");
    assert!(out.contains(&"1. Parse the config file".to_owned()), "{out:#?}");
    assert!(out.contains(&"   • nested item".to_owned()), "{out:#?}");
    assert!(out.contains(&"      print(\"hi\")".to_owned()), "{out:#?}");
    let quote = out.iter().position(|l| l == "▌ quoted line one").unwrap();
    assert_eq!(out[quote - 1], "", "the separator before a quote sits outside it");
    assert!(out.iter().any(|l| l.starts_with("name │ value")), "{out:#?}");
    assert!(out.contains(&"▌ quoted line one".to_owned()), "{out:#?}");
    assert!(out.iter().any(|l| l.contains("link (https://example.com)")), "{out:#?}");
}

#[test]
fn markdown_lines_fit_the_width() {
    for width in [20, 33, 60] {
        for l in render_markdown(REPLY, width) {
            let text = plain(&l.line);
            assert!(display_width(&text) <= width, "{width}: {text:?}");
        }
    }
}

#[test]
fn markdown_tags_each_line_with_its_source_line_start() {
    let src = "one\ntwo\n\nthree\n";
    let lines = render_markdown(src, 40);
    let tags: Vec<_> = lines.iter().map(|l| (plain(&l.line), l.src)).collect();
    assert_eq!(
        tags,
        vec![
            ("one".into(), 0),
            ("two".into(), 4),
            ("".into(), 9),
            ("three".into(), 9)
        ]
    );
}

#[test]
fn stream_holds_back_the_partial_line_and_a_table_until_it_ends() {
    let mut s = MdStream::new();
    s.push("hello wor");
    assert_eq!(s.stable_end(), 0);
    assert_eq!(plain(&s.pending_lines(40)[0]), "hello wor");
    s.push("ld\n| a | b |\n|---|---|\n");
    assert_eq!(s.stable_end(), "hello world\n".len());
    s.push("| 1 | 2 |\n\nafter\n");
    assert_eq!(s.stable_end(), s.source().len());
}

#[test]
fn stream_does_not_hold_back_pipes_inside_a_code_fence() {
    let mut s = MdStream::new();
    s.push("```\n| not a table\n");
    assert_eq!(s.stable_end(), s.source().len());
}

#[test]
fn streamed_flush_of_the_reply_equals_its_final_render() {
    let (flushed, _) = stream_flush(REPLY, &[5, 17, 40, 41, 90, 130, 200, 260], &[60]);
    assert_eq!(texts(&flushed), texts(&render_markdown(REPLY, 60)));
}

proptest! {
    #[test]
    fn any_chunking_flushes_exactly_the_final_render(
        cuts in proptest::collection::vec(0usize..400, 0..24),
    ) {
        let (flushed, _) = stream_flush(REPLY, &cuts, &[48]);
        prop_assert_eq!(texts(&flushed), texts(&render_markdown(REPLY, 48)));
    }

    #[test]
    fn width_changes_mid_stream_flush_every_source_line_once(
        cuts in proptest::collection::vec(0usize..400, 1..24),
    ) {
        let (flushed, s) = stream_flush(REPLY, &cuts, &[30, 72, 45]);
        let mut starts: Vec<usize> = flushed.iter().map(|l| l.src).collect();
        starts.dedup();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        sorted.dedup();
        prop_assert_eq!(&starts, &sorted, "a source line was flushed twice or out of order");
        let final_starts: Vec<usize> = {
            let mut v: Vec<usize> = s.stable_lines(30).iter().map(|l| l.src).collect();
            v.dedup();
            v
        };
        prop_assert_eq!(starts, final_starts);
    }
}
