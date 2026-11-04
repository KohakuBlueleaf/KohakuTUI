"""Contract: ``_rs.pyi`` declares exactly what the compiled ``kohakutui._rs`` exports."""

import ast
from pathlib import Path

from kohakutui import _rs

STUB = Path(__file__).resolve().parents[2] / "src" / "kohakutui" / "_rs.pyi"


def stub_names(source: str) -> set[str]:
    """Public top-level names a stub declares."""
    names = set()
    for node in ast.parse(source).body:
        match node:
            case ast.FunctionDef(name=name) | ast.ClassDef(name=name):
                names.add(name)
            case ast.AnnAssign(target=ast.Name(id=name)):
                names.add(name)
    return {n for n in names if not n.startswith("__")}


def module_names(module) -> set[str]:
    return {n for n in dir(module) if not n.startswith("__")}


def test_stub_names_reads_functions_classes_and_annotations():
    source = "X: int\n\ndef f() -> None: ...\n\nclass _C: ...\n"
    assert stub_names(source) == {"X", "f", "_C"}


def test_stub_matches_extension_exports():
    declared = stub_names(STUB.read_text(encoding="utf-8"))
    exported = module_names(_rs)
    assert declared == exported, (
        f"missing from stub: {sorted(exported - declared)}; "
        f"stub-only: {sorted(declared - exported)}"
    )
