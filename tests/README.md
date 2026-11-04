# Tests

The engine is a pipeline of stages. Each stage is correct when its output, read on the level
below, matches its input. So a test proves **one stage on the level below and nothing further**,
and the chain of stage proofs covers the whole path from a Python call to the bytes on the
terminal.

| Stage | Input → output | Read on | Lives in |
| ----- | -------------- | ------- | -------- |
| proto | Python values / msgpack bytes → `Op`, `Event` | serde round trip, per-op decode | `src/kohakutui-rust/proto/tests.rs` |
| model | `Op` sequence → node tree, seal and flush state | the tree and `take_flushable` | `model/tests.rs` |
| text | Markdown source (any chunking, any width) → lines with `src_start` | the final render; source coverage | `text/tests.rs` |
| widgets / view | model + view state → ratatui `Buffer` | `TestBackend` snapshots (insta) | `widgets/tests.rs`, `view/tests.rs` |
| term | lines and frames → terminal bytes | a `vt100` emulator: scrollback + screen | `term/tests.rs` |
| SDK | `App` / `Node` calls → `Op` batches | the ops sent to `_Session` | `tests/unit/` |

## Directories

```text
tests/
├── static/        layout and contracts: file sizes, Rust module layout, import rules, the _rs stub
├── e2e/           whole journeys on the test backend: a script drives App, the screen is asserted
├── integration/   one SDK package → one test class, each method a complete workflow
└── unit/          one source file → one test file
```

Write tiers **e2e first, then integration, then unit**. The journey fixes what must work; the
lower tiers pin the parts that make it work.

Rules (from KohakuTerrarium `tests/README.md`):

- **Behavior asserts, not shape asserts.** Every test observes a side effect: a screen, a
  scrollback, an emitted op. Shape checks (`isinstance`, key present) belong in `unit/` only.
- **Real collaborators, not mocks.** The only seam is the terminal: `_Session(backend="test")`.
- **A check that cannot fail on a broken stage is not a check.** Assert the property, not an
  exit code.

## Running

```bash
uv pip install -e ".[dev]"            # builds the extension with maturin
pytest tests/ -q                      # static + Python tiers
cargo test --no-default-features      # Rust stage tests
```
