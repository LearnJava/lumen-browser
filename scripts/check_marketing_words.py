#!/usr/bin/env python3
"""Red-line lint (ADR-007, roadmap task 9G.2): forbid abuse-marketing vocabulary
in user-facing product copy.

ADR-007 §"Red lines" requires that Lumen never be marketed as a "scraping
browser" / "stealth automation" / "anti-bot bypass" tool. The anti-detection
stack is a *privacy* feature delivered by default, communicated to developers in
technical docs — never in product copy. This check fails CI if forbidden words
appear in the marketing surface (README.md by default).

Scope note: technical docs (lumen-plan.md, docs/decisions/ADR-007*, subsystems/*)
legitimately *discuss* these terms — including to forbid them — and to explain
mechanics like "bypass the cache" / "bypass InlineRun". They are intentionally
NOT scanned. Only product-facing copy is.

Usage:
    python scripts/check_marketing_words.py             # scan marketing copy
    python scripts/check_marketing_words.py --self-test # validate the matcher logic
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Repo root = parent of this script's directory (scripts/ -> repo root).
REPO_ROOT = Path(__file__).resolve().parent.parent

# Files treated as user-facing marketing / product copy. Keep this list tight:
# adding a technical doc here would create false positives (see "Scope note").
MARKETING_FILES = ("README.md",)

# Forbidden marketing words, matched as whole words, case-insensitive.
# Variants like "scrape"/"scraper" are covered by the "scrap" stem boundary check.
FORBIDDEN_WORDS = ("scraping", "scraper", "scrape", "stealth", "bypass")

_WORD_RE = re.compile(
    r"\b(" + "|".join(re.escape(w) for w in FORBIDDEN_WORDS) + r")\b",
    re.IGNORECASE,
)


def find_in_text(text: str) -> list[tuple[int, str]]:
    """Return ``(line_number, matched_word)`` pairs for forbidden words in ``text``."""
    hits: list[tuple[int, str]] = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        for m in _WORD_RE.finditer(line):
            hits.append((lineno, m.group(0)))
    return hits


def scan(root: Path) -> list[str]:
    """Return violation messages for marketing files under ``root`` (empty if clean)."""
    violations: list[str] = []
    for rel_name in MARKETING_FILES:
        path = root / rel_name
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for lineno, word in find_in_text(text):
            violations.append(f"{rel_name}:{lineno}: forbidden marketing word '{word}'")
    return violations


def _self_test() -> int:
    """Validate the matcher; returns process exit code (0 = pass)."""
    flag_text = "Lumen is the best scraping browser with stealth mode to bypass bots."
    pass_text = (
        "Lumen is a private, lightweight browser. It renders pages with its own "
        "engine and protects your privacy by default."
    )
    failures = []
    flagged = {w.lower() for _, w in find_in_text(flag_text)}
    for expected in ("scraping", "stealth", "bypass"):
        if expected not in flagged:
            failures.append(f"expected to flag {expected!r} in marketing-y text")
    if find_in_text(pass_text):
        failures.append(f"unexpected flags in clean text: {find_in_text(pass_text)}")
    # Substring inside a larger word must NOT match (whole-word boundary).
    if find_in_text("The subscriber list is private."):  # 'scribe' != 'scrape'
        failures.append("false positive: matched inside an unrelated word")
    if failures:
        print("check_marketing_words self-test FAILED:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("check_marketing_words self-test passed.")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return _self_test()
    violations = scan(REPO_ROOT)
    if violations:
        print("Forbidden marketing vocabulary detected (ADR-007, task 9G.2):")
        for v in violations:
            print(f"  - {v}")
        print(
            "\nLumen is positioned as a privacy browser. Its clean automation "
            "surface (ADR-006) is documented for developers in technical docs, "
            "not in product copy. See docs/decisions/ADR-007-anti-detection-stack.md."
        )
        return 1
    print("Marketing-words red-lines OK: no forbidden vocabulary.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
