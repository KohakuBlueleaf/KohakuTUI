# KohakuTUI

A terminal UI engine written in Rust (ratatui + crossterm) and driven from Python.

Python describes *what* is on screen as a tree of nodes. Rust owns the terminal, the frame loop
and every keystroke, so streaming output never flickers or reflows, and typing stays smooth
while Python is busy. The same Python API runs with the engine in-process, in a subprocess, or
on another machine you attach to.

> Status: **early development.** The API is not stable yet.

## What it is for

- **Agent front ends.** An inline mode where finished output goes to the terminal's real
  scrollback, and a full-screen mode with panels, switchable at runtime. It is the planned
  replacement for KohakuTerrarium's `cli_rich` and Textual front ends.
- **Training scripts.** Progress, loss curves and logs that a stray `print` cannot corrupt, and a
  dashboard you can attach to from your laptop while the job runs on a cluster.

## Development setup

Requires a Rust toolchain (stable) and Python 3.10+.

```bash
uv venv
uv pip install -e ".[dev]"     # maturin builds kohakutui._rs into the venv
python -c "import kohakutui; print(kohakutui.version())"
```

After changing Rust code, run `uv pip install -e ".[dev]"` again (or `maturin develop`) to
rebuild the extension.

Checks:

```bash
pytest tests/ -q
cargo test --no-default-features
cargo fmt --check && cargo clippy --all-targets --features python,highlight,testing -- -D warnings
black --check src/ tests/ && ruff check src/ tests/
```

## Layout

```text
src/kohakutui-rust/   the engine crate (kohakutui._rs with the `python` feature)
src/kohakutui/        the Python SDK
tests/                static contracts + e2e / integration / unit (see tests/README.md)
```

Contributor rules are in [`CLAUDE.md`](CLAUDE.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE).
