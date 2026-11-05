"""``python -m kohakutui``: demos and terminal repair."""

import sys

from kohakutui._rs import reset_terminal, version
from kohakutui.demo.chat import chat
from kohakutui.demo.train import train

USAGE = """usage: python -m kohakutui <command>

  demo [chat|train]   run a demo (default: chat)
  reset               restore a terminal left in raw mode or on the alternate screen
  version             print the version
"""


def main(argv: list[str]) -> int:
    match argv:
        case ["demo"] | ["demo", "chat"]:
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
