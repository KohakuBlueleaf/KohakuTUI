"""Agent demo: a scripted coding-agent turn in the style of Codex / Claude Code / KT's CLI.

Streaming reasoning, collapsed tool calls (ctrl+o or a click in full screen opens them), a
sub-agent with nested tools, a diff, an approval card, background tasks (ctrl+b lists them,
choosing one follows its output), a plan panel (full screen, ctrl+t), a boxed input and a
footer with token counts. Type anything to replay the turn.
"""

import time
import random
import threading

from kohakutui.app import App, Event
from kohakutui.demo.parts import DiffView, PlanPanel, ToolCall
from kohakutui.demo.tasks import BackgroundTasks
from kohakutui.ui import KV, Cell, Composer, Footer, Markdown, Status, Text

MODEL = "gpt-5.5 · high"
CWD = "~/code/kohaku-client"

COMMANDS = [
    {"name": "model", "desc": "switch the model"},
    {"name": "approvals", "desc": "change the approval policy"},
    {"name": "tasks", "desc": "background tasks (ctrl+b)"},
    {"name": "fullscreen", "desc": "same as ctrl+t"},
    {"name": "quit", "desc": "leave the demo"},
]

PLAN = [
    "Read the HTTP client",
    "Find every caller",
    "Add retry with backoff",
    "Run the tests",
]

THINKING = """I'll start by reading the HTTP client to see how requests are made, then find \
its callers so the retry doesn't change behaviour anywhere it shouldn't.
"""

CLIENT_SRC = """import requests

class Client:
    def __init__(self, base: str, timeout: float = 10.0):
        self.base, self.timeout = base, timeout

    def fetch(self, path: str) -> dict:
        r = requests.get(self.base + path, timeout=self.timeout)
        r.raise_for_status()
        return r.json()"""

DIFF = """@@ -6,7 +6,15 @@ class Client:
     def fetch(self, path: str) -> dict:
-        r = requests.get(self.base + path, timeout=self.timeout)
-        r.raise_for_status()
-        return r.json()
+        for attempt in range(self.retries + 1):
+            try:
+                r = requests.get(self.base + path, timeout=self.timeout)
+                r.raise_for_status()
+                return r.json()
+            except requests.RequestException:
+                if attempt == self.retries:
+                    raise
+                time.sleep(self.backoff * 2**attempt)"""

SUMMARY = """Done. `Client.fetch` now retries failed requests with exponential backoff.

| change | detail |
|--------|--------|
| `retries` | new constructor arg, default **3** |
| `backoff` | base delay in seconds, default **0.5** |
| callers | none needed changes (3 found) |

All **12 tests pass**. The dev server is still running in the background.
"""


