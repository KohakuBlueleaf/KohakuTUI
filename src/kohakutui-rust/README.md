# kohakutui-rust/

The engine crate (`kohakutui`, lib `kohakutui_rs`). With the `python` feature it builds the
`kohakutui._rs` extension module; without it, it is pure Rust and `cargo test
--no-default-features` links no Python.

## Layout

Every module is a directory whose only entry point is `mod.rs`: declarations, the module's
public types and re-exports. Logic lives in sibling files, unit tests in `tests.rs`, pyo3
bindings in `python.rs` (gated on the `python` feature, registered from `lib.rs`).

| Module | Responsibility | Design |
| ------ | -------------- | ------ |
| `lib.rs` | Crate root, `VERSION`, the `_rs` pymodule | §8 |
| `proto/` | Ops, Events, the dynamic value type, wire framing, `PROTOCOL` | §4 |
| `model/` | Retained document: regions, node tree, seal and flush state | §5 |
| `text/` | Markdown rendering, streaming hold-back, wrapping | §6.4, §6.6 |
| `widgets/` | Component trait, registry, every component | §4.4, §12 |
| `view/` | Focus, key routing, overlays, layout, theme | §7 |
| `term/` | TTY ownership, inline viewport, history insertion, alternate screen | §6.1 |
| `runtime/` | Render thread, input thread, stdio capture | §6.2, §10.5 |
| `transport/` | Bus, framed streams, Host, Viewer | §11 |

## Dependency direction

`proto` ← `model` ← `text`/`widgets` ← `view` ← `term` ← `runtime` ← `transport` ← `lib.rs`
bindings. A module never imports one to its right.

## Features

| Feature | Effect |
| ------- | ------ |
| `python` | pyo3 bindings and the `_rs` module |
| `highlight` | syntect syntax highlighting for `code` and fenced blocks |
| `testing` | test-only hooks such as `_Session.inject`; CI wheels only |
