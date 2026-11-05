"""Journey: the shipped demos and CLI run end to end on the emulated terminal."""

import pytest

import kohakutui.demo.train as train_demo
from kohakutui import App
from kohakutui.__main__ import main


def test_train_demo_runs_to_the_end(monkeypatch: pytest.MonkeyPatch):
    apps: list[App] = []

    def test_app() -> App:
        app = App(backend="test", size=(70, 20), interrupt="event")
        apps.append(app)
        return app

    monkeypatch.setattr(train_demo, "App", test_app)
    train_demo.train(steps=60)
    history = apps[0].history()
    assert history[0] == "• training demo", history
    assert history[-1].startswith("• done: final loss"), history


def test_cli_prints_version_and_rejects_unknown_commands(capsys: pytest.CaptureFixture):
    assert main(["version"]) == 0
    assert capsys.readouterr().out.strip() == "0.0.1"
    assert main(["nope"]) == 2
    assert "usage:" in capsys.readouterr().out
