"""Component specs: plain dataclasses that describe one node and become ``create`` ops.

A spec holds only props. ``Node.add(spec)`` turns it into ops (containers also emit their
children) and returns a handle to the created node.
"""

import dataclasses
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, ClassVar


@dataclass
class Component:
    """Base of every spec: a kind name plus the props the engine reads."""

    kind: ClassVar[str] = ""

    def props(self) -> dict[str, Any]:
        """The props dict sent to the engine; unset (None) props are left out."""
        out = {}
        for f in dataclasses.fields(self):
            if f.name == "children":
                continue
            value = getattr(self, f.name)
            if value is not None:
                out[f.name] = value
        return out

    def to_ops(
        self,
        new_id: Callable[[], int],
        parent: int,
        index: int | None = None,
        node_id: int | None = None,
    ) -> list[dict[str, Any]]:
        """``create`` ops for this spec and its children, the spec's own op first."""
        own = node_id if node_id is not None else new_id()
        ops = [
            {
                "op": "create",
                "id": own,
                "kind": self.kind,
                "parent": parent,
                "index": index,
                "props": self.props(),
            }
        ]
        for child in getattr(self, "children", None) or []:
            ops.extend(child.to_ops(new_id, own))
        return ops


@dataclass
class Text(Component):
    """Wrapped text in one style; ``append`` adds text. With ``link`` the whole text is a
    hyperlink to that URL (http, https, mailto or file)."""

    kind: ClassVar[str] = "text"
    value: str = ""
    tone: str | None = None
    bold: bool | None = None
    dim: bool | None = None
    italic: bool | None = None
    link: str | None = None


@dataclass
class Markdown(Component):
    """Markdown; with ``streaming=True``, ``append`` streams source in line by line."""

    kind: ClassVar[str] = "markdown"
    source: str = ""
    streaming: bool = False


@dataclass
class Code(Component):
    """Monospace text behind a gutter; ``max_lines`` keeps only the tail."""

    kind: ClassVar[str] = "code"
    value: str = ""
    lang: str | None = None
    max_lines: int | None = None


@dataclass
class Log(Component):
    """A scrolling log: ``\\n`` ends a line, ``\\r`` rewrites the current one."""

    kind: ClassVar[str] = "log"
    title: str | None = None
    max_lines: int | None = None
    height: int | None = None


@dataclass
class Status(Component):
    """A spinner with a label and elapsed time."""

    kind: ClassVar[str] = "status"
    label: str = ""
    spinner: bool = True
    tone: str | None = None


@dataclass
class Progress(Component):
    """A bar with value/total, rate and ETA; ``append(n)`` sets the value."""

    kind: ClassVar[str] = "progress"
    value: float = 0
    total: float | None = None
    label: str = ""
    unit: str | None = None


@dataclass
class Sparkline(Component):
    """A one-line chart of recent values; ``append`` takes a number or a list. NaN is a gap."""

    kind: ClassVar[str] = "sparkline"
    label: str = ""
    values: list[float] | None = None
    max_points: int | None = None
    digits: int | None = None


@dataclass
class Chart(Component):
    """A multi-row line chart over the whole run, downsampled to the width.

    ``append`` takes a number or a list (one series named after ``label``), or a dict
    ``{"loss": 0.41, "ema": 0.43}`` to feed several series at once. NaN leaves a gap.
    """

    kind: ClassVar[str] = "chart"
    label: str = ""
    height: int | None = None
    unit: str | None = None
    digits: int | None = None
    y_min: float | None = None
    y_max: float | None = None
    max_points: int | None = None


@dataclass
class KV(Component):
    """Aligned label/value rows, from ``[{"label", "value"}]`` or a dict."""

    kind: ClassVar[str] = "kv"
    fields: list[dict[str, Any]] | dict[str, Any] = field(default_factory=list)


@dataclass
class Table(Component):
    """Rows under a header; rows are lists or dicts keyed by column key."""

    kind: ClassVar[str] = "table"
    columns: list[Any] = field(default_factory=list)
    rows: list[Any] = field(default_factory=list)
    max_rows: int | None = None


