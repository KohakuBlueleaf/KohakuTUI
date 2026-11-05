# Components

Every component is a dataclass in `kohakutui` (also `kohakutui.ui`). Pass it to `add`; change
it later with `node.update(prop=value)`. Props left as `None` use the default. Containers
(`Stack`, `Cell`, `Scroll`, `Columns`, `Tabs`) take `children=[...]`.

`tone` props take a colour role (`muted`, `faint`, `accent`, `tool`, `agent`, `running`,
`attention`, `danger`, `ok`, …), a palette name or `#rrggbb`; see [Themes](themes.md).

## Layout

### `Stack`

A vertical container.

| Prop | Meaning |
| ---- | ------- |
| `title` | a bold line above the children |
| `gap` | blank lines between children |

### `Cell`

One transcript entry: an icon by role, an optional title row, children indented under it.

| Prop | Meaning |
| ---- | ------- |
| `role` | `system` (default), `user`, `assistant`, `tool`, `error`; picks the icon and colours. `user` cells (what the person typed) sit on a tinted band |
| `title` | the header text (bold) |
| `detail` | muted text after the title, such as a command's arguments |
| `status` | `running` (spinner), `ok`, `error`, `interrupted`, `waiting…`, or free text shown after the title |
| `icon` | overrides the role's icon |
| `tone` | colour of the icon and title |
| `trailer` | a muted line printed when the cell seals, such as a duration |
| `collapsed`, `summary` | fold to the header and `summary` until opened (ctrl+o, click, keyboard) |
| `stream_flush` | let a streaming `Markdown` last child reach scrollback line by line before sealing |
| `batch` | group child cells: invisible below `batch` children, from there one "N `title`" header ("N items" untitled) |

### `Scroll`

Children seen through a window of `height` rows, following the end until scrolled.

| Prop | Meaning |
| ---- | ------- |
| `height` | visible rows (0: all) |
| `title` | a bold line above |
| `keys` | `{"s": "stop"}`: pressing a key selects that action (`select` with the action as value) |
| `dock` | `"input"` to use it as a dialog at the input box |

↑↓, pgup/pgdn and home/end scroll it as a dialog; in full screen the wheel scrolls a box in the
transcript.

### `Columns`

Children side by side.

| Prop | Meaning |
| ---- | ------- |
| `count` | columns per row, equal widths; further children start a new row (0: all in one row) |
| `gap` | blank columns between them |
| `min_width` | narrower than this, the columns stack instead |
| `pack` | each child at its own width, wrapping when the row is full (a row of badges or buttons) |

### `Tabs`

A row of `labels` over its children, one pane each; only pane `active` is drawn, and controls
in a closed pane cannot take focus. Focused, ←/→ switch; in full screen a click on a label
does. Each switch emits `change` with the new index.

### `Divider`

A horizontal rule; `label` is written into it.

## Text

### `Text`

