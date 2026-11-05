"""Chat demo: a fake assistant that streams Markdown token by token."""

import time
import random
import threading

from kohakutui.app import App, Event
from kohakutui.ui import Cell, Composer, Footer, Markdown, Status

REPLY = """Here is a **streamed** reply, rendered as Markdown while it arrives.

1. Finished lines go to the terminal's own scrollback
2. The open line stays in the viewport
   - nested items work, and so does `inline code`

```python
def hello(name: str) -> str:
    return f"hello {name}"
```

| mode | where finished output goes |
|------|----------------------------|
| inline | real scrollback |
| fullscreen | a scrollable pane |

> Try `ctrl+t` for full screen, `/model` for a picker, `/confirm` for a dialog.

中文與 English 混排的段落也會正確換行，不會把寬字元切成兩半。
"""

COMMANDS = [
    {"name": "model", "desc": "pick a model"},
    {"name": "confirm", "desc": "ask a yes/no question"},
    {"name": "quit", "desc": "leave the demo"},
]


def chat() -> None:
    """Runs until ``/quit`` or ctrl+c."""
    models = [
        {"label": f"model-{i:03d}", "detail": f"{random.randint(8, 400)}B"}
        for i in range(200)
    ]
    with App() as app:
        app.bottom.add(
            Composer(placeholder="say something · /model · ctrl+t", commands=COMMANDS)
        )
        footer = app.bottom.add(Footer(left="kohakutui demo", right="model-000"))
        done = threading.Event()

        def reply(text: str) -> None:
            app.print(text, role="user")
            status = app.live.add(Status(label="thinking"))
            time.sleep(0.6)
            status.remove()

            cell = app.transcript.add(Cell(role="assistant", stream_flush=True))
            body = cell.add(Markdown(streaming=True))
            for token in REPLY.replace("\n", "\n ").split(" "):
                body.append(token + ("" if token.endswith("\n") else " "))
                time.sleep(0.01)
            cell.seal()

        def pick() -> None:
            model = app.choose("Model", models)
            if model is not None:
                footer.update(right=model)
                app.notify(f"switched to {model}", level="success")

        def ask() -> None:
            answer = app.confirm("Ship it?", default=True)
            app.print(f"answer: {answer}", role="system")

        @app.on("submit")
        def _(ev: Event) -> None:
            match ev.value.strip():
                case "/quit":
                    done.set()
                case "/model":
                    threading.Thread(target=pick, daemon=True).start()
                case "/confirm":
                    threading.Thread(target=ask, daemon=True).start()
                case text:
                    threading.Thread(target=reply, args=(text,), daemon=True).start()

        done.wait()
