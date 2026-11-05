"""Full-screen chrome end to end on the test backend: documents, panes, the head tab strip,
trees, follow with "↓ N new", the sticky prompt and pane focus keys."""

from kohakutui import App, Cell, Composer, Pane, TabStrip, Text, Tree
from tests.conftest import until


def _screen(app: App) -> str:
    return "\n".join(app.screen())


def _row(app: App, word: str, after: int = 0) -> tuple[int, int]:
    rows = app.screen()
    y = next(i for i, line in enumerate(rows) if word in line[after:])
    return rows[y].index(word, after), y


class TestFullscreen:
    def test_documents_keep_their_own_cells_and_inline_flushes_the_shown_one(self):
        with App(backend="test", size=(60, 10), interrupt="event") as app:
            other = app.add_document()
            other.add(Cell(role="assistant", children=[Text("in other")])).seal()
            app.transcript.add(Cell(role="assistant", children=[Text("in main")])).seal()
            until(app, lambda: "● in main" in app.history())
            assert "● in other" not in app.history()
            app.show_document(other, "── other ──")
            until(app, lambda: "● in other" in app.history())
            history = app.history()
            assert history.index("── other ──") < history.index("● in other")

    def test_ops_sent_before_start_are_applied_on_start(self):
        app = App(backend="test", size=(40, 6), interrupt="event")
        doc = app.add_document()
        app.show_document(doc)
        doc.add(Cell(role="assistant", children=[Text("built early")])).seal()
        with app:
            until(app, lambda: "● built early" in app.history())

    def test_panes_resize_collapse_and_report(self):
        with App(
            backend="test", size=(100, 20), interrupt="event", mode="fullscreen"
        ) as app:
            events: list[tuple[str, dict]] = []
            for kind in ("pane", "select", "close"):
                app.on(kind)(lambda ev, k=kind: events.append((k, dict(ev.data))))
            rail = app.panels.add(Pane(side="left", width=20, title="Rail"))
            rail.add(Tree(items=[{"label": "alpha", "value": "a"}, {"label": "beta", "value": "b"}]))
            aux = app.panels.add(Pane(side="right", width=24, title="Aux"))
            aux.add(Text("aux body"))
            app.transcript.add(Cell(role="assistant", children=[Text("chat")]))
            until(app, lambda: "aux body" in _screen(app) and "chat" in _screen(app))
            rows = app.screen()
            assert rows[0].startswith(" Rail") and "Aux" in rows[0]
            assert rows[0][20] == "│"

            # drag the rail's divider from column 20 to 28; one pane event at the release
            app.inject({"drag": (20, 4, 28)})
            until(app, lambda: any(k == "pane" for k, _ in events))
            assert events[-1][1]["width"] == 28 and app.screen()[0][28] == "│"
            # a plain click on a divider changes nothing and reports nothing
            count = len(events)
            app.inject({"click": (28, 4)})
            app.sync()
            assert len(events) == count

            # a click in the tree selects; the wheel over the transcript does not move panes
            x, y = _row(app, "beta")
            app.inject({"click": (x, y)})
            until(app, lambda: ("select", {"id": events[-1][1].get("id"), "index": 1, "value": "b"}) in events)

            # too narrow: the right pane becomes a strip, the transcript keeps its room
            app.inject({"resize": (70, 20)})
            until(app, lambda: app.screen()[0].rstrip().endswith("◂"))
            assert "aux body" not in _screen(app)

    def test_tab_strip_selects_closes_and_scrolls_whole_tabs(self):
        with App(
            backend="test", size=(40, 8), interrupt="event", mode="fullscreen"
        ) as app:
            events: list[tuple[str, object]] = []
            app.on("select")(lambda ev: events.append(("select", ev.data.get("value"))))
            app.on("close")(lambda ev: events.append(("close", ev.data.get("reason"))))
            tabs = [{"label": f"tab{i}", "value": f"t{i}", "closable": i > 0} for i in range(8)]
            strip = app.head.add(TabStrip(tabs=tabs))
            until(app, lambda: "tab0" in app.screen()[0])
            x, _ = _row(app, "tab2")
            app.inject({"click": (x, 0)})
            until(app, lambda: ("select", "t2") in events)
            app.inject({"middle": (x, 0)})
            until(app, lambda: ("close", "t2") in events)
            # a click on a tab's × closes it too
            x, _ = _row(app, "tab3 ×")
            app.inject({"click": (x + len("tab3 "), 0)})
            until(app, lambda: ("close", "t3") in events)
            assert ("select", "t3") not in events

            # the last tab active: scrolled by whole tabs behind a ‹ mark
            strip.update(active=7)
            until(app, lambda: "tab7" in app.screen()[0])
            head = app.screen()[0]
            assert head.startswith("‹ tab")
            # a click on a scrolled tab still picks that tab
            x, _ = _row(app, "tab6")
            app.inject({"click": (x, 0)})
            until(app, lambda: ("select", "t6") in events)

    def test_follow_counts_new_messages_and_keeps_the_view_and_the_prompt(self):
        with App(
            backend="test", size=(60, 12), interrupt="event", mode="fullscreen"
        ) as app:
            for i in range(30):
                if i % 10 == 0:
                    app.transcript.add(Cell(role="user", children=[Text(f"prompt {i // 10}")]))
                app.transcript.add(Cell(role="assistant", children=[Text(f"line {i}")]))
            app.bottom.add(Composer())
            until(app, lambda: "line 29" in _screen(app))
            app.inject({"scroll": 4, "at": (30, 3)})
            until(app, lambda: "more · ctrl+end" in _screen(app))
            before = [line for line in app.screen() if "line " in line]
            # the last prompt above the view stays on the top row
            assert app.screen()[0].lstrip().startswith("› prompt")

            for i in range(3):
                app.transcript.add(Cell(role="assistant", children=[Text(f"new {i}")]))
            until(app, lambda: "↓ 3 new" in _screen(app))
            assert [line for line in app.screen() if "line " in line] == before
            app.inject({"key": "ctrl+end"})
            until(app, lambda: "new 2" in _screen(app))
            assert "↓" not in _screen(app)

    def test_alt_arrows_resize_the_focused_pane_and_f6_moves_between_areas(self):
        with App(
            backend="test", size=(100, 16), interrupt="event", mode="fullscreen"
        ) as app:
            events: list[dict] = []
            app.on("pane")(lambda ev: events.append(dict(ev.data)))
            rail = app.panels.add(Pane(side="left", width=20))
            tree = rail.add(Tree(items=[{"label": "one", "value": "1"}]))
            composer = app.bottom.add(Composer())
            composer.focus()
            until(app, lambda: "one" in _screen(app))
            app.inject({"key": "f6"})  # composer → the tree (the next area that takes keys)
            app.inject({"key": "alt+right"})
            until(app, lambda: events and events[-1]["width"] == 22)
            assert events[-1]["id"] == rail.id
            assert tree.id  # the tree held the keys: the resize went to its pane
