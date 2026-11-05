"""The widget contract: a Python class that owns semantic state and drives primitives.

A widget builds its primitives once (``build``), keeps the meaningful state itself (a tool's
status, a model choice) and changes the primitives through node handles. Events from every node
it created arrive at ``handle``; keystroke-level state stays in the engine.
"""

from typing import TYPE_CHECKING

from kohakutui.ui.components import Component
from kohakutui.ui.node import Node

if TYPE_CHECKING:
    from kohakutui.app.events import Event


class Widget:
    """Base for Python-side components; subclass and implement ``build``."""

    root: Node | None = None

    def build(self) -> Component:
        """The primitive tree this widget starts as."""
        raise NotImplementedError

    def handle(self, ev: "Event") -> None:
        """Receives events (submit, select, change, close) from the widget's own nodes."""

    def mount(self, parent: Node, index: int | None = None) -> Node:
        """Creates the widget under ``parent``; returns its root node."""
        self.root = parent.add(self.build(), index=index, owner=self)
        return self.root

    def unmount(self) -> None:
        """Removes the widget's nodes."""
        if self.root is not None:
            self.root.app.disown_widget(self)
            self.root.remove()
            self.root = None
