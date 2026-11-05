"""Unit: ``kohakutui/app/events.py`` event access and waiter timing."""

import threading

import pytest

from kohakutui.app.events import Event, Waiter


def test_event_reads_kind_and_fields():
    ev = Event.from_dict({"ev": "select", "id": 7, "index": 1, "value": "b"})
    assert (ev.kind, ev.id, ev.index, ev.value) == ("select", 7, 1, "b")
    assert Event.from_dict({"ev": "stopped"}).id is None
    with pytest.raises(AttributeError):
        _ = ev.reason


def test_waiter_times_out_then_a_later_resolve_is_seen():
    w = Waiter({"submit"})
    assert w.wait(0.15) is None
    ev = Event("submit", {"id": 1, "value": "x"})
    threading.Timer(0.05, w.resolve, args=(ev,)).start()
    assert w.wait(2) is ev


def test_cancel_wakes_a_waiter_with_no_event():
    w = Waiter({"submit"})
    threading.Timer(0.05, w.cancel).start()
    assert w.wait(2) is None