class AgentDemo:
    """Owns the app and plays scripted turns; ``speed`` scales every pause."""

    def __init__(self, app: App, speed: float = 1.0, interactive: bool = True):
        self.app = app
        self.speed = speed
        self.interactive = interactive
        self.tokens_in = 0
        self.tokens_out = 0
        self.model = MODEL
        self.busy = threading.Lock()
        self.queued: list[str] = []
        self.footer = None
        self.plan: PlanPanel | None = None
        self.tasks: BackgroundTasks | None = None

    def pause(self, seconds: float) -> None:
        time.sleep(seconds * self.speed)

    def setup(self) -> None:
        app = self.app
        banner = Cell(
            role="system",
            title="KohakuTerrarium  ·  kohakutui demo",
            children=[
                KV(fields={"model": MODEL, "cwd": CWD, "approvals": "on request"})
            ],
        )
        app.transcript.add(banner).seal()
        self.composer = app.bottom.add(
            Composer(
                placeholder="ask anything · /model · ctrl+o expand tools",
                commands=COMMANDS,
                boxed=True,
            )
        )
        self.footer = app.bottom.add(Footer())
        self.tasks = BackgroundTasks(app, status_index=1, speed=self.speed)
        self.plan = PlanPanel("Plan", PLAN)
        self.plan.mount(app.panels)
        app.panels.add(
            KV(fields={"session": "demo-7f3a", "tools": "6 enabled", "mcp": "2"})
        )
        self.update_footer()

    def update_footer(self, add_in: int = 0, add_out: int = 0) -> None:
        self.tokens_in += add_in
        self.tokens_out += add_out
        ctx = min(99, (self.tokens_in + self.tokens_out) * 100 // 200_000)
        right = f"ctx {ctx}% · in {self.tokens_in / 1000:.1f}k out {self.tokens_out / 1000:.1f}k"
        self.footer.update(left=f"{self.model} · {CWD}", right=right)

    def stream(self, text: str, chunk: int = 6) -> None:
        """An assistant message streamed in small chunks, like tokens."""
        cell = self.app.transcript.add(Cell(role="assistant", stream_flush=True))
        body = cell.add(Markdown(streaming=True))
        for i in range(0, len(text), chunk):
            body.append(text[i : i + chunk])
            self.pause(0.012)
        cell.seal()
        self.update_footer(add_out=len(text) // 4)

    def think(self, label: str, seconds: float) -> None:
        status = self.app.live.add(Status(label=label))
        self.pause(seconds)
        status.remove()

    def approve(self, command: str) -> bool:
        if not self.interactive:
            return True
        choice = self.app.card(
            "Run this command?",
            f"The agent wants to run:\n\n```\n{command}\n```",
            [
                {"id": "once", "label": "Yes, once"},
                {"id": "always", "label": "Always for this session"},
                {"id": "deny", "label": "No"},
            ],
        )
        return choice in ("once", "always")

    def turn(self, prompt: str) -> None:
        app, plan = self.app, self.plan
        app.print(prompt, role="user")
        self.update_footer(add_in=1800 + len(prompt))

        self.think("thinking", 1.0)
        self.stream(THINKING)

        plan.mark(0, "doing")
        read = ToolCall("read_file", '{"path": "src/client.py"}', "json")
        read.mount(app.transcript)
        self.pause(0.5)
        read.preview(CLIENT_SRC)
        read.done("10 lines")
        plan.mark(0, "done")

        plan.mark(1, "doing")
        sub = ToolCall("subagent · explore", "find every caller of Client.fetch")
        sub.mount(app.transcript)
        for name, args, result in [
            ("grep", "fetch\\(", "3 matches in 2 files"),
            ("read_file", "src/sync.py", "41 lines"),
            ("read_file", "src/cli.py", "88 lines"),
        ]:
            call = sub.child(name, args)
            self.pause(0.4)
            call.done(result)
        sub.root.add(
            Text("3 callers, none depend on single-attempt behaviour", tone="muted")
        )
        sub.done("3 callers found · 3 tool calls")
        plan.mark(1, "done")
        self.update_footer(add_in=4200, add_out=600)

        plan.mark(2, "doing")
        self.think("editing", 0.6)
        edit = ToolCall(
            "apply_patch", '{"path": "src/client.py"}', "json", collapsed=False
        )
        edit.mount(app.transcript)
        DiffView("src/client.py", DIFF).mount(edit.root)
        edit.done("+9 -3")
        plan.mark(2, "done")

        plan.mark(3, "doing")
        command = "pytest -q tests/"
        if self.approve(command):
            self.run_tests(command)
            plan.mark(3, "done")
        else:
            app.print("command denied by the user", role="error")

        self.start_background()
        self.stream(SUMMARY)
        self.update_footer(add_in=900, add_out=320)
        app.notify("turn finished", level="success")

    def run_tests(self, command: str) -> None:
        shell = ToolCall("shell", command, "bash")
        shell.mount(self.app.transcript)
        for i in range(1, 13):
            dots = "." * i
            shell.output(f"\r{dots:<12} [{i * 100 // 12:>3}%]")
            self.pause(0.12)
        shell.output("\n12 passed in 1.84s\n")
        shell.done("12 passed")

    def start_background(self) -> None:
        running = {t.command for t in self.tasks.running()}
        if "npm run dev" not in running:
            self.tasks.start("npm run dev", dev_server())
        if "tail -f logs/app.log" not in running:
            self.tasks.start("tail -f logs/app.log", app_log())

    def pick_model(self) -> None:
        models = [
            {"label": "gpt-5.5 · high", "detail": "reasoning"},
            {"label": "claude-opus-5", "detail": "1M context"},
            {"label": "gemini-3-pro", "detail": "multimodal"},
            *(
                {"label": f"local/qwen-{n}b", "detail": "ollama"}
                for n in (8, 14, 32, 72)
            ),
        ]
        choice = self.app.choose("Model", models)
        if choice is not None:
            self.model = choice
            self.update_footer()
            self.app.notify(f"model: {choice}", level="success")

    def submitted(self, ev: Event) -> None:
        text = ev.value.strip()
        match text:
            case "/quit":
                self.quit.set()
            case "/model":
                threading.Thread(target=self.pick_model, daemon=True).start()
            case "/fullscreen":
                self.app.set_mode("fullscreen")
            case "/approvals":
                self.app.notify("approval policy: on request", level="info")
            case "/tasks":
                self.tasks.open()
            case _ if self.busy.locked():
                self.queued.append(text)
                self.app.notify(f"queued: {text}", level="info")
            case _:
                threading.Thread(
                    target=self.locked_turn, args=(text,), daemon=True
                ).start()

    def locked_turn(self, prompt: str) -> None:
        """Plays the turn, then every message queued while it ran."""
        with self.busy:
            self.turn(prompt)
            while self.queued:
                self.turn(self.queued.pop(0))

    def run(self) -> None:
        self.quit = threading.Event()
        self.setup()
        self.app.on("submit", self.composer)(self.submitted)
        self.locked_turn("Add retry with backoff to the HTTP client and run the tests")
        if self.interactive:
            self.quit.wait()


def dev_server():
    """A dev server's output: a banner, then a request log."""
    yield 0.3, "  VITE v6.2.0  ready in 312 ms"
    yield 0.05, "  ➜  Local:   http://localhost:5173/"
    yield 0.05, "  ➜  Network: use --host to expose"
    routes = ["/", "/api/items", "/api/items/42", "/assets/index.js", "/api/user"]
    for i in range(400):
        ms = random.randint(3, 180)
        yield 1.2, f"{time.strftime('%H:%M:%S')} GET {routes[i % len(routes)]} 200 {ms}ms"


def app_log():
    """A monitored log file: mostly info, sometimes a warning."""
    for i in range(400):
        level = "WARN" if i % 9 == 4 else "INFO"
        yield 0.8, f"{time.strftime('%H:%M:%S')} {level} worker-{i % 3} processed batch {i}"


def agent(speed: float = 1.0, interactive: bool = True, app: App | None = None) -> None:
    """Runs the agent demo until ``/quit`` (or after one turn when not interactive)."""
    random.seed(7)
    with app or App() as running:
        AgentDemo(running, speed, interactive).run()
