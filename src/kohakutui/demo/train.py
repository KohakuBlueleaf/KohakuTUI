"""Training demo: progress, a loss sparkline, stats and a captured log."""

import math
import time
import random

from kohakutui.app import App
from kohakutui.ui import KV, Cell, Log, Progress, Sparkline, Text


def train(steps: int = 2000) -> None:
    """A fake loop of ``steps`` steps; every 500th loss is NaN to show the gap marker."""
    with App() as app:
        intro = Cell(
            role="system", title="training demo", children=[Text("ctrl+c stops")]
        )
        app.transcript.add(intro).seal()
        progress = app.live.add(Progress(total=steps, label="epoch 1", unit="step"))
        loss_line = app.live.add(Sparkline(label="loss", digits=4))
        stats = app.live.add(KV(fields={"lr": "3e-4", "step": 0}))
        log = app.live.add(Log(title="stdout", height=5))
        app.capture_stdio(target=log)

        loss = 2.5
        for step in range(1, steps + 1):
            loss = max(0.05, loss * 0.998 + random.gauss(0, 0.02))
            shown = float("nan") if step % 500 == 0 else loss
            with app.batch():
                progress.append(step)
                loss_line.append(shown)
                if step % 25 == 0:
                    lr = 3e-4 * math.cos(step / 1300)
                    stats.update(fields={"lr": f"{lr:.2e}", "step": step})
            if step % 100 == 0:
                print(f"step {step}: loss {loss:.4f}")
            time.sleep(0.002)

        app.release_capture()
        app.print(f"done: final loss {loss:.4f}", role="system")
