"""``App``: one terminal UI session, assembled from one mixin per concern."""

import os

from kohakutui import _rs
from kohakutui.app.capture import CaptureMixin
from kohakutui.app.control import ControlMixin
from kohakutui.app.core import CoreMixin
from kohakutui.app.dialogs import DialogsMixin
from kohakutui.ui.node import Node

TRANSCRIPT, LIVE, BOTTOM, PANELS, OVERLAYS = 1, 2, 3, 4, 5

THEMES = (
    "kohaku-dark",
    "kohaku-light",
    "terminal",
    "colorblind-dark",
    "colorblind-light",
    "solarized-dark",
    "solarized-light",
    "tokyo-night",
)


class App(CoreMixin, ControlMixin, DialogsMixin, CaptureMixin):
    """A terminal UI: describe the screen with nodes, react to events.

    ``mode`` is ``"inline"`` (finished output goes to scrollback) or ``"fullscreen"``; ctrl+t
    switches. ``backend`` is ``"auto"`` (the terminal, else plain stdout), ``"tty"``,
    ``"plain"`` or ``"test"`` (an emulated ``size`` screen). ``interrupt`` decides ctrl+c:
    ``"raise"`` (KeyboardInterrupt in the main thread), ``"signal"`` (SIGINT, Unix),
    ``"event"`` (only an ``interrupt`` event) or ``"exit"``; ``"auto"`` is signal on Unix and
    raise elsewhere. ``theme`` is a scheme from ``THEMES`` applied at start. ``links`` is
    ``"auto"`` (OSC 8 hyperlinks on a terminal, ``text (url)`` on plain output), ``"osc8"`` or
    ``"text"`` (for terminals that drop OSC 8). With ``open_links``, a link clicked in full
    screen (a ``link`` event) opens in the browser: http, https and mailto only.
    """

    def __init__(
        self,
        *,
        mode: str = "inline",
        backend: str = "auto",
        size: tuple[int, int] | None = None,
        interrupt: str = "auto",
        max_fps: int = 60,
        theme: str | None = None,
        links: str = "auto",
        open_links: bool = True,
    ):
        if interrupt == "auto":
            interrupt = "signal" if os.name == "posix" else "raise"
        session = _rs._Session(
            backend=backend,
            mode=mode,
            size=size,
            interrupt=interrupt,
            max_fps=max_fps,
            links=links,
        )
        self._init_core_state(
            session, raise_on_interrupt=interrupt == "raise", open_links=open_links
        )
        self._init_control_state()
        self._init_capture_state()

        self.transcript = Node(self, TRANSCRIPT, "region")
        self.live = Node(self, LIVE, "region")
        self.bottom = Node(self, BOTTOM, "region")
        self.panels = Node(self, PANELS, "region")
        self.overlays = Node(self, OVERLAYS, "region")
        self._theme = theme

    def start(self) -> "App":
        """Takes the terminal, starts the engine and the pump, and applies the theme."""
        started = self.running
        super().start()
        if not started and self._theme:
            self.set_theme(self._theme)
        return self
