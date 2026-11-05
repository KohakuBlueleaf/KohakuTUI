"""Routing ``print`` and ``logging`` into the UI, so they cannot corrupt the screen.

This captures Python-level writes to ``sys.stdout`` / ``sys.stderr``; writes made directly to
file descriptor 1 by native code are not captured (design §10.5 describes the fd-level form).
"""

import io
import sys
import logging
import threading
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from kohakutui.ui.node import Node

LEVEL_ROLE = {
    logging.DEBUG: "output",
    logging.INFO: "output",
    logging.WARNING: "system",
    logging.ERROR: "error",
    logging.CRITICAL: "error",
}


class UIWriter(io.TextIOBase):
    """A text stream that sends what is written to the UI.

    With a ``target`` node (a ``Log``), raw text is appended as it comes, so ``\\r`` progress
    bars rewrite their line; without one, each complete line becomes a transcript cell.
    """

    def __init__(self, app: Any, target: "Node | None"):
        self.app = app
        self.target = target
        self._partial = ""
        self._lock = threading.Lock()

    @property
    def encoding(self) -> str:
        return "utf-8"

    def writable(self) -> bool:
        return True

    def isatty(self) -> bool:
        return False

    def write(self, s: str) -> int:
        if not s:
            return 0
        if self.target is not None:
            self.target.append(s)
            return len(s)
        with self._lock:
            *lines, self._partial = (self._partial + s).split("\n")
        for line in lines:
            self.app.print(line)
        return len(s)

    def flush(self) -> None:
        with self._lock:
            rest, self._partial = self._partial, ""
        if rest:
            self.app.print(rest)


class UILogHandler(logging.Handler):
    """A logging handler that prints each record as a transcript cell (or into ``target``)."""

    def __init__(
        self, app: Any, target: "Node | None" = None, level: int = logging.INFO
    ):
        super().__init__(level)
        self.app = app
        self.target = target
        self.setFormatter(logging.Formatter("[%(levelname)s] %(name)s: %(message)s"))

    def emit(self, record: logging.LogRecord) -> None:
        try:
            text = self.format(record)
        except Exception:
            self.handleError(record)
            return
        if self.target is not None:
            self.target.append(text + "\n")
        else:
            self.app.print(text, role=LEVEL_ROLE.get(record.levelno, "output"))


class CaptureMixin:
    """Swapping stdio and attaching a logging handler for the lifetime of a capture."""

    def _init_capture_state(self) -> None:
        self._saved_stdio: tuple[Any, Any] | None = None
        self._writers: list[UIWriter] = []
        self._log_handler: UILogHandler | None = None

    def capture_stdio(
        self, target: "Node | None" = None, *, stdout: bool = True, stderr: bool = True
    ) -> None:
        """Routes ``print`` and other Python-level writes into ``target`` or the transcript."""
        self._release_stdio()
        self._saved_stdio = (sys.stdout, sys.stderr)
        if stdout:
            sys.stdout = self._writer(target)
        if stderr:
            sys.stderr = self._writer(target)

    def _writer(self, target: "Node | None") -> UIWriter:
        writer = UIWriter(self, target)
        self._writers.append(writer)
        return writer

    def capture_logging(
        self, level: int = logging.INFO, target: "Node | None" = None
    ) -> None:
        """Sends root-logger records to the UI."""
        self._release_logging()
        self._log_handler = UILogHandler(self, target, level)
        logging.getLogger().addHandler(self._log_handler)

    def release_capture(self) -> None:
        """Restores stdout/stderr and detaches the logging handler."""
        self._release_stdio()
        self._release_logging()

    def _release_stdio(self) -> None:
        for w in self._writers:
            w.flush()
        self._writers.clear()
        if self._saved_stdio is not None:
            sys.stdout, sys.stderr = self._saved_stdio
            self._saved_stdio = None

    def _release_logging(self) -> None:
        if self._log_handler is not None:
            logging.getLogger().removeHandler(self._log_handler)
            self._log_handler = None
