"""``python -m kohakutui``: demos and terminal repair."""

import sys

from kohakutui._rs import reset_terminal, version
from kohakutui.demo.agent import agent
from kohakutui.demo.chat import chat
from kohakutui.demo.dashboard import dashboard
from kohakutui.demo.train import train

USAGE = """usage: python -m kohakutui <command>

  demo [agent|dashboard|chat|train]   run a demo (default: agent)
  reset               restore a terminal left in raw mode or on the alternate screen
  version             print the version
"""


def main(argv: list[str]) -> int:
    match argv:
        case ["demo"] | ["demo", "agent"]:
            agent()
        case ["demo", "dashboard"]:
            dashboard()
        case ["demo", "chat"]:
            chat()
        case ["demo", "train"]:
            train()
        case ["reset"]:
            reset_terminal()
        case ["version"]:
            print(version())
        case _:
            print(USAGE, end="")
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
