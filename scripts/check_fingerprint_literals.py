#!/usr/bin/env python3
"""Drift-guard фингерпринта (FINGERPRINT-MANIFEST, §5.2.4).

Падает, если в `crates/**/*.rs` вне разрешённых мест встречается:

  * строковый литерал `"157"` — мажор Chrome должен жить только в
    `crates/core/src/identity.rs` (`chrome_major!`) и
    `crates/network/src/http/chrome_manifest.rs` (`CHROME_157.major`);
  * скопированный Chrome UA — строка с `AppleWebKit/537.36` и `Chrome/<цифра>`
    на одной строке (хардкод версии вместо `chrome_major!()`/манифеста).

Разрешено: `crates/core/src/identity.rs`, `crates/network/src/http/chrome_manifest.rs`,
тесты (`*/tests/*`, `*_tests.rs`, `*/testdata/*`).

    python scripts/check_fingerprint_literals.py

Exit 1 при находках. Только stdlib.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRATES = REPO / "crates"

ALLOWED = {
    "crates/core/src/identity.rs",
    "crates/network/src/http/chrome_manifest.rs",
}

QUOTED_MAJOR = re.compile(r'"157"')
# Chrome UA с зашитой версией: `Chrome/126.0.0.0`, `Chrome/153`, …
# `Chrome/{major}` (шаблон скриптов) и `Chrome/", chrome_major!()` (identity)
# под паттерн не попадают — после `Chrome/` нет цифры.
HARDCODED_UA = re.compile(r"AppleWebKit/537\.36.*Chrome/\d|Chrome/\d.*AppleWebKit/537\.36")


def is_test_path(rel: str) -> bool:
    return (
        "/tests/" in rel
        or rel.endswith("_tests.rs")
        or "/testdata/" in rel
        or "/benches/" in rel
    )


def main() -> int:
    hits: list[str] = []
    for path in sorted(CRATES.rglob("*.rs")):
        rel = path.relative_to(REPO).as_posix()
        if rel in ALLOWED or is_test_path(rel):
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for i, line in enumerate(lines, 1):
            code = line.split("//", 1)[0]  # комментарии не фингерпринт
            if QUOTED_MAJOR.search(code):
                hits.append(f"{rel}:{i}: \"157\" literal — major must come from chrome_major!/manifest")
            if HARDCODED_UA.search(code):
                hits.append(f"{rel}:{i}: hardcoded Chrome UA version — derive from identity/manifest")
    if hits:
        print("fingerprint literal drift:")
        for h in hits:
            print(" ", h)
        return 1
    print("check_fingerprint_literals: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
