//! Kind name to component: the only place a new kind is registered.

use super::{
    Button, Card, Cell, Chart, Code, Component, Composer, Confirm, Divider, Footer, Kv, Log,
    Markdown, Picker, Progress, Prompt, Scroll, Sparkline, Stack, Status, Table, Text, WidgetError,
};
use crate::proto::Value;

/// Every kind `create` accepts.
pub const KINDS: &[&str] = &[
    "stack",
    "cell",
    "text",
    "markdown",
    "code",
    "log",
    "status",
    "progress",
    "sparkline",
    "chart",
    "kv",
    "table",
    "divider",
    "footer",
    "composer",
    "picker",
    "confirm",
    "prompt",
    "card",
    "button",
    "scroll",
];

/// Builds a component of `kind` and applies its initial props.
pub fn build(kind: &str, props: &Value) -> Result<Box<dyn Component>, WidgetError> {
    let mut comp: Box<dyn Component> = match kind {
        "stack" => Box::<Stack>::default(),
        "cell" => Box::<Cell>::default(),
        "text" => Box::<Text>::default(),
        "markdown" => Box::<Markdown>::default(),
        "code" => Box::<Code>::default(),
        "log" => Box::<Log>::default(),
        "status" => Box::<Status>::default(),
        "progress" => Box::<Progress>::default(),
        "sparkline" => Box::<Sparkline>::default(),
        "chart" => Box::<Chart>::default(),
        "kv" => Box::<Kv>::default(),
        "table" => Box::<Table>::default(),
        "divider" => Box::<Divider>::default(),
        "footer" => Box::<Footer>::default(),
        "composer" => Box::<Composer>::default(),
        "picker" => Box::<Picker>::default(),
        "confirm" => Box::<Confirm>::default(),
        "prompt" => Box::<Prompt>::default(),
        "card" => Box::<Card>::default(),
        "button" => Box::<Button>::default(),
        "scroll" => Box::<Scroll>::default(),
        other => return Err(WidgetError::UnknownKind(other.to_owned())),
    };
    comp.update(props)?;
    Ok(comp)
}
