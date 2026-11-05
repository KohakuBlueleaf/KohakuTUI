//! Kind name to component: the only place a new kind is registered.

use super::{
    Bars, Button, Card, Cell, Chart, Code, Columns, Component, Composer, Confirm, Divider, Field,
    Footer, Kv, Log, Markdown, Pane, Picker, Progress, Prompt, Scroll, Sparkline, Stack, Status,
    TabStrip, Table, Tabs, Text, Tree, WidgetError,
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
    "field",
    "columns",
    "tabs",
    "bars",
    "document",
    "pane",
    "tabstrip",
    "tree",
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
        "field" => Box::<Field>::default(),
        "columns" => Box::<Columns>::default(),
        "tabs" => Box::<Tabs>::default(),
        "bars" => Box::<Bars>::default(),
        // a document is a transcript: a plain container, kept apart by the docs region
        "document" => Box::<Stack>::default(),
        "pane" => Box::<Pane>::default(),
        "tabstrip" => Box::<TabStrip>::default(),
        "tree" => Box::<Tree>::default(),
        "scroll" => Box::<Scroll>::default(),
        other => return Err(WidgetError::UnknownKind(other.to_owned())),
    };
    comp.update(props)?;
    Ok(comp)
}
