# Getting started

## Install

```bash
pip install kohakutui
```

One abi3 wheel per platform covers CPython 3.10 and newer: Linux x86_64 and aarch64
(manylinux 2.28), Windows x86_64, macOS arm64 and x86_64. There are no runtime dependencies.
Building from source needs a stable Rust toolchain (`pip` builds the sdist with maturin).

Check it:

```bash
python -m kohakutui version
python -m kohakutui demo dashboard
```

## The first app

A small interactive tool: type a glob, get a table of matching files.

```python
from pathlib import Path

import kohakutui as ktui

with ktui.App(interrupt="event") as app:
    box = app.bottom.add(ktui.Composer(placeholder="a glob, e.g. **/*.py · ctrl+c quits"))
    box.focus()

    for ev in app.events():
        if ev.kind == "interrupt":
            break
        if ev.kind == "submit" and ev.id == box.id:
            files = sorted(Path(".").glob(ev.value))[:20]
            rows = [[str(f), f.stat().st_size] for f in files]
            table = ktui.Table(columns=["file", {"key": "bytes", "align": "right"}], rows=rows)
            title = f"{ev.value}: {len(files)} files"
            app.transcript.add(ktui.Cell(role="system", title=title, children=[table])).seal()
```

What happens:

1. `App()` starts the engine on its own thread and takes the terminal. Leaving the `with` block
   (or `app.stop()`) restores it, also after an exception.
2. `app.bottom.add(...)` creates an input box in the bottom region and returns a `Node` handle;
   `focus()` gives it the keys.
3. `app.events()` yields semantic events: here `submit` with the typed text.
4. Each answer is a `Cell` added to the transcript and sealed at once. In inline mode a sealed
   node is written to the terminal's real scrollback, once, so the results stay there after the
   program exits.

For output that changes while it runs (progress, charts, a spinner), add nodes to `app.live`
and update them; the README's Quick start does that with a training loop.

## Modes

```python
ktui.App(mode="inline")      # default: scrollback + a live area at the bottom
ktui.App(mode="fullscreen")  # the alternate screen, with panels and mouse support
```

`ctrl+t` switches at runtime; `app.set_mode("fullscreen")` does it from code, and a
`mode_changed` event reports every switch. In inline mode the terminal keeps its own wheel
scrolling and text selection; full screen captures the mouse (wheel, clicks, hover, drag to
select and copy).

## Events

Every event has a `kind`, an `id` (the node it came from, when there is one) and its fields as
attributes or `ev.data`:

| Kind | From | Fields |
| ---- | ---- | ------ |
| `submit` | composer, prompt, confirm | `value` |
| `select` | picker, card, button, scroll keys | `value`, `index` |
| `change` | inputs that report edits | `value` |
| `close` | a dialog closed without an answer | `reason` (`escape`, `back`, …) |
| `action` | a bound chord (`app.bind`), or `escape` with no dialog open | `name` |
| `interrupt` | ctrl+c with `interrupt="event"` | `count` |
| `mode_changed` | inline ↔ full screen | `mode` |
| `copy` | text selected in full screen | `text` |
| `resize`, `ready`, `stopped`, `error` | the engine | |

Three ways to receive them:

```python
@app.on("submit", composer)         # a callback (runs on the event pump thread)
def on_submit(ev): ...

for ev in app.events(): ...         # the calling thread; ends after "stopped"

async for ev in app.aevents(): ...  # an asyncio loop; ends after "stopped"
```

## ctrl+c

`interrupt=` decides what ctrl+c does:

- `"raise"`: `KeyboardInterrupt` in the main thread.
- `"signal"`: SIGINT, Unix only.
- `"event"`: only an `interrupt` event, carrying the press count.
- `"exit"`: the process exits.
- `"auto"` (the default): `signal` on Unix and `raise` elsewhere.

Three presses within a short window always exit after a grace period, so a stuck program can
still be left.

## Threads and asyncio

The engine never blocks Python, and the SDK is thread-safe: any thread may add, update or
append. Blocking dialogs (`app.choose`, `app.ask`, `app.confirm`, `app.card`) must not run on
the event pump thread, so use them from your own thread or the main one. In asyncio, use the
`a`-prefixed forms:

```python
model = await app.achoose("Model", ["gpt", "claude"])
name = await app.aask("Your name?")
```

## Printing and logging

```python
app.capture_stdio()                # print() and sys.stderr go into the transcript
app.capture_logging(logging.INFO)  # root-logger records too
app.release_capture()              # back to normal
```

Without capture, a stray `print` would tear the screen; with it, the text becomes transcript
lines like everything else.

## Next

- [Concepts](concepts.md): how regions, sealing and frames fit together.
- [Components](components.md): what you can put on screen.
