"""KohakuTUI: a terminal UI engine in Rust, driven from Python.

The Rust extension (``kohakutui._rs``) owns the terminal, the frame loop and every
keystroke; this package is the Python SDK that describes what is on screen.
"""

from kohakutui._rs import PROTOCOL, version

__all__ = ["PROTOCOL", "version"]
