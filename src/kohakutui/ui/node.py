"""Node handles: the Python side of one node in the engine's document."""

from typing import TYPE_CHECKING, Any

from kohakutui.ui.components import Component

if TYPE_CHECKING:
    from kohakutui.app.app import App
    from kohakutui.ui.widget import Widget


class Node:
    """A handle on a created node; every method sends one op and returns at once."""

    __slots__ = ("app", "id", "kind")

    def __init__(self, app: "App", node_id: int, kind: str):
        self.app = app
        self.id = node_id
        self.kind = kind

    def __repr__(self) -> str:
        return f"Node({self.kind}#{self.id})"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Node) and other.id == self.id and other.app is self.app

    def __hash__(self) -> int:
        return hash(self.id)

    def add(
        self,
        spec: Component,
        *,
        index: int | None = None,
        id: int | None = None,
        owner: "Widget | None" = None,
    ) -> "Node":
        """Creates ``spec`` (and its children) under this node; returns the new node.

        ``owner`` routes events from every node created here to that widget.
        """
        ops = spec.to_ops(self.app.new_id, self.id, index, id)
        if owner is not None:
            self.app.own([op["id"] for op in ops], owner)
        self.app.send(ops)
        return Node(self.app, ops[0]["id"], spec.kind)

    def update(self, **props: Any) -> "Node":
        """Changes props; a prop set to None returns to its default. Every kind takes
        ``hidden=True``: not drawn, focused or given keys, its state kept."""
        self.app.send([{"op": "update", "id": self.id, "props": props}])
        return self

    def append(self, data: Any) -> "Node":
        """Streams data in: text for text/markdown/code/log, numbers for progress/sparkline,
        rows for table."""
        self.app.send([{"op": "append", "id": self.id, "data": data}])
        return self

    def seal(self) -> None:
        """Freezes the node and its children; a sealed transcript cell goes to scrollback."""
        self.app.send([{"op": "seal", "id": self.id}])

    def remove(self) -> None:
        """Removes the node and its children."""
        self.app.disown(self.id)
        self.app.send([{"op": "remove", "id": self.id}])

    def move(self, parent: "Node", index: int | None = None) -> None:
        """Moves the node under ``parent``, e.g. a long-running tool cell into ``app.live``."""
        self.app.send(
            [{"op": "move", "id": self.id, "parent": parent.id, "index": index}]
        )

    def focus(self) -> None:
        """Sends keys to this node (it must be an input)."""
        self.app.send([{"op": "focus", "id": self.id}])
