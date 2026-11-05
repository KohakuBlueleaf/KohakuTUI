# Concepts

## Two halves

```text
Python (your program)                         Rust (kohakutui._rs, its own thread)
───────────────────────                       ─────────────────────────────────────
app.transcript.add(Cell(...))   ── ops ──▶    model: the node tree
node.update(status="ok")        (msgpack)     view: layout, wrapping, Markdown, frames
node.append("token")                          term: scrollback, viewport, diffing
                                ◀─ events ──  input: keys, paste, mouse → semantic events
for ev in app.events(): ...                   (submit, select, close, action, …)
```

Python never draws and never sees raw keys. It sends small batches of operations (create,
update, append, seal, move, remove) and receives events that already mean something: "the
composer submitted this text", "the picker selected this item". Rust renders at most
`max_fps` frames a second (60 by default), only when something changed, and it owns every
keystroke, so typing stays smooth while Python is busy.

## Regions

The screen is five fixed regions. Every node lives in one of them.

| Region | Holds | Inline | Full screen |
| ------ | ----- | ------ | ----------- |
| `transcript` | the record of what happened: results, logs, messages, finished charts | written to scrollback once sealed | the scrollable main pane |
| `live` | what is happening now: spinners, progress, running tools | redrawn above the bottom | under the main pane |
| `bottom` | input and status: composer, buttons, footer | always at the bottom | bottom |
| `panels` | side information | hidden | right-hand pane |
| `overlays` | dialogs | at the input, or above it | at the input (docked) or a centred box |

## Nodes

`region.add(spec)` creates a node from a component spec and returns a `Node`:

```python
cell = app.transcript.add(ktui.Cell(role="tool", title="grep", status="running"))
out = cell.add(ktui.Code(""))       # children render indented under the cell
out.append("match 1\n")             # streaming: append to the node's content
cell.update(status="ok", summary="3 matches")
cell.seal()                         # finished; no further changes
```

| Call | Does |
| ---- | ---- |
| `add(spec, index=None)` | creates a child; a spec's `children` are created with it |
| `update(**props)` | changes props; a prop set to `None` returns to its default |
| `append(data)` | streams into the node (text, Markdown source, log lines, chart points) |
| `seal()` | finishes the node and its subtree |
| `move(parent, index=None)` | re-parents it |
| `remove()` | deletes it and its subtree |
| `focus()` | gives it the keys |

Calls from many threads are safe. Within one thread they keep their order, and
`with app.batch():` sends a group of them as one frame.

## Sealing and scrollback

In inline mode the transcript is written to the terminal's scrollback, the real one you scroll,
search and copy from. Lines go there only once they are final, because scrollback cannot be
edited:

- A **sealed** cell is final. Its lines are flushed in order, once.
- An **open** cell stays in the live area at the bottom and is redrawn as it changes. It holds
  back every transcript line after it, so the scrollback never interleaves.
- A cell with `stream_flush=True` and a streaming `Markdown` last child flushes line by line as
  the Markdown becomes stable (a paragraph or list item is done), before the cell seals. That is
  how a long answer reaches scrollback while it is still streaming.

So keep open cells short-lived: seal a step when it ends, and keep long-running state (progress,
live charts) in `app.live`, moving or summarising it into the transcript when it is done.

## Folding and batches

A titled `Cell(collapsed=True, summary=...)` shows its header and one summary line until it is
opened: ctrl+o opens all, and in full screen a click or keyboard navigation (ctrl+↑, then ↑↓
and enter) opens one. Foldable headers carry `▸` (folded) or `▾` (open).

`Cell(batch=3, title="jobs")` groups the cells inside it. Below three children it is invisible
and they render like ordinary transcript entries; from three on they fold under one header,
`✓ 5 jobs  fetch ×2, build, test ×2  1 ✗ ▸`, with the members' names and their running, failed
and interrupted counts. The title is the noun (untitled: "5 items"). Each member still opens on
its own.

## Frames

On every frame Rust lays the regions out for the current mode:

- **Inline:** unflushed transcript lines, the live region, toasts, a dialog, the bottom region.
  The bottom always keeps one blank line from what is above it.
- **Full screen:** the transcript as a scrollable pane (following the end until you scroll up;
  ctrl+end jumps back), an optional panels pane, the live and bottom regions.

Sealed cells are cached per width, so a long transcript costs nothing to redraw.

## Widgets

For a reusable piece of UI with its own state, subclass `ktui.Widget`:

```python
class Counter(ktui.Widget):
    def __init__(self):
        self.n = 0

    def build(self):
        return ktui.Button(label="clicked 0 times")

    def handle(self, ev):
        if ev.kind == "select":
            self.n += 1
            self.root.update(label=f"clicked {self.n} times")

Counter().mount(app.bottom)
```

A widget receives the events of its own nodes in `handle`; `unmount()` removes it.
