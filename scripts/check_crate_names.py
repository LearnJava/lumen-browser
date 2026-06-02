#!/usr/bin/env python3
"""Red-line lint (ADR-006/007, roadmap task 9G.1): forbid crate names that
imply abuse-tooling.

Lumen is a privacy browser with a clean automation surface (ADR-006), explicitly
**not** an "anti-bot"/"scraping" product (ADR-007 §"Red lines"). Crate names like
``captcha-solver``, ``ip-rotation`` or ``proxy-pool`` would signal exactly the
SaaS-tier abuse capabilities the project rules out. This check fails CI if any
workspace crate (package name or its directory) matches a forbidden pattern.

Usage:
    python scripts/check_crate_names.py            # scan the repo, exit 1 on violation
    python scripts/check_crate_names.py --self-test # validate the matcher logic

Matching rules (case-insensitive, after normalising '-' to '_'):
    * single forbidden segments: ``captcha``, ``solver`` — matched as a whole
      token bounded by start/end or an '_' separator, so legitimate names such as
      ``lumen-resolver`` (contains the substring "solver") are NOT flagged.
    * forbidden multi-word substrings: ``ip_rotation``, ``proxy_pool``.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Repo root = parent of this script's directory (scripts/ -> repo root).
REPO_ROOT = Path(__file__).resolve().parent.parent

# Forbidden whole-token segments (after '-' -> '_' normalisation, split on '_').
FORBIDDEN_TOKENS = {"captcha", "solver"}

# Forbidden multi-word substrings checked against the normalised name.
FORBIDDEN_SUBSTRINGS = ("ip_rotation", "proxy_pool")

# Directories never scanned for Cargo.toml (build output, sibling worktrees).
SKIP_DIRS = {"target", ".git"}

_PACKAGE_NAME_RE = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.MULTILINE)


def violation_reason(name: str) -> str | None:
    """Return a human-readable reason if ``name`` is a forbidden crate name, else None.

    ``name`` is any crate identifier (package ``name`` or directory name).
    """
    normalised = name.strip().lower().replace("-", "_")
    tokens = set(normalised.split("_"))
    hit = tokens & FORBIDDEN_TOKENS
    if hit:
        return f"contains forbidden token {sorted(hit)!r}"
    for sub in FORBIDDEN_SUBSTRINGS:
        if sub in normalised:
            return f"contains forbidden substring {sub!r}"
    return None


def _package_name(cargo_toml: Path) -> str | None:
    """Extract the ``[package] name = "..."`` value from a Cargo.toml, if present."""
    text = cargo_toml.read_text(encoding="utf-8", errors="replace")
    # Only consider the name inside the [package] table; virtual manifests have none.
    pkg_idx = text.find("[package]")
    if pkg_idx == -1:
        return None
    match = _PACKAGE_NAME_RE.search(text, pkg_idx)
    return match.group(1) if match else None


def _iter_cargo_tomls(root: Path):
    """Yield every Cargo.toml under ``root``, skipping build/worktree dirs."""
    for path in root.rglob("Cargo.toml"):
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        yield path


def scan(root: Path) -> list[str]:
    """Return a list of violation messages for crates under ``root`` (empty if clean)."""
    violations: list[str] = []
    for cargo_toml in _iter_cargo_tomls(root):
        rel = cargo_toml.relative_to(root)
        crate_dir = cargo_toml.parent.name

        reason = violation_reason(crate_dir)
        if reason:
            violations.append(f"{rel}: directory name '{crate_dir}' {reason}")

        name = _package_name(cargo_toml)
        if name:
            reason = violation_reason(name)
            if reason:
                violations.append(f"{rel}: package name '{name}' {reason}")
    return violations


def _self_test() -> int:
    """Validate the matcher; returns process exit code (0 = pass)."""
    must_flag = [
        "captcha",
        "captcha-solver",
        "lumen-captcha",
        "anti-bot-solver",
        "ip-rotation",
        "ip_rotation_pool",
        "proxy-pool",
        "proxy_pool_manager",
    ]
    must_pass = [
        "lumen",
        "lumen-network",
        "lumen-resolver",  # substring "solver" must NOT trip the token matcher
        "lumen-dns-resolver",
        "lumen-js",
        "lumen-proxy",  # "proxy" alone is fine; only "proxy_pool" is forbidden
    ]
    failures = []
    for name in must_flag:
        if violation_reason(name) is None:
            failures.append(f"expected FLAG but passed: {name!r}")
    for name in must_pass:
        reason = violation_reason(name)
        if reason is not None:
            failures.append(f"expected PASS but flagged: {name!r} ({reason})")
    if failures:
        print("check_crate_names self-test FAILED:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("check_crate_names self-test passed.")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return _self_test()
    violations = scan(REPO_ROOT)
    if violations:
        print("Forbidden crate names detected (ADR-006/007, task 9G.1):")
        for v in violations:
            print(f"  - {v}")
        print(
            "\nLumen is a privacy browser, not an abuse-tooling product. "
            "Rename the crate; see docs/decisions/ADR-007-anti-detection-stack.md."
        )
        return 1
    print("Crate-name red-lines OK: no forbidden names.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
