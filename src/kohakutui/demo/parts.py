"""Python-side composites the demos are built from: each owns its state and drives primitives.

These are examples of the ``Widget`` contract, not library API: a tool call, a diff, a plan.
"""

import time

from kohakutui.ui import Cell, Code, Log, Node, Stack, Text, Widget

STEP_MARK = {"todo": ("◻", "muted"), "doing": ("▶", "accent"), "done": ("✓", "ok")}


class ToolCall(Widget):
    """One tool call: a header with status, its arguments, streamed output, a closing trailer.

    Collapsed by default, Claude Code style: one summary line under the header until ctrl+o
    (or a click in full screen) opens it.
    """

    def __init__(
        self, name: str, args: str = "", lang: str | None = None, collapsed: bool = True
    ):
        self.name = name
        self.args = args
        self.lang = lang
        self.collapsed = collapsed
        self.started = time.monotonic()
        self.log: Node | None = None

    def build(self) -> Cell:
        children = [Code(self.args, lang=self.lang, max_lines=4)] if self.args else []
        return Cell(
            role="tool",
            title=self.name,
            status="running",
            collapsed=self.collapsed or None,
            children=children,
        )

    def output(self, text: str, height: int = 6) -> None:
        """Streams tool output; ``\\r`` rewrites the last line like a progress bar would."""
        if self.log is None:
            self.log = self.root.add(Log(height=height))
        self.log.append(text)

    def preview(self, text: str, max_lines: int = 6) -> None:
        """Shows a finished result as a short code block."""
        self.root.add(Code(text, max_lines=max_lines))

    def child(self, name: str, args: str = "") -> "ToolCall":
        """A nested tool call (a sub-agent's tools render inside its cell)."""
        call = ToolCall(name, args)
        call.mount(self.root)
        return call

    def done(self, summary: str = "", ok: bool = True) -> None:
        """Marks the call finished with its elapsed time and seals it."""
        took = time.monotonic() - self.started
        trailer = f"{took:.1f}s" + (f" · {summary}" if summary else "")
        status = "ok" if ok else "error"
        self.root.update(status=status, trailer=trailer, summary=summary or None)
        self.root.seal()


class DiffView(Widget):
    """A unified diff with a +/- summary, colored by line kind."""

    def __init__(self, path: str, diff: str):
        self.path = path
        self.lines = diff.strip("\n").split("\n")

    def build(self) -> Stack:
        adds = sum(1 for line in self.lines if line.startswith("+"))
        dels = sum(1 for line in self.lines if line.startswith("-"))
        header = Text(f"{self.path}  +{adds} -{dels}", bold=True)
        return Stack(
            children=[header, *[Text(line, tone=tone_of(line)) for line in self.lines]]
        )


def tone_of(line: str) -> str:
    match line[:1]:
        case "+":
            return "ok"
        case "-":
            return "danger"
        case "@":
            return "accent"
        case _:
            return "muted"


class PlanPanel(Widget):
    """A checklist whose steps move todo → doing → done."""

    def __init__(self, title: str, steps: list[str]):
        self.title = title
        self.steps = steps
        self.state = ["todo"] * len(steps)
        self.rows: list[Node] = []

    def build(self) -> Stack:
        return Stack(title=self.title)

    def mount(self, parent: Node, index: int | None = None) -> Node:
        super().mount(parent, index)
        self.rows = [
            self.root.add(Text(self._label(i))) for i in range(len(self.steps))
        ]
        return self.root

    def _label(self, i: int) -> str:
        return f"{STEP_MARK[self.state[i]][0]} {self.steps[i]}"

    def mark(self, i: int, state: str) -> None:
        self.state[i] = state
        mark, tone = STEP_MARK[state]
        self.rows[i].update(value=f"{mark} {self.steps[i]}", tone=tone)
