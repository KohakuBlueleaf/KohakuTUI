"""Mode, bindings, notices, transcript printing, event subscription, widget ownership and the
test-backend readers of ``App``."""

import os
import queue
import asyncio
import shlex
import subprocess
import sys
import tempfile
from collections.abc import AsyncIterator, Callable, Iterator, Sequence
from pathlib import Path
from typing import TYPE_CHECKING, Any

from kohakutui.app.events import Event
from kohakutui.ui.components import Cell, Text

if TYPE_CHECKING:
    from kohakutui.ui.node import Node
    from kohakutui.ui.widget import Widget

TRANSCRIPT = 1


class ControlMixin:
    """Everything that is neither lifecycle, dialogs nor capture."""

    def _init_control_state(self) -> None:
        self._owners: dict[int, "Widget"] = {}

    def set_mode(self, mode: str) -> None:
        """Switches between ``"inline"`` and ``"fullscreen"``."""
        self.send([{"op": "set_mode", "mode": mode}])

    def set_theme(
        self,
        scheme: str = "kohaku-dark",
        overrides: dict[str, str] | None = None,
        truecolor: bool | None = None,
    ) -> None:
        """Switches the colour theme (see ``THEMES``).

        ``overrides`` maps a role (``accent``, ``tool``, ``agent``, ``running``, ``attention``,
        ``danger``, ``ok``, ``muted`` …) or a palette name (``iolite``, ``sapphire`` …) to a
        ``#rrggbb`` colour or another name; ``truecolor`` forces 24-bit colour on or off.
        """
        op = {"op": "set_theme", "scheme": scheme, "overrides": dict(overrides or {})}
        if truecolor is not None:
            op["truecolor"] = truecolor
        self.send([op])

    def bind(self, chord: str, action: str) -> None:
        """Makes ``chord`` (e.g. ``"ctrl+k"``) send an ``action`` event named ``action``."""
        self.send([{"op": "bind", "chord": chord, "action": action}])

    def unbind(self, chord: str) -> None:
        self.send([{"op": "unbind", "chord": chord}])

    def notify(self, text: str, level: str = "info", ttl: float = 3.0) -> None:
        """Shows a transient one-line notice above the bottom region."""
        self.send(
            [{"op": "notify", "text": text, "level": level, "ttl_ms": int(ttl * 1000)}]
        )

    def bell(self) -> None:
        self.send([{"op": "bell"}])

    def set_title(self, text: str) -> None:
        """Sets the terminal window's title (OSC 0); ``""`` hands it back to the shell."""
        self.send([{"op": "title", "text": text}])

    def alert(self, body: str, title: str = "") -> None:
        """A desktop notification through the terminal (OSC 9 / OSC 777), where supported.

        Pair it with the ``focus`` event (``gained``) to alert only while the window is away.
        """
        self.send([{"op": "alert", "title": title, "body": body}])

    def scroll_to(self, top: bool = True) -> None:
        """Full screen: scrolls the shown document to its top (or, ``top=False``, its end)."""
        self.send([{"op": "scroll", "top": top}])

    def suspend(self) -> None:
        """Gives the terminal to another program until ``resume``: no drawing, no keys read."""
        self.send([{"op": "suspend"}])
        self.sync()

    def resume(self) -> None:
        """Takes the terminal back after ``suspend`` and redraws everything."""
        self.send([{"op": "resume"}])
        self.sync()

    def run_external(self, argv: Sequence[str]) -> int:
        """Runs ``argv`` with the terminal (suspended around it); returns its exit code.

        Blocks; from asyncio run it in a thread (``asyncio.to_thread``).
        """
        self.suspend()
        try:
            return subprocess.call(list(argv))
        except OSError:
            return 127
        finally:
            self.resume()

    def edit_text(self, text: str = "", suffix: str = ".md") -> str | None:
        """Opens ``text`` in ``$VISUAL`` / ``$EDITOR`` (notepad on Windows, vi elsewhere) and
        returns what was saved; None when the editor could not run or failed.

        Blocks; from asyncio run it in a thread (``asyncio.to_thread``).
        """
        editor = os.environ.get("VISUAL") or os.environ.get("EDITOR")
        if not editor:
            editor = "notepad" if sys.platform == "win32" else "vi"
        fd, name = tempfile.mkstemp(suffix=suffix, prefix="kohakutui-")
        path = Path(name)
        try:
            with os.fdopen(fd, "w", encoding="utf-8", newline="") as f:
                f.write(text)
            argv = shlex.split(editor, posix=sys.platform != "win32") + [str(path)]
            if self.run_external(argv) != 0:
                return None
            return path.read_text(encoding="utf-8")
        finally:
            path.unlink(missing_ok=True)

    def copy(self, text: str) -> None:
        """Puts ``text`` on the system clipboard (OSC 52, on a real terminal) with a notice."""
        self.send([{"op": "copy", "text": text}])

    def print(
        self, *args: Any, sep: str = " ", role: str = "output", tone: str | None = None
    ):
        """Adds a finished line to the transcript, ordered with every other op."""
        text = sep.join(str(a) for a in args)
        node_id = self.new_id()
        spec = Cell(role=role, children=[Text(text, tone=tone)])
        ops = spec.to_ops(self.new_id, TRANSCRIPT, None, node_id)
        ops.append({"op": "seal", "id": node_id})
        self.send(ops)

    def on(self, kind: str, node: "Node | None" = None) -> Callable:
        """Decorator: calls the function (on the pump thread) for each ``kind`` event,
        optionally only from ``node``; ``"*"`` matches every kind."""

        def register(fn: Callable[[Event], Any]) -> Callable[[Event], Any]:
            self.dispatcher.on(kind, node.id if node is not None else None, fn)
            return fn

        return register

    def off(self, fn: Callable[[Event], Any]) -> None:
        self.dispatcher.off(fn)

    def events(self, timeout: float | None = None) -> Iterator[Event]:
        """Yields events in the calling thread; ends after ``stopped`` or a quiet ``timeout``."""
        q = self.dispatcher.open_sync()
        while True:
            try:
                ev = q.get(timeout=timeout)
            except queue.Empty:
                return
            yield ev
            if ev.kind == "stopped":
                return

    async def aevents(self) -> AsyncIterator[Event]:
        """Yields events in the running asyncio loop; ends after ``stopped``."""
        q = self.dispatcher.open_async(asyncio.get_running_loop())
        while True:
            ev = await q.get()
            yield ev
            if ev.kind == "stopped":
                return

    def own(self, ids: list[int], widget: "Widget") -> None:
        """Routes events from nodes ``ids`` to ``widget``."""
        for i in ids:
            self._owners[i] = widget

    def disown(self, node_id: int) -> None:
        self._owners.pop(node_id, None)

    def disown_widget(self, widget: "Widget") -> None:
        for i in [i for i, w in self._owners.items() if w is widget]:
            del self._owners[i]

    def owner_of(self, node_id: int | None) -> "Widget | None":
        return self._owners.get(node_id) if node_id is not None else None

    def screen(self) -> list[str]:
        """Visible rows of a ``backend="test"`` app."""
        return self._session.screen()

    def history(self) -> list[str]:
        """Scrollback then visible rows of a ``backend="test"`` app."""
        return self._session.history()

    def screen_cells(self) -> list[list[tuple[str, str | None, str | None, list[str]]]]:
        """Visible rows of a ``backend="test"`` app with their look: per row, runs of
        ``(text, fg, bg, flags)``. Colours are None (the terminal default), an index ("0" to
        "255") or ``"#rrggbb"``; flags name ``bold``, ``dim``, ``italic``, ``underline`` and
        ``inverse``. Screenshots and snapshot tests read it."""
        return self._session.screen_cells()

    def cursor(self) -> tuple[int, int] | None:
        """The test terminal's cursor as (row, col), None while hidden."""
        return self._session.cursor()

    def inject(self, *events: dict[str, Any]) -> None:
        """Synthetic input for a ``backend="test"`` app: ``{"text": s}`` types at human pace,
        ``{"burst": s}`` at paste pace, ``{"key": "ctrl+t"}``, ``{"paste": s}``,
        ``{"resize": (cols, rows)}``, ``{"click": (col, row)}`` (press and release, full
        screen), ``{"drag": (from_col, row, to_col)}``, ``{"middle": (col, row)}``,
        ``{"scroll": n}`` (wheel steps, positive is up; ``"at": (col, row)`` places the
        pointer)."""
        self._session.inject(list(events))
