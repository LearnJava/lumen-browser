#!/usr/bin/env python3
"""WPT-RUN-5 slice 20: how much of the corpus is addressed to a hostname this
machine cannot resolve — and what that costs the number and the clock.

WPT is a *multi-origin* test suite. Its server publishes the same tree under a
main host, a set of subdomains (`www`, `www1`, `www2`, plus two IDN ones) and a
second, deliberately unrelated domain (`not-web-platform.test` and its own
subdomains), so that a test can check what happens across an origin boundary.
Upstream that works because the standard setup step (`wpt make-hosts-file`,
appended to `/etc/hosts`) points every one of those names at 127.0.0.1.

Lumen's runner never did that step: `browsers/lumen.py::env_options` pins
`browser_host` to the literal `127.0.0.1` precisely to avoid needing a
machine-wide hosts file (the pilot's scope was same-origin `dom/` tests, where
it is free). At corpus scale it is not free. `wptserve` builds the subdomain
family by *prefixing* the browser host, so with an IP it hands the browser
`www1.127.0.0.1` and `www2.127.0.0.1` — names with no resolution anywhere —
and the alternate family stays `not-web-platform.test`, which resolves only if
a hosts file says so. A test that reaches for another origin therefore does not
fail on the engine's behaviour; it waits for a load that can never start.

What this script measures, in three independent pieces:

* **exposure** — how many automatable manifest ids reference a foreign host at
  all, by type and category. A file-content scan (a test that asks for another
  origin has to name it, or name the helper that does), so it is a *lower*
  bound: an id whose helper names it indirectly is missed.
* **cost** — against a run directory: the verdict mix of those ids versus every
  other id that ran, and the same comparison *inside each category*, which is
  the control that matters (the affected ids are the cross-origin ones, so they
  are harder than average on their own merits — the within-category gap is what
  the unresolvable name adds on top). Plus the wall-clock the extra TIMEOUTs
  cost, priced with `score_audit.wall_clock`'s fitted seconds-per-TIMEOUT.
* **probe** — what the servers actually hand out right now: every configured
  domain resolved through the OS, and the origins the live server substitutes
  into `/common/get-host-info.sub.js`, the helper 138 fetch tests alone read
  their "other origin" from. This is the end-to-end claim, and it needs a
  running corpus (or `serve.py`) to answer.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/host_audit.py [--out-dir .tmp/wpt-corpus]
        [--manifest PATH] [--probe] [--json OUT] [--ids-json PATH]
    <venv>/python tests/wpt/host_audit.py --selftest

Safe to run against a live run's `--out-dir`: it only reads.
"""

import argparse
import collections
import json
import os
import socket
import sys
import tempfile
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import corpus_stats  # noqa: E402  (path set above)
import run_corpus  # noqa: E402
import score_audit  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
WPT_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
CONFIG_PATH = os.path.join(WPT_ROOT, "config.json")

#: Substrings that mean "this file addresses a host other than the one it was
#: loaded from". Deliberately textual: the substitution happens in `wptserve`
#: at serve time, so the *source* is where the intent is visible.
#:
#: * `get-host-info` — `/common/get-host-info.sub.js`, the shared helper that
#:   defines `REMOTE_ORIGIN`/`OTHER_ORIGIN`/`HTTP_NOTSAMESITE_ORIGIN`; a test
#:   that wants another origin almost always gets it from here.
#: * `{{domains[` / `{{hosts[` — the raw `.sub.` substitution a test does when
#:   it builds the URL itself.
#: * the two literal domains — hardcoded by a handful of tests and by the
#:   `.headers`/handler files that back them.
MARKERS = (b"get-host-info", b"{{domains[", b"{{hosts[",
           b"not-web-platform.test", b"www.web-platform.test")

#: Read cap per file. The marker, if present, is in an import/`src`/constant
#: near the top of anything of normal size; the cap only bites on the handful of
#: multi-megabyte generated tests, where reading the whole file would cost far
#: more than the tail could add.
READ_CAP = 512 * 1024


