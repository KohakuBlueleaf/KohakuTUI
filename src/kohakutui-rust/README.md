# kohakutui-rust/

The engine crate (`kohakutui`, lib `kohakutui_rs`). With the `python` feature it builds the
`kohakutui._rs` extension module; without it, it is pure Rust and `cargo test
--no-default-features` links no Python.

## Layout

Every module is a directory whose only entry point is `mod.rs`: declarations, the module's
public types and re-exports. Logic lives in sibling files, unit tests in `tests.rs`, pyo3
bindings in `python.rs` (gated on the `python` feature, registered from `lib.rs`).

| Module | Responsibility | Key files |
| ------ | -------------- | --------- |
| `lib.rs` | Crate root, `VERSION`, the `_rs` pymodule | |
| `proto/` | `Op`, `Event`, value helpers, Python ↔ value conversion | `decode.rs`, `encode.rs`, `value.rs`, `python.rs` |
| `text/` | Wrapping by display width, Markdown to lines tagged with source offsets, streaming hold-back, theme | `wrap.rs`, `markdown.rs`, `stream.rs`, `theme.rs` |
| `widgets/` | The `Component` trait, the registry, every component | `registry.rs`, `cell.rs`, `composer.rs`, `editor.rs`, `picker.rs`, ... |
| `model/` | Regions, the node tree, seal and flush rules | `tree.rs`, `apply.rs` |
| `view/` | Scrollback flushing, inline and full-screen layout, key routing, chords | `flush.rs`, `layout.rs`, `render.rs`, `input.rs` |
| `term/` | TTY output, history insertion, viewport, alternate screen, restore guard, test screen | `history.rs`, `screen.rs`, `out.rs`, `guard.rs` |
| `runtime/` | Engine and input threads, `Session`, the `_Session` pyclass | `engine.rs`, `session.rs`, `input.rs`, `python.rs` |

## Dependency direction

`proto` ← `text` ← `widgets` ← `model` ← `view` ← `term` ← `runtime` ← `lib.rs` bindings. A
module never imports one to its right.

## Features

| Feature | Effect |
| ------- | ------ |
| `python` | pyo3 bindings and the `_rs` module |
