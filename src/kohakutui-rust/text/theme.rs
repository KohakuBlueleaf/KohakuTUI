//! Semantic styles shared by the Markdown renderer and every component.

use ratatui::style::{Color, Modifier, Style};

pub fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn user() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn assistant() -> Style {
    Style::default().fg(Color::Magenta)
}

pub fn ok() -> Style {
    Style::default().fg(Color::Green)
}

pub fn warn() -> Style {
    Style::default().fg(Color::Yellow)
}

pub fn danger() -> Style {
    Style::default().fg(Color::Red)
}

pub fn code() -> Style {
    Style::default().fg(Color::LightYellow)
}

pub fn selection() -> Style {
    Style::default().fg(Color::Black).bg(Color::Cyan)
}

/// A named tone from props (`tone="warn"`) as a style; unknown names are unstyled.
pub fn tone(name: &str) -> Style {
    match name {
        "accent" | "info" => accent(),
        "muted" | "dim" => muted(),
        "ok" | "success" => ok(),
        "warn" | "warning" => warn(),
        "danger" | "error" => danger(),
        "user" => user(),
        "assistant" => assistant(),
        "bold" => bold(),
        _ => Style::default(),
    }
}

/// Braille spinner frame for a moment `ms` milliseconds after the engine started.
pub fn spinner(ms: u128) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(ms / 80 % 10) as usize]
}
