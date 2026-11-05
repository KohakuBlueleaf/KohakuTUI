# kohakutui/

The Python SDK. It describes what is on screen as a tree of nodes and sends that to the Rust
extension `kohakutui._rs` (built from `../kohakutui-rust/`). Rust owns the terminal, the frame
loop and every keystroke; Python receives semantic events only.

## Layout

| Path | Responsibility |
| ---- | -------------- |
| `__init__.py` | Public API, re-exported from the packages below |
| `app/` | `App`: lifecycle, sending, the event pump and dispatch, dialogs, capture |
| `ui/` | `Node` handles, the `Widget` contract, component specs |
| `demo/` | `python -m kohakutui demo chat / train` |
| `__main__.py` | `python -m kohakutui demo / reset / version` |
| `_rs.pyi` | Type stub of the extension; `tests/static` checks it matches the module |
| `py.typed` | PEP 561 marker |

## Dependency direction

`ui` ← `app` ← `demo`, `__init__`. Imports `kohakutui._rs`; no third-party runtime
dependencies.

## See also

- `../kohakutui-rust/README.md`: the engine
- `../../tests/README.md`: the test stages and tiers
