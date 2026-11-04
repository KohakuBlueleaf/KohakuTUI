"""Unit: ``kohakutui/__init__.py`` re-exports the extension's version and protocol."""

import re
from pathlib import Path

import kohakutui

CARGO = Path(__file__).resolve().parents[2] / "Cargo.toml"


def cargo_version() -> str:
    text = CARGO.read_text(encoding="utf-8")
    match = re.search(r'^\[package\][^\[]*?^version\s*=\s*"([^"]+)"', text, re.M | re.S)
    assert match, "Cargo.toml has no [package] version"
    return match.group(1)


def test_version_is_the_cargo_version():
    assert kohakutui.version() == cargo_version()


def test_protocol_is_major_minor_ints():
    major, minor = kohakutui.PROTOCOL
    assert (major, minor) == (1, 0)
    assert all(isinstance(v, int) for v in kohakutui.PROTOCOL)


def test_public_api_is_exactly_declared():
    assert set(kohakutui.__all__) == {"PROTOCOL", "version"}
    for name in kohakutui.__all__:
        assert getattr(kohakutui, name) is getattr(kohakutui._rs, name)