@dataclass
class Divider(Component):
    """A horizontal rule with an optional label."""

    kind: ClassVar[str] = "divider"
    label: str | None = None


@dataclass
class Footer(Component):
    """One status line: ``left`` text and ``right`` text aligned to the edge."""

    kind: ClassVar[str] = "footer"
    left: str = ""
    right: str = ""


@dataclass
class Stack(Component):
    """A vertical container with an optional title and blank-line ``gap``."""

    kind: ClassVar[str] = "stack"
    children: list[Component] = field(default_factory=list)
    title: str | None = None
    gap: int | None = None


@dataclass
class Cell(Component):
    """A transcript entry: role icon, optional title, status, indented children.

    ``stream_flush=True`` lets a streaming ``Markdown`` last child reach scrollback line by line
    before the cell seals. ``trailer`` is printed under the cell when it seals. A titled cell
    with ``collapsed=True`` folds to its header and a ``summary`` line; ctrl+o, a click or
    keyboard navigation (ctrl+↑) in full screen opens it.

    ``batch=n`` makes the cell a group of child cells: invisible below n children, from n on one
    folded header with the count, member names and status counts, opening to the members. The
    title is the noun: ``Cell(batch=3, title="tool calls")`` reads "5 tool calls", an untitled
    group "5 items".
    """

    kind: ClassVar[str] = "cell"
    role: str = "system"
    children: list[Component] = field(default_factory=list)
    title: str | None = None
    status: str | None = None
    trailer: str | None = None
    stream_flush: bool | None = None
    icon: str | None = None
    collapsed: bool | None = None
    summary: str | None = None
    tone: str | None = None
    detail: str | None = None
    batch: int | None = None


@dataclass
class Scroll(Component):
    """Children seen through a window of ``height`` rows (0: all); ↑↓, pgup/pgdn, home/end and the
    wheel scroll it, and at the bottom it follows new lines. ``keys`` maps characters to actions
    (``{"k": "stop"}``): pressing one emits ``select`` with the action. Takes ``dock="input"`` as a
    dialog; esc closes it then.
    """

    kind: ClassVar[str] = "scroll"
    children: list[Component] = field(default_factory=list)
    title: str | None = None
    height: int | None = None
    keys: dict[str, str] | None = None
    dock: str | None = None


@dataclass
class Composer(Component):
    """The chat input: history, multi-line editing, a slash-command popup. Emits ``submit``.

    ``boxed=True`` draws it inside a rounded border. ``rich_submit=True`` makes ``submit`` carry
    ``{"text": ..., "now": bool}``, ``now`` set by ctrl+enter.
    """

    kind: ClassVar[str] = "composer"
    placeholder: str | None = None
    boxed: bool | None = None
    rich_submit: bool | None = None
    history: list[str] | None = None
    commands: list[Any] | None = None
    value: str | None = None


@dataclass
class Picker(Component):
    """A fuzzy-filtered list; items are strings or ``{"label", "value", "detail"}``.

    Enter or a click selects; the wheel moves. With ``back=True``, ← closes it with reason
    ``"back"`` (one level up in a stack of dialogs). An item may also carry ``group`` (a section
    header; the filter matches ``group/label``), ``current`` (✓, the list opens on it),
    ``available=False`` (dimmed) and ``choices``: ``[[name, [options]], …]`` with an initial
    ``chosen`` ``{name: option}``; selecting such a row gives
    ``{"value": ..., "choices": {...}}``.

    ``multi=True`` puts ☐/☑ on the rows (an item may start ``checked``): space ticks, and the
    answer is the list of ticked values in item order (the highlighted one when none is).

    Keys act on one focused area: the sections (``sidebar=True`` turns groups into a category
    column, a tab row when narrow), the list, or the highlighted row's options. ←/→ (or tab /
    shift+tab) move the focus across them, ↑↓ act on the focused one (section, item, option
    value), enter selects, typing filters. ``dock="input"`` (every dialog kind) draws the
    dialog where the input box is, in both modes, instead of over the content.
    """

    kind: ClassVar[str] = "picker"
    items: list[Any] = field(default_factory=list)
    title: str | None = None
    placeholder: str | None = None
    max_visible: int | None = None
    back: bool | None = None
    sidebar: bool | None = None
    dock: str | None = None
    multi: bool | None = None
    section: str | None = None


