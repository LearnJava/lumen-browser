#!/usr/bin/env python3
"""WPT-RUN-5 slice 22 (`docs/tasks/p2-wpt-runner-throughput.md`): check that a
*running* `wptserve` hands out what this checkout would hand out.

Why this exists. Slice 18 (`tests/wpt/port_guard.py`) found that every shard of
the Linux corpus half was served by an orphaned server started 4.5 minutes
before the run, and closed with an admission: "this run is trustworthy only
because the run that died had identical options — nothing in the report says
so, and nothing checked". This module is that check.

What can differ between an orphan's answers and a self-started server's, and
why the difference is invisible in a `wptreport.json`:

* **static routes are frozen.** `TestEnvironment.get_routes`
  (`tools/wptrunner/wptrunner/environment.py`) registers six URLs as
  `StaticHandler`s — `/resources/testharnessreport.js`,
  `/resources/testdriver.js`, `/testharness_runner.html`,
  `/print_pdf_runner.html` and the two `/_pdf_js/*` scripts. A `StaticHandler`
  reads its file(s) on the first request and caches the bytes for the lifetime
  of the server process, so an *edit* to `tests/wpt/resources/
  testharnessreport.js` does not reach an in-flight run at all: the page keeps
  getting the version the squatter read yesterday. Nothing errors;
* **their substitutions come from the dead run.** The report script is a
  template: `output`, `timeout_multiplier`, `explicit_timeout` and `debug` are
  `%(...)s` placeholders filled in from the *options of the run that built the
  handler*. A squatter started by, say, `run_report.py --timeout-multiplier 3`
  silently gives every test of the live run a 3x harness timeout;
* **file-backed test content is not frozen** — it is read per request off the
  doc root, so the tests themselves are the live tree's. That half is good
  news and is also worth proving rather than assuming, because a squatter left
  by *another worktree* would be serving that worktree's files.

So the audit answers three questions, and it is the answer to all three
together that makes an orphan-served run publishable:

1. who holds the configured ports (`port_guard.survey`, reused verbatim);
2. is the doc root this checkout — probed by creating a file, fetching it and
   deleting it again, not by comparing paths;
3. does each static route match what this checkout would build for it — the
   same concatenation `get_routes` does, substituted with the option values
   this repo's runners actually pass.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/route_audit.py              # audit the live server
    <venv>/python tests/wpt/route_audit.py --json OUT
    <venv>/python tests/wpt/route_audit.py --selftest   # prove the checks bite
"""

import argparse
import ast
import json
import os
import sys
import tempfile
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import port_guard  # noqa: E402  (same directory, no package)

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
WPTRUNNER_ROOT = os.path.join(REPO_ROOT, "tools", "wptrunner", "wptrunner")
BROWSER_PY = os.path.join(WPTRUNNER_ROOT, "browsers", "lumen.py")

#: The static routes `environment.py::get_routes` registers, as
#: `(route, [paths relative to WPTRUNNER_ROOT], substituted)`. `substituted` is
#: whether `get_routes` passes non-empty `format_args` for it — only the report
#: script carries placeholders. `/resources/testharnessreport.js` is listed
#: with no paths because they come from `browsers/lumen.py::env_options`
#: (`testharnessreport` override); everything else is fixed upstream.
STATIC_ROUTES = [
    ("/resources/testharnessreport.js", None, True),
    ("/resources/testdriver.js",
     [os.path.join(REPO_ROOT, "resources", "testdriver.js"),
      "executors/message-queue.js",
      "testdriver-extra.js"], False),
    ("/testharness_runner.html", ["testharness_runner.html"], False),
    ("/print_pdf_runner.html", ["print_pdf_runner.html"], False),
    ("/_pdf_js/pdf.js", ["../third_party/pdf_js/pdf.js"], False),
    ("/_pdf_js/pdf.worker.js", ["../third_party/pdf_js/pdf.worker.js"], False),
]

