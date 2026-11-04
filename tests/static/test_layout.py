"""Guard: repository layout rules from CLAUDE.md that a reviewer would otherwise check by eye."""

import ast
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
PY_SRC = ROOT / "src" / "kohakutui"
RS_SRC = ROOT / "src" / "kohakutui-rust"


def rust_module_dirs() -> list[Path]:
    return sorted(p for p in RS_SRC.rglob("*") if p.is_dir())


def python_package_dirs() -> list[Path]:
    return sorted({p.parent for p in PY_SRC.rglob("__init__.py")})


def function_level_imports(source: str) -> list[int]:
    """Line numbers of import statements nested inside a function body."""
    lines = []
    for node in ast.walk(ast.parse(source)):
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        for inner in ast.walk(node):
            if isinstance(inner, (ast.Import, ast.ImportFrom)):
                lines.append(inner.lineno)
    return sorted(set(lines))


@pytest.mark.parametrize(
    "path", rust_module_dirs(), ids=lambda p: p.relative_to(RS_SRC).as_posix()
)
def test_rust_module_dir_has_mod_rs(path):
    assert (
        path / "mod.rs"
    ).is_file(), f"{path.relative_to(ROOT)} has no mod.rs entry point"


def test_rust_no_file_module_beside_its_dir():
    clashes = [
        f.relative_to(ROOT).as_posix()
        for f in RS_SRC.rglob("*.rs")
        if f.name != "mod.rs" and (f.parent / f.stem).is_dir()
    ]
    assert not clashes, f"use <name>/mod.rs, not <name>.rs beside <name>/: {clashes}"


def test_function_level_imports_detects_nested_import():
    source = "import os\n\n\ndef f():\n    import sys\n    return sys\n"
    assert function_level_imports(source) == [5]


@pytest.mark.parametrize(
    "path", sorted(PY_SRC.rglob("*.py")), ids=lambda p: p.relative_to(PY_SRC).as_posix()
)
def test_no_imports_inside_functions(path):
    lines = function_level_imports(path.read_text(encoding="utf-8"))
    assert (
        not lines
    ), f"{path.relative_to(ROOT)} imports inside functions at lines {lines}"


@pytest.mark.parametrize(
    "path",
    [RS_SRC, *python_package_dirs()],
    ids=lambda p: p.relative_to(ROOT).as_posix(),
)
def test_package_dir_has_readme(path):
    assert (path / "README.md").is_file(), f"{path.relative_to(ROOT)} has no README.md"
