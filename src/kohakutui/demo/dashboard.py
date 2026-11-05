"""Dashboard demo: a full-screen training monitor.

The main pane holds progress and three charts (loss with its moving average, learning rate,
gradient norm); the right panel holds run info, throughput, the eval table, GPU stats and an
event log. ``q`` quits, ctrl+t switches to inline mode.
"""

import math
import time
import random
import threading

from kohakutui.app import App, Event
from kohakutui.ui import KV, Chart, Divider, Footer, Log, Progress, Stack, Table

RUN = {
    "run": "hdm-xl-v3 / stage2",
    "model": "1.3B DiT",
    "data": "danbooru-2026 (31M)",
    "batch": "4096 (8×512)",
    "precision": "bf16",
}
SECONDS_PER_STEP = 0.82


def dashboard(
    epochs: int = 3,
    steps: int = 400,
    speed: float = 1.0,
    hold: bool = True,
    app: App | None = None,
) -> None:
    """Runs the dashboard in full screen; with ``hold`` it stays up until ``q`` is pressed."""
    random.seed(3)
    with app or App(mode="fullscreen") as running:
        board = Dashboard(running, epochs, steps, speed, hold)
        try:
            board.run()
        except KeyboardInterrupt:
            running.print("interrupted", role="system")


class Dashboard:
    """A fake training run drawn as a dashboard; ``speed`` scales the step time."""

    def __init__(
        self,
        app: App,
        epochs: int = 3,
        steps: int = 400,
        speed: float = 1.0,
        hold: bool = True,
    ):
        self.app = app
        self.epochs = epochs
        self.steps = steps
        self.speed = speed
        self.hold = hold
        self.stop = threading.Event()

    def setup(self) -> None:
        app = self.app
        total = self.epochs * self.steps
        with app.batch():
            main = app.transcript.add(Stack(gap=1))
            self.total = main.add(Progress(total=total, label="total  ", unit="step"))
            self.epoch = main.add(
                Progress(total=self.steps, label="epoch 1", unit="step")
            )
            self.loss = main.add(Chart(label="loss", height=12, unit="step"))
            self.lr = main.add(
                Chart(label="learning rate", height=4, unit="step", digits=6)
            )
            self.grad = main.add(
                Chart(label="grad norm", height=4, unit="step", digits=3)
            )

            app.panels.add(Stack(title="Run", children=[KV(fields=RUN)]))
            app.panels.add(Divider(label="speed"))
            self.speed_kv = app.panels.add(KV(fields={"throughput": "-", "eta": "-"}))
            app.panels.add(Divider(label="eval"))
            self.evals = app.panels.add(Table(columns=["epoch", "loss", "fid", "clip"]))
            app.panels.add(Divider(label="gpu"))
            self.gpu = app.panels.add(KV(fields=self.gpu_fields(0)))
            app.panels.add(Divider(label="events"))
            self.events = app.panels.add(Log(height=6))

            footer = "q quit · ctrl+t inline · pgup/pgdn scroll"
            app.bottom.add(Footer(left=footer, right=RUN["run"]))

        app.bind("q", "quit")
        app.on("action")(self.on_action)

    def on_action(self, ev: Event) -> None:
        if ev.name == "quit":
            self.stop.set()

    def event(self, text: str) -> None:
        self.events.append(text + "\n")

    def gpu_fields(self, step: int) -> dict[str, str]:
        wave = math.sin(step / 37)
        return {
            f"gpu{i}": f"{92 + 6 * wave + random.uniform(-2, 2):3.0f}%  "
            f"{71.2 + i * 0.4:.1f}/80G  {64 + i + 3 * wave:.0f}°C"
            for i in range(4)
        }

    def run(self) -> None:
        self.setup()
        self.event("training started")
        loss, ema, step = 1.9, 1.9, 0
        total = self.epochs * self.steps

        for epoch in range(1, self.epochs + 1):
            self.epoch.update(label=f"epoch {epoch}", value=0)
            for i in range(1, self.steps + 1):
                if self.stop.is_set():
                    return
                step += 1
                loss = max(0.08, loss * 0.9985 + random.gauss(0, 0.012))
                ema = 0.97 * ema + 0.03 * loss
                grad = abs(random.gauss(0.9, 0.2)) * (3.2 if step % 157 == 0 else 1)
                lr = 3e-4 * 0.5 * (1 + math.cos(math.pi * step / total))
                nan = step % 333 == 0

                with self.app.batch():
                    self.total.append(step)
                    self.epoch.append(i)
                    self.loss.append(
                        {"loss": float("nan") if nan else loss, "ema": ema}
                    )
                    self.lr.append({"lr": lr})
                    self.grad.append({"grad": grad})
                    if step % 20 == 0:
                        self.update_speed(step, total)
                    if nan:
                        self.event(f"step {step}: NaN loss, batch skipped")
                    if grad > 2.0:
                        self.event(f"step {step}: grad norm spike {grad:.2f}")
                        self.app.notify(f"grad norm spike: {grad:.2f}", level="warn")
                time.sleep(0.004 * self.speed)

            fid = 14.0 / epoch + random.uniform(-0.3, 0.3)
            clip = 0.29 + 0.012 * epoch
            self.evals.append([epoch, f"{loss:.4f}", f"{fid:.2f}", f"{clip:.3f}"])
            self.event(f"epoch {epoch} done · FID {fid:.2f}")

        if self.hold:
            self.app.notify("training finished · press q", level="success", ttl=30)
            self.stop.wait()

    def update_speed(self, step: int, total: int) -> None:
        jitter = random.uniform(-120, 120)
        left = (total - step) * SECONDS_PER_STEP
        self.speed_kv.update(
            fields={
                "throughput": f"{4096 / SECONDS_PER_STEP + jitter:,.0f} img/s",
                "eta": f"{int(left // 3600)}h{int(left % 3600 // 60):02d}m",
            }
        )
        self.gpu.update(fields=self.gpu_fields(step))
