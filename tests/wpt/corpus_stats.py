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
manifest, no exemptions** — types with no executor at all (`crashtest`,
`wdspec`, `print-reftest`, `aamtest`; `WPT-RUN-8`) and categories
`docs/wpt-status.md` marks out of scope (media, hardware APIs, ad-tech) all
stay in. Anything not run counts as not passed. The one type
excluded is `support`, which holds fixtures (images, helper scripts), not
tests. `manual`/`visual` are counted but reported on their own line: no
automated runner executes them, upstream included, so folding them into the
headline number would misstate what was measured either way.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/corpus_stats.py [--manifest PATH] [--json OUT]
        [--category NAME ...]
"""

import argparse
import ast
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

#: `browsers/lumen.py`, the single source of truth for which test types Lumen
#: has an executor for at all.
LUMEN_BROWSER_PY = os.path.join(REPO_ROOT, "tools", "wptrunner", "wptrunner",
                                "browsers", "lumen.py")

#: Fallback for `supported_types()` when `lumen.py` cannot be parsed — the set
#: as of 2026-08-20 (`testharness` since WPT-RUN-2, `reftest` since TEST-4).
SUPPORTED_TYPES_FALLBACK = frozenset({"testharness", "reftest"})


def supported_types(path: str = LUMEN_BROWSER_PY) -> frozenset:
    """Test types `browsers/lumen.py` registers an executor for, read from it.

    Hard-coding this list is what made the ceiling in `docs/wpt/pass-rate.md`
    stale: `TEST-4` registered `reftest` on 2026-08-19, the constant here kept
    saying `testharness` only, and every later print understated the ceiling by
    27 386 reftest ids. So the list is parsed out of the `__wptrunner__`
    literal instead — with `ast`, not `import`, because importing that module
    drags in the whole `wptrunner` package.

    Every automatable type NOT in this set is a runner gap, not an engine
    result (`WPT-RUN-8`, `docs/tasks/p2-test-track.md`).
    """
    try:
        tree = ast.parse(open(path, encoding="utf-8").read(), filename=path)
    except (OSError, SyntaxError) as exc:
        print(f"warning: cannot read executors from {path} ({exc}); "
              f"falling back to {sorted(SUPPORTED_TYPES_FALLBACK)}", file=sys.stderr)
        return SUPPORTED_TYPES_FALLBACK

    for node in ast.walk(tree):
        if not isinstance(node, ast.Assign):
            continue
        if not any(isinstance(t, ast.Name) and t.id == "__wptrunner__" for t in node.targets):
            continue
        if not isinstance(node.value, ast.Dict):
            break
        for key, value in zip(node.value.keys, node.value.values):
            if isinstance(key, ast.Constant) and key.value == "executor" and isinstance(value, ast.Dict):
                return frozenset(k.value for k in value.keys if isinstance(k, ast.Constant))
        break

    print(f"warning: no __wptrunner__[\"executor\"] mapping in {path}; "
          f"falling back to {sorted(SUPPORTED_TYPES_FALLBACK)}", file=sys.stderr)
    return SUPPORTED_TYPES_FALLBACK


def iter_entries(manifest: dict):
    """Yield `(test_type, category, test_id, extras)` for every test in the manifest.

    `test_id` is the URL `wptrunner` addresses the test by (leading `/`), which
    is also the id `run_report.py`/`run_smoke.py` take on the command line. A
    manifest leaf is `[hash, entry, ...]` with one entry per generated id; an
    entry's first field is the URL, or `None` when it equals the file path
    (the common case for a plain `.html` test with no variants); its second is
    the per-test metadata dict (`{"timeout": "long"}` and friends), absent for
    the majority of tests and normalised to `{}` here.

    `extras` is what `shard_timeout` budgets a shard's wall-clock from: the
    manifest already states which tests wptrunner will wait 60 s on rather
    than 10, so the budget can be derived instead of tuned (WPT-RUN-5 slice 15).
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
                extras = entry[1] if len(entry) > 1 and isinstance(entry[1], dict) else {}
                yield test_type, category, "/" + url.lstrip("/"), extras


def iter_ids(manifest: dict):
    """`iter_entries` without the per-test metadata — the shape every caller
    that only needs the id set has used since WPT-RUN-4."""
    for test_type, category, test_id, _extras in iter_entries(manifest):
        yield test_type, category, test_id


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
    supported_set = supported_types()
    supported = {t: n for t, n in automatable.items() if t in supported_set}
    return {
        "per_type": totals,
        "total_ids": sum(totals.values()),
        "automatable_ids": sum(automatable.values()),
        "non_automatable_ids": sum(totals[t] for t in NON_AUTOMATABLE_TYPES if t in totals),
        "executable_ids": sum(supported.values()),
        "executable_types": sorted(supported_set),
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
    ceiling = 100.0 * summary["executable_ids"] / summary["automatable_ids"] if summary["automatable_ids"] else 0.0
    print(f"  executable today:    {summary['executable_ids']}   "
          f"({', '.join(summary['executable_types'])} only) -> ceiling {ceiling:.1f}%")

    if args.json_out:
        os.makedirs(os.path.dirname(os.path.abspath(args.json_out)), exist_ok=True)
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump({"summary": summary, "per_category": per_category}, fh, indent=2, sort_keys=True)
        print(f"\nwritten: {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