def iter_sources(manifest: dict):
    """`(test_type, category, test_id, file_path)` for every test in the manifest.

    `corpus_stats.iter_entries` drops the file path (it yields the URL, which is
    what a runner needs); the path is what a content scan needs, and one
    manifest leaf can carry several ids that all share it.
    """
    for test_type, tree in manifest.get("items", {}).items():
        if test_type in corpus_stats.NON_TEST_TYPES:
            continue
        stack = [((), tree)]
        while stack:
            path, node = stack.pop()
            if isinstance(node, dict):
                for name, child in node.items():
                    stack.append((path + (name,), child))
                continue
            file_path = "/".join(path)
            category = path[0] if path else ""
            for entry in node[1:]:
                url = entry[0] if entry and entry[0] else file_path
                yield test_type, category, "/" + url.lstrip("/"), file_path


def file_names_foreign_host(abs_path: str) -> bool:
    """True when the file's own text reaches for another host."""
    try:
        with open(abs_path, "rb") as fh:
            head = fh.read(READ_CAP)
    except OSError:
        return False
    return any(marker in head for marker in MARKERS)


def scan(manifest: dict, tests_root: str = WPT_ROOT) -> dict:
    """Classify every automatable manifest id by whether its source names a
    foreign host. One read per *file*, not per id: `.any.js` fans out into up to
    four ids off one source, and `css/` variants into dozens."""
    verdict_by_file = {}
    marked_ids, all_ids = set(), set()
    per_type = collections.Counter()
    marked_type = collections.Counter()
    per_category = collections.Counter()
    marked_category = collections.Counter()
    marked_files = set()

    for test_type, category, test_id, file_path in iter_sources(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        all_ids.add(test_id)
        per_type[test_type] += 1
        per_category[category] += 1
        hit = verdict_by_file.get(file_path)
        if hit is None:
            hit = file_names_foreign_host(os.path.join(tests_root, file_path))
            verdict_by_file[file_path] = hit
        if hit:
            marked_ids.add(test_id)
            marked_type[test_type] += 1
            marked_category[category] += 1
            marked_files.add(file_path)

    return {"ids": sorted(marked_ids), "files": len(marked_files),
            "scanned_files": len(verdict_by_file), "total_ids": len(all_ids),
            "per_type": dict(per_type), "marked_by_type": dict(marked_type),
            "per_category": dict(per_category), "marked_by_category": dict(marked_category)}


def statuses_from_run(out_dir: str) -> dict:
    """`{test_id: (status, subtests_passed, subtests_total)}` for a run directory."""
    results, _recovered, _empty = run_corpus.load_results(out_dir)
    statuses = {}
    for test_id, result in results.items():
        subtests = result.get("subtests") or []
        statuses[test_id] = (result.get("status", ""),
                             sum(1 for s in subtests if s.get("status") == "PASS"),
                             len(subtests))
    return statuses


def _score(status: str, passed: int, total: int) -> float:
    """The scorer's rule, restated: subtest fraction when there are subtests,
    else 1 for a PASS. Kept in step with `run_corpus.score_reports` by hand —
    importing it is not possible without re-reading every report."""
    return (passed / total) if total else (1.0 if status == "PASS" else 0.0)


def compare(marked: set, statuses: dict, categories: dict) -> dict:
    """Verdict mix of the marked ids against every other id with a verdict,
    overall and per category."""
    groups = {"foreign": collections.Counter(), "rest": collections.Counter()}
    score = {"foreign": [0.0, 0], "rest": [0.0, 0]}
    rows = collections.defaultdict(lambda: {"foreign": [0, 0, 0.0], "rest": [0, 0, 0.0]})

    for test_id, (status, passed, total) in statuses.items():
        key = "foreign" if test_id in marked else "rest"
        groups[key][status] += 1
        value = _score(status, passed, total)
        score[key][0] += value
        score[key][1] += 1
        category = categories.get(test_id)
        if category is not None:
            row = rows[category][key]
            row[0] += 1
            row[1] += status == "TIMEOUT"
            row[2] += value

    def summarize(key):
        counts = groups[key]
        ran = sum(counts.values())
        return {"ran": ran, "statuses": dict(counts),
                "timeout": counts.get("TIMEOUT", 0),
                "timeout_share": round(counts.get("TIMEOUT", 0) / ran, 4) if ran else 0.0,
                "score": round(score[key][0], 2),
                "pass_rate": round(score[key][0] / ran, 4) if ran else 0.0}

    return {"foreign": summarize("foreign"), "rest": summarize("rest"),
            "by_category": {c: v for c, v in rows.items() if v["foreign"][0]},
            "no_verdict": len(marked - set(statuses))}


def configured_domains() -> dict:
    """Every hostname `wptserve` will publish, straight from its own config
    builder — the point is what the *server* believes, and hardcoding the
    subdomain list here would be a second copy of it that can drift."""
    sys.path.insert(0, os.path.join(REPO_ROOT, "tools"))
    import logging

    from serve.serve import ConfigBuilder  # noqa: PLC0415  (optional dependency)

    with open(CONFIG_PATH, encoding="utf-8") as fh:
        override = json.load(fh)
    builder = ConfigBuilder(logging.getLogger("host_audit"),
                            browser_host="127.0.0.1", bind_address=True,
                            ports=override["ports"])
    with builder as config:
        return {family: dict(domains) for family, domains in config.all_domains.items()}


def resolves(hostname: str) -> bool:
    try:
        socket.getaddrinfo(hostname, None)
        return True
    except socket.gaierror:
        return False


#: Where `/common/get-host-info.sub.js` states each host. Read off the served
#: file rather than the on-disk one: the substitution is what this checks.
#: The `*_HOST` constants are the ones carrying a literal — the `*_ORIGIN`
#: fields further down the file are built from them at run time.
HOST_INFO_URL = "/common/get-host-info.sub.js"
HOST_KEYS = ("ORIGINAL_HOST", "REMOTE_HOST", "OTHER_HOST",
             "NOTSAMESITE_HOST", "OTHER_NOTSAMESITE_HOST")


def _quoted_hosts(line: str) -> list:
    """Hostnames a `var X_HOST = …` line states literally.

    Several of them are a ternary on `ORIGINAL_HOST` (`'localhost' ? … : …`),
    so one line can name two candidates plus a subdomain prefix (`'www1.' +
    ORIGINAL_HOST`). A trailing dot marks that prefix form and the caller
    completes it, which is what the served file itself does at run time.
    """
    pieces, rest = [], line
    while "'" in rest:
        _before, _sep, rest = rest.partition("'")
        value, _sep, rest = rest.partition("'")
        if value and value != "localhost":
            pieces.append(value)
    return pieces


def probe(timeout: float = 5.0) -> dict:
    """Resolve every configured domain, and ask a running server what origins it
    hands the browser."""
    domains = configured_domains()
    unresolved = {family: sorted(h for h in hosts.values() if not resolves(h))
                  for family, hosts in domains.items()}
    total = sum(len(h) for h in domains.values())
    result = {"domains": total,
              "unresolvable": sum(len(v) for v in unresolved.values()),
              "unresolvable_by_family": {k: len(v) for k, v in unresolved.items()},
              "examples": {k: v[:4] for k, v in unresolved.items()}}

    with open(CONFIG_PATH, encoding="utf-8") as fh:
        port = json.load(fh)["ports"]["http"][0]
    url = f"http://127.0.0.1:{port}{HOST_INFO_URL}"
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:  # noqa: S310
            served = response.read().decode("utf-8", "replace")
    except OSError as exc:
        result["served"] = {"error": f"{url}: {exc}"}
        return result

    origins, pieces = {}, set()
    for raw in served.splitlines():
        line = raw.strip().removeprefix("var ").strip()
        for key in HOST_KEYS:
            if line.startswith(key) and "'" in line:
                origins.setdefault(key, raw.strip())
                pieces.update(_quoted_hosts(line))
    original = _quoted_hosts(origins.get("ORIGINAL_HOST", ""))
    browser_host = original[0] if original else "127.0.0.1"
    hosts = {(p + browser_host if p.endswith(".") else p) for p in pieces}
    result["served"] = {"url": url, "lines": origins,
                        "hosts": {h: resolves(h) for h in sorted(hosts)}}
    return result


def print_report(scan_result: dict, cost: dict, clock: dict) -> None:
    marked = len(scan_result["ids"])
    print(f"exposure: {marked} of {scan_result['total_ids']} automatable ids "
          f"({marked / max(scan_result['total_ids'], 1) * 100:.1f}%) name a foreign host, "
          f"in {scan_result['files']} of {scan_result['scanned_files']} source files")
    for test_type, n in sorted(scan_result["marked_by_type"].items(), key=lambda kv: -kv[1]):
        print(f"  {test_type:14} {n:6} of {scan_result['per_type'][test_type]}")
    print("  top categories: " + ", ".join(
        f"{c}={n}/{scan_result['per_category'][c]}"
        for c, n in sorted(scan_result["marked_by_category"].items(),
                           key=lambda kv: -kv[1])[:8]))
    if not cost:
        return

    foreign, rest = cost["foreign"], cost["rest"]
    print(f"\ncost: of the {foreign['ran']} marked ids with a verdict, "
          f"{foreign['timeout_share'] * 100:.1f}% TIMEOUT and they score "
          f"{foreign['pass_rate'] * 100:.2f}%; the other {rest['ran']} ids "
          f"{rest['timeout_share'] * 100:.1f}% / {rest['pass_rate'] * 100:.2f}%")
    print(f"  ({cost['no_verdict']} marked ids have no verdict in this run yet)")
    print(f"\n{'category':28} {'marked':>6} {'TO%':>6} {'score%':>7} | "
          f"{'rest':>7} {'TO%':>6} {'score%':>7}")
    for category, row in sorted(cost["by_category"].items(), key=lambda kv: -kv[1]["foreign"][0]):
        f, r = row["foreign"], row["rest"]
        if f[0] < 15:
            continue
        print(f"{category:28} {f[0]:6} {f[1] / f[0] * 100:5.1f}% {f[2] / f[0] * 100:6.1f}% | "
              f"{r[0]:7} {(r[1] / r[0] * 100 if r[0] else 0):5.1f}% "
              f"{(r[2] / r[0] * 100 if r[0] else 0):6.1f}%")

    per_timeout = clock.get("seconds_per_timeout")
    if per_timeout:
        excess = foreign["timeout"] - foreign["ran"] * rest["timeout_share"]
        print(f"\nclock: {foreign['timeout']} of these ids timed out, "
              f"{excess:.0f} more than the rest of the corpus times out at; at the run's "
              f"fitted {per_timeout} s per TIMEOUT that is {excess * per_timeout / 3600:.1f} h "
              f"of wall-clock already spent, "
              f"{len(scan_result['ids']) * (foreign['timeout_share'] - rest['timeout_share']) * per_timeout / 3600:.1f} h "
              f"over the whole corpus")


def selftest() -> int:
    """Assert the scan and the comparison on a synthetic tree, so a change to
    either can be checked without a corpus run."""
    with tempfile.TemporaryDirectory() as root:
        os.makedirs(os.path.join(root, "alpha"))
        os.makedirs(os.path.join(root, "beta"))
        with open(os.path.join(root, "alpha", "cross.html"), "w", encoding="utf-8") as fh:
            fh.write("<script src='/common/get-host-info.sub.js'></script>")
        with open(os.path.join(root, "alpha", "plain.html"), "w", encoding="utf-8") as fh:
            fh.write("<script>assert_true(true)</script>")
        with open(os.path.join(root, "beta", "sub.any.js"), "w", encoding="utf-8") as fh:
            fh.write("fetch('http://{{domains[www1]}}:1/x')")
        with open(os.path.join(root, "beta", "manual.html"), "w", encoding="utf-8") as fh:
            fh.write("not-web-platform.test")
        manifest = {"items": {
            "testharness": {
                "alpha": {"cross.html": ["h", [None, {}]], "plain.html": ["h", [None, {}]]},
                "beta": {"sub.any.js": ["h", ["beta/sub.any.html", {}],
                                        ["beta/sub.any.worker.html", {}]]},
            },
            "manual": {"beta": {"manual.html": ["h", [None, {}]]}},
        }}
        result = scan(manifest, tests_root=root)
        marked = set(result["ids"])
        assert marked == {"/alpha/cross.html", "/beta/sub.any.html",
                          "/beta/sub.any.worker.html"}, marked
        # One source, two ids: both marked, one file read.
        assert result["files"] == 2, result["files"]
        assert result["scanned_files"] == 3, result["scanned_files"]
        # A `manual` id is not automatable and must not reach the denominator,
        # however loudly its text names the alternate domain.
        assert result["total_ids"] == 4, result["total_ids"]
        assert result["marked_by_type"] == {"testharness": 3}, result["marked_by_type"]

        statuses = {"/alpha/cross.html": ("TIMEOUT", 0, 0),
                    "/beta/sub.any.html": ("TIMEOUT", 0, 0),
                    "/alpha/plain.html": ("OK", 3, 4),
                    "/beta/other.html": ("PASS", 0, 0)}
        categories = {"/alpha/cross.html": "alpha", "/beta/sub.any.html": "beta",
                      "/alpha/plain.html": "alpha", "/beta/other.html": "beta"}
        cost = compare(marked, statuses, categories)
        assert cost["foreign"]["ran"] == 2, cost["foreign"]
        assert cost["foreign"]["timeout_share"] == 1.0, cost["foreign"]
        assert cost["foreign"]["pass_rate"] == 0.0, cost["foreign"]
        assert cost["rest"]["ran"] == 2, cost["rest"]
        assert cost["rest"]["timeout_share"] == 0.0, cost["rest"]
        # 0.75 of a subtest-scored OK plus a bare PASS over two ids.
        assert cost["rest"]["pass_rate"] == 0.875, cost["rest"]
        # The third marked id never ran: it is a gap in the run, not a verdict.
        assert cost["no_verdict"] == 1, cost["no_verdict"]
        assert cost["by_category"]["alpha"]["foreign"][0] == 1, cost["by_category"]
        assert cost["by_category"]["alpha"]["rest"][0] == 1, cost["by_category"]
    print("selftest: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out-dir", default=".tmp/wpt-corpus",
                        help="run directory to price the exposure against ('' to skip)")
    parser.add_argument("--manifest", default=None, help="MANIFEST.json (default: vendored)")
    parser.add_argument("--probe", action="store_true",
                        help="resolve every configured domain and ask a running server "
                             "which origins it hands out")
    parser.add_argument("--ids-json", default=None,
                        help="write the marked id list here (for a follow-up run)")
    parser.add_argument("--json", dest="json_out", default=None, help="write the full result here")
    parser.add_argument("--selftest", action="store_true", help="run the built-in assertions")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if args.manifest:
        run_corpus.MANIFEST_PATH = args.manifest
    manifest = run_corpus.load_manifest()
    started = time.time()
    scan_result = scan(manifest)
    print(f"scanned {scan_result['scanned_files']} source files in "
          f"{time.time() - started:.0f}s", flush=True)

    cost, clock = {}, {}
    if args.out_dir and os.path.isdir(args.out_dir):
        categories = {test_id: category
                      for _t, category, test_id in corpus_stats.iter_ids(manifest)}
        statuses = statuses_from_run(args.out_dir)
        cost = compare(set(scan_result["ids"]), statuses, categories)
        state_path = os.path.join(args.out_dir, "state.json")
        if os.path.isfile(state_path):
            with open(state_path, encoding="utf-8") as fh:
                clock = score_audit.wall_clock(json.load(fh), args.out_dir)

    print_report(scan_result, cost, clock)

    result = {"exposure": {k: v for k, v in scan_result.items() if k != "ids"},
              "cost": cost, "clock": clock}
    if args.probe:
        result["probe"] = probe()
        served = result["probe"].get("served", {})
        print(f"\nprobe: {result['probe']['unresolvable']} of {result['probe']['domains']} "
              f"configured domains do not resolve on this machine "
              f"({result['probe']['unresolvable_by_family']})")
        if "error" in served:
            print(f"  no server answering: {served['error']}")
        else:
            for key, line in sorted(served.get("lines", {}).items()):
                print(f"  {line}")
            for host, ok in sorted(served.get("hosts", {}).items()):
                print(f"  {host:32} {'resolves' if ok else 'NO RESOLUTION'}")

    if args.ids_json:
        with open(args.ids_json, "w", encoding="utf-8") as fh:
            json.dump(scan_result["ids"], fh)
        print(f"\nwritten: {args.ids_json}")
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump(result, fh, indent=1, ensure_ascii=False)
        print(f"written: {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
