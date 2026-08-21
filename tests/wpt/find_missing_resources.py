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
distinct test files reference each. `--ids` adds the unit that actually matters
for the pass-rate — how many *manifest ids* each missing file blocks. The two
counts are not proportional: one `encoding` file carrying 17 `<meta
name="variant">` lines is one file and seventeen ids, so ranking the vendoring
backlog by files under-weights exactly the categories where the backlog is
worst (WPT-RUN-5 slice 30) — the same shape of information
`docs/wpt-status.md` already reports by hand for smaller categories (e.g.
"56 попаданий" for `check-layout-th.js`), generalized into a repeatable tool
instead of manual counting from a run log.
"""

import argparse
import collections
import json
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

#: URLs `wptrunner` serves from a static route rather than from the doc root
#: (`environment.py::get_routes`). They are absent from `tests/wpt/` on purpose
#: and a run does not 404 on them, so counting them as a vendoring gap
#: overstates the backlog — `/resources/testdriver.js` alone is 3 236 ids, a
#: third of the raw total (WPT-RUN-5 slice 30). `/resources/testdriver.js` is
#: assembled from the repo-root `resources/testdriver.js` plus two wptrunner
#: files; see the CLAUDE.md gotcha about why that root file must stay put.
ROUTED_NOT_VENDORED = frozenset({
    "/resources/testharnessreport.js",
    "/resources/testdriver.js",
    "/testharness_runner.html",
    "/print_pdf_runner.html",
    "/_pdf_js/pdf.js",
    "/_pdf_js/pdf.worker.js",
})


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


def count_blocked_ids(files_by_missing: dict, manifest_path=None) -> dict:
    """`{missing_ref: how many automatable manifest ids that file carries}`.

    A test file is one row in the manifest only when it declares no variants;
    `encoding/legacy-mb-korean/euc-kr/euckr-decode-cseuckr.html` declares 17 and
    is therefore 17 ids, every one of which hangs on the same absent helper. The
    pass-rate denominator counts ids, so this is the number a vendoring decision
    should be ranked by (WPT-RUN-5 slice 30 measured 1 582 ids on
    `/common/subset-tests.js` alone against its 156 files).
    """
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import corpus_stats  # noqa: PLC0415 - optional dependency of --ids only

    with open(manifest_path or corpus_stats.DEFAULT_MANIFEST, encoding="utf-8") as fh:
        manifest = json.load(fh)
    ids_by_file = collections.Counter()
    for test_type, _category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        ids_by_file[test_id.split("?")[0]] += 1

    out = {}
    for ref, paths in files_by_missing.items():
        total = 0
        for path in paths:
            rel = "/" + os.path.relpath(path, TESTS_ROOT).replace(os.sep, "/")
            total += ids_by_file.get(rel, 0)
        out[ref] = total
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="css")
    parser.add_argument("--ext", nargs="+", default=list(DEFAULT_EXTS))
    parser.add_argument("--ids", action="store_true",
                        help="also count the manifest ids each missing file blocks")
    parser.add_argument("--manifest", default=None,
                        help="MANIFEST.json for --ids (default: corpus_stats.DEFAULT_MANIFEST)")
    args = parser.parse_args()

    root_dir = os.path.join(TESTS_ROOT, *args.root.split("/"))
    if not os.path.isdir(root_dir):
        print(f"no such root: {root_dir}", file=sys.stderr)
        return 1

    hit_counts = collections.Counter()
    routed = collections.Counter()
    files_by_missing = collections.defaultdict(set)
    scanned = 0
    for path in iter_test_files(root_dir, tuple(args.ext)):
        scanned += 1
        for ref in find_refs(path):
            if ref in ROUTED_NOT_VENDORED:
                routed[ref] += 1
                continue
            on_disk = os.path.join(TESTS_ROOT, ref.lstrip("/"))
            if not os.path.isfile(on_disk):
                hit_counts[ref] += 1
                files_by_missing[ref].add(path)

    print(f"scanned {scanned} files under {args.root}/")
    if routed:
        served = ", ".join(f"{ref} ({n})" for ref, n in routed.most_common())
        print(f"not counted, served by a wptrunner static route: {served}")
    print(f"{len(hit_counts)} distinct missing resource paths\n")
    ids_by_missing = {}
    if args.ids:
        ids_by_missing = count_blocked_ids(files_by_missing, args.manifest)

    for ref, count in hit_counts.most_common():
        line = f"{count:6d}  {ref}  ({len(files_by_missing[ref])} files)"
        if args.ids:
            line += f"  {ids_by_missing.get(ref, 0)} manifest ids"
        print(line)
    if args.ids:
        ranked = sorted(ids_by_missing.items(), key=lambda kv: -kv[1])
        total = sum(ids_by_missing.values())
        print(f"\nby blocked manifest ids ({total} in total, ids can be counted "
              f"under more than one missing file):")
        for ref, n in ranked[:15]:
            print(f"{n:6d}  {ref}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
