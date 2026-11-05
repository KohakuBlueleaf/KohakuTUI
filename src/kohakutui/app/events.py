"""Engine events and their delivery: dialog waiters, owners, callbacks and one stream.

Every event is delivered in this order: a dialog waiter on its node consumes it; otherwise its
owning widget handles it, then every matching callback observes it, then it goes to the one
open stream (``App.events`` or ``App.aevents``).
"""

import queue
import asyncio
import threading
import traceback
from collections.abc import Callable
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from kohakutui.app.app import App

STREAM_LIMIT = 10_000


class Event:
    """One event from the engine: ``kind`` plus its fields (``id``, ``value``, ...)."""

    __slots__ = ("kind", "data")

    def __init__(self, kind: str, data: dict[str, Any]):
        self.kind = kind
        self.data = data

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Event":
        """Builds an event from the engine's ``{"ev": kind, ...}`` dict."""
        data = dict(d)
        return cls(data.pop("ev", ""), data)

    def __getattr__(self, name: str) -> Any:
        try:
            return self.data[name]
        except KeyError:
            raise AttributeError(name) from None

    @property
    def id(self) -> int | None:
        return self.data.get("id")

    @property
    def value(self) -> Any:
        return self.data.get("value")

    def __repr__(self) -> str:
        fields = ", ".join(f"{k}={v!r}" for k, v in self.data.items())
        return f"Event({self.kind}{', ' if fields else ''}{fields})"


class Waiter:
    """A one-shot wait for an event of ``kinds`` on one node, from a thread or a coroutine."""

    def __init__(self, kinds: set[str], loop: asyncio.AbstractEventLoop | None = None):
        self.kinds = kinds
        self.event: Event | None = None
        self._done = threading.Event()
        self._loop = loop
        self._future = loop.create_future() if loop is not None else None

    def resolve(self, ev: Event) -> None:
        """Delivers the event; called from the pump thread."""
        self.event = ev
        self._done.set()
        if self._loop is not None and self._future is not None:
            self._loop.call_soon_threadsafe(_settle, self._future, ev)

    def cancel(self) -> None:
        """Wakes the waiter with no event (the app stopped)."""
        self._done.set()
        if self._loop is not None and self._future is not None:
            self._loop.call_soon_threadsafe(_settle, self._future, None)

    def wait(self, timeout: float | None = None) -> Event | None:
        """Blocks until resolved or ``timeout``; short slices keep ctrl+c responsive."""
        remaining = timeout
        while not self._done.wait(0.1 if remaining is None else min(0.1, remaining)):
            if remaining is not None:
                remaining -= 0.1
                if remaining <= 0:
                    return None
        return self.event

    async def wait_async(self, timeout: float | None = None) -> Event | None:
        """Awaits the event; None on timeout."""
        try:
            return await asyncio.wait_for(asyncio.shield(self._future), timeout)
        except asyncio.TimeoutError:
            return None


def _settle(future: asyncio.Future, ev: Event | None) -> None:
    if not future.done():
        future.set_result(ev)


class Dispatcher:
    """Routes events from the pump thread to waiters, owners, callbacks and the stream."""

    def __init__(self, app: "App"):
        self.app = app
        self._lock = threading.Lock()
        self._waiters: dict[int, Waiter] = {}
        self._callbacks: list[tuple[str, int | None, Callable[[Event], Any]]] = []
        self._sync: queue.Queue[Event] = queue.Queue()
        self._mode: str | None = None
        self._async_queue: asyncio.Queue | None = None
        self._loop: asyncio.AbstractEventLoop | None = None
        self._closed = False

    def register(self, node_id: int, waiter: Waiter) -> Waiter:
        """Starts waiting on ``node_id``; register before the node is created. After
        ``close`` the waiter is cancelled at once."""
        with self._lock:
            closed = self._closed
            if not closed:
                self._waiters[node_id] = waiter
        if closed:
            waiter.cancel()
        return waiter

    def unregister(self, node_id: int) -> None:
        with self._lock:
            self._waiters.pop(node_id, None)

    def on(self, kind: str, node_id: int | None, fn: Callable[[Event], Any]) -> None:
        with self._lock:
            self._callbacks.append((kind, node_id, fn))

    def off(self, fn: Callable[[Event], Any]) -> None:
        with self._lock:
            self._callbacks = [c for c in self._callbacks if c[2] is not fn]

    def dispatch(self, ev: Event) -> None:
        """Delivers one event; an exception in user code is reported, never raised."""
        with self._lock:
            waiter = self._waiters.get(ev.id) if ev.id is not None else None
            if waiter is not None and ev.kind in waiter.kinds:
                self._waiters.pop(ev.id, None)
            else:
                waiter = None
            callbacks = [
                fn
                for kind, nid, fn in self._callbacks
                if kind in (ev.kind, "*") and nid in (None, ev.id)
            ]
        if waiter is not None:
            waiter.resolve(ev)
            return

        owner = self.app.owner_of(ev.id)
        if owner is not None:
            self._guard(owner.handle, ev)
        for fn in callbacks:
            self._guard(fn, ev)
        self._stream(ev)

    def _guard(self, fn: Callable[[Event], Any], ev: Event) -> None:
        try:
            fn(ev)
        except Exception:
            self.app.print(traceback.format_exc().rstrip(), role="error")

    def _stream(self, ev: Event) -> None:
        with self._lock:
            loop, aq = self._loop, self._async_queue
        if loop is not None and aq is not None:
            try:
                loop.call_soon_threadsafe(aq.put_nowait, ev)
                return
            except RuntimeError:
                with self._lock:
                    self._loop = self._async_queue = None
        if self._sync.qsize() >= STREAM_LIMIT:
            try:
                self._sync.get_nowait()
            except queue.Empty:
                pass
        self._sync.put(ev)

    def close(self) -> None:
        """Wakes every waiting dialog with no answer; later waiters are cancelled at once."""
        with self._lock:
            self._closed = True
            waiters = list(self._waiters.values())
            self._waiters.clear()
        for w in waiters:
            w.cancel()

    def open_sync(self) -> "queue.Queue[Event]":
        """The sync stream; it cannot be opened alongside the async one."""
        with self._lock:
            if self._mode == "async":
                raise RuntimeError("events() and aevents() cannot both be used")
            self._mode = "sync"
        return self._sync

    def open_async(self, loop: asyncio.AbstractEventLoop) -> asyncio.Queue:
        """The async stream on ``loop``, holding every event queued so far."""
        with self._lock:
            if self._mode == "sync":
                raise RuntimeError("events() and aevents() cannot both be used")
            if self._async_queue is None:
                self._mode = "async"
                self._async_queue = asyncio.Queue()
                self._loop = loop
                while True:
                    try:
                        self._async_queue.put_nowait(self._sync.get_nowait())
                    except queue.Empty:
                        break
            return self._async_queue
