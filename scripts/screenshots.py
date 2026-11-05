"""Teaser images for the README, drawn from what the engine really put on screen.

Every scene runs headless on the emulated terminal (``backend="test"``), exactly as the tests
do, and ``App.screen_cells()`` gives the visible cells with their colours and attributes. They
are painted cell by cell, so alignment and box drawing are exactly what a terminal shows.

    uv run --with pillow --with fonttools python scripts/screenshots.py [--out docs/images]

Fonts: a monospace font first (Cascadia Mono on Windows, DejaVu Sans Mono elsewhere) and a
symbol font for the glyphs it lacks; pass ``--font`` / ``--symbols`` to override.
"""

import argparse
import threading
import time
import unicodedata
from pathlib import Path

from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

from kohakutui import App
from kohakutui.demo.agent import AgentDemo
from kohakutui.demo.dashboard import Dashboard
from kohakutui.demo.train import TrainDemo
from kohakutui.ui import Cell, Composer, Footer, Markdown, Picker, Text

THEME = "kohaku-dark"
# scenes shown side by side in the README share one terminal size, so their images match
PAIR = (100, 26)
BACKGROUND = "#15121F"
FOREGROUND = "#E4DFEE"
WINDOW = "#0C0A12"
FONT_CANDIDATES = [
    "C:/Windows/Fonts/CascadiaMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/System/Library/Fonts/Menlo.ttc",
]
SYMBOL_CANDIDATES = [
    "C:/Windows/Fonts/seguisym.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/System/Library/Fonts/Apple Symbols.ttf",
]
CJK_CANDIDATES = [
    "C:/Windows/Fonts/NotoSansTC-VF.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
]


# -- scenes -------------------------------------------------------------------------------------


def until(app: App, predicate, timeout: float = 10.0) -> None:
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() > deadline:
            raise TimeoutError("\n".join(app.screen()))
        time.sleep(0.02)
    app.sync()


def shown(app: App, text: str) -> bool:
    return any(text in line for line in app.screen())


def new_app(cols: int, rows: int, mode: str = "inline") -> App:
    app = App(backend="test", size=(cols, rows), mode=mode, interrupt="event").start()
    app.set_theme(THEME, truecolor=True)
    return app


def scene_agent() -> list:
    """A coding-agent turn: folded tool calls, a diff summary, background tasks."""
    app = new_app(*PAIR)
    AgentDemo(app, speed=0.0, interactive=False).run()
    until(app, lambda: shown(app, "2 background tasks"))
    cells = app.screen_cells()
    app.stop()
    return cells


def scene_approval() -> list:
    """An approval card docked where the input box was, while the agent waits."""
    app = new_app(*PAIR)
    demo = AgentDemo(app, speed=0.0, interactive=True)
    worker = threading.Thread(target=demo.run, daemon=True)
    worker.start()
    until(app, lambda: shown(app, "Run this command?"))
    time.sleep(0.2)
    app.sync()
    cells = app.screen_cells()
    demo.quit.set()
    app.stop()
    return cells


def scene_picker() -> list:
    """A categorised picker: section sidebar, focus areas, option chips."""
    app = new_app(*PAIR)
    user = app.transcript.add(
        Cell(role="user", children=[Text("switch to something faster")])
    )
    user.seal()
    reply = "Sure, pick one below; **←/→** move between the sections, the list and the options."
    app.transcript.add(Cell(role="assistant", children=[Markdown(source=reply)])).seal()
    app.bottom.add(Composer(placeholder="message · / commands", boxed=True))
    app.bottom.add(Footer(left="solo · codex/gpt-6-astra", right="ctx 18%"))

    def model(name, group, current=False, choices=None, chosen=None, detail=None):
        return {
            "label": name,
            "value": f"{group}/{name}",
            "group": group,
            "current": current,
            "choices": choices or [],
            "chosen": chosen or {},
            "detail": detail,
        }

    effort = [
        ["reasoning", ["default", "low", "medium", "high"]],
        ["speed", ["normal", "fast"]],
    ]
    items = [
        model(
            "gpt-6-astra", "codex", True, effort, {"reasoning": "high", "speed": "fast"}
        ),
        model("gpt-6.1-sol", "codex", choices=effort),
        model("gpt-6-luna", "codex", choices=effort),
        model("gpt-5.6-terra", "codex", choices=effort),
        model("claude-opus", "anthropic"),
        model("claude-sonnet", "anthropic"),
        model("gemini-4-argon", "gemini"),
        model("kimi-k3", "openrouter"),
        model("glm-5", "openrouter"),
    ]
    spec = Picker(
        items=items,
        title="Model · solo · now codex/gpt-6-astra",
        placeholder="type to filter by provider or model",
        sidebar=True,
        max_visible=6,
        dock="input",
    )
    threading.Thread(target=app.run_dialog, args=(spec,), daemon=True).start()
    until(app, lambda: shown(app, "Model · solo"))
    app.inject({"key": "right"})  # focus the options of the current model
    time.sleep(0.15)
    app.sync()
    cells = app.screen_cells()
    app.stop()
    return cells