@dataclass
class Confirm(Component):
    """A yes/no question; submits a bool."""

    kind: ClassVar[str] = "confirm"
    question: str = ""
    default: bool | None = None
    yes: str | None = None
    no: str | None = None
    dock: str | None = None


@dataclass
class Prompt(Component):
    """A question with a text answer; submits the text."""

    kind: ClassVar[str] = "prompt"
    question: str = ""
    placeholder: str | None = None
    multiline: bool | None = None
    value: str | None = None
    dock: str | None = None
    secret: bool | None = None


@dataclass
class Card(Component):
    """A titled Markdown body with action buttons; choosing one (enter, its digit or a click)
    selects its id. With ``back=True``, ← closes it with reason ``"back"``."""

    kind: ClassVar[str] = "card"
    title: str | None = None
    body: str = ""
    actions: list[Any] | None = None
    back: bool | None = None
    dock: str | None = None


@dataclass
class Button(Component):
    """One focusable line (↓ from the input reaches it); enter, space or a click selects it.

    ``select`` carries ``value`` (the label when unset). ``hint`` follows the label while not
    focused, ``focused_hint`` while focused. With ``segments`` (``[{"label", "value"}]``) it is
    several buttons in one row: ←/→ choose one while focused, and ``select`` carries that
    segment's value and index.
    """

    kind: ClassVar[str] = "button"
    label: str = ""
    value: Any = None
    hint: str | None = None
    focused_hint: str | None = None
    segments: list[Any] | None = None


@dataclass
class Field(Component):
    """One form control on one line; focusable wherever it is, also in an open transcript cell.

    ``control``: ``slider`` / ``number`` (←/→ step by ``step`` within ``min``/``max``,
    pgup/pgdn ten steps; a number also takes typed digits), ``select`` / ``radio`` (←/→ choose
    one of ``options``: strings or ``{"label", "value"}``), ``toggle`` / ``checkbox`` (space
    flips), ``rating`` (←/→, 0 to ``max``, default 5), ``text`` / ``date`` (an editable line,
    ``placeholder`` while empty). Every change emits ``change`` with the new value; enter in a
    text field also emits ``submit``. Echoing the value back with ``update(value=…)`` is safe:
    an echo of a value the field sent itself is ignored. Esc gives the keys back to the input.
    """

    kind: ClassVar[str] = "field"
    control: str = "text"
    label: str = ""
    value: Any = None
    options: list[Any] | None = None
    min: float | None = None
    max: float | None = None
    step: float | None = None
    placeholder: str | None = None
    disabled: bool | None = None


@dataclass
class Columns(Component):
    """Children side by side, ``count`` per row (0: all in one row), ``gap`` columns apart.

    Where a column would be narrower than ``min_width`` the children stack instead. With
    ``pack`` every child keeps its own width (a row of badges) and the row wraps when full.
    """

    kind: ClassVar[str] = "columns"
    children: list[Component] = field(default_factory=list)
    count: int | None = None
    gap: int | None = None
    min_width: int | None = None
    pack: bool | None = None


@dataclass
class Tabs(Component):
    """A row of ``labels`` over its children (one pane each); only pane ``active`` is drawn.

    Focusable: ←/→ or a click on a label switch, emitting ``change`` with the new index.
    """

    kind: ClassVar[str] = "tabs"
    children: list[Component] = field(default_factory=list)
    labels: list[str] = field(default_factory=list)
    active: int = 0


@dataclass
class Bars(Component):
    """Horizontal bars, one row per label (one per label and series when several series).

    ``series`` is ``[{"name", "data"}]``. ``stacked`` draws the series as segments of one bar;
    ``max`` fixes the full length (a gauge, shown as "value / max"); ``percent`` writes each
    value as its share of the total (a pie as bars). ``digits`` defaults to none for whole
    numbers, else two.
    """

    kind: ClassVar[str] = "bars"
    labels: list[str] = field(default_factory=list)
    series: list[Any] = field(default_factory=list)
    stacked: bool | None = None
    percent: bool | None = None
    max: float | None = None
    digits: int | None = None
