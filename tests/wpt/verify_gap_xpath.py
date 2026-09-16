#!/usr/bin/env python3
"""GAP-XPATH (BUG-891) live-window probe.

`crates/js/src/xpath.rs` is exercised by Rust-level unit tests against a
hand-built mock DOM shape. This probe closes the remaining gap: does
`document.evaluate` work against the **real** live-window DOM (real
`NamedNodeMap`, `Node.compareDocumentPosition`, `Node.lookupNamespaceURI`),
not just the generic mock. Drives `--mcp-live-port`'s `eval` tool against a
real page served over http (never `file://` — CLAUDE.md/docs/probe-method.md
§2), same `Client`/`wait{document_ready}` pattern as
`verify_gap_navctx_named_target_reuse.py`.

Usage (from repo root):

    python tests/wpt/verify_gap_xpath.py [--binary target/dev-release/lumen.exe]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import socket
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(REPO, "scripts"))

from scroll_perf import Client  # noqa: E402  (после sys.path)

PAGE = """<!doctype html><meta charset="utf-8"><title>gap-xpath probe</title>
<body>
<div id="root"><p class="a">one</p><p class="b">two</p><p class="a">three</p></div>
<script>console.log('PROBE ready');</script>
</body>
"""


class _Quiet(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def log_message(self, *args):
        pass


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO, "target", "dev-release", "lumen.exe")
    )
    args = parser.parse_args()

    page_name = ".gap-xpath-probe.html"
    page_path = os.path.join(HERE, page_name)
    with open(page_path, "w", encoding="utf-8") as f:
        f.write(PAGE)

    http_port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", http_port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "gap-xpath-probe.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{http_port}/{page_name}"
    print(f"{url} -> {log_path}")

    checks = [
        ("has-evaluate", "typeof document.evaluate"),
        ("has-XPathResult", "typeof XPathResult"),
        ("has-XPathEvaluator", "typeof XPathEvaluator"),
        ("has-XPathException", "typeof XPathException"),
        ("snapshot-length",
         "document.evaluate('//p[@class=\"a\"]', document, null, "
         "XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null).snapshotLength"),
        ("item0-text",
         "document.evaluate('//p[@class=\"a\"]', document, null, "
         "XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null).snapshotItem(0).textContent"),
        ("item1-text",
         "document.evaluate('//p[@class=\"a\"]', document, null, "
         "XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null).snapshotItem(1).textContent"),
        ("count-p",
         "document.evaluate('count(//p)', document, null, "
         "XPathResult.NUMBER_TYPE, null).numberValue"),
        ("second-p-class",
         "document.evaluate('//div[@id=\"root\"]/p[2]', document, null, "
         "XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue"
         ".getAttribute('class')"),
        ("createExpression-second-p",
         "new XPathEvaluator().createExpression('//div[@id=\"root\"]/p[2]', null)"
         ".evaluate(document, XPathResult.FIRST_ORDERED_NODE_TYPE, null)"
         ".singleNodeValue.getAttribute('class')"),
    ]

    results: dict[str, object] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            for name, code in checks:
                try:
                    raw = client.call("eval", {"code": code}).get("result")
                    val = json.loads(raw) if isinstance(raw, str) else raw
                except Exception as e:  # noqa: BLE001 — измерение, не гейт
                    val = f"EXCEPTION: {e}"
                results[name] = val
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()
            try:
                os.remove(page_path)
            except OSError:
                pass

    for name, val in results.items():
        print(f"{name} = {val}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
