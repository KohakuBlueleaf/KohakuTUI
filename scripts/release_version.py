"""Resolve or stamp the Cargo package version for a stable or nightly build.

The tracked version stays ``X.Y.Z``. A nightly stamps only the build checkout with
``X.Y.Z-dev.TIMESTAMP`` (Cargo) / ``X.Y.Z.devTIMESTAMP`` (Python), never a version tag.
maturin reads the wheel's version from ``Cargo.toml``, so the stamp is all a build needs.

    python scripts/release_version.py                     # print the outputs for a stable build
    python scripts/release_version.py --nightly --github-output "$GITHUB_OUTPUT"
    python scripts/release_version.py --stamp 0.1.0-dev.20261105030000
"""

import argparse
import re
from datetime import datetime, timezone
from pathlib import Path

PACKAGE_VERSION = re.compile(r'(\[package\][\s\S]*?\nversion\s*=\s*")([^"]+)(")')


def stamp(text: str, version: str) -> str:
    """``Cargo.toml`` text with its package version replaced."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-dev\.\d+)?", version):
        raise ValueError("expected X.Y.Z or X.Y.Z-dev.TIMESTAMP")
    changed, count = PACKAGE_VERSION.subn(
        lambda m: m[1] + version + m[3], text, count=1
    )
    if count != 1:
        raise ValueError("Cargo package version not found")
    return changed


def resolve(text: str, nightly: bool = False, now: datetime | None = None) -> dict:
    """The versions and tag one build uses on every platform."""
    match = PACKAGE_VERSION.search(text)
    if not match or not re.fullmatch(r"\d+\.\d+\.\d+", match[2]):
        raise ValueError("tracked Cargo package version must be X.Y.Z")
    base = match[2]
    now = now or datetime.now(timezone.utc)
    version = base + ("-dev." + now.strftime("%Y%m%d%H%M%S") if nightly else "")
    return {
        "cargo_version": version,
        "python_version": version.replace("-dev.", ".dev"),
        "tag": "nightly-" + now.strftime("%Y%m%d") if nightly else "v" + base,
        "nightly": str(nightly).lower(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo", type=Path, default=Path("Cargo.toml"))
    parser.add_argument("--nightly", action="store_true")
    parser.add_argument("--stamp")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    text = args.cargo.read_text(encoding="utf-8")
    if args.stamp:
        args.cargo.write_text(stamp(text, args.stamp), encoding="utf-8")
        return
    outputs = resolve(text, args.nightly)
    payload = "".join(f"{key}={value}\n" for key, value in outputs.items())
    print(payload, end="")
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as stream:
            stream.write(payload)


if __name__ == "__main__":
    main()
