"""Integration: ``kohakutui.App`` with its nodes, events, dialogs, widgets and capture.

Each method is one complete workflow, driven through the public API and read back on the
emulated terminal.
"""

import sys
import asyncio
import logging
import threading

import pytest

from kohakutui import (
    KV,
    App,
    Card,
    Cell,
    Code,
    Composer,
    Log,
    Markdown,
    Progress,
    Sparkline,
    Stack,
    Table,
    Text,
    Widget,
)
from tests.conftest import until


class Counter(Widget):
    """Counts submits from its own prompt."""

    def __init__(self) -> None:
        self.count = 0

    def build(self) -> Stack:
        return Stack(children=[Text("count: 0"), Composer(placeholder="widget input")])

    def handle(self, ev) -> None:
        if ev.kind == "submit":
            self.count += 1


class TestApp:
    def test_node_lifecycle_updates_the_screen_and_reports_bad_ops(self, app: App):
        errors = []
        app.on("error")(errors.append)

        with app.batch():
            panel = app.live.add(Stack(title="Status", children=[Text("first")]))
            line = panel.add(Text("second"))
        until(app, lambda: app.screen()[:4] == ["", "Status", "first", "second"])

        line.update(value="second, changed")
        line.append(" + more")
        until(app, lambda: "second, changed + more" in app.screen())

        line.remove()
        app.transcript.add(Text("in transcript")).seal()
        until(app, lambda: "second, changed + more" not in app.screen())
        assert "in transcript" in app.history()

        sealed = app.transcript.add(Cell(role="system", title="sealed"))
        sealed.seal()
        sealed.update(title="nope")
        line.append("gone")
        until(app, lambda: len(errors) == 2)
        assert "sealed" in errors[0].message
        assert "no node" in errors[1].message

    def test_data_components_render_values(self, app: App):
        progress = app.live.add(Progress(total=10, label="epoch"))
        spark = app.live.add(Sparkline(label="loss", digits=2))
        app.live.add(KV(fields={"lr": "3e-4", "step": 5}))
        table = app.live.add(Table(columns=["name", "score"]))
        progress.append(5)
        spark.append([1.0, float("nan"), 0.5])
        table.append({"name": "a", "score": 1})
        until(app, lambda: any("5/10" in line for line in app.screen()))
        screen = app.screen()
        assert any(line.startswith("loss █·▁ 0.50") for line in screen), screen
        assert any(line.startswith("lr") and "3e-4" in line for line in screen), screen
        assert any(line.startswith("a    │ 1") for line in screen), screen

    def test_sync_dialogs_answer_by_keys_cancel_and_time_out(self, app: App):
        results = {}

        def run(name, fn):
            t = threading.Thread(target=lambda: results.update({name: fn()}))
            t.start()
            return t

        t = run("ask", lambda: app.ask("Name?", placeholder="type"))
        until(app, lambda: any("Name?" in line for line in app.screen()))
        app.inject({"text": "Kohaku"}, {"key": "enter"})
        t.join(3)

        t = run("confirm", lambda: app.confirm("Sure?"))
        until(app, lambda: any("Sure?" in line for line in app.screen()))
        app.inject({"text": "y"})
        t.join(3)

        t = run("card", lambda: app.card("Deploy", "**now**?", ["go", "stop"]))
        until(app, lambda: any("Deploy" in line for line in app.screen()))
        app.inject({"key": "right"}, {"key": "enter"})
        t.join(3)

        t = run("cancel", lambda: app.choose("Pick", ["a"]))
        until(app, lambda: any("Pick" in line for line in app.screen()))
        app.inject({"key": "esc"})
        t.join(3)

        assert results == {
            "ask": "Kohaku",
            "confirm": True,
            "card": "stop",
            "cancel": None,
        }
        assert app.choose("Nobody answers", ["a"], timeout=0.2) is None
        until(app, lambda: not any("│" in line for line in app.screen()))

    def test_dialogs_answer_clicks_in_full_screen(self, app: App):
        app.set_mode("fullscreen")
        results = {}

        def run(name, fn):
            t = threading.Thread(target=lambda: results.update({name: fn()}))
            t.start()
            return t

        def find(word: str) -> tuple[int, int] | None:
            for row, line in enumerate(app.screen()):
                if f" {word} " in line or line.rstrip(" │").endswith(f" {word}"):
                    return line.index(word) + 1, row
            return None

        t = run("choose", lambda: app.choose("Pick", ["alpha", "beta", "gamma"]))
        until(app, lambda: find("beta") is not None)
        app.inject({"click": find("beta")})
        t.join(3)

        t = run("card", lambda: app.card("Deploy", "now?", ["go", "stop"]))
        until(app, lambda: find("stop") is not None)
        app.inject({"click": find("stop")})
        t.join(3)
        assert results == {"choose": "beta", "card": "stop"}

    def test_typing_past_an_open_dialog_reaches_the_input(self, app: App):
        composer = app.bottom.add(Composer())
        sent = []
        app.on("submit", composer)(lambda ev: sent.append(ev.value))
        answer = {}
        t = threading.Thread(
            target=lambda: answer.update(v=app.card("Run?", "", ["yes", "no"]))
        )
        t.start()
        until(app, lambda: any(" yes " in line for line in app.screen()))

        app.inject({"text": "/tasks"}, {"key": "enter"})
        until(app, lambda: sent == ["/tasks"])
        assert t.is_alive(), "the dialog is still waiting"
        app.inject({"key": "right"}, {"key": "enter"})
        t.join(3)
        assert answer == {"v": "no"}

    def test_dialogs_refuse_to_block_the_pump(self, app: App):
        errors = []
        app.on("error")(errors.append)
        seen = []

        @app.on("submit")
        def blocked(ev):
            try:
                app.confirm("from the pump")
            except RuntimeError as e:
                seen.append(str(e))

        app.bottom.add(Composer())
        app.inject({"text": "x"}, {"key": "enter"})
        until(app, lambda: bool(seen))
        assert "event pump" in seen[0]

    def test_async_events_and_dialogs(self, app: App):
        async def flow():
            task = asyncio.create_task(app.achoose("Async pick", ["one", "two"]))
            await asyncio.sleep(0.2)
            app.sync()
            app.inject({"text": "two"}, {"key": "enter"})
            choice = await asyncio.wait_for(task, 3)

            app.bottom.add(Composer())
            app.inject({"text": "hey"}, {"key": "enter"})
            async for ev in app.aevents():
                if ev.kind == "submit":
                    return choice, ev.value

        assert asyncio.run(flow()) == ("two", "hey")
        with pytest.raises(RuntimeError):
            next(app.events())

    def test_widgets_receive_their_own_events_only(self, app: App):
        widget = Counter()
        widget.mount(app.bottom)
        other = []
        app.on("submit")(other.append)

        app.inject({"text": "a"}, {"key": "enter"}, {"text": "b"}, {"key": "enter"})
        until(app, lambda: widget.count == 2)
        assert len(other) == 2, "callbacks still observe owned events"

        widget.unmount()
        until(app, lambda: "widget input" not in "".join(app.screen()))

    def test_capture_routes_print_and_logging(self, app: App):
        log = app.live.add(Log(title="out", height=4))
        app.capture_stdio(target=log)
        print("progress 10%", end="\r")
        print("progress 100%")
        app.release_capture()
        assert sys.stdout is sys.__stdout__ or not hasattr(sys.stdout, "app")

        app.capture_stdio()
        print("as a cell")
        app.release_capture()
        app.capture_logging(logging.WARNING)
        logging.getLogger("kt.test").warning("careful")
        app.release_capture()

        until(app, lambda: "progress 100%" in app.screen())
        history = app.history()
        assert "progress 10%" not in history
        assert "as a cell" in history
        assert any("[WARNING] kt.test: careful" in line for line in history)

    def test_stream_events_from_the_main_thread_and_stop(self):
        app = App(backend="test", size=(40, 10), interrupt="event").start()
        app.bottom.add(Composer())
        app.bind("f5", "refresh")
        app.inject({"key": "f5"}, {"key": "ctrl+c"})
        kinds = []
        for ev in app.events(timeout=1.0):
            kinds.append(ev.kind)
            if ev.kind == "interrupt":
                break
        assert kinds == ["ready", "action", "interrupt"]

        cell = app.transcript.add(Cell(role="assistant", stream_flush=True))
        cell.add(Markdown("never sealed", streaming=True))
        app.stop()
        app.stop()
        assert app.history() == ["● never sealed"]

    def test_stop_keeps_batched_ops_and_quiets_every_later_call(self):
        app = App(backend="test", size=(40, 10), interrupt="event").start()
        with app.batch():
            app.transcript.add(Text("batched before stop")).seal()
            app.stop()
        assert app.history() == ["batched before stop"], "pending ops land before stop"

        node = app.live.add(Text("after stop"))
        node.update(value="still quiet")
        app.print("ignored")
        assert app.choose("too late", ["a"]) is None
        assert not app.running

    def test_an_engine_side_stop_releases_dialogs_and_marks_the_app_stopped(self):
        app = App(backend="test", size=(40, 10), interrupt="event").start()
        answer = {}
        worker = threading.Thread(
            target=lambda: answer.update(v=app.choose("Pick", ["a"]))
        )
        worker.start()
        until(app, lambda: any("Pick" in line for line in app.screen()))

        app._session.stop()
        worker.join(timeout=3)
        assert not worker.is_alive(), "the dialog must not hang once the engine is gone"
        assert answer == {"v": None}
        app.pump_thread.join(timeout=2)
        assert not app.running
        app.transcript.add(Text("no raise"))
        app.stop()

    def test_batches_belong_to_their_thread(self, app: App):
        inside = threading.Event()
        release = threading.Event()

        def batching():
            with app.batch():
                app.live.add(Text("from the batch"))
                inside.set()
                release.wait(3)

        worker = threading.Thread(target=batching)
        worker.start()
        inside.wait(3)
        app.live.add(Text("direct"))
        until(app, lambda: "direct" in app.screen())
        assert (
            "from the batch" not in app.screen()
        ), "another thread's batch is not flushed early"
        release.set()
        worker.join(3)
        until(app, lambda: "from the batch" in app.screen())

    def test_folded_tool_cells_open_with_ctrl_o_and_clicks(self, app: App):
        tool = Cell(
            role="tool",
            title="grep",
            collapsed=True,
            summary="3 matches",
            children=[Code("rg --json pattern")],
        )
        app.transcript.add(tool).seal()
        until(app, lambda: "  ⎿  3 matches  (ctrl+o to expand)" in app.history())
        assert not any("rg --json" in line for line in app.history())

        app.inject({"key": "ctrl+o"})
        until(app, lambda: any("rg --json" in line for line in app.screen()))
        header = app.screen().index("◆ grep")
        app.inject({"click": (2, header)})
        until(app, lambda: not any("rg --json" in line for line in app.screen()))
        app.inject({"click": (2, header)})
        until(app, lambda: any("rg --json" in line for line in app.screen()))

        app.inject({"key": "ctrl+t"})
        until(app, lambda: not any("rg --json" in line for line in app.screen()))

    def test_card_in_the_transcript_is_static(self, app: App):
        app.transcript.add(Card(title="Result", body="- one\n- two")).seal()
        until(app, lambda: "Result" in app.history())
        assert "• one" in app.history()
