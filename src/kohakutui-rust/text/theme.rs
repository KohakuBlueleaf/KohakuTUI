//! Semantic styles shared by the Markdown renderer and every component, read from one
//! process-wide theme that Python can switch at runtime.
//!
//! A theme maps semantic roles (accent, tool, agent, running, attention, danger, ok, …) and
//! palette names (the KT gem colours) to colours. `tone(name)` accepts a role, a palette name or
//! a `#rrggbb` literal. Truecolor is used when the terminal reports it, otherwise colours are
//! reduced to the nearest of the 256-colour cube.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ratatui::style::{Color, Modifier, Style};

/// Built-in scheme names, in the order a picker lists them.
pub const SCHEMES: &[&str] = &[
    "kohaku-dark",
    "kohaku-light",
    "terminal",
    "colorblind-dark",
    "colorblind-light",
    "solarized-dark",
    "solarized-light",
    "tokyo-night",
];

/// Every role a component may ask for.
pub const ROLES: &[&str] = &[
    "text",
    "muted",
    "faint",
    "accent",
    "user",
    "assistant",
    "tool",
    "agent",
    "running",
    "attention",
    "danger",
    "ok",
    "code",
    "border",
    "selection_fg",
    "selection_bg",
    "user_bg",
    "focus_bg",
];

/// A resolved theme: role and palette colours, plus whether to emit 24-bit colour.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    colors: HashMap<String, Color>,
    truecolor: bool,
}

static CURRENT: RwLock<Option<Arc<Theme>>> = RwLock::new(None);

/// The active theme (the default one until Python sets another).
pub fn current() -> Arc<Theme> {
    if let Some(t) = CURRENT.read().ok().and_then(|g| g.clone()) {
        return t;
    }
    let t = Arc::new(Theme::scheme("kohaku-dark", detect_truecolor()).expect("built-in"));
    if let Ok(mut g) = CURRENT.write() {
        g.get_or_insert_with(|| t.clone());
    }
    t
}

/// Installs `theme` for every later frame.
pub fn set_current(theme: Theme) {
    if let Ok(mut g) = CURRENT.write() {
        *g = Some(Arc::new(theme));
    }
}

/// Whether the terminal advertises 24-bit colour (or is a host known to support it).
pub fn detect_truecolor() -> bool {
    let colorterm = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_lowercase();
    colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || std::env::var_os("WT_SESSION").is_some()
        || std::env::var("TERM_PROGRAM")
            .is_ok_and(|p| matches!(p.as_str(), "vscode" | "iTerm.app" | "WezTerm" | "ghostty"))
}

/// A `#rrggbb` (or `#rgb`) literal as a colour.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    let digits: Vec<u8> = match h.len() {
        6 => (0..3)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok())
            .collect::<Option<_>>()?,
        3 => h
            .chars()
            .map(|c| c.to_digit(16).map(|d| (d * 17) as u8))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    Some(Color::Rgb(digits[0], digits[1], digits[2]))
}

