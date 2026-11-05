"""Unit: ``kohakutui/ui/components.py`` specs become the create ops the engine reads."""

import itertools

from kohakutui import Cell, Markdown, Progress, Stack, Text


def ids(start: int = 100):
    return itertools.count(start).__next__


def test_props_leave_out_unset_values_and_children():
    assert Text("hi").props() == {"value": "hi"}
    assert Progress(total=5).props() == {"value": 0, "total": 5, "label": ""}
    assert "children" not in Cell(children=[Text("x")]).props()


def test_containers_emit_their_children_after_themselves():
    spec = Cell(role="assistant", children=[Text("a"), Stack(children=[Markdown("b")])])
    ops = spec.to_ops(ids(), parent=1, index=2)
    assert [(op["id"], op["kind"], op["parent"]) for op in ops] == [
        (100, "cell", 1),
        (101, "text", 100),
        (102, "stack", 100),
        (103, "markdown", 102),
    ]
    assert ops[0]["index"] == 2 and ops[1]["index"] is None
    assert ops[0]["props"] == {"role": "assistant"}


def test_an_explicit_id_is_used_for_the_root_only():
    ops = Stack(children=[Text("a")]).to_ops(ids(), parent=2, node_id=55)
    assert [op["id"] for op in ops] == [55, 100]
