# Components

Every component is a dataclass in `kohakutui` (also `kohakutui.ui`). Pass it to `add`; change
it later with `node.update(prop=value)`. Props left as `None` use the default. Containers
(`Stack`, `Cell`, `Scroll`) take `children=[...]`.

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

### `Divider`

A horizontal rule; `label` is written into it.

## Text

### `Text`

Wrapped text in one style. `value`, `tone`, `bold`, `dim`, `italic`; `append` adds text.

### `Markdown`

Markdown with headings, lists, tables, code blocks, quotes and inline styles. `source` sets it;
with `streaming=True`, `append` streams source in and only finished blocks are laid out as
final.

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

### `Footer`

One status line: `left` and `right` text.

## Dialogs

`Picker`, `Prompt`, `Confirm`, `Card` and `Scroll` are dialogs when added to `app.overlays` or
shown with `app.run_dialog(spec)`; see [Dialogs and input](dialogs.md) for their props.
