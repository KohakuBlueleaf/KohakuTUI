"""Background tasks, Claude Code style: a status line, a list view and a live detail view.

Starting a task writes one sealed line to the transcript and nothing more; while any task
runs, a status line sits under the input. ↓ from the input focuses it and enter opens the list
(ctrl+b does too); enter on a task opens a card following its output, with Back and Kill.
← goes back one level (detail → list → status line), esc closes everything.
"""

import time
import threading
from collections.abc import Iterable

from kohakutui.app import App, Event
from kohakutui.ui import Button, Card, Cell, Node, Picker, Text

TAIL_LINES = 14
KEEP_LINES = 500


class Task:
    """One background command: its output and whether it still runs."""

    def __init__(self, command: str):
        self.command = command
        self.started = time.monotonic()
        self.state = "running"
        self.total = 0
        self._lines: list[str] = []
        self._lock = threading.Lock()

    @property
    def running(self) -> bool:
        return self.state == "running"

    def write(self, line: str) -> None:
        with self._lock:
            self._lines.append(line)
            self.total += 1
            del self._lines[:-KEEP_LINES]

    def tail(self, n: int) -> list[str]:
        with self._lock:
            return self._lines[-n:]

    def elapsed(self) -> str:
        s = int(time.monotonic() - self.started)
        return f"{s // 60}m{s % 60:02d}s" if s >= 60 else f"{s}s"


class BackgroundTasks:
    """Owns the tasks, the status line in ``app.bottom`` and the ctrl+b browser.

    ``status_index`` places the status line among the bottom region's children (under the
    input, above the footer); ``speed`` scales the scripted output's pauses.
    """

    def __init__(self, app: App, status_index: int | None = None, speed: float = 1.0):
        self.app = app
        self.status_index = status_index
        self.speed = speed
        self.tasks: list[Task] = []
        self.status: Node | None = None
        self._status_lock = threading.Lock()
        self._browsing = threading.Lock()
        app.bind("ctrl+b", "tasks")
        app.on("action")(self._on_action)
        app.on("select")(self._on_select)

    def _on_action(self, ev: Event) -> None:
        if ev.name == "tasks":
            self.open()

    def _on_select(self, ev: Event) -> None:
        status = self.status
        if status is not None and ev.id == status.id:
            self.open()

    def running(self) -> list[Task]:
        return [t for t in self.tasks if t.running]

    def start(self, command: str, script: Iterable[tuple[float, str]]) -> Task:
        """Runs ``script`` (pause, output line) pairs as the task's output in a thread."""
        task = Task(command)
        self.tasks.append(task)
        note = Text("running in the background · ctrl+b to view", tone="muted")
        cell = Cell(role="tool", icon="⏵", title=command, children=[note])
        self.app.transcript.add(cell).seal()
        threading.Thread(target=self._run, args=(task, script), daemon=True).start()
        self.refresh()
        return task

    def _run(self, task: Task, script: Iterable[tuple[float, str]]) -> None:
        for pause, line in script:
            time.sleep(max(0.02, pause * self.speed))
            if not task.running:
                return
            task.write(line)
        if task.running:
            task.state = "done"
            self.app.notify(f"{task.command} finished", level="success")
            self.refresh()

    def refresh(self) -> None:
        """Shows, updates or removes the status line to match the running tasks."""
        n = len(self.running())
        label = f"⏵ {n} background task{'' if n == 1 else 's'}"
        with self._status_lock:
            if n and self.status is None:
                line = Button(
                    label=label, hint="↓ to view", focused_hint="enter to view"
                )
                self.status = self.app.bottom.add(line, index=self.status_index)
            elif n:
                self.status.update(label=label)
            elif self.status is not None:
                self.status.remove()
                self.status = None

    def kill(self, task: Task) -> None:
        if task.running:
            task.state = "killed"
            self.app.print(f"killed background task: {task.command}", role="system")
            self.refresh()

    def open(self) -> None:
        """Opens the browser on its own thread (dialogs block); one at a time."""
        if self._browsing.acquire(blocking=False):
            threading.Thread(target=self._browse_locked, daemon=True).start()

    def _browse_locked(self) -> None:
        try:
            self.browse()
        finally:
            self._browsing.release()

    def browse(self) -> None:
        """The list, then the detail of the chosen task; ← steps back, esc closes all."""
        if not self.tasks:
            self.app.notify("no background tasks")
            return
        while True:
            listing = Picker(title="Background tasks", items=self.items(), back=True)
            ev = self.app.run_dialog(listing, event=True)
            if ev is None or ev.kind == "close":
                return
            if self.expand(self.tasks[ev.value]) == "close":
                return

    def items(self) -> list[dict]:
        out = []
        for i, t in enumerate(self.tasks):
            last = (t.tail(1) or [""])[0]
            detail = f"{t.state} · {t.elapsed()}" + (f" · {last}" if last else "")
            out.append({"label": t.command, "value": i, "detail": detail})
        return out

    def expand(self, task: Task) -> str:
        """The task's live output in a card; Kill stops it. Returns ``"close"`` when esc closed
        everything, ``"list"`` when the list should come back."""
        stop = threading.Event()

        def follow(node: Node) -> None:
            def loop() -> None:
                shown, could_kill = None, task.running
                while not stop.wait(0.25):
                    view = self.view(task)
                    if could_kill and not task.running:
                        view["actions"] = [{"id": "back", "label": "Back"}]
                        could_kill = False
                    if view != shown and not stop.is_set():
                        node.update(**view)
                        shown = {k: v for k, v in view.items() if k != "actions"}

            threading.Thread(target=loop, daemon=True).start()

        actions = [{"id": "back", "label": "Back"}]
        if task.running:
            actions.append({"id": "kill", "label": "Kill"})
        card = Card(actions=actions, back=True, **self.view(task))
        try:
            ev = self.app.run_dialog(card, on_open=follow, event=True)
        finally:
            stop.set()
        if ev is None or (ev.kind == "close" and ev.data.get("reason") != "back"):
            return "close"
        if ev.kind == "select" and ev.value == "kill":
            self.kill(task)
        return "list"

    def view(self, task: Task) -> dict[str, str]:
        tail = task.tail(TAIL_LINES) or ["(no output yet)"]
        title = f"{task.command} · {task.state} · {task.elapsed()} · {task.total} lines"
        return {"title": title, "body": "```\n" + "\n".join(tail) + "\n```"}
