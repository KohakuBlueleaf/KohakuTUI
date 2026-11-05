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
    """Wrapped text in one style; ``append`` adds text."""

    kind: ClassVar[str] = "text"
    value: str = ""
    tone: str | None = None
    bold: bool | None = None
    dim: bool | None = None
    italic: bool | None = None


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
    before the cell seals. ``trailer`` is printed under the cell when it seals.
    """

    kind: ClassVar[str] = "cell"
    role: str = "system"
    children: list[Component] = field(default_factory=list)
    title: str | None = None
    status: str | None = None
    trailer: str | None = None
    stream_flush: bool | None = None
    icon: str | None = None


@dataclass
class Composer(Component):
    """The chat input: history, multi-line editing, a slash-command popup. Emits ``submit``."""

    kind: ClassVar[str] = "composer"
    placeholder: str | None = None
    history: list[str] | None = None
    commands: list[Any] | None = None
    value: str | None = None


@dataclass
class Picker(Component):
    """A fuzzy-filtered list; items are strings or ``{"label", "value", "detail"}``."""

    kind: ClassVar[str] = "picker"
    items: list[Any] = field(default_factory=list)
    title: str | None = None
    placeholder: str | None = None
    max_visible: int | None = None


@dataclass
class Confirm(Component):
    """A yes/no question; submits a bool."""

    kind: ClassVar[str] = "confirm"
    question: str = ""
    default: bool | None = None
    yes: str | None = None
    no: str | None = None


@dataclass
class Prompt(Component):
    """A question with a text answer; submits the text."""

    kind: ClassVar[str] = "prompt"
    question: str = ""
    placeholder: str | None = None
    multiline: bool | None = None
    value: str | None = None


@dataclass
class Card(Component):
    """A titled Markdown body with action buttons; choosing one selects its id."""

    kind: ClassVar[str] = "card"
    title: str | None = None
    body: str = ""
    actions: list[Any] | None = None