Wrapped text in one style. `value`, `tone`, `bold`, `dim`, `italic`; `append` adds text. With
`link="https://…"` the whole text is a hyperlink (see [Links](#links)).

### `Markdown`

Markdown with headings, lists, tables, code blocks, quotes and inline styles. `source` sets it;
with `streaming=True`, `append` streams source in and only finished blocks are laid out as
final. `[name](url)` shows `name` as a hyperlink, and bare `http(s)://` URLs in prose are
links too (not in code).

### Links

Links are OSC 8 hyperlinks: the text is shown, the URL travels with it, and the terminal opens
it (ctrl+click or cmd+click in most terminals). A link wrapped over several lines stays one
link. In full screen the mouse is the app's, so a click on a link sends a `link` event with
its `url`, and the app opens http, https and mailto URLs in the browser (`App(open_links=False)`
turns that off; handle `link` yourself instead).

Only http, https, mailto and file URLs become links; anything else, or a URL with control
characters, stays text with the URL written after it. `App(links="text")` writes every link as
`name (url)`, for terminals or multiplexers that drop OSC 8 (tmux without its `hyperlinks`
feature); plain output does that by itself.

### `Code`

Monospace text behind a gutter. `value`, `lang`, `max_lines` (keep only the tail).

### `Log`

A scrolling log: `\n` ends a line, `\r` rewrites the current one (progress output works).
`title`, `max_lines`, `height`.

### `KV`

Aligned label/value rows from `fields=[{"label", "value"}]` or a dict.

### `Table`

`columns` (keys or `{key, label, align}`), `rows` (lists, or dicts keyed by column key),
`max_rows`.

## Status

### `Status`

A spinner with a label and the elapsed time. `label`, `spinner`, `tone`.

### `Progress`

A bar with value/total, rate and ETA. `value`, `total`, `label`, `unit`; `append(n)` sets the
value.

### `Sparkline`

A one-line chart of recent values. `label`, `values`, `max_points`, `digits`; `append` takes a
number or a list, and NaN leaves a gap.

### `Chart`

A multi-row line chart over the whole run, downsampled to the width. `label`, `height`, `unit`,
`digits`, `y_min`, `y_max`, `max_points`. `append` takes a number or a list (one series named
after `label`), or a dict such as `{"loss": 0.41, "ema": 0.43}` to feed several series at once;
NaN leaves a gap.

### `Bars`

Horizontal bars in eighth-of-a-cell steps, one row per label; the bar room is capped at 48
cells, so wide terminals do not stretch them.

| Prop | Meaning |
| ---- | ------- |
| `labels` | one per row |
| `series` | `[{"name", "data"}]`; with several, each label gets one row per series and a legend |
| `stacked` | the series as coloured segments of one bar per label |
| `max` | the full length (a gauge, shown as `value / max`); otherwise the longest bar fills |
| `percent` | write each value as its share of the total, drawn against the total (a pie) |
| `digits` | decimals; none for whole numbers, else two |

## Input and status rows

### `Composer`

The input box: multi-line editing, history (↑↓), a `/` command popup, paste detection.

| Prop | Meaning |
| ---- | ------- |
| `placeholder` | shown while empty |
| `boxed` | draw it in a rounded box |
| `commands` | `[{"name", "desc"}]` for the `/` popup |
| `history` | previous entries |
| `rich_submit` | submit `{"text", "now"}`: `now` is true for ctrl+enter or ctrl+s |
| `value` | set the text |

Keys: enter submits; shift+enter, alt+enter, ctrl+j or `\` before enter insert a newline.

### `Button`

One focusable line (↓ from the composer reaches it). `label`, `value`, `hint`,
`focused_hint`. With `segments=[{"label", "value"}]` it is several buttons in a row: ←/→ choose
one while focused, and `select` carries that segment's value and index.

### `Field`

One form control, focusable inside the transcript: tab enters it, ↓↑ move between controls,
esc hands the keys back. Each edit emits one `change` with the new value.

| Prop | Meaning |
| ---- | ------- |
| `control` | `slider`, `number`, `radio`, `select`, `checkbox`, `toggle`, `rating`, `text`, `date` (typed, like `text`) |
| `label` | shown before the control |
| `value` | the current value |
| `options` | `[{"label", "value"}]` (or strings) for `radio` and `select` |
| `min`, `max`, `step` | for `slider` and `number` (←/→ step; shift+→ steps by ten) |
| `placeholder` | for an empty `text` |
| `disabled` | draw it without taking focus |

Typing into a `number` replaces its value; a focused `text` field draws a caret.

### `Footer`

One status line: `left` and `right` text.

## Full-screen chrome

Drawn in full screen only, so an inline app can build them and lose nothing.

### `Pane`

A side column under `app.panels`: `side` `"left"` or `"right"`, `width` columns (at least
`min`), `title`, `collapsed` to a one-column strip. Children stack inside; the wheel scrolls
it. The user drags its divider, double-clicks the divider to collapse it, clicks the strip
to open it, and, with keys inside it, moves the divider with alt+←/→. Each change is a `pane`
event (`id`, `width`, `collapsed`) for the app to remember. When the main pane would get
narrower than 30 columns, panes collapse for that frame (right ones first).

```python
rail = app.panels.add(Pane(side="left", width=24, title="Agents"))
tree = rail.add(Tree(items=[{"label": "root", "value": "root"}]))
```

### `TabStrip`

A row of tabs, usually in `app.head`: `tabs` as `[{"label", "value", "dot", "badge",
"closable"}]` (`dot` a tone for a status dot, `badge` a count), `active` the index. Click or
←/→ selects (`select` with the value), a middle click or ctrl+w on a closable tab closes it
(`close` with the value as the reason). A strip wider than its row scrolls by whole tabs
behind `‹` / `›`.

### `Tree`

An outline: `items` as `[{"label", "value", "detail", "icon", "tone", "open", "children"}]`.
↑↓ move, → opens, ← closes or steps out, enter or a click selects (`select` with the value),
a click on ▸ toggles. `children: True` (no list) can open and sends `change` with
`{"expand": value}` so the app can fill it in. `height` keeps that many rows; `selected`
moves the cursor to a value.

### Follow and focus

Scrolled up, the full-screen transcript stays where it is while messages arrive below, and
the "↓ N new" line counts them (ctrl+end goes back to following). The last user message
above the view stays on its top row. F6 / shift+F6 move the keys between areas: the
document, the head row, each open pane and the bottom.

## Dialogs

`Picker`, `Prompt`, `Confirm`, `Card` and `Scroll` are dialogs when added to `app.overlays` or
shown with `app.run_dialog(spec)`; see [Dialogs and input](dialogs.md) for their props.