def scene_train() -> list:
    """An inline training loop mid-run: the live chart, progress and stats redraw in place
    while the run's header stays in scrollback above them."""
    app = new_app(*PAIR)
    demo = TrainDemo(app, epochs=3, steps=300, speed=2.0)
    worker = threading.Thread(target=demo.run, daemon=True)
    worker.start()
    until(app, lambda: shown(app, "2/3"), timeout=30)
    time.sleep(1.2)
    app.sync()
    cells = app.screen_cells()
    app.stop()
    return cells


def scene_dashboard() -> list:
    """The full-screen training monitor: charts, eval table, GPU panel. Wide: the hero."""
    app = new_app(150, 38, "fullscreen")
    board = Dashboard(app, epochs=1, steps=90, speed=0.0, hold=True)
    worker = threading.Thread(target=board.run, daemon=True)
    worker.start()
    until(app, lambda: shown(app, "epoch 1 done"))
    cells = app.screen_cells()
    app.inject({"key": "q"})
    worker.join(timeout=3)
    app.stop()
    return cells


SCENES = {
    "dashboard": scene_dashboard,
    "train": scene_train,
    "picker": scene_picker,
    "approval": scene_approval,
    "agent": scene_agent,
}


# -- painting -----------------------------------------------------------------------------------


def xterm(index: int) -> str:
    """The xterm 256-colour palette."""
    base = [
        "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
        "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
    ]  # fmt: skip
    if index < 16:
        return base[index]
    if index < 232:
        i = index - 16
        steps = [0, 95, 135, 175, 215, 255]
        r, g, b = steps[i // 36], steps[i // 6 % 6], steps[i % 6]
        return f"#{r:02x}{g:02x}{b:02x}"
    level = 8 + (index - 232) * 10
    return f"#{level:02x}{level:02x}{level:02x}"


def resolve(color: str | None, default: str) -> str:
    if color is None:
        return default
    return xterm(int(color)) if color.isdigit() else color


def mix(a: str, b: str, t: float) -> str:
    ca = [int(a[i : i + 2], 16) for i in (1, 3, 5)]
    cb = [int(b[i : i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{round(x + (y - x) * t):02x}" for x, y in zip(ca, cb))


class Fonts:
    """A font per glyph: the monospace font if it has it, else the symbol or CJK font."""

    def __init__(self, mono: str, symbols: str, size: int):
        self.size = size
        self.faces = []
        for path in [mono, symbols, *CJK_CANDIDATES]:
            if not Path(path).exists():
                continue
            cmap = TTFont(path, fontNumber=0, lazy=True).getBestCmap() or {}
            regular = ImageFont.truetype(path, size)
            bold = ImageFont.truetype(path, size)
            try:
                bold.set_variation_by_name("Bold")
            except (OSError, ValueError):
                bold = regular
            self.faces.append((set(cmap), regular, bold))
        if not self.faces:
            raise SystemExit("no usable font: pass --font")
        mono_face = self.faces[0][1]
        self.cell_w = round(mono_face.getlength("M"))
        ascent, descent = mono_face.getmetrics()
        self.cell_h = round((ascent + descent) * 1.18)
        self.baseline = round((self.cell_h - (ascent + descent)) / 2)

    def pick(self, ch: str, bold: bool):
        for cmap, regular, heavy in self.faces:
            if ord(ch[0]) in cmap:
                return heavy if bold else regular
        return self.faces[0][2] if bold else self.faces[0][1]


# braille dot bits in reading order: (column, row) of each of the 2x4 dots
BRAILLE_DOTS = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)]


def draw_braille(
    draw: ImageDraw.ImageDraw, ch: str, x0: int, y: int, fonts: Fonts, fg: str
):
    """Braille drawn as dots, so charts look as dense as in a terminal whatever the font."""
    bits = ord(ch) - 0x2800
    dw, dh = fonts.cell_w / 2, fonts.cell_h / 4
    r = max(1.5, min(dw, dh) * 0.32)
    for bit, (cx, cy) in enumerate(BRAILLE_DOTS):
        if bits & (1 << bit):
            mx, my = x0 + (cx + 0.5) * dw, y + (cy + 0.5) * dh
            draw.ellipse((mx - r, my - r, mx + r, my + r), fill=fg)


def cell_width(ch: str) -> int:
    return 2 if unicodedata.east_asian_width(ch[0]) in ("W", "F") else 1


def paint(cells: list, fonts: Fonts, title: str) -> Image.Image:
    cols = max(sum(sum(cell_width(c) for c in run[0]) for run in row) for row in cells)
    rows = len(cells)
    pad, bar, radius = 28, 40, 14
    width = cols * fonts.cell_w + pad * 2
    height = rows * fonts.cell_h + pad * 2 + bar
    image = Image.new("RGB", (width + 48, height + 48), "#00000000")
    draw = ImageDraw.Draw(image)

    # the window: a rounded frame, a title bar with three dots and the title
    draw.rounded_rectangle((24, 24, 24 + width, 24 + height), radius, fill=BACKGROUND)
    draw.rounded_rectangle((24, 24, 24 + width, 24 + bar + radius), radius, fill=WINDOW)
    draw.rectangle((24, 24 + bar, 24 + width, 24 + bar + radius), fill=BACKGROUND)
    for i, dot in enumerate(["#ff5f57", "#febc2e", "#28c840"]):
        cx, cy = 24 + 22 + i * 22, 24 + bar // 2
        draw.ellipse((cx - 6, cy - 6, cx + 6, cy + 6), fill=dot)
    label = fonts.pick("a", False)
    tw = draw.textlength(title, font=label)
    draw.text(
        (24 + (width - tw) / 2, 24 + bar / 2),
        title,
        font=label,
        fill="#8a8399",
        anchor="lm",
    )

    top = 24 + bar + pad
    left = 24 + pad
    for r, row in enumerate(cells):
        x = 0
        y = top + r * fonts.cell_h
        for text, fg, bg, flags in row:
            fgc = resolve(fg, FOREGROUND)
            bgc = resolve(bg, BACKGROUND)
            if "inverse" in flags:
                fgc, bgc = bgc, fgc
            if "dim" in flags:
                fgc = mix(fgc, bgc, 0.45)
            for ch in text:
                w = cell_width(ch)
                x0 = left + x * fonts.cell_w
                if bgc != BACKGROUND:
                    draw.rectangle(
                        (x0, y, x0 + w * fonts.cell_w, y + fonts.cell_h), fill=bgc
                    )
                if 0x2801 <= ord(ch[0]) <= 0x28FF:
                    draw_braille(draw, ch, x0, y, fonts, fgc)
                elif ch.strip():
                    font = fonts.pick(ch, "bold" in flags)
                    draw.text((x0, y + fonts.baseline), ch, font=font, fill=fgc)
                if "underline" in flags:
                    uy = y + fonts.cell_h - 3
                    draw.line((x0, uy, x0 + w * fonts.cell_w, uy), fill=fgc, width=1)
                x += w
    return image


def first(paths: list[str]) -> str:
    return next((p for p in paths if Path(p).exists()), paths[0])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("docs/images"))
    parser.add_argument("--font", default=first(FONT_CANDIDATES))
    parser.add_argument("--symbols", default=first(SYMBOL_CANDIDATES))
    parser.add_argument(
        "--size", type=int, default=30, help="font size in pixels (2x scale)"
    )
    parser.add_argument("scenes", nargs="*", default=list(SCENES))
    args = parser.parse_args()

    fonts = Fonts(args.font, args.symbols, args.size)
    args.out.mkdir(parents=True, exist_ok=True)
    for name in args.scenes:
        cells = SCENES[name]()
        image = paint(cells, fonts, f"kohakutui · {name}")
        path = args.out / f"{name}.png"
        image.save(path, optimize=True)
        print(f"{path}  {image.width}x{image.height}")


if __name__ == "__main__":
    main()
