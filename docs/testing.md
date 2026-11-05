# Testing

Every app runs headless on an emulated terminal (a vt100 parser with scrollback), so tests type
keys and read exactly what a user would see: the same engine, the same bytes.

```python
import kohakutui as ktui

app = ktui.App(backend="test", size=(80, 24), interrupt="event").start()
composer = app.bottom.add(ktui.Composer())
composer.focus()

app.inject({"text": "hello"}, {"key": "enter"})
app.sync()                           # wait until the engine drew everything sent so far

assert any("hello" in line for line in app.screen())
app.stop()
```

## Reading the screen

| Call | Returns |
| ---- | ------- |
| `app.screen()` | the visible rows, trailing spaces trimmed |
| `app.history()` | scrollback followed by the visible rows: everything printed so far |
| `app.screen_cells()` | the visible rows as runs of `(text, fg, bg, flags)` |
| `app.cursor()` | the caret as `(row, col)`, or `None` while hidden |

`screen_cells` colours are `None` (the terminal default), a palette index (`"0"` to `"255"`) or
`"#rrggbb"`; flags name `bold`, `dim`, `italic`, `underline` and `inverse`. Use it to assert a
look (an error is red, a selection is highlighted) or to paint screenshots.

## Injecting input

| Event | Sends |
| ----- | ----- |
| `{"text": "abc"}` | typed characters at human pace |
| `{"burst": "abc"}` | characters at paste pace (an enter inside is a newline) |
| `{"paste": "abc"}` | a bracketed paste |
| `{"key": "ctrl+t"}` | a key or chord: `enter`, `esc`, `tab`, `up`, `pgdn`, `ctrl+enter`, `alt+j`, … |
| `{"click": (col, row)}` | a left click (full screen) |
| `{"scroll": n}` | mouse-wheel steps, positive is up (full screen) |
| `{"resize": (cols, rows)}` | a terminal resize |

## Waiting

The engine runs on its own thread, so tests wait for a condition instead of sleeping:

```python
def until(app, predicate, timeout=5.0):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, "\n".join(app.screen())
        time.sleep(0.02)
    app.sync()

until(app, lambda: any("Model" in line for line in app.screen()))
```

A blocking dialog runs in a thread while the test drives it:

```python
answer = []
threading.Thread(target=lambda: answer.append(app.choose("Pick", ["a", "b"]))).start()
until(app, lambda: any("Pick" in line for line in app.screen()))
app.inject({"key": "down"}, {"key": "enter"})
until(app, lambda: answer == ["b"])
```

## Screenshots

`scripts/screenshots.py` plays the demos headless and paints `screen_cells()` to PNG, cell by
cell, with a monospace font and a symbol fallback:

```bash
uv run --with pillow --with fonttools python scripts/screenshots.py            # all scenes
uv run --with pillow --with fonttools python scripts/screenshots.py picker     # one
```

Add a scene by writing a function that returns `app.screen_cells()` and listing it in `SCENES`.

## KohakuTUI's own tests

```bash
pytest tests/ -q                   # Python: static contracts, unit, integration, e2e journeys
cargo test --no-default-features   # Rust: every stage of the engine
```

[`tests/README.md`](../tests/README.md) explains how the stages are tested on the level below
each one.
