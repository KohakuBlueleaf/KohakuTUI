# kohakutui/app/

`App`, one terminal UI session. It is a pseudo-class package: one mixin per concern, each
owning its state through `_init_<part>_state()`, assembled in `app.py`.

## Files

| File | Responsibility |
| ---- | -------------- |
| `app.py` | `App`: builds the `_Session`, inits every mixin, creates the five region nodes |
| `core.py` | `CoreMixin`: start/stop, node ids, `send` and per-thread `batch()`, `sync`, the pump thread |
| `control.py` | `ControlMixin`: mode, bindings, `notify`, `print`, `on`/`events`/`aevents`, widget ownership, test-backend readers |
| `dialogs.py` | `DialogsMixin`: `choose`, `ask`, `confirm`, `card`, their async forms, and `run_dialog` (any overlay spec; `on_open` gets its node for live updates) |
| `capture.py` | `CaptureMixin`: routing `print` and `logging` into the UI |
| `events.py` | `Event`, `Waiter`, and the `Dispatcher` that delivers events from the pump thread |

## Invariants

- After `stop()` (or when the engine ends by itself) every send is a no-op and every pending
  dialog returns None; nothing raises in user threads.
- A `batch()` belongs to its thread; ops from one thread are applied in the order sent.
- Dialogs register their waiter before the overlay is created.

## Dependency direction

Imports `kohakutui._rs` and `kohakutui.ui`. Imported by `kohakutui/__init__.py`.
