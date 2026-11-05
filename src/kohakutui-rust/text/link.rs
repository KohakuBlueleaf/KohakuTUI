//! Hyperlinks.
//!
//! A URL rides on a span's style: it is interned here and its id stored in the style's
//! `underline_color` as an RGB value (`underline_color` is reserved for this; nothing else in
//! kohakutui sets it). Widths and diffs stay exact because no escape ever enters a cell: the
//! two writers (the viewport diff and the scrollback lines) turn the mark into OSC 8 around the
//! cells they print and strip it before printing. In full screen the view records where linked
//! cells are, so a click can report the URL (`link` event).
//!
//! Terminals without OSC 8 support ignore the sequence and show only the text, so the mode is a
//! per-engine setting: `osc8` (a real terminal) or `text`, where renderers mark nothing and
//! write the URL after the text instead (plain output, where nothing can be clicked).

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ratatui::style::{Color, Modifier, Style};

use super::theme;

/// Longer URLs are not linked: terminals cap OSC 8 URIs at about this length.
const MAX_URL: usize = 2048;
/// Schemes that become links; anything else stays text.
const SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "file://"];

#[derive(Default)]
struct Registry {
    urls: Vec<String>,
    ids: HashMap<String, u32>,
}

fn registry() -> &'static Mutex<Registry> {
    static REG: OnceLock<Mutex<Registry>> = OnceLock::new();
    REG.get_or_init(Mutex::default)
}

thread_local! {
    /// Renders on this thread mark links (true) or write their URL as text (false). Every render
    /// runs on the engine's thread, which sets it once from the backend and the app's option.
    static OSC8: Cell<bool> = const { Cell::new(true) };
}

/// Whether renders on this thread mark links for OSC 8 (else they write the URL as text).
pub fn osc8() -> bool {
    OSC8.with(Cell::get)
}

pub fn set_osc8(on: bool) {
    OSC8.with(|c| c.set(on));
}

/// Whether `url` may become a link: a known scheme, no control characters, not too long.
pub fn linkable(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let Some(scheme) = SCHEMES.iter().find(|s| lower.starts_with(*s)) else {
        return false;
    };
    url.len() > scheme.len()
        && url.len() <= MAX_URL
        && !url.chars().any(|c| c.is_control() || c == ' ')
}

fn intern(url: &str) -> u32 {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(id) = reg.ids.get(url) {
        return *id;
    }
    let id = reg.urls.len() as u32;
    reg.urls.push(url.to_owned());
    reg.ids.insert(url.to_owned(), id);
    id
}

/// The mark alone: patch it onto any style to make that text a link to `url`. Nothing when
/// the URL may not be linked or this thread writes links as text.
pub fn mark(url: &str) -> Style {
    if !osc8() || !linkable(url) {
        return Style::default();
    }
    let id = intern(url);
    let [_, r, g, b] = id.to_be_bytes();
    Style::default().underline_color(Color::Rgb(r, g, b))
}

/// How link text looks: the theme's link colour, underlined, plus the mark.
pub fn style(url: &str) -> Style {
    theme::accent()
        .add_modifier(Modifier::UNDERLINED)
        .patch(mark(url))
}

/// The link id a cell or span carries (its `underline_color`).
pub fn id_of(color: Option<Color>) -> Option<u32> {
    match color {
        Some(Color::Rgb(r, g, b)) => Some(u32::from_be_bytes([0, r, g, b])),
        _ => None,
    }
}

pub fn url(id: u32) -> Option<String> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.urls.get(id as usize).cloned()
}

/// OSC 8 opening `id`'s link; the `id=` parameter lets a terminal treat the pieces of a wrapped
/// link as one.
pub fn open(id: u32) -> String {
    match url(id) {
        Some(u) => format!("\x1b]8;id=kt{id};{u}\x1b\\"),
        None => String::new(),
    }
}

pub const CLOSE: &str = "\x1b]8;;\x1b\\";

/// Bare URLs in `text`: byte ranges of `http://` / `https://` runs up to whitespace, without
/// trailing punctuation (and without a closing bracket that has no opening one in the URL).
pub fn find_urls(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find("http") {
        let start = from + rel;
        let rest = &text[start..];
        let scheme = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            from = start + 4;
            continue;
        };
        // a URL starts a word: "xhttp://" is not one
        let word_start = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let len = rest
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(rest.len());
        let mut end = start + len;
        loop {
            let tail = &text[start..end];
            let last = tail.chars().next_back();
            let unbalanced = |open: char, close: char| {
                last == Some(close) && tail.matches(open).count() < tail.matches(close).count()
            };
            match last {
                Some('.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '_') => {}
                Some(_) if unbalanced('(', ')') || unbalanced('[', ']') => {}
                _ => break,
            }
            end -= last.map_or(0, char::len_utf8);
        }
        if word_start && end > start + scheme {
            out.push((start, end));
        }
        from = end.max(start + scheme);
    }
    out
}