/// The nearest xterm-256 index for an RGB colour (cube or grey ramp).
fn to_256(r: u8, g: u8, b: u8) -> Color {
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|&i| (i32::from(STEPS[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (STEPS[ri], STEPS[gi], STEPS[bi]);
    let grey_level = ((u32::from(r) + u32::from(g) + u32::from(b)) / 3) as u8;
    let gi_ramp = if grey_level < 8 {
        0
    } else {
        ((grey_level - 8) / 10).min(23)
    };
    let grey = 8 + gi_ramp * 10;
    let dist = |c: (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(c.0, r) + d(c.1, g) + d(c.2, b)
    };
    if dist((grey, grey, grey)) < dist(cube) {
        Color::Indexed(232 + gi_ramp)
    } else {
        Color::Indexed(16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8)
    }
}

const GEMS: &[(&str, &str, &str, &str)] = &[
    ("iolite", "#DDD0F0", "#5A4FCF", "#312A7A"),
    ("sapphire", "#D6E3F8", "#0F52BA", "#082567"),
    ("taaffeite", "#E8D5ED", "#A57EAE", "#6B4670"),
    ("aquamarine", "#D4EDE8", "#4C9989", "#1B6B5A"),
    ("amber", "#F5E6C8", "#D4920A", "#8B5E00"),
    ("coral", "#F5D5D5", "#D46B6B", "#8B3A3A"),
    ("sage", "#D5E8DA", "#5A9E6F", "#3A6B48"),
];

/// Gem glyph colours for a dark background: the web's DEFAULT shade, lifted for contrast.
const DARK_GEMS: &[(&str, &str)] = &[
    ("iolite", "#9D94F0"),
    ("sapphire", "#6F9BE6"),
    ("taaffeite", "#C9A4D2"),
    ("aquamarine", "#73C2B1"),
    ("amber", "#E6A92A"),
    ("coral", "#E48A8A"),
    ("sage", "#7CC493"),
];

/// Role → palette entry, shared by the kohaku and colourblind schemes.
const KOHAKU_ROLES: &[(&str, &str)] = &[
    ("accent", "iolite"),
    ("user", "iolite"),
    ("assistant", "aquamarine"),
    ("tool", "sapphire"),
    ("agent", "taaffeite"),
    ("running", "aquamarine"),
    ("attention", "amber"),
    ("danger", "coral"),
    ("ok", "sage"),
    ("code", "amber"),
];

impl Theme {
    /// A built-in scheme by name.
    pub fn scheme(name: &str, truecolor: bool) -> Option<Theme> {
        let mut c: HashMap<String, Color> = HashMap::new();
        let mut put = |k: &str, hex: &str| {
            if let Some(col) = parse_hex(hex) {
                c.insert(k.to_owned(), col);
            }
        };
        for (gem, light, default, shadow) in GEMS {
            put(&format!("{gem}-light"), light);
            put(&format!("{gem}-shadow"), shadow);
            put(&format!("{gem}-default"), default);
        }

        match name {
            "kohaku-dark" | "colorblind-dark" => {
                for (gem, hex) in DARK_GEMS {
                    put(gem, hex);
                }
                for (k, hex) in [
                    ("text", "#E8E0D8"),
                    ("muted", "#8A8078"),
                    ("faint", "#5A5550"),
                    ("border", "#3D3835"),
                    ("selection_fg", "#DDD0F0"),
                    ("selection_bg", "#312A7A"),
                    ("user_bg", "#2A2640"),
                    ("focus_bg", "#1E1A2E"),
                ] {
                    put(k, hex);
                }
                if name == "colorblind-dark" {
                    put("sage", "#56B4E9");
                    put("coral", "#E69F00");
                    put("amber", "#F0E442");
                }
            }
            "kohaku-light" | "colorblind-light" => {
                for (gem, _, default, _) in GEMS {
                    put(gem, default);
                }
                for (k, hex) in [
                    ("text", "#2E2A26"),
                    ("muted", "#6C655D"),
                    ("faint", "#8F8880"),
                    ("border", "#E5DFD7"),
                    ("selection_fg", "#312A7A"),
                    ("selection_bg", "#DDD0F0"),
                    ("user_bg", "#EEE9F7"),
                    ("focus_bg", "#F4F0FA"),
                ] {
                    put(k, hex);
                }
                if name == "colorblind-light" {
                    put("sage", "#0072B2");
                    put("coral", "#D55E00");
                    put("amber", "#B08800");
                }
            }
            "solarized-dark" | "solarized-light" => {
                for (k, hex) in [
                    ("iolite", "#6C71C4"),
                    ("sapphire", "#268BD2"),
                    ("taaffeite", "#D33682"),
                    ("aquamarine", "#2AA198"),
                    ("amber", "#B58900"),
                    ("coral", "#DC322F"),
                    ("sage", "#859900"),
                ] {
                    put(k, hex);
                }
                let dark = name == "solarized-dark";
                put("text", if dark { "#93A1A1" } else { "#586E75" });
                put("muted", if dark { "#657B83" } else { "#93A1A1" });
                put("faint", if dark { "#586E75" } else { "#EEE8D5" });
                put("border", if dark { "#073642" } else { "#EEE8D5" });
                put("selection_fg", if dark { "#FDF6E3" } else { "#002B36" });
                put("selection_bg", "#6C71C4");
                put("user_bg", if dark { "#073642" } else { "#EEE8D5" });
                put("focus_bg", if dark { "#0A3440" } else { "#F5EFDC" });
            }
            "tokyo-night" => {
                for (k, hex) in [
                    ("iolite", "#BB9AF7"),
                    ("sapphire", "#7AA2F7"),
                    ("taaffeite", "#C0A0D8"),
                    ("aquamarine", "#73DACA"),
                    ("amber", "#E0AF68"),
                    ("coral", "#F7768E"),
                    ("sage", "#9ECE6A"),
                    ("text", "#C0CAF5"),
                    ("muted", "#737AA2"),
                    ("faint", "#565F89"),
                    ("border", "#3B4261"),
                    ("selection_fg", "#C0CAF5"),
                    ("selection_bg", "#364A82"),
                    ("user_bg", "#292E42"),
                    ("focus_bg", "#202334"),
                ] {
                    put(k, hex);
                }
            }
            "terminal" => {
                let ansi = [
                    ("iolite", Color::Magenta),
                    ("sapphire", Color::Blue),
                    ("taaffeite", Color::LightMagenta),
                    ("aquamarine", Color::Cyan),
                    ("amber", Color::Yellow),
                    ("coral", Color::Red),
                    ("sage", Color::Green),
                    ("text", Color::Reset),
                    ("muted", Color::DarkGray),
                    ("faint", Color::DarkGray),
                    ("border", Color::DarkGray),
                    ("selection_fg", Color::Black),
                    ("selection_bg", Color::Cyan),
                    // inherit the terminal's background: no band
                    ("user_bg", Color::Reset),
                    ("focus_bg", Color::Reset),
                ];
                for (k, col) in ansi {
                    c.insert(k.to_owned(), col);
                }
            }
            _ => return None,
        }

        for (role, gem) in KOHAKU_ROLES {
            if let Some(col) = c.get(*gem).copied() {
                c.insert((*role).to_owned(), col);
            }
        }
        Some(Theme { name: name.to_owned(), colors: c, truecolor })
    }

    /// Replaces entries: keys are roles or palette names, values `#rrggbb` or other key names.
    pub fn with_overrides(mut self, overrides: &[(String, String)]) -> Theme {
        for (k, v) in overrides {
            let col = parse_hex(v).or_else(|| self.colors.get(v).copied());
            if let Some(col) = col {
                self.colors.insert(k.clone(), col);
            }
        }
        self
    }

    pub fn truecolor(&self) -> bool {
        self.truecolor
    }

    /// The colour for a role or palette name, reduced to 256 colours when needed.
    pub fn color(&self, key: &str) -> Option<Color> {
        let col = match parse_hex(key) {
            Some(c) => c,
            None => *self.colors.get(key)?,
        };
        Some(match col {
            Color::Rgb(r, g, b) if !self.truecolor => to_256(r, g, b),
            other => other,
        })
    }

    fn fg(&self, key: &str) -> Style {
        match self.color(key) {
            Some(c) => Style::default().fg(c),
            None => Style::default(),
        }
    }
}

fn fg(key: &str) -> Style {
    current().fg(key)
}

pub fn accent() -> Style {
    fg("accent")
}

pub fn muted() -> Style {
    fg("muted")
}

pub fn faint() -> Style {
    fg("faint")
}

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn user() -> Style {
    fg("user").add_modifier(Modifier::BOLD)
}

pub fn assistant() -> Style {
    fg("assistant")
}

pub fn ok() -> Style {
    fg("ok")
}

pub fn warn() -> Style {
    fg("attention")
}

pub fn danger() -> Style {
    fg("danger")
}

pub fn code() -> Style {
    fg("code")
}

pub fn border() -> Style {
    fg("border")
}

/// The band behind the user's own messages.
pub fn user_band() -> Style {
    match current().color("user_bg") {
        Some(Color::Reset) | None => Style::default(),
        Some(c) => Style::default().bg(c),
    }
}

/// The tint behind the area of a dialog that has the keys (a picker's sections or list).
pub fn focus_area() -> Style {
    match current().color("focus_bg") {
        Some(Color::Reset) | None => Style::default(),
        Some(c) => Style::default().bg(c),
    }
}

/// Only the selection's background, for a whole row whose text keeps its own colours.
/// A link under the pointer: the selection colours, bold, still underlined.
pub fn link_hover() -> Style {
    selection().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub fn selection_bg() -> Style {
    match current().color("selection_bg") {
        Some(c) => Style::default().bg(c),
        None => Style::default(),
    }
}

pub fn selection() -> Style {
    let t = current();
    let mut s = Style::default();
    if let Some(c) = t.color("selection_fg") {
        s = s.fg(c);
    }
    if let Some(c) = t.color("selection_bg") {
        s = s.bg(c);
    }
    s
}

/// A named tone from props (`tone="tool"`, `tone="amber"`, `tone="#ff8800"`); unknown names
/// are unstyled. Aliases keep the older names (`info`, `warn`, `success`, …) working.
pub fn tone(name: &str) -> Style {
    match name {
        "info" => accent(),
        "dim" => muted(),
        "success" => ok(),
        "warn" | "warning" => warn(),
        "error" => danger(),
        "user" => user(),
        "bold" => bold(),
        other => fg(other),
    }
}

/// Braille spinner frame for a moment `ms` milliseconds after the engine started.
pub fn spinner(ms: u128) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(ms / 80 % 10) as usize]
}
