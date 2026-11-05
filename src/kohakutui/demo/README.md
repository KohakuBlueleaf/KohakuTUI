# kohakutui/demo/

Runnable demos; each is also a usage example of the public API.

| File | Responsibility |
| ---- | -------------- |
| `agent.py` | A Codex / KT-cli style coding-agent turn: streamed reasoning, tool calls, a sub-agent, a diff, an approval card, a shell run, a background task, a plan panel, a token footer |
| `dashboard.py` | A full-screen training monitor: event log, run/eval/GPU panel, progress bars and sparklines |
| `chat.py` | The minimal chat: composer, streamed Markdown, a picker and a confirm dialog |
| `train.py` | The minimal inline training loop: progress, a loss sparkline, captured `print` |
| `tasks.py` | Claude Code style background tasks: a status line, a ctrl+b list view, a live expanded view with Kill |
| `parts.py` | The composites the demos use (`ToolCall`, `DiffView`, `PlanPanel`): examples of the `Widget` contract |

Run them with `python -m kohakutui demo agent|dashboard|chat|train`.
