"""Journey: the shipped demos and CLI run end to end on the emulated terminal."""

import importlib.metadata
import time
import threading

import pytest

from kohakutui import App
from kohakutui.__main__ import main
from kohakutui.demo.agent import AgentDemo, agent
from kohakutui.demo.dashboard import Dashboard
from kohakutui.demo.tasks import BackgroundTasks
from kohakutui.demo.train import train
from tests.conftest import until


def make_app(cols: int = 90, rows: int = 30, mode: str = "inline") -> App:
    return App(backend="test", size=(cols, rows), mode=mode, interrupt="event")


def test_train_demo_keeps_scrollback_still_until_the_run_ends():
    app = make_app()
    kwargs = {"epochs": 2, "steps": 200, "app": app}
    worker = threading.Thread(target=train, kwargs=kwargs)
    worker.start()
    until(app, lambda: app.running and any("1/2" in line for line in app.screen()))
    mid_run = app.history()
    chart_at = next(i for i, line in enumerate(mid_run) if line.startswith("loss  ■"))
    written = [line for line in mid_run[:chart_at] if line.strip()]
    assert written == [
        "• training started",
        "  model  tiny-dit (demo)",
        "  batch  256",
        "  lr     3e-4 cosine",
    ], "nothing but the start cell is written mid-run"
    worker.join(timeout=10)

    history = app.history()
    assert not any("loss is NaN" in line for line in history), "events stay out"
    assert any(line.startswith("loss  ■ loss") for line in history), "the curve is kept"
    rows = [line for line in history if line.startswith(("1 ", "2 "))]
    assert len(rows) == 2, "every epoch row"
    assert history[-1].startswith("• done in 400 steps"), history
    assert history[-1].endswith("1 NaN batch skipped"), history


def test_agent_demo_plays_a_whole_turn():
    app = make_app()
    agent(speed=0.0, interactive=False, app=app)
    history = app.history()
    for needle in ["✓ read_file", "src/client.py  +9 -3", "⏵ npm run dev"]:
        assert any(needle in line for line in history), (needle, history)
    assert "  ⎿  12 passed  (ctrl+o to expand)" in history, "tool runs are folded"
    assert not any(
        "12 passed in 1.84s" in line for line in history
    ), "their output is hidden"
    assert any(line.startswith("● Done.") for line in history)


def test_background_tasks_list_expand_and_kill():
    app = make_app(100, 40).start()
    demo = AgentDemo(app, speed=0.0, interactive=False)
    demo.run()
    until(app, lambda: any("2 background tasks" in line for line in app.screen()))

    app.inject({"key": "ctrl+b"})
    until(app, lambda: any("Background tasks" in line for line in app.screen()))
    assert any("tail -f logs/app.log" in line for line in app.screen())

    app.inject({"text": "npm"}, {"key": "enter"})
    until(app, lambda: any("npm run dev · running" in line for line in app.screen()))
    until(app, lambda: any("GET /" in line for line in app.screen()), timeout=5)

    app.inject({"key": "right"}, {"key": "enter"})
    until(app, lambda: any("Background tasks" in line for line in app.screen()))
    app.inject({"key": "esc"})
    until(app, lambda: any("1 background task ·" in line for line in app.screen()))
    assert any("killed background task: npm run dev" in line for line in app.history())
    app.stop()


def test_background_tasks_by_keyboard_down_enter_left_and_esc():
    app = make_app(100, 40).start()
    AgentDemo(app, speed=0.0, interactive=False).run()
    until(app, lambda: any("2 background tasks · ↓ to view" in l for l in app.screen()))

    def shown(text: str) -> bool:
        return any(text in line for line in app.screen())

    app.inject({"key": "down"})
    until(app, lambda: shown("2 background tasks  · enter to view"))
    app.inject({"key": "enter"})
    until(app, lambda: shown("Background tasks"))
    app.inject({"key": "down"}, {"key": "enter"})
    until(app, lambda: shown("tail -f logs/app.log · running"))

    app.inject({"key": "left"})
    until(app, lambda: shown("Background tasks") and not shown("app.log · running"))
    app.inject({"key": "left"})
    until(app, lambda: not shown("Background tasks"))
    assert shown("enter to view"), "← from the list lands on the status line"

    app.inject({"key": "enter"})
    until(app, lambda: shown("Background tasks"))
    app.inject({"key": "enter"})
    until(app, lambda: shown("npm run dev · running"))
    app.inject({"key": "esc"})
    until(
        app,
        lambda: not shown("npm run dev · running") and not shown("Background tasks"),
    )

    app.inject({"key": "up"}, {"text": "hi"})
    until(app, lambda: shown("> hi"))
    app.stop()


def test_the_approval_card_never_traps_the_user():
    app = make_app(100, 44).start()
    demo = AgentDemo(app, speed=0.0, interactive=True)
    worker = threading.Thread(target=demo.run, daemon=True)
    worker.start()

    def shown(text: str) -> bool:
        return any(text in line for line in app.screen())

    until(app, lambda: shown("Run this command?"), timeout=10)
    app.inject({"text": "/model"}, {"key": "enter"})
    until(app, lambda: shown("│ Model"))
    app.inject({"key": "esc"})
    until(app, lambda: shown("Run this command?") and not shown("│ Model"))

    app.inject({"text": "and update the docs"}, {"key": "enter"})
    until(app, lambda: shown("queued: and update the docs"))
    app.inject({"key": "down"}, {"text": "x"})
    until(app, lambda: shown("> x"))
    app.inject({"key": "tab"}, {"key": "esc"})
    until(app, lambda: any("command denied by the user" in l for l in app.history()))

    # the queued message is the next turn; it asks again, and enter approves this time
    until(
        app,
        lambda: any("› and update the docs" in l for l in app.history()),
        timeout=10,
    )
    until(app, lambda: shown("Run this command?"), timeout=10)
    app.inject({"key": "enter"})
    until(app, lambda: sum("12 passed" in l for l in app.history()) >= 1, timeout=10)
    demo.quit.set()
    app.stop()


def test_opening_the_task_list_twice_quickly_opens_it_once():
    app = make_app().start()
    tasks = BackgroundTasks(app)
    tasks.start("job", iter([(0.05, "tick")] * 200))
    tasks.open()
    tasks.open()
    until(app, lambda: any("Background tasks" in line for line in app.screen()))
    app.inject({"key": "esc"})
    until(app, lambda: not any("Background tasks" in line for line in app.screen()))
    time.sleep(0.3)
    app.sync()
    assert not any(
        "Background tasks" in line for line in app.screen()
    ), "no second list"
    app.stop()


def test_dashboard_demo_fills_its_main_pane_and_panel():
    app = make_app(110, 40, "fullscreen").start()
    board = Dashboard(app, epochs=1, steps=60, speed=0.0, hold=True)
    worker = threading.Thread(target=board.run)
    worker.start()

    until(app, lambda: any("epoch 1 done" in line for line in app.screen()))
    screen = app.screen()
    for needle in [
        "total",
        "loss  ■ loss",
        "learning rate",
        "grad norm",
        "gpu0",
        "eval",
    ]:
        assert any(needle in line for line in screen), (needle, screen)

    app.inject({"key": "q"})
    worker.join(timeout=3)
    assert not worker.is_alive(), "q ends the dashboard"
    app.stop()


def test_cli_prints_version_and_rejects_unknown_commands(capsys: pytest.CaptureFixture):
    assert main(["version"]) == 0
    assert capsys.readouterr().out.strip() == importlib.metadata.version("kohakutui")
    assert main(["nope"]) == 2
    assert "usage:" in capsys.readouterr().out
