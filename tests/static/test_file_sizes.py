"""Guard: Python source files stay under 1000 statements (hard); over 500 (soft) warns."""

import ast
import warnings
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "src" / "kohakutui"

SOFT_CAP_STATEMENTS = 500
HARD_CAP_STATEMENTS = 1000


def count_statements(source: str) -> int:
    """Count AST statements, excluding imports and docstrings."""
    total = 0
    for node in ast.walk(ast.parse(source)):
        if not isinstance(node, ast.stmt):
            continue
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            continue
        if (
            isinstance(node, ast.Expr)
            and isinstance(node.value, ast.Constant)
            and isinstance(node.value.value, str)
        ):
            continue
        total += 1
    return total


def test_count_statements_ignores_imports_and_docstrings():
    source = '"""doc"""\nimport os\nfrom a import b\n\n\ndef f():\n    """doc"""\n    return 1\n'
    assert count_statements(source) == 2


@pytest.mark.parametrize(
    "path", sorted(SRC.rglob("*.py")), ids=lambda p: p.relative_to(SRC).as_posix()
)
def test_file_statement_caps(path):
    """Over the soft cap warns; over the hard cap fails."""
    rel = path.relative_to(SRC).as_posix()
    statements = count_statements(path.read_text(encoding="utf-8"))
    assert (
        statements <= HARD_CAP_STATEMENTS
    ), f"{rel} has {statements} statements (hard max {HARD_CAP_STATEMENTS})"
    if statements > SOFT_CAP_STATEMENTS:
        warnings.warn(
            f"{rel} has {statements} statements (soft cap {SOFT_CAP_STATEMENTS}): "
            "keep it only if it is one large coherent unit",
            stacklevel=1,
        )
