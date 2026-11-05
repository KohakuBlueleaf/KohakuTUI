"""The release version script: one version per build, nightlies never touch the tracked one."""

import importlib.util
from datetime import datetime, timezone
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "release_version.py"
spec = importlib.util.spec_from_file_location("release_version", SCRIPT)
release_version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_version)

CARGO = '[package]\nname = "kohakutui"\nversion = "0.1.0"\n\n[dependencies]\nversion = "9"\n'
NOW = datetime(2026, 11, 5, 3, 0, 7, tzinfo=timezone.utc)


def test_a_stable_build_uses_the_tracked_version_and_its_tag():
    out = release_version.resolve(CARGO, now=NOW)
    assert out == {
        "cargo_version": "0.1.0",
        "python_version": "0.1.0",
        "tag": "v0.1.0",
        "nightly": "false",
    }


def test_a_nightly_is_a_dev_version_with_a_dated_tag():
    out = release_version.resolve(CARGO, nightly=True, now=NOW)
    assert out["cargo_version"] == "0.1.0-dev.20261105030007"
    assert out["python_version"] == "0.1.0.dev20261105030007"
    assert out["tag"] == "nightly-20261105"


def test_stamping_changes_only_the_package_version():
    stamped = release_version.stamp(CARGO, "0.1.0-dev.20261105030007")
    assert 'version = "0.1.0-dev.20261105030007"' in stamped
    assert 'version = "9"' in stamped, "a dependency's version is left alone"
    with pytest.raises(ValueError):
        release_version.stamp(CARGO, "0.1")
    with pytest.raises(ValueError):
        release_version.resolve(CARGO.replace('"0.1.0"', '"0.1.0-dev.1"'))


def test_the_repository_version_is_a_plain_release_version():
    text = (SCRIPT.parents[1] / "Cargo.toml").read_text(encoding="utf-8")
    assert release_version.resolve(text)["nightly"] == "false"
