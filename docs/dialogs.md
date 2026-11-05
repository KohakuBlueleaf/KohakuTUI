# Dialogs and input

## Showing a dialog

The quick forms block until answered and return the answer, or `None` for cancelled (esc),
stopped or timed out:

```python
model = app.choose("Model", ["gpt", "claude", "gemini"])
name = app.ask("Project name?", placeholder="my-app")
ok = app.confirm("Delete the build folder?", default=False)
action = app.card("Deploy", "Push **v1.2** to production?", ["Deploy", "Cancel"])
```

Each has an async twin (`achoose`, `aask`, `aconfirm`, `acard`). For full control, build the
spec yourself:

```python
answer = app.run_dialog(ktui.Picker(items=items, sidebar=True, dock="input"))
answer = await app.run_dialog_async(spec, on_open=lambda node: ...)
```

`on_open` receives the dialog's node while it is open, so a live dialog (a log that follows a
running job) can keep changing. Blocking forms must not run on the event pump thread (inside an
`@app.on` callback); use the async forms there, or another thread.

## Dialogs never trap

- `dock="input"` draws the dialog where the input box is, in both modes, so the eyes stay where
  the user just typed; esc gives the input back. Without it, an inline dialog sits above the
  input and a full-screen one is a centred box.
- With a non-docked dialog open, typed characters still reach the composer. Tab or esc after
  typing returns the keys to the dialog.
- esc closes a dialog with reason `escape`; the program decides what that means (hide a question
  until later, cancel a choice).

## Picker

A fuzzy-filtered list.

| Prop | Meaning |
| ---- | ------- |
| `items` | strings, or `{"label", "value", "detail", "group", "current", "available", "checked", "choices", "chosen"}` |
| `title`, `placeholder` | heading and filter hint |
| `max_visible` | rows shown at once |
| `sidebar` | `group`s become a section column on the left (a tab row when narrow) |
| `section` | open on this section |
| `multi` | ☐/☑ rows: space ticks, the answer is the list of ticked values (the highlighted one when none is) |
| `back` | ← with nothing to its left closes with reason `back`, for nested pickers |
| `dock` | `"input"` |

Items with `choices=[["reasoning", ["low", "high"]], ["speed", ["normal", "fast"]]]` show option
chips; selecting one answers `{"value": ..., "choices": {"reasoning": "high", "speed": "fast"}}`.
`current` marks the item it opens on, with ✓; `available: false` greys an item out.

**Keys work on one focused area at a time.** A picker has up to three areas: the *sections*
(sidebar), the *list*, and the highlighted row's *options*. The focused area is tinted and its
selected element is a solid bar; the hint line names only that area's keys.

| Key | Does |
| --- | ---- |
| ←/→, tab / shift+tab | move the focus across sections, list and options (inside options, ←/→ first step across the option groups) |
| ↑↓ | act on the focused area: next section, next item, next value |
| pgup/pgdn | page the list |
| enter | select, from any area |
| typing, backspace, ctrl+u | filter; the focus goes to the list. While filtering, the sidebar counts matches per section |
| space | tick a row (`multi`) |
| esc | cancel |

## Prompt

A question with a text answer. `question`, `placeholder`, `value` (prefill), `multiline`
(alt+enter for a newline), `secret` (shown as •, answered as typed), `dock`.

## Confirm

Yes or no; submits a bool. `question`, `default`, `yes`, `no` (labels), `dock`.

## Card

A titled Markdown body with buttons. `title`, `body`, `actions` (strings or `{"id", "label"}`),
`back`, `dock`. Enter, a button's digit or a click chooses; the answer is the action's id.

## Scroll as a dialog

A `Scroll` with `dock="input"` is a read-only view (a log, a trace) with action keys:

```python
answer = app.run_dialog(ktui.Scroll(
    title="npm run dev", height=15, keys={"s": "stop", "b": "background"}, dock="input",
    children=[ktui.Log()],
))
```

↑↓, pgup/pgdn and home/end scroll it; a mapped key answers with its action; esc closes it.

## Built-in keys

| Key | Does |
| --- | ---- |
| ctrl+t | inline ↔ full screen |
| ctrl+o | open every folded cell; from inline, a full-screen expanded view and back on the next ctrl+o |
| ctrl+c | depends on `interrupt=` (see [Getting started](getting-started.md#ctrlc)) |
| esc | closes the top dialog; with none open, an `action` event named `escape` |
| ↓ from the composer | steps onto a button row under it; ↑ back |
| pgup/pgdn, ctrl+home/ctrl+end | scroll the full-screen transcript, jump to its top or back to following its end |
| ctrl+↑ (full screen) | keyboard navigation over foldable headers: ↑↓ or j/k move, enter or space toggle, esc leaves |
| mouse (full screen) | wheel scrolls (a scroll box under the pointer first), click toggles a header, hover highlights it, drag selects and copies |

`app.bind("ctrl+b", "jobs")` turns a chord into an `action` event named `jobs`;
`app.unbind(chord)` removes it.
