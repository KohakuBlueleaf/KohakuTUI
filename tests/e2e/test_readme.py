"""Journey: the README's Quick start runs as written, on the emulated terminal."""

import re
import threading
import time
from pathlib import Path

import kohakutui

README = Path(__file__).resolve().parents[2] / "README.md"


def quick_start() -> str:
    section = README.read_text(encoding="utf-8").split("## Quick start", 1)[1]
    return re.search(r"```python\n(.*?)```", section, re.S)[1]


def test_the_quick_start_runs_and_leaves_its_results_in_scrollback(monkeypatch):
    apps = []
    real_app = kohakutui.App

    def headless(**kwargs):
        app = real_app(backend="test", size=(80, 24), interrupt="event")
        apps.append(app)
        return app

    # the example's own sleeps are skipped; this thread waits with an Event instead
    def answer_the_confirm():
        pause = threading.Event()
        while not apps or not any("Save a checkpoint" in l for l in apps[0].screen()):
            pause.wait(0.02)
        apps[0].inject({"key": "enter"})

    monkeypatch.setattr(kohakutui, "App", headless)
    monkeypatch.setattr(time, "sleep", lambda seconds: None)
    threading.Thread(target=answer_the_confirm, daemon=True).start()

    exec(compile(quick_start(), str(README), "exec"), {"__name__": "readme"})

    history = apps[0].history()
    assert any(l.startswith("• run started") for l in history), history
    assert any(
        l.startswith("loss  ■ loss") for l in history
    ), "the curve is in scrollback"
    assert history[-1].strip() == "saved ckpt-300.pt", history
