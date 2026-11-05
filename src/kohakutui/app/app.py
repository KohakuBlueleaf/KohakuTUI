"""``App``: one terminal UI session, assembled from one mixin per concern."""

import os

from kohakutui import _rs
from kohakutui.app.capture import CaptureMixin
from kohakutui.app.control import ControlMixin
from kohakutui.app.core import CoreMixin
from kohakutui.app.dialogs import DialogsMixin
from kohakutui.ui.components import Document
from kohakutui.ui.node import Node

TRANSCRIPT, LIVE, BOTTOM, PANELS, OVERLAYS, DOCS, HEAD = 1, 2, 3, 4, 5, 6, 7

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
        # full screen only: a row over the transcript, for a tab strip
        self.head = Node(self, HEAD, "region")
        self._docs = Node(self, DOCS, "region")
        self._theme = theme

    def add_document(self) -> Node:
        """A further transcript (one per conversation): add cells to it like to
        ``app.transcript``; it is drawn, and inline reaches scrollback, while it is shown.
        """
        return self._docs.add(Document())

    def show_document(
        self, document: Node, note: str | None = None, top: bool = False
    ) -> None:
        """Shows ``document`` (or ``app.transcript``) where the transcript is. Full screen keeps
        each document's scroll; inline, its lines not yet in scrollback are written now, after
        ``note`` (a divider line that belongs to the switch, not to either document). With
        ``top`` (a page rather than a conversation) full screen first shows its top.
        """
        op: dict = {"op": "show_doc", "id": document.id}
        if note:
            op["note"] = note
        if top:
            op["top"] = True
        self.send([op])

    def start(self) -> "App":
        """Takes the terminal, starts the engine and the pump, and applies the theme."""
        started = self.running
        super().start()
        if not started and self._theme:
            self.set_theme(self._theme)
        return self