#: What `get_routes` substitutes into the report template when the run sets no
#: relevant option: `output` is `pause_after_test` (`%(output)d` -> 0),
#: `timeout_multiplier` is wptrunner's default 1, `explicit_timeout` is true
#: only under a debugger, `debug` only under `--debug-test`.
DEFAULT_FORMAT_ARGS = {
    "output": 0,
    "timeout_multiplier": 1,
    "explicit_timeout": "false",
    "debug": "false",
}

#: Command-line flags that would move any value in `DEFAULT_FORMAT_ARGS`. If a
#: runner in this repo grows one of these, the defaults above stop being what
#: a self-started server would substitute and this audit must be told so.
OPTION_FLAGS = ("--timeout-multiplier", "--pause-after-test", "--debug-test",
                "--debugger", "--debug-info")

#: Runners whose argv is scanned for `OPTION_FLAGS`.
RUNNER_SOURCES = ("run_corpus.py", "run_smoke.py", "run_report.py", "run_suite.py")

#: Directory the doc-root probe writes into — served at `/resources/` and
#: already full of similar helper scripts, so a stray file is harmless.
PROBE_DIR = os.path.join(TESTS_ROOT, "resources")


def fetch(url: str, timeout: float = 15.0):
    """GET `url`, returning `(status, body_bytes)`; `(None, b"")` if unreachable."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read()
    except OSError:
        return None, b""


def lumen_testharnessreport(path: str = BROWSER_PY) -> list:
    """Paths `browsers/lumen.py::env_options` overrides the report route with.

    Read with `ast`, not `import`, for the reason `corpus_stats.supported_types`
    gives: importing that module drags in the whole `wptrunner` package. The
    value is a list of module-level constants, each assigned an
    `os.path.abspath(os.path.join(os.path.dirname(__file__), ...))` chain of
    string literals — resolved here by walking that chain rather than
    hard-coding the answer, so moving the file in `lumen.py` moves it here too.
    """
    tree = ast.parse(open(path, encoding="utf-8").read(), filename=path)
    consts = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and \
                isinstance(node.targets[0], ast.Name):
            resolved = _resolve_path_expr(node.value, path)
            if resolved:
                consts[node.targets[0].id] = resolved

    for node in ast.walk(tree):
        if not (isinstance(node, ast.FunctionDef) and node.name == "env_options"):
            continue
        for sub in ast.walk(node):
            if not isinstance(sub, ast.Dict):
                continue
            for key, value in zip(sub.keys, sub.values):
                if not (isinstance(key, ast.Constant) and key.value == "testharnessreport"):
                    continue
                if not isinstance(value, ast.List):
                    return []
                out = []
                for item in value.elts:
                    if isinstance(item, ast.Name) and item.id in consts:
                        out.append(consts[item.id])
                    elif isinstance(item, ast.Constant) and isinstance(item.value, str):
                        out.append(os.path.normpath(os.path.join(WPTRUNNER_ROOT, item.value)))
                return out
    return []


def _resolve_path_expr(node, module_path: str):
    """Fold an `os.path.abspath/normpath/join/dirname(__file__)` expression.

    `module_path` is the file the expression lives in, i.e. what `__file__`
    means there. Returns None for anything else — a constant this module does
    not understand is better skipped than guessed at.
    """
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.Name) and node.id == "__file__":
        return module_path
    if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute):
        return None
    name = node.func.attr
    args = [_resolve_path_expr(arg, module_path) for arg in node.args]
    if not args or any(a is None for a in args):
        return None
    if name == "dirname":
        return os.path.dirname(args[0])
    if name in ("abspath", "normpath"):
        return os.path.normpath(args[0])
    if name == "join":
        return os.path.join(*args)
    return None


def route_sources(route: str, paths, report_paths: list) -> list:
    """Absolute source files `get_routes` concatenates for one static route."""
    if paths is None:
        return list(report_paths)
    return [p if os.path.isabs(p) else os.path.normpath(os.path.join(WPTRUNNER_ROOT, p))
            for p in paths]


def expected_bytes(sources: list, substituted: bool, format_args: dict) -> bytes:
    """Rebuild what a self-started `StaticHandler` would serve for a route.

    Mirrors `wptserve.handlers.StaticHandler.__call__`: concatenate the files in
    order, then `%`-substitute if the route carries format args. Missing files
    raise, because a route this checkout cannot build is a finding about the
    checkout, not about the server.
    """
    data = ""
    for path in sources:
        with open(path, encoding="utf-8") as fh:
            data += fh.read()
    if substituted:
        data = data % format_args
    return data.encode("utf-8")


def compare_routes(base_url: str, format_args: dict = None, report_paths: list = None) -> list:
    """Fetch every static route and diff it against this checkout's version.

    Returns one record per route: `status`, `verdict` (`match` / `differs` /
    `absent` / `unbuildable`), sizes, and — for the substituted route — the
    values the server actually baked in, which is the whole point: they are the
    dead run's options and cannot be read anywhere else.
    """
    format_args = DEFAULT_FORMAT_ARGS if format_args is None else format_args
    report_paths = lumen_testharnessreport() if report_paths is None else report_paths
    out = []
    for route, paths, substituted in STATIC_ROUTES:
        status, body = fetch(base_url.rstrip("/") + route)
        record = {"route": route, "status": status, "served_bytes": len(body)}
        sources = route_sources(route, paths, report_paths)
        record["sources"] = [os.path.relpath(p, REPO_ROOT) for p in sources]
        try:
            want = expected_bytes(sources, substituted, format_args)
        except (OSError, KeyError, ValueError) as exc:
            # A route this checkout cannot build is only a finding if the
            # server answers it anyway. `/_pdf_js/*` is the standing case:
            # `third_party/pdf_js` is not vendored here, so `StaticHandler`
            # raises and wptserve returns 500 — both sides agree the route does
            # not exist, and no test that Lumen runs asks for it.
            record.update(verdict="unvendored" if status != 200 else "unbuildable",
                          detail=str(exc))
            out.append(record)
            continue
        record["expected_bytes"] = len(want)
        if status != 200:
            record["verdict"] = "absent"
        elif body == want:
            record["verdict"] = "match"
        else:
            record["verdict"] = "differs"
        if substituted and status == 200:
            record["served_options"] = served_options(body)
        out.append(record)
    return out


def served_options(body: bytes) -> dict:
    """Read the four substituted values back out of a served report script.

    The template writes them as `key: value,` inside one object literal, so a
    line scan is enough and is deliberately not a JS parse: this has to keep
    working on a file the squatter froze months ago.
    """
    found = {}
    for line in body.decode("utf-8", "replace").splitlines():
        stripped = line.strip().rstrip(",")
        for key in DEFAULT_FORMAT_ARGS:
            if stripped.startswith(key + ":"):
                found[key] = stripped.split(":", 1)[1].strip()
    return found


def doc_root_is_live(base_url: str, probe_dir: str = None) -> dict:
    """Prove the server reads test files off *this* checkout, per request.

    Writes a uniquely-named file into `tests/wpt/resources/`, fetches it,
    deletes it and fetches again. 200-then-404 is the only outcome that rules
    out both a frozen copy and a doc root pointing at another worktree; the
    file is removed even if the fetch raises.
    """
    probe_dir = PROBE_DIR if probe_dir is None else probe_dir
    fd, path = tempfile.mkstemp(prefix="lumen-route-probe-", suffix=".js", dir=probe_dir)
    marker = f"// lumen route probe {os.path.basename(path)}\n"
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(marker)
        url = f"{base_url.rstrip('/')}/resources/{os.path.basename(path)}"
        present_status, body = fetch(url)
        served_marker = body.decode("utf-8", "replace") == marker
    finally:
        os.unlink(path)
    absent_status, _ = fetch(url)
    return {"probe": os.path.basename(path),
            "present_status": present_status,
            "content_matches": served_marker,
            "after_delete_status": absent_status,
            "live": present_status == 200 and served_marker and absent_status == 404}


def runners_set_options(sources_root: str = TESTS_ROOT) -> dict:
    """Which runners in this repo pass a flag that moves `DEFAULT_FORMAT_ARGS`.

    Informational, not part of the verdict: the ground truth is the values the
    server actually baked in, which `compare_routes` reads back. This says how
    *reachable* the hazard is — `run_report.py` accepts `--timeout-multiplier`
    (default unset), so a squatter left behind by one of those runs really can
    be handing every test of the next run someone else's timeout.
    """
    hits = {}
    for name in RUNNER_SOURCES:
        path = os.path.join(sources_root, name)
        if not os.path.isfile(path):
            continue
        text = open(path, encoding="utf-8").read()
        found = [flag for flag in OPTION_FLAGS if flag in text]
        if found:
            hits[name] = found
    return hits


def audit(base_url: str, with_holders: bool = True) -> dict:
    """Run every check and fold them into one verdict."""
    result = {"base_url": base_url}
    if with_holders:
        result["holders"] = [{"port": port, "pid": pid, "kind": kind, "command": command}
                             for port, pid, kind, command in port_guard.survey()]
    result["doc_root"] = doc_root_is_live(base_url)
    result["routes"] = compare_routes(base_url)
    result["runners_can_set_options"] = runners_set_options()
    result["trustworthy"] = bool(
        result["doc_root"]["live"]
        and all(r["verdict"] in ("match", "unvendored") for r in result["routes"]))
    return result


def print_audit(result: dict) -> None:
    for holder in result.get("holders", []):
        print(f"  port {holder['port']}: {holder.get('kind', '?')} "
              f"pid={holder.get('pid')} {holder.get('command', '')[:60]}")
    doc = result["doc_root"]
    print(f"doc root live: {doc['live']} (probe {doc['present_status']} -> "
          f"{doc['after_delete_status']}, content match {doc['content_matches']})")
    for record in result["routes"]:
        line = (f"  {record['verdict']:11s} {record['route']} "
                f"served={record['served_bytes']}b")
        if "expected_bytes" in record:
            line += f" expected={record['expected_bytes']}b"
        print(line)
        if record.get("served_options"):
            print(f"      served options: {record['served_options']}")
    flags = result["runners_can_set_options"]
    print(f"runners that can move the harness options: {flags or 'none'}")
    print(f"trustworthy: {result['trustworthy']}")


def _selftest() -> int:
    """Prove each check bites, against a server this test controls.

    Four mutations, one per failure mode the audit exists to catch: a route
    frozen at an older version of the file, a route substituted with another
    run's options, a route that is not served at all, and a doc root that is a
    copy rather than the live tree.
    """
    import http.server
    import threading

    failures = []

    def check(label, condition, detail=""):
        print(f"  {'ok  ' if condition else 'FAIL'} {label}{(' — ' + detail) if detail else ''}")
        if not condition:
            failures.append(label)

    tmp = tempfile.mkdtemp(prefix="route-audit-selftest-")
    served = os.path.join(tmp, "served")
    checkout = os.path.join(tmp, "checkout", "resources")
    os.makedirs(os.path.join(served, "resources"))
    os.makedirs(checkout)

    template = ("var x = {\n  timeout_multiplier: %(timeout_multiplier)s,\n"
                "  output: %(output)d,\n  explicit_timeout: %(explicit_timeout)s,\n"
                "  debug: %(debug)s\n};\n")
    report_src = os.path.join(checkout, "testharnessreport.js")
    with open(report_src, "w", encoding="utf-8") as fh:
        fh.write(template)

    def publish(fmt_args, body=None):
        """Write what the fake server serves at the report route."""
        data = body if body is not None else template % fmt_args
        with open(os.path.join(served, "resources", "testharnessreport.js"),
                  "w", encoding="utf-8") as fh:
            fh.write(data)

    publish(DEFAULT_FORMAT_ARGS)

    handler = http.server.SimpleHTTPRequestHandler
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), lambda *a, **k:
                                            handler(*a, directory=served, **k))
    httpd.log_message = lambda *a: None
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{httpd.server_address[1]}"

    single = [("/resources/testharnessreport.js", None, True)]
    saved_routes = list(STATIC_ROUTES)
    STATIC_ROUTES[:] = single
    try:
        clean = compare_routes(base, DEFAULT_FORMAT_ARGS, [report_src])
        check("identical route matches", clean[0]["verdict"] == "match", clean[0]["verdict"])
        check("substituted values are read back",
              clean[0]["served_options"].get("timeout_multiplier") == "1",
              str(clean[0].get("served_options")))

        publish({**DEFAULT_FORMAT_ARGS, "timeout_multiplier": 3})
        other = compare_routes(base, DEFAULT_FORMAT_ARGS, [report_src])
        check("foreign options are caught", other[0]["verdict"] == "differs", other[0]["verdict"])
        check("foreign options are named",
              other[0]["served_options"].get("timeout_multiplier") == "3",
              str(other[0].get("served_options")))

        publish(DEFAULT_FORMAT_ARGS, body="// yesterday's version\n")
        stale = compare_routes(base, DEFAULT_FORMAT_ARGS, [report_src])
        check("frozen older file is caught", stale[0]["verdict"] == "differs", stale[0]["verdict"])

        os.unlink(os.path.join(served, "resources", "testharnessreport.js"))
        gone = compare_routes(base, DEFAULT_FORMAT_ARGS, [report_src])
        check("missing route is caught", gone[0]["verdict"] == "absent", gone[0]["verdict"])

        probe_dir = os.path.join(served, "resources")
        live = doc_root_is_live(base, probe_dir)
        check("live doc root is recognised", live["live"], json.dumps(live))

        copy_dir = os.path.join(tmp, "elsewhere")
        os.makedirs(copy_dir)
        dead = doc_root_is_live(base, copy_dir)
        check("doc root pointing elsewhere is caught", not dead["live"], json.dumps(dead))

        missing = compare_routes(base, DEFAULT_FORMAT_ARGS,
                                 [os.path.join(tmp, "no-such-file.js")])
        check("route absent on both sides reads as unvendored",
              missing[0]["verdict"] == "unvendored", missing[0]["verdict"])

        publish(DEFAULT_FORMAT_ARGS)
        anomaly = compare_routes(base, DEFAULT_FORMAT_ARGS,
                                 [os.path.join(tmp, "no-such-file.js")])
        check("served route this checkout cannot build is a finding",
              anomaly[0]["verdict"] == "unbuildable", anomaly[0]["verdict"])
    finally:
        STATIC_ROUTES[:] = saved_routes
        httpd.shutdown()

    report_paths = lumen_testharnessreport()
    check("report override read out of lumen.py",
          len(report_paths) == 1 and report_paths[0].endswith(
              os.path.join("tests", "wpt", "resources", "testharnessreport.js")),
          str(report_paths))

    print("selftest: " + (f"{len(failures)} failed" if failures else "all checks passed"))
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base-url", default=None,
                        help="server to audit; default is http://127.0.0.1:<first http port>")
    parser.add_argument("--json", metavar="PATH", help="write the full result as JSON")
    parser.add_argument("--no-holders", action="store_true",
                        help="skip the port survey (it reads /proc for every process)")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

    base_url = args.base_url
    if not base_url:
        # `configured_ports` is a flat sorted list of every port in
        # `config.json`; the first http one is the lowest of the two http
        # ports, which is the origin wptrunner hands tests by default.
        ports = port_guard.configured_ports()
        base_url = f"http://127.0.0.1:{ports[0] if ports else 18300}"

    result = audit(base_url, with_holders=not args.no_holders)
    print_audit(result)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump(result, fh, indent=1, sort_keys=True)
        print(f"wrote {args.json}")
    return 0 if result["trustworthy"] else 1


if __name__ == "__main__":
    sys.exit(main())
