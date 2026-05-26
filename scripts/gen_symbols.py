#!/usr/bin/env python3
"""Generate SYMBOLS.md — public API index for all lumen crates.

Extracts every `pub fn/struct/enum/trait/type` with its file:line and
first /// doc comment line. Grouped by crate.

Usage:
    python scripts/gen_symbols.py
"""

import re
from pathlib import Path
from collections import defaultdict

ROOT = Path(__file__).parent.parent
CRATES_DIR = ROOT / "crates"
OUTPUT = ROOT / "SYMBOLS.md"

PUB_ITEM = re.compile(r"pub\s+(fn|struct|enum|trait|type)\s+(\w+)")
DOC_LINE = re.compile(r"^\s*///\s?(.*)")


def crate_name(path: Path) -> str:
    parts = path.relative_to(CRATES_DIR).parts
    return f"lumen-{parts[1]}" if parts[0] == "engine" else f"lumen-{parts[0]}"


def rel(path: Path) -> str:
    return str(path.relative_to(ROOT)).replace("\\", "/")


def extract(filepath: Path):
    symbols = []
    lines = filepath.read_text(encoding="utf-8", errors="ignore").splitlines()
    pending_doc: list[str] = []

    for i, line in enumerate(lines, 1):
        stripped = line.strip()

        m = DOC_LINE.match(line)
        if m:
            pending_doc.append(m.group(1))
            continue

        # Blank lines and attributes don't break the doc accumulator
        if not stripped or stripped.startswith("#["):
            continue

        pm = PUB_ITEM.search(stripped)
        if pm:
            doc = pending_doc[0].rstrip(".") if pending_doc else ""
            symbols.append((i, pm.group(1), pm.group(2), doc))

        pending_doc = []

    return symbols


def main():
    by_crate: dict[str, list] = defaultdict(list)

    for rs in sorted(CRATES_DIR.rglob("*.rs")):
        # Skip test files and entry points
        parts = rs.parts
        if "tests" in parts or rs.stem in ("main", "build"):
            continue

        crate = crate_name(rs)
        r = rel(rs)
        for lineno, kind, name, doc in extract(rs):
            by_crate[crate].append((r, lineno, kind, name, doc))

    out = [
        "# SYMBOLS",
        "",
        "Auto-generated public API index. Regenerate: `python scripts/gen_symbols.py`",
        "",
        "**Usage:** grep for a symbol → get `file:line` → `Read file offset=N limit=30`.",
        "",
    ]

    total = 0
    for crate in sorted(by_crate):
        items = sorted(by_crate[crate], key=lambda x: (x[0], x[1]))
        out.append(f"## {crate}  ({len(items)} symbols)")
        out.append("")
        for r, lineno, kind, name, doc in items:
            doc_part = f" — {doc}" if doc else ""
            out.append(f"`{r}:{lineno}` **{kind}** `{name}`{doc_part}")
        out.append("")
        total += len(items)

    out.append(f"---")
    out.append(f"*Total: {total} symbols in {len(by_crate)} crates*")
    out.append("")

    OUTPUT.write_text("\n".join(out), encoding="utf-8")
    print(f"SYMBOLS.md: {total} symbols in {len(by_crate)} crates -> {OUTPUT}")


if __name__ == "__main__":
    main()
