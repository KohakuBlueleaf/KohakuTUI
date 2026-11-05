//! Key chords such as `ctrl+t`, `alt+enter` or `f2`, parsed from bindings and matched to keys.

use crossterm::event::{KeyCode, KeyModifiers};

/// A key plus modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    code: KeyCode,
    mods: KeyModifiers,
}

impl Chord {
    /// Parses `mod+mod+key`; None for an unknown key or modifier.
    pub fn parse(s: &str) -> Option<Chord> {
        let lower = s.trim().to_lowercase();
        let mut parts: Vec<&str> = lower.split('+').collect();
        let key = parts.pop()?;
        if key.is_empty() {
            return None;
        }

        let mut mods = KeyModifiers::NONE;
        for m in parts {
            mods |= match m {
                "ctrl" | "control" => KeyModifiers::CONTROL,
                "alt" | "meta" | "option" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return None,
            };
        }

        let code = match key {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "space" => KeyCode::Char(' '),
            f if f.len() > 1 && f.starts_with('f') => KeyCode::F(f[1..].parse().ok()?),
            c if c.chars().count() == 1 => KeyCode::Char(c.chars().next()?),
            _ => return None,
        };
        Some(Chord { code, mods })
    }

    /// The key and modifiers, for synthesizing the key in tests.
    pub fn parts(&self) -> (KeyCode, KeyModifiers) {
        (self.code, self.mods)
    }

    /// Whether a key event is this chord. Letters ignore case and shift.
    pub fn matches(&self, code: KeyCode, mods: KeyModifiers) -> bool {
        match (self.code, code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => {
                let strip = |m: KeyModifiers| m - KeyModifiers::SHIFT;
                a.to_lowercase().eq(b.to_lowercase()) && strip(self.mods) == strip(mods)
            }
            (a, b) => a == b && self.mods == mods,
        }
    }
}
