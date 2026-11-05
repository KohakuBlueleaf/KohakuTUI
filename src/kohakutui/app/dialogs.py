"""Blocking and async dialogs of ``App`` over overlay components: choose, ask, confirm, card.

Each dialog registers its waiter before the overlay exists, so an answer can never be missed,
and removes the overlay when it returns. None means cancelled, timed out, stopped or no
terminal.
"""

import asyncio
import threading
from collections.abc import Callable
from typing import Any

from kohakutui.app.events import Event, Waiter
from kohakutui.ui.components import Card, Component, Confirm, Picker, Prompt
from kohakutui.ui.node import Node

OVERLAYS = 5

ANSWERS = {
    "picker": {"select", "close"},
    "prompt": {"submit", "close"},
    "confirm": {"submit", "close"},
    "card": {"select", "close"},
}


def answer_of(ev: Event | None) -> Any:
    if ev is None or ev.kind == "close":
        return None
    return ev.value


class DialogsMixin:
    """Overlay dialogs, sync (from any thread but the pump) and async."""

    def _open(self, spec: Component, waiter: Waiter) -> int:
        node_id = self.new_id()
        self.dispatcher.register(node_id, waiter)
        self.send(spec.to_ops(self.new_id, OVERLAYS, None, node_id))
        return node_id

    def _close(self, node_id: int) -> None:
        self.dispatcher.unregister(node_id)
        self.send([{"op": "remove", "id": node_id}])

    def run_dialog(
        self,
        spec: Component,
        timeout: float | None = None,
        on_open: Callable[[Node], Any] | None = None,
        event: bool = False,
    ) -> Any:
        """Shows ``spec`` as an overlay and blocks until it is answered.

        ``on_open`` gets the dialog's node once it exists, so a caller can keep updating a
        live dialog (a card following a task's output) while it waits. With ``event=True`` it
        returns the answering ``Event`` itself (a ``close`` keeps its reason, such as
        ``"back"``), None when cancelled by a stop or a timeout.
        """
        if threading.current_thread() is self.pump_thread:
            raise RuntimeError(
                "dialogs cannot block the event pump; use the async form or a thread"
            )
        if not self.running:
            return None
        waiter = Waiter(ANSWERS[spec.kind])
        node_id = self._open(spec, waiter)
        if on_open is not None:
            on_open(Node(self, node_id, spec.kind))
        try:
            ev = waiter.wait(timeout)
            return ev if event else answer_of(ev)
        finally:
            self._close(node_id)

    async def run_dialog_async(
        self, spec: Component, timeout: float | None = None
    ) -> Any:
        """Shows ``spec`` as an overlay and awaits the answer."""
        if not self.running:
            return None
        waiter = Waiter(ANSWERS[spec.kind], asyncio.get_running_loop())
        node_id = self._open(spec, waiter)
        try:
            return answer_of(await waiter.wait_async(timeout))
        finally:
            self._close(node_id)

    def choose(
        self,
        title: str,
        options: list[Any],
        *,
        placeholder: str | None = None,
        timeout=None,
    ) -> Any:
        """Shows a filterable list; returns the chosen item's value."""
        spec = Picker(items=list(options), title=title, placeholder=placeholder)
        return self.run_dialog(spec, timeout)

    def ask(
        self,
        question: str,
        *,
        placeholder: str | None = None,
        multiline: bool = False,
        value: str | None = None,
        timeout=None,
    ) -> str | None:
        """Asks for text and returns it."""
        spec = Prompt(
            question=question, placeholder=placeholder, multiline=multiline, value=value
        )
        return self.run_dialog(spec, timeout)

    def confirm(
        self, question: str, *, default: bool = False, timeout=None
    ) -> bool | None:
        """Asks yes or no."""
        return self.run_dialog(Confirm(question=question, default=default), timeout)

    def card(
        self, title: str, body: str = "", actions: list[Any] = ("OK",), *, timeout=None
    ) -> Any:
        """Shows a card with action buttons; returns the chosen action's id."""
        return self.run_dialog(
            Card(title=title, body=body, actions=list(actions)), timeout
        )

    async def achoose(
        self, title: str, options: list[Any], *, placeholder=None, timeout=None
    ):
        spec = Picker(items=list(options), title=title, placeholder=placeholder)
        return await self.run_dialog_async(spec, timeout)

    async def aask(
        self,
        question: str,
        *,
        placeholder=None,
        multiline=False,
        value=None,
        timeout=None,
    ):
        spec = Prompt(
            question=question, placeholder=placeholder, multiline=multiline, value=value
        )
        return await self.run_dialog_async(spec, timeout)

    async def aconfirm(self, question: str, *, default: bool = False, timeout=None):
        return await self.run_dialog_async(
            Confirm(question=question, default=default), timeout
        )

    async def acard(self, title: str, body: str = "", actions=("OK",), *, timeout=None):
        spec = Card(title=title, body=body, actions=list(actions))
        return await self.run_dialog_async(spec, timeout)
