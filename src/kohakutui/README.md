# kohakutui/

The Python SDK. It describes what is on screen as a tree of nodes and sends that to the Rust
extension `kohakutui._rs` (built from `../kohakutui-rust/`). Rust owns the terminal, the frame
loop and every keystroke; Python receives semantic events only (design §1, §3).

## Files

| File | Responsibility |
| ---- | -------------- |
| `__init__.py` | Public API: `version`, `PROTOCOL` |
| `_rs.pyi` | Type stub of the extension module; `tests/static` checks it matches the module's exports |
| `py.typed` | PEP 561 marker |

Planned packages are listed in design §9 (`app`, `node`, `components`, `events`,
`dialogs`, `capture`, `reconcile`, `transport`).

## Dependency direction

Imports: `kohakutui._rs` only. No third-party runtime dependencies; `tqdm` is an optional extra.

Imported by: applications (KohakuTerrarium `builtins/ktui/`, training scripts).

## See also

- `../kohakutui-rust/README.md`: the engine
