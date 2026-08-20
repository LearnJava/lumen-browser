#!/usr/bin/env python3
"""WPT-RUN-5: how much of the corpus number [BUG-786](../../bugs/BUG-786-OPEN.md)
distorts — including the PASSes it *invents*.

`<style><![CDATA[ ... ]]></style>` is the standard idiom of the old CSS2.1
`.xht` references, and Lumen drops the whole rule block (no XML path for
`application/xhtml+xml`). In a reftest that cuts both ways:

* **both sides carry CDATA** — both lose their styling, both render the same
  unstyled document, and the pair scores **PASS** without the engine having
  applied a single declaration under test.
* **only the test carries CDATA** — usually a FAIL, but not always: a large
  family of CSS2.1 tests matches against a generic "there is no red"
  reference, and dropping the stylesheet drops the red along with everything
  else, so those PASS for exactly the wrong reason.

Hence the reported surface is *any* PASS on a test whose inline stylesheet was
dropped whole: the engine provably applied none of the declarations under
test, whatever the verdict says. This script gives the manifest-wide exposure
(how many ids the bug can reach, before any run) and, if a run directory is
given, what the executed tests actually scored, split the same way. Re-run it
after BUG-786 is fixed — those PASSes turn into real verdicts, so the headline
number may legitimately go *down*.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/cdata_audit.py [--out-dir .tmp/wpt-corpus]
        [--manifest PATH] [--json OUT]
"""

import argparse
import collections
import glob
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import corpus_stats  # noqa: E402  (path set above)

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
WPT_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
DEFAULT_MANIFEST = os.path.join(WPT_ROOT, "metadata", "MANIFEST.json")

#: Extensions `wptserve` serves as `application/xhtml+xml`, i.e. the documents
#: that take the XML path upstream and the HTML path in Lumen.
XML_EXTS = frozenset({"xht", "xhtml"})

_STYLE_RE = re.compile(r"<style[^>]*>(.*?)</style>", re.S | re.I)
_MATCH_RE = (re.compile(r"<link[^>]*rel=[\"']match[\"'][^>]*href=[\"']([^\"']+)", re.I),
             re.compile(r"<link[^>]*href=[\"']([^\"']+)[\"'][^>]*rel=[\"']match[\"']", re.I))

_cdata_cache = {}


def is_xml_doc(test_id: str) -> bool:
    """True if the id addresses a document `wptserve` labels as XML."""
    base = test_id.split("?")[0]
    return base.rsplit(".", 1)[-1].lower() in XML_EXTS


def has_style_cdata(path: str):
    """True/False if the file has a CDATA marker inside `<style>`, None if unreadable.

    Deliberately textual: the point is what the *source* looks like, and the
    file cannot be asked its parsed form — that is exactly what the bug breaks.
    """
    if path in _cdata_cache:
        return _cdata_cache[path]
    try:
        with open(os.path.join(WPT_ROOT, path.lstrip("/")), encoding="utf-8",
                  errors="replace") as fh:
            source = fh.read()
    except OSError:
        _cdata_cache[path] = None
        return None
    value = any("CDATA" in m.group(1) for m in _STYLE_RE.finditer(source))
    _cdata_cache[path] = value
    return value


def reference_of(test_id: str):
    """Path of the test's `rel=match` reference, or None if it has none."""
    base = test_id.split("?")[0]
    try:
        with open(os.path.join(WPT_ROOT, base.lstrip("/")), encoding="utf-8",
                  errors="replace") as fh:
            source = fh.read()
    except OSError:
        return None
    for pattern in _MATCH_RE:
        m = pattern.search(source)
        if m:
            joined = os.path.join(os.path.dirname(base), m.group(1))
            return os.path.normpath(joined).replace(os.sep, "/")
    return None


def classify(test_id: str) -> str:
    """One of `plain` / `test_only` / `both_cdata` / `ref_unknown`.

    `plain` covers everything the bug cannot touch (no CDATA in the test), so
    it also absorbs non-XML documents. The other three all mean "the test's
    own inline stylesheet was dropped"; they differ only in what happened to
    the reference on the other side of the comparison.
    """
    base = test_id.split("?")[0]
    if not is_xml_doc(base) or not has_style_cdata(base):
        return "plain"
    ref = reference_of(base)
    if ref is None:
        return "ref_unknown"
    ref_cdata = has_style_cdata(ref)
    if ref_cdata is None:
        return "ref_unknown"
    return "both_cdata" if ref_cdata else "test_only"


