#!/usr/bin/env python3
"""Static 404 finder for a vendored WPT category (WPT-RUN-1, `ROADMAP.md`).

`run_report.py --all --root css --recursive` cannot finish (34607 id,
~a day at the observed rate — see `docs/wpt-status.md` row `css`), so a live
run cannot enumerate every out-of-category helper a `css/` test 404s on the
way `docs/wpt-status.md` did for smaller categories (FileAPI, IndexedDB, ...).

This script gets the same answer — which absolute-path resources referenced
by test files don't exist in the vendored tree — without executing a single
test: it greps every test file under `<root>` for `src=`/`href=`/`url(...)`
references that start with `/` (site-absolute, i.e. potentially out of
category) and checks each one against disk. Static text scanning of vendored,
unmodified upstream source is exact for literal paths (the overwhelming
majority) and only misses paths built via runtime string concatenation, which
this style of test essentially never does — `check-layout-th.js` and
`ahem.css` are both referenced as plain literal `src=`/`href=`.

Usage:

    python tests/wpt/find_missing_resources.py [--root css] [--ext .html .htm .js]

Prints missing paths sorted by hit count (most-referenced first) and how many
distinct test files reference each — the same shape of information
`docs/wpt-status.md` already reports by hand for smaller categories (e.g.
"56 попаданий" for `check-layout-th.js`), generalized into a repeatable tool
instead of manual counting from a run log.
"""

import argparse
import collections
import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")

# src="/..." / href="/..." / url(/...) — the three attribute/CSS-function
# shapes that carry a resource reference in these test files. Quotes are
# optional for url(...) per CSS syntax.
REF_RE = re.compile(
    r"""(?:src|href)\s*=\s*["']?(/[^"'\s>]+)"""
    r"""|url\(\s*["']?(/[^"'\)\s]+)"""
)

DEFAULT_EXTS = (".html", ".htm", ".xht", ".xhtml", ".js")


def iter_test_files(root_dir: str, exts: tuple) -> "list[str]":
    for dirpath, dirnames, filenames in os.walk(root_dir):
        for fn in filenames:
            if fn.endswith(exts):
                yield os.path.join(dirpath, fn)


def find_refs(path: str) -> "set[str]":
    try:
        with open(path, encoding="utf-8", errors="ignore") as f:
            text = f.read()
    except OSError:
        return set()
    refs = set()
    for m in REF_RE.finditer(text):
        ref = m.group(1) or m.group(2)
        # Strip query/fragment; wptserve pipe substitutions (?pipe=sub etc.)
        # don't change which file on disk is served.
        ref = ref.split("#", 1)[0].split("?", 1)[0]
        if "{{" in ref:
            # wptserve .sub. template variable — not a literal path.
            continue
        refs.add(ref)
    return refs


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="css")
    parser.add_argument("--ext", nargs="+", default=list(DEFAULT_EXTS))
    args = parser.parse_args()

    root_dir = os.path.join(TESTS_ROOT, *args.root.split("/"))
    if not os.path.isdir(root_dir):
        print(f"no such root: {root_dir}", file=sys.stderr)
        return 1

    hit_counts = collections.Counter()
    files_by_missing = collections.defaultdict(set)
    scanned = 0
    for path in iter_test_files(root_dir, tuple(args.ext)):
        scanned += 1
        for ref in find_refs(path):
            on_disk = os.path.join(TESTS_ROOT, ref.lstrip("/"))
            if not os.path.isfile(on_disk):
                hit_counts[ref] += 1
                files_by_missing[ref].add(path)

    print(f"scanned {scanned} files under {args.root}/")
    print(f"{len(hit_counts)} distinct missing resource paths\n")
    for ref, count in hit_counts.most_common():
        print(f"{count:6d}  {ref}  ({len(files_by_missing[ref])} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
