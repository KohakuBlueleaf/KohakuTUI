"""Journey: a whole chat session on the emulated terminal, the way an agent front end runs it.

Typing and submitting, a streamed Markdown reply, a long-running tool moved to the live region,
a picker dialog answered by keys, a widget, full screen and back, stop. The screen and the
scrollback are the only things asserted.
"""

import threading

from kohakutui import App, Cell, Code, Composer, Footer, Markdown, Text, Widget
from tests.conftest import until


class ModelBadge(Widget):
    """A widget: owns the chosen model and renders it as text."""

    def __init__(self) -> None:
        self.model = "model-a"

    def build(self) -> Text:
        return Text(f"model: {self.model}", tone="accent")

    def set_model(self, model: str) -> None:
        self.model = model
        self.root.update(value=f"model: {model}")


def index_of(lines: list[str], needle: str) -> int:
    return next(i for i, line in enumerate(lines) if needle in line)


def test_chat_session_journey():
    app = App(backend="test", size=(60, 16), interrupt="event").start()
    composer = app.bottom.add(Composer(placeholder="say", commands=[{"name": "model"}]))
    app.bottom.add(Footer(left="demo", right="ready"))
    badge = ModelBadge()
    badge.mount(app.panels)
    submitted: list[str] = []
    app.on("submit", composer)(lambda ev: submitted.append(ev.value))

    # typing at human pace submits; an Enter inside a paste-paced burst is a newline
    app.inject({"text": "hello"}, {"key": "enter"})
    until(app, lambda: submitted == ["hello"])
    app.inject({"burst": "line1\nline2"})
    app.sync()
    assert submitted == ["hello"]
    assert app.history()[-3:-1] == ["› line1", "  line2"]
    app.inject({"key": "enter"})
    until(app, lambda: submitted == ["hello", "line1\nline2"])

    # the user turn and a streamed assistant reply: stable lines reach scrollback, the open
    # line stays in the viewport
    app.print("hello", role="user")
    cell = app.transcript.add(Cell(role="assistant", stream_flush=True))
    body = cell.add(Markdown(streaming=True))
    for i in range(20):
        body.append(f"reply line {i}\n")
    body.append("partial tail")
    until(app, lambda: any("reply line 19" in line for line in app.history()))
    assert app.screen().count("  partial tail") == 1

    # an open tool cell holds later cells below it; moved to live, they flow past it
    tool = app.transcript.add(Cell(role="tool", title="grep", status="running"))
    tool.add(Code(value="rg --json pattern"))
    cell.seal()
    app.print("after the tool", role="system")
    app.sync()
    lines = app.history()
    assert index_of(lines, "grep") < index_of(lines, "after the tool")
    tool.move(app.live)
    app.sync()
    lines = app.history()
    assert index_of(lines, "after the tool") < index_of(lines, "grep")

    # a dialog from a worker thread, answered by keys; the widget shows the result
    answer: dict = {}

    def ask() -> None:
        answer["v"] = app.choose("Model", ["model-a", "model-b"])

    worker = threading.Thread(target=ask)
    worker.start()
    until(app, lambda: any("│ Model" in line for line in app.screen()))
    app.inject({"text": "-b"}, {"key": "enter"})
    worker.join(timeout=3)
    assert answer == {"v": "model-b"}
    badge.set_model(answer["v"])
    until(app, lambda: not any("│ Model" in line for line in app.screen()))

    # full screen shows the transcript, the panel and the live tool; ctrl+t comes back
    app.inject({"key": "ctrl+t"})
    until(app, lambda: any("model: model-b" in line for line in app.screen()))
    full = app.screen()
    assert any("grep" in line for line in full)
    assert any("reply line 19" in line for line in full)
    app.inject({"key": "ctrl+t"})
    until(app, lambda: not any("model: model-b" in line for line in app.screen()))
    assert app.history()[-2] == "› say"

    # the tool finishes: its live cell goes, a summary lands in the transcript; stop flushes
    tool.remove()
    app.print("grep · 3 matches", role="system")
    app.stop()
    final = app.history()
    assert sum("reply line 7" in line for line in final) == 1, final
    assert final[-1] == "• grep · 3 matches", final
    assert not any("say" in line for line in final), "the composer is cleared on stop"