def manifest_exposure(manifest: dict) -> dict:
    """How many reftest ids the bug can reach, before any run."""
    counts = collections.Counter()
    for test_type, _category, test_id in corpus_stats.iter_ids(manifest):
        if test_type != "reftest":
            continue
        counts["reftest_total"] += 1
        if not is_xml_doc(test_id):
            continue
        counts["xml"] += 1
        counts[classify(test_id)] += 1
    return dict(counts)


def run_verdicts(out_dir: str) -> dict:
    """PASS/FAIL of executed reftests in `out_dir`, split by CDATA class."""
    by_class = collections.defaultdict(collections.Counter)
    for path in sorted(glob.glob(os.path.join(out_dir, "*.json"))):
        try:
            with open(path, encoding="utf-8") as fh:
                report = json.load(fh)
        except (OSError, json.JSONDecodeError):
            # A shard still being written has no readable report yet; skipping
            # it keeps the audit runnable mid-run, which is when it is useful.
            continue
        for result in report.get("results", []):
            # A reftest reports one verdict and no subtests; testharness tests
            # report OK/ERROR/TIMEOUT with subtests underneath.
            if result.get("subtests") or result["status"] not in ("PASS", "FAIL"):
                continue
            by_class[classify(result["test"])][result["status"]] += 1
    return {k: dict(v) for k, v in by_class.items()}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", default=DEFAULT_MANIFEST)
    parser.add_argument("--out-dir", default=os.path.join(REPO_ROOT, ".tmp", "wpt-corpus"),
                        help="run directory with per-shard wptreport.json files")
    parser.add_argument("--json", default=None, help="write the numbers here as JSON")
    args = parser.parse_args()

    with open(args.manifest, encoding="utf-8") as fh:
        manifest = json.load(fh)

    exposure = manifest_exposure(manifest)
    print("manifest exposure (reftest ids):")
    print(f"  reftests total          {exposure.get('reftest_total', 0)}")
    print(f"  .xht/.xhtml             {exposure.get('xml', 0)}")
    suspect_ids = (exposure.get("test_only", 0) + exposure.get("both_cdata", 0)
                   + exposure.get("ref_unknown", 0))
    print(f"    test CDATA, ref plain {exposure.get('test_only', 0)}")
    print(f"    CDATA on both sides   {exposure.get('both_cdata', 0)}")
    print(f"    no CDATA in test      {exposure.get('plain', 0)}")
    print(f"    reference unresolved  {exposure.get('ref_unknown', 0)}")
    print(f"  -> {suspect_ids} ids run with their inline stylesheet dropped")

    verdicts = {}
    if os.path.isdir(args.out_dir):
        verdicts = run_verdicts(args.out_dir)
        print(f"\nexecuted reftests in {args.out_dir}:")
        for name in ("plain", "test_only", "both_cdata", "ref_unknown"):
            counts = verdicts.get(name)
            if not counts:
                continue
            total = sum(counts.values())
            passed = counts.get("PASS", 0)
            print(f"  {name:<12} executed {total:>6}  PASS {passed:>5} "
                  f"({100.0 * passed / total:.1f} %)  FAIL {counts.get('FAIL', 0):>5}")
        suspect = sum(counts.get("PASS", 0) for name, counts in verdicts.items()
                      if name != "plain")
        every_pass = sum(c.get("PASS", 0) for c in verdicts.values())
        if every_pass:
            print(f"\n  {suspect} of {every_pass} reftest PASSes ({100.0 * suspect / every_pass:.1f} %) "
                  f"scored on a test whose inline stylesheet was dropped whole — an "
                  f"upper bound on the inflation, not a claim that each one is false")
    else:
        print(f"\n(no run directory at {args.out_dir}, manifest exposure only)")

    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"manifest": exposure, "verdicts": verdicts}, fh, indent=2)
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
