# KohakuTUI

A terminal UI engine written in Rust (ratatui + crossterm) and driven from Python.

Python describes *what* is on screen as a tree of nodes. Rust owns the terminal, the frame loop
and every keystroke, so streaming output never flickers or reflows, and typing stays smooth
while Python is busy.

## What it is for

- **Agent front ends.** An inline mode where finished output goes to the terminal's real
  scrollback, and a full-screen mode (`ctrl+t`) over the same transcript. It is the planned
  replacement for KohakuTerrarium's `cli_rich` and Textual front ends.
- **Training scripts.** Progress, loss curves and logs that a stray `print` cannot corrupt.

## Try it

```bash
python -m kohakutui demo agent      # a coding-agent turn: folded tool calls (ctrl+o), an
                                    # approval card, background tasks (↓ or ctrl+b), ctrl+t
python -m kohakutui demo dashboard  # full-screen training monitor: charts, eval table, GPU panel
python -m kohakutui demo chat       # streamed Markdown, a composer, /model picker
python -m kohakutui demo train      # inline training: a loss chart that lands in scrollback at the end
python -m kohakutui reset           # fix a terminal left in raw mode
```

## Using it

```python
import kohakutui as ktui

with ktui.App() as app:
    composer = app.bottom.add(ktui.Composer(placeholder="say something"))

    @app.on("submit", composer)
    def reply(ev):
        cell = app.transcript.add(ktui.Cell(role="assistant", stream_flush=True))
        body = cell.add(ktui.Markdown(streaming=True))
        for token in ["**Hello** ", "from ", "Rust\n"]:
            body.append(token)
        cell.seal()  # the whole cell is now in the terminal's scrollback

    model = app.choose("Model", ["gpt", "claude", "gemini"])  # blocks until answered
```

The screen has five regions, each a node you add children to:

| Region | Inline mode | Full-screen mode |
| ------ | ----------- | ---------------- |
| `app.transcript` | sealed cells go to real scrollback | scrollable main pane |
| `app.live` | spinners, progress, running tools | under the main pane |
| `app.bottom` | composer and footer | bottom |
| `app.panels` | hidden | right-hand pane |
| `app.overlays` | dialogs (`choose`, `ask`, `confirm`, `card`) | centered modal |

Nodes change with `update(**props)`, stream with `append(data)`, finish with `seal()`, and move
with `move(parent)`. Events arrive through `@app.on(kind, node)`, `app.events()`,
`async for ev in app.aevents()`, or a `ktui.Widget` subclass that owns its nodes. Everything runs
headless with `App(backend="test", size=(80, 24))`, whose `screen()` and `history()` read the
emulated terminal.

## Development setup

Requires a Rust toolchain (stable) and Python 3.10+.

```bash
uv venv
uv pip install -e ".[dev]"     # maturin builds kohakutui._rs into the venv
python -c "import kohakutui; print(kohakutui.version())"
```

After changing Rust code, run `uv pip install -e ".[dev]"` again (or `maturin develop`) to
rebuild the extension.

Checks:

```bash
pytest tests/ -q
cargo test --no-default-features
cargo fmt --check && cargo clippy --all-targets --features python -- -D warnings
black --check src/ tests/ && ruff check src/ tests/
```

## Layout

```text
src/kohakutui-rust/   the engine crate (kohakutui._rs with the `python` feature)
src/kohakutui/        the Python SDK
tests/                static contracts + e2e / integration / unit (see tests/README.md)
```

Contributor rules are in [`CLAUDE.md`](CLAUDE.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE).
