"""Lifecycle, node ids, op sending and the event pump: the part of ``App`` every other part uses.

Ops go to the engine without waiting. A ``batch()`` belongs to the thread that opened it, so
one thread's batch never absorbs or delays another thread's ops. Once the app has stopped,
from ``stop()`` or because the engine ended, every send is a silent no-op.
"""

import atexit
import threading
import webbrowser
import itertools
import _thread
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any

from kohakutui import _rs
from kohakutui.app.events import Dispatcher, Event

# what a click may open: never file:// or a custom scheme that starts some other program
OPENABLE = ("http://", "https://", "mailto:")


def open_link(url: str) -> bool:
    """Opens ``url`` in the browser on its own thread (a slow launcher never stalls events);
    False for a URL that may not be opened."""
    if not isinstance(url, str) or not url.lower().startswith(OPENABLE):
        return False
    threading.Thread(target=webbrowser.open, args=(url,), daemon=True).start()
    return True


FIRST_ID = 16


class CoreMixin:
    """Engine session, ids, sending and batching, start/stop and the pump thread."""

    def _init_core_state(
        self,
        session: "_rs._Session",
        raise_on_interrupt: bool,
        open_links: bool = True,
    ) -> None:
        self._session = session
        self._raise_on_interrupt = raise_on_interrupt
        self._open_links = open_links
        self._ids = itertools.count(FIRST_ID)
        self._id_lock = threading.Lock()
        self._send_lock = threading.Lock()
        self._state_lock = threading.Lock()
        self._local = threading.local()
        self._started = False
        # ops sent before start (None once started)
        self._early: list[dict[str, Any]] | None = []
        self._stopping = False
        self._stopped = False
        self.dispatcher = Dispatcher(self)
        self.pump_thread: threading.Thread | None = None
        self.ready: Event | None = None

    def start(self) -> "CoreMixin":
        """Takes the terminal and starts the engine and the event pump."""
        with self._state_lock:
            if self._started:
                return self
            self._started = True
        self._session.start()
        with self._send_lock:
            with self._state_lock:
                early, self._early = self._early or [], None
            if early:
                try:
                    self._session.apply(early)
                except RuntimeError:
                    self._mark_stopped()
        self.pump_thread = threading.Thread(
            target=self._pump, name="kohakutui-pump", daemon=True
        )
        self.pump_thread.start()
        atexit.register(self.stop)
        return self

    def stop(self) -> None:
        """Flushes the transcript to scrollback, restores the terminal and ends the pump.

        Safe to call more than once, from any thread and from a callback. Ops sent inside an
        open ``batch()`` on this thread are delivered first.
        """
        with self._state_lock:
            if not self._started or self._stopping:
                return
            self._stopping = True
        atexit.unregister(self.stop)
        self.release_capture()

        pending = getattr(self._local, "pending", None)
        if pending:
            self._local.pending = []
            self._apply(pending)

        self._mark_stopped()
        self._session.stop()
        if (
            self.pump_thread is not None
            and self.pump_thread is not threading.current_thread()
        ):
            self.pump_thread.join(timeout=2.0)

    def _mark_stopped(self) -> None:
        self._stopped = True
        self.dispatcher.close()

    def __enter__(self):
        return self.start()

    def __exit__(self, *exc: object) -> None:
        self.stop()

    async def __aenter__(self):
        return self.start()

    async def __aexit__(self, *exc: object) -> None:
        self.stop()

    @property
    def running(self) -> bool:
        return self._started and not self._stopped

    def _pump(self) -> None:
        while True:
            try:
                batch = self._session.poll(None)
            except RuntimeError:
                batch = [{"ev": "stopped"}]
            for d in batch:
                ev = Event.from_dict(d)
                match ev.kind:
                    case "ready":
                        self.ready = ev
                    case "interrupt" if self._raise_on_interrupt:
                        _thread.interrupt_main()
                    case "link" if self._open_links:
                        open_link(ev.data.get("url", ""))
                    case "stopped":
                        self._mark_stopped()
                self.dispatcher.dispatch(ev)
                if ev.kind == "stopped":
                    return

    def new_id(self) -> int:
        """A fresh node id."""
        with self._id_lock:
            return next(self._ids)

    def send(self, ops: list[dict[str, Any]]) -> None:
        """Sends ops now, or collects them when this thread is inside ``batch()``."""
        pending = getattr(self._local, "pending", None)
        if pending is not None:
            pending.extend(ops)
            return
        self._apply(ops)

    def _apply(self, ops: list[dict[str, Any]]) -> None:
        if not ops or self._stopped:
            return
        with self._state_lock:
            if self._early is not None:
                # built before start: delivered when the engine starts, in order
                self._early.extend(ops)
                return

        # one lock around the hand-off keeps ops from two threads in the order they were sent
        with self._send_lock:
            try:
                self._session.apply(ops)
            except RuntimeError:
                self._mark_stopped()

    @contextmanager
    def batch(self) -> Iterator[None]:
        """Groups this thread's ops into one delivery, so they land in one frame."""
        depth = getattr(self._local, "depth", 0)
        if depth == 0:
            self._local.pending = []
        self._local.depth = depth + 1
        try:
            yield
        finally:
            self._local.depth -= 1
            if self._local.depth == 0:
                ops, self._local.pending = self._local.pending, None
                self._apply(ops)

    def sync(self, timeout: float = 2.0) -> bool:
        """Waits until everything sent so far is applied and on screen; False if stopped."""
        if not self.running:
            return False
        try:
            return self._session.sync(timeout)
        except RuntimeError:
            return False
