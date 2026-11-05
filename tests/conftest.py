"""Shared fixtures: an app on the emulated terminal, and a wait-until helper."""

import time
from collections.abc import Callable, Iterator

import pytest

from kohakutui import App


def until(app: App, cond: Callable[[], bool], timeout: float = 3.0) -> None:
    """Syncs the engine until ``cond`` holds; fails the test after ``timeout``."""
    deadline = time.monotonic() + timeout
    while True:
        app.sync()
        if cond():
            return
        if time.monotonic() > deadline:
            raise AssertionError(
                f"condition not met; screen:\n" + "\n".join(app.history())
            )
        time.sleep(0.02)


@pytest.fixture
def app() -> Iterator[App]:
    """A started app on a 60×16 emulated terminal; ctrl+c only sends events."""
    a = App(backend="test", size=(60, 16), interrupt="event").start()
    try:
        yield a
    finally:
        a.stop()
