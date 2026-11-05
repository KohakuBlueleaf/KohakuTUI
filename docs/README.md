# KohakuTUI documentation

KohakuTUI is a terminal UI engine: a Rust extension (`kohakutui._rs`) that owns the terminal,
and a Python SDK (`kohakutui`) that describes what is on screen.

| Page | What it covers |
| ---- | -------------- |
| [Getting started](getting-started.md) | Install, the first app, modes, events, threads and asyncio |
| [Concepts](concepts.md) | Regions, nodes, sealing and scrollback, how a frame is made |
| [Components](components.md) | Every component kind and its props |
| [Dialogs and input](dialogs.md) | Pickers, prompts, cards, docking, the composer, keys |
| [Themes](themes.md) | Schemes, colour roles, overrides, truecolor |
| [Testing](testing.md) | The headless backend: inject input, read the screen, screenshots |
| [Releasing](releasing.md) | CI, nightlies, stable releases, PyPI trusted publishing |

For contributors, the engine's module map is in
[`../src/kohakutui-rust/README.md`](../src/kohakutui-rust/README.md), the SDK's in
[`../src/kohakutui/README.md`](../src/kohakutui/README.md).

`images/` holds the README pictures. They are real output, painted from the emulated terminal by
[`../scripts/screenshots.py`](../scripts/screenshots.py); regenerate them after a visual change.
