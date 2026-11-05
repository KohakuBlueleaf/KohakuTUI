# KohakuTUI

A terminal UI engine in Rust, driven from Python through a retained node-tree protocol. Python
describes what is on screen; Rust owns the terminal, the frame loop and every keystroke.

The architecture is `plans/design.md` (local, gitignored, like all of `plans/`). Section references
such as "design §5" in code and READMEs point into it. Read §1–§5 before changing anything in
`proto/` or `model/`.

## Project layout

```text
src/kohakutui-rust/   engine crate; lib.rs holds the `_rs` pymodule (feature `python`)
src/kohakutui/        Python SDK over kohakutui._rs
tests/                static/, e2e/, integration/, unit/ (Python); Rust tests in <module>/tests.rs
docs/                 public docs (protocol, sdk), added as they are written
plans/                local design and working plans (gitignored)
```

## Development setup

- `uv venv`, then `uv pip install -e ".[dev]"`. maturin builds the extension into the venv;
  rerun after any Rust change.
- **Never use `sys.path.insert` hacks** in examples or tests; import `kohakutui.*` from the
  installed package.

## Rust rules

1. **Every module is a directory whose only entry point is `mod.rs`**: submodule declarations,
   the module's public types and re-exports. Logic goes in sibling files. Never `name.rs` beside
   `name/` (`tests/static/test_layout.py` enforces this).
2. Unit tests live in `<module>/tests.rs` (`#[cfg(test)] mod tests;` in `mod.rs`).
3. pyo3 code lives in `<module>/python.rs` behind `#[cfg(feature = "python")]`, exposes
   `register(m)`, and is registered from `lib.rs`. Pure Rust never imports pyo3, so
   `cargo test --no-default-features` links no Python.
4. pyclasses are underscore-prefixed (`_Session`) and wrapped by a public Python class.
5. Errors: one `thiserror` enum per module, with `From<Error> for PyErr` in its `python.rs`.
   The engine never panics on input from Python or the wire; a bad op becomes `Event::Error`.
6. `//!` docs at the top of every file say what the module is for. `pub(crate)` unless the
   item is part of the crate API.
7. `cargo fmt` (max width 100) and `cargo clippy --all-targets -- -D warnings`, with and without
   `--features python`.
8. Code adapted from Codex (Apache-2.0) or ratatui (MIT) keeps its license header.

## Python rules

### Style
- Target Python 3.10+ (abi3 wheel). Modern hints: `list`, `dict`, `X | None`.
- Prefer `match`/`case` over deep `if`/`elif`.
- No `print()` in library code. CLI entry points (`__main__.py`) may print.
- black (default width) and `ruff check` over `src/` and `tests/`.

### Imports
1. No imports inside functions (except an optional dependency, such as `tqdm`).
   `tests/static/test_layout.py` enforces this.
2. Groups: built-in, third-party, `kohakutui`.
3. Within a group: `import` before `from`, shorter dotted paths first, then a–z.

### Files
- One responsibility per module. Every package folder has a `README.md` with a file table.
- File size is capped by AST statements (imports and docstrings excluded): over 500 warns,
  over 1000 fails (`tests/static/test_file_sizes.py`).

## Comments and docstrings

Comment, code and docstring all say **what**. *How* and *why* live in `docs/` and `plans/`.

1. No memo, note or editorial comments.
2. A comment says what the code itself cannot say clearly.
3. A docstring says what the class or function is *for*, so a reader can place it.
4. Soft limits: inline comment blocks ≤ 2 lines, docstrings ≤ 14 lines.
5. Doc comments (`//!`, `///`, docstrings) say what a file, type or function is for. An inline
   comment (1–2 lines) heads a logic block only when the code cannot say what it does (a tricky
   API, a non-obvious algorithm step).
6. Separate logic blocks inside a function with one blank line.
7. Formatting is the formatter's: rustfmt for Rust, black for Python (black already wraps long
   expressions in parentheses).

## Tests

The engine is a pipeline of stages (proto → model → text → widgets/view → term, then the SDK on
top). **A test proves one stage on the level below it and nothing further.** `tests/README.md`
has the stage table.

- Write tiers **e2e first, then integration, then unit**: the journey fixes what must work, the
  lower tiers pin the parts.
- `static/`: layout and contract guards (module layout, imports, file sizes, the `_rs.pyi` stub).
- Behavior asserts, not shape asserts; real collaborators, not mocks. The only seam is the
  terminal: `_Session(backend="test")`.
- A check that cannot fail on a broken stage is not a check.

## Post-implementation

1. Check the rules above, especially in-function imports and the `mod.rs` layout.
2. `black src/ tests/`, `ruff check src/ tests/`, `cargo fmt`, both clippy runs.
3. `pytest tests/ -q` and `cargo test --no-default-features`.
4. New behaviour has tests at the right stage and tier.
5. Logically separate commits (conventional style: `feat(model): ...`). **Do not commit unless
   asked.**

## Audit loop (multi-step work: required)

Implement → write tests that pin the behaviour (negative cases first) → run every suite and lint
→ audit the diff for clear bugs, integrity bugs (broken invariants, races, stale caches) and
behaviour bugs (wrong default, swallowed error) → if the audit finds a bug the tests missed,
first make a test fail on it, then fix → repeat until the audit is clean and everything is green.
