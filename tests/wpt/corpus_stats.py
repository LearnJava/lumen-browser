#!/usr/bin/env python3
"""WPT-RUN-4 (`docs/tasks/p2-wpt-runner-throughput.md`): the *denominator* of
Lumen's WPT pass-rate, read straight from `MANIFEST.json`.

Why this exists: every earlier count in this repo came from
`run_report.py::all_vendored_test_ids()`, which globs `*.html` off the file
system. That number is not comparable to anyone else's — it over-counts
(reftest halves and `-ref.html` files are files, not runnable ids) and
under-counts (a single file can expand into several ids via `?variant`
query strings and `.any.js` -> `.any.html`/`.any.worker.html`). `MANIFEST.json`
is what `wptrunner` itself selects tests from, and what wpt.fyi/Servo/Ladybird
report against, so it is the only defensible denominator.

Scope decision (user, 2026-08-18): the denominator is the **whole vendored
manifest, no exemptions** — reftests we cannot execute yet (`TEST-4`) and
categories `docs/wpt-status.md` marks out of scope (media, hardware APIs,
ad-tech) all stay in. Anything not run counts as not passed. The one type
excluded is `support`, which holds fixtures (images, helper scripts), not
tests. `manual`/`visual` are counted but reported on their own line: no
automated runner executes them, upstream included, so folding them into the
headline number would misstate what was measured either way.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/corpus_stats.py [--manifest PATH] [--json OUT]
        [--category NAME ...]
"""

import argparse
import json
import os
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_MANIFEST = os.path.join(REPO_ROOT, "tests", "wpt", "metadata", "MANIFEST.json")

#: Manifest item types that hold fixtures rather than tests — never part of any
#: denominator.
NON_TEST_TYPES = frozenset({"support"})

#: Test types no automated runner executes (upstream `wptrunner` included):
#: counted and reported, but kept out of the headline automatable denominator.
NON_AUTOMATABLE_TYPES = frozenset({"manual", "visual"})

#: Test types `browsers/lumen.py` currently registers an executor for. Every
#: other automatable type is a runner gap, not an engine result — see `TEST-4`
#: (reftest executor) and `docs/tasks/p2-test-track.md`.
SUPPORTED_TYPES = frozenset({"testharness"})


def iter_ids(manifest: dict):
    """Yield `(test_type, category, test_id)` for every test in the manifest.

    `test_id` is the URL `wptrunner` addresses the test by (leading `/`), which
    is also the id `run_report.py`/`run_smoke.py` take on the command line. A
    manifest leaf is `[hash, entry, ...]` with one entry per generated id; an
    entry's first field is the URL, or `None` when it equals the file path
    (the common case for a plain `.html` test with no variants).
    """
    for test_type, tree in manifest.get("items", {}).items():
        if test_type in NON_TEST_TYPES:
            continue
        stack = [((), tree)]
        while stack:
            path, node = stack.pop()
            if isinstance(node, dict):
                for name, child in node.items():
                    stack.append((path + (name,), child))
                continue
            # Leaf: [hash, entry, entry, ...]
            file_path = "/".join(path)
            category = path[0] if path else ""
            for entry in node[1:]:
                url = entry[0] if entry and entry[0] else file_path
                yield test_type, category, "/" + url.lstrip("/")


def collect(manifest: dict) -> dict:
    """Aggregate `iter_ids` into `{category: {type: id_count}}` plus file counts."""
    per_category = {}
    for test_type, category, _ in iter_ids(manifest):
        per_category.setdefault(category, {}).setdefault(test_type, 0)
        per_category[category][test_type] += 1
    return per_category


def summarize(per_category: dict) -> dict:
    """Roll `collect`'s table up into the numbers the journal quotes."""
    totals = {}
    for types in per_category.values():
        for test_type, count in types.items():
            totals[test_type] = totals.get(test_type, 0) + count

    automatable = {t: n for t, n in totals.items() if t not in NON_AUTOMATABLE_TYPES}
    supported = {t: n for t, n in automatable.items() if t in SUPPORTED_TYPES}
    return {
        "per_type": totals,
        "total_ids": sum(totals.values()),
        "automatable_ids": sum(automatable.values()),
        "non_automatable_ids": sum(totals[t] for t in NON_AUTOMATABLE_TYPES if t in totals),
        "executable_ids": sum(supported.values()),
        "categories": len(per_category),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", default=DEFAULT_MANIFEST)
    parser.add_argument("--json", dest="json_out", default=None, help="also write the full table as JSON")
    parser.add_argument("--category", action="append", default=None, help="restrict output to these top-level categories (repeatable)")
    args = parser.parse_args()

    if not os.path.isfile(args.manifest):
        print(f"manifest not found: {args.manifest}\n"
              f"generate it with any run_report.py run (wptrunner updates it on start)",
              file=sys.stderr)
        return 1

    with open(args.manifest, encoding="utf-8") as fh:
        manifest = json.load(fh)

    per_category = collect(manifest)
    if args.category:
        wanted = set(args.category)
        per_category = {k: v for k, v in per_category.items() if k in wanted}
    summary = summarize(per_category)

    types = sorted(summary["per_type"], key=lambda t: -summary["per_type"][t])
    print(f"{'category':<34}{'total':>8}" + "".join(f"{t:>16}" for t in types))
    for category in sorted(per_category, key=lambda c: -sum(per_category[c].values())):
        row = per_category[category]
        print(f"{category:<34}{sum(row.values()):>8}" + "".join(f"{row.get(t, 0):>16}" for t in types))

    print()
    print(f"categories:            {summary['categories']}")
    print(f"total ids:             {summary['total_ids']}")
    print(f"  automatable:         {summary['automatable_ids']}   <- pass-rate denominator")
    print(f"  manual/visual:       {summary['non_automatable_ids']}   (no runner executes these)")
    print(f"  executable today:    {summary['executable_ids']}   ({', '.join(sorted(SUPPORTED_TYPES))} only)")

    if args.json_out:
        os.makedirs(os.path.dirname(os.path.abspath(args.json_out)), exist_ok=True)
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump({"summary": summary, "per_category": per_category}, fh, indent=2, sort_keys=True)
        print(f"\nwritten: {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
