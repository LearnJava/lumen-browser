#!/usr/bin/env python3
"""Sync the `dom/nodes` detail table in `docs/wpt-status.md` from an already
generated `run_report.py --all` HTML report — does not drive wptrunner itself.

Rationale: a full `--all` run over `dom/nodes` (168 tests, most never vetted
for this project's minimal single-window BiDi executor) takes several minutes
and produces a lot of TIMEOUT noise (see `run_report.py`'s own docstring) —
driving it a second time from this script just to re-derive the same numbers
`run_report.py` already computed would double that cost for no reason. So the
two steps are decoupled: run the (slow) test suite via `run_report.py --all`
whenever you actually want fresh numbers, then run this (fast, no test
execution) script to sync the tracked Markdown from that report.

Unlike the HTML report (disposable, `.tmp/`, not committed), this writes into
a tracked Markdown file and preserves the hand-edited "Владелец"/"Баг"/
"Заметка" columns across syncs — it merges by test id instead of overwriting
the whole table, so re-running this after a fix does not wipe out assignments
made by `docs/wpt-status.md`'s maintainer.

Only rewrites the `dom/nodes` detail table between the
`<!-- gen:dom/nodes:start -->` / `<!-- gen:dom/nodes:end -->` markers; the
category index above it is hand-maintained (see `docs/wpt-status.md` itself).

Usage (from repo root, after the venv setup in tests/wpt/README.md):

    export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
    BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
    tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --out .tmp/wpt-report-all.html --all
    tests/wpt/.venv/Scripts/python.exe tests/wpt/gen_status_md.py
"""

import argparse
import html
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_smoke  # noqa: E402

REPO_ROOT = run_smoke.REPO_ROOT
STATUS_MD = os.path.join(REPO_ROOT, "docs", "wpt-status.md")
DEFAULT_REPORT = os.path.join(REPO_ROOT, ".tmp", "wpt-report-all.html")
MARKER_START = "<!-- gen:dom/nodes:start -->"
MARKER_END = "<!-- gen:dom/nodes:end -->"

# Matches a rendered result row in run_report.py's HTML output:
# <tr class="test CLASS"><td class="status">STATUS</td><td class="name"><code>ID</code></td><td class="subcount">N/M</td>
ROW_RE = re.compile(
    r'<tr class="test [^"]*"><td class="status">([^<]*)</td>'
    r'<td class="name"><code>([^<]*)</code></td>'
    r'<td class="subcount">([^<]*)</td>'
)
MISSING_RE = re.compile(r"<li><code>([^<]*)</code></li>")

# Matches a previously generated row: | `/dom/nodes/x.html` | OK | 12/12 | owner | bug | note |
PRIOR_ROW_RE = re.compile(
    r"^\|\s*`(?P<test>/dom/nodes/[^`]+)`\s*\|[^|]*\|[^|]*\|\s*(?P<owner>[^|]*)\|\s*(?P<bug>[^|]*)\|\s*(?P<note>[^|]*)\|\s*$"
)


def esc(s) -> str:
    return html.escape("" if s is None else str(s), quote=False)


def find_marker(text: str, marker: str) -> int:
    """Index of `marker` when it is the entire content of its own line.

    Plain substring search is not safe here: prose elsewhere in the file may
    mention the marker text (e.g. explaining what it is) without it being the
    real splice point, and matching that mention instead of the real one
    silently corrupts the file (this happened once — see git history).
    """
    m = re.search(r"^" + re.escape(marker) + r"\s*$", text, re.MULTILINE)
    return m.start() if m else -1


def parse_report(report_path: str) -> dict:
    """test id -> (status, subcount) parsed out of run_report.py's HTML."""
    with open(report_path, encoding="utf-8") as f:
        text = f.read()
    data = {}
    for status, test_id, sub in ROW_RE.findall(text):
        data[html.unescape(test_id)] = (html.unescape(status), sub)
    m = re.search(r"<h2>Not run</h2>.*?</ul>", text, re.S)
    if m:
        for test_id in MISSING_RE.findall(m.group(0)):
            data[html.unescape(test_id)] = ("NOT RUN", "0/0")
    return data


def load_existing_annotations(path: str) -> dict:
    """test id -> (owner, bug, note), read from the previous generated block."""
    if not os.path.isfile(path):
        return {}
    with open(path, encoding="utf-8") as f:
        text = f.read()
    start = find_marker(text, MARKER_START)
    end = find_marker(text, MARKER_END)
    if start == -1 or end == -1:
        return {}
    block = text[start:end]
    out = {}
    for line in block.splitlines():
        m = PRIOR_ROW_RE.match(line)
        if m:
            out[m.group("test")] = (
                m.group("owner").strip(),
                m.group("bug").strip(),
                m.group("note").strip(),
            )
    return out


def render_table(data: dict, prior: dict) -> str:
    lines = [
        MARKER_START,
        "",
        "| Тест | Статус | Сабтесты | Владелец | Баг | Заметка |",
        "|---|---|---|---|---|---|",
    ]
    for test_id in sorted(data):
        status, sub = data[test_id]
        owner, bug, note = prior.get(test_id, ("", "", ""))
        lines.append(
            f"| `{esc(test_id)}` | {esc(status)} | {sub} | {esc(owner)} | {esc(bug)} | {esc(note)} |"
        )
    lines.append("")
    lines.append(MARKER_END)
    return "\n".join(lines)


def splice(path: str, new_block: str) -> None:
    if not os.path.isfile(path):
        print(f"{path} does not exist yet — create it with the category index first", file=sys.stderr)
        sys.exit(1)
    with open(path, encoding="utf-8") as f:
        text = f.read()
    start = find_marker(text, MARKER_START)
    end = find_marker(text, MARKER_END)
    if start == -1 or end == -1:
        print(f"markers {MARKER_START}/{MARKER_END} not found in {path}", file=sys.stderr)
        sys.exit(1)
    end += len(MARKER_END)
    text = text[:start] + new_block + text[end:]
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--report", default=DEFAULT_REPORT, help="run_report.py --all HTML output to sync from")
    args = parser.parse_args()

    if not os.path.isfile(args.report):
        print(
            f"{args.report} not found — run run_report.py --all first (see this script's docstring)",
            file=sys.stderr,
        )
        return 1

    data = parse_report(args.report)
    prior = load_existing_annotations(STATUS_MD)
    block = render_table(data, prior)
    splice(STATUS_MD, block)
    print(f"wrote {len(data)} rows into {STATUS_MD} (from {args.report})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
