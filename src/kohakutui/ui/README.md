# kohakutui/ui/

What the screen is made of, on the Python side.

## Files

| File | Responsibility |
| ---- | -------------- |
| `components.py` | Component specs (dataclasses) and the `create` ops they become |
| `node.py` | `Node`: a handle on one node (`add`, `update`, `append`, `seal`, `remove`, `move`, `focus`) |
| `widget.py` | `Widget`: the contract for Python components that own semantic state and their nodes' events |

## Dependency direction

`components` ← `node` ← `widget`. Nodes reach the app only through the handle they hold
(`node.app`); nothing here imports `kohakutui.app` at runtime.
