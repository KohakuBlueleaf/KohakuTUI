"""Train demo: an inline training loop that never grows the scrollback while it runs.

Everything live sits in the fixed-height live region: the loss curve, progress, stats, the
last few epochs and a bounded event log (captured ``print`` lands there too). When the run
ends, the curve and the full epoch table move to scrollback once, with a one-line summary.
"""

import math
import time
import random

from kohakutui.app import App
from kohakutui.ui import KV, Cell, Chart, Log, Progress, Table


def train(
    epochs: int = 3, steps: int = 500, speed: float = 1.0, app: App | None = None
) -> None:
    """Runs a fake training loop of ``epochs`` × ``steps``; ``speed`` scales the step time."""
    random.seed(1)
    with app or App() as running:
        TrainDemo(running, epochs, steps, speed).run()


class TrainDemo:
    """The loop and the nodes it updates."""

    def __init__(self, app: App, epochs: int, steps: int, speed: float):
        self.app = app
        self.epochs = epochs
        self.steps = steps
        self.speed = speed
        self.nan_batches = 0

    def setup(self) -> None:
        app = self.app
        run = KV(
            fields={"model": "tiny-dit (demo)", "batch": "256", "lr": "3e-4 cosine"}
        )
        app.transcript.add(
            Cell(role="system", title="training started", children=[run])
        ).seal()

        total = self.epochs * self.steps
        self.chart = app.live.add(Chart(label="loss", height=10, unit="step"))
        self.progress = app.live.add(Progress(total=total, label="train", unit="step"))
        self.stats = app.live.add(KV(fields={"epoch": "1", "lr": "-", "loss": "-"}))
        self.epoch_table = app.live.add(
            Table(columns=["epoch", "loss", "ema", "lr"], max_rows=3)
        )
        self.events = app.live.add(Log(title="events", height=3))
        app.capture_stdio(target=self.events)

    def run(self) -> None:
        self.setup()
        loss, ema, step = 2.4, 2.4, 0
        total = self.epochs * self.steps

        for epoch in range(1, self.epochs + 1):
            for _ in range(self.steps):
                step += 1
                loss = max(0.05, loss * 0.997 + random.gauss(0, 0.03))
                ema = 0.97 * ema + 0.03 * loss
                lr = 3e-4 * 0.5 * (1 + math.cos(math.pi * step / total))
                nan = step % 400 == 0
                if nan:
                    self.nan_batches += 1
                    print(f"step {step}: loss is NaN, batch skipped")

                with self.app.batch():
                    self.chart.append(
                        {"loss": float("nan") if nan else loss, "ema": ema}
                    )
                    self.progress.append(step)
                    if step % 10 == 0:
                        fields = {"epoch": f"{epoch}/{self.epochs}", "lr": f"{lr:.2e}"}
                        fields["loss"] = f"{loss:.4f}  (ema {ema:.4f})"
                        self.stats.update(fields=fields)
                time.sleep(0.004 * self.speed)

            self.epoch_table.append([epoch, f"{loss:.4f}", f"{ema:.4f}", f"{lr:.2e}"])

        self.finish(step, loss)

    def finish(self, step: int, loss: float) -> None:
        """Writes the run's record to scrollback once: the curve, every epoch, a summary."""
        app = self.app
        app.release_capture()
        with app.batch():
            for node in (self.progress, self.stats, self.events):
                node.remove()
            self.epoch_table.update(max_rows=None)
            for node in (self.chart, self.epoch_table):
                node.move(app.transcript)
                node.seal()
            batches = "batch" if self.nan_batches == 1 else "batches"
            app.print(
                f"done in {step} steps · final loss {loss:.4f} · "
                f"{self.nan_batches} NaN {batches} skipped",
                role="system",
            )
