#!/usr/bin/env python3
"""BUG-792: an https response closed without TLS `close_notify` loses its body.

Isolates, without wptserve in the picture, what the WPT-RUN-5 corpus run sees on
every `.https.` test: `read body: peer closed connection without sending TLS
close_notify`. Serves the SAME page three ways over TLS and reports which of
them the engine manages to lay out:

  eof-abrupt   no `Content-Length`, socket closed with no TLS shutdown  (wptserve)
  eof-clean    no `Content-Length`, proper `close_notify` before close
  length-abrupt`Content-Length` present, socket closed with no TLS shutdown

`eof-clean` and `length-abrupt` passing while `eof-abrupt` fails pins the defect
to the combination — EOF-framed body plus a missing `close_notify` — and rules
out both "TLS is broken" (BUG-785, fixed) and "abrupt close is broken".

Usage (from repo root):  python tests/wpt/verify_bug792_tls_eof_body.py [--binary PATH]
"""

import argparse
import os
import socket
import ssl
import subprocess
import sys
import threading

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CERTS = os.path.join(REPO_ROOT, "tests", "wpt", "certs")

#: The marker is a width the default UA stylesheet cannot produce by accident,
#: so finding it in the layout dump proves the body arrived and was parsed.
PAGE = b"<html><body><div style='width:333px;height:77px'></div></body></html>"


def serve_once(mode: str, port_holder: list, ready: threading.Event) -> None:
    """Accept exactly one TLS connection, answer `PAGE` per `mode`, close."""
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(os.path.join(CERTS, "host-cert.pem"),
                        os.path.join(CERTS, "host-key.pem"))
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port_holder.append(listener.getsockname()[1])
    ready.set()
    listener.settimeout(30)
    try:
        raw, _ = listener.accept()
    except socket.timeout:
        listener.close()
        return
    try:
        conn = ctx.wrap_socket(raw, server_side=True)
    except ssl.SSLError as exc:
        print(f"  [server] TLS handshake failed: {exc}", file=sys.stderr)
        raw.close()
        listener.close()
        return
    try:
        conn.settimeout(10)
        try:
            conn.recv(65536)
        except (socket.timeout, ssl.SSLError):
            pass
        head = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n"
        if mode == "length-abrupt":
            head += b"Content-Length: %d\r\n" % len(PAGE)
        else:
            head += b"Connection: close\r\n"
        conn.sendall(head + b"\r\n" + PAGE)
        if mode == "eof-clean":
            conn.unwrap()          # sends close_notify
    except OSError:
        pass
    finally:
        # No `conn.unwrap()` in the abrupt modes: the socket dies mid-TLS-session,
        # which is exactly what wptserve does.
        try:
            conn.close()
        except OSError:
            pass
        listener.close()


def probe(mode: str, binary: str) -> bool:
    port_holder: list = []
    ready = threading.Event()
    thread = threading.Thread(target=serve_once, args=(mode, port_holder, ready), daemon=True)
    thread.start()
    ready.wait(10)
    url = f"https://127.0.0.1:{port_holder[0]}/probe.html"
    env = dict(os.environ, LUMEN_EXTRA_CA_CERT=os.path.join(CERTS, "ca-cert.pem"))
    proc = subprocess.run([binary, "--dump-layout", url], cwd=REPO_ROOT, env=env,
                          capture_output=True, text=True, timeout=120)
    thread.join(timeout=5)
    got = "333" in proc.stdout and "77" in proc.stdout
    print(f"{mode:<14} {'LAID OUT' if got else 'LOST'}")
    if not got:
        for line in (proc.stderr or "").splitlines():
            if "error" in line.lower() or "ошибка" in line.lower():
                print(f"  {line.strip()[:160]}")
    return got


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen"))
    args = parser.parse_args()
    results = {mode: probe(mode, args.binary)
               for mode in ("eof-clean", "length-abrupt", "eof-abrupt")}
    print()
    if results["eof-clean"] and results["length-abrupt"] and not results["eof-abrupt"]:
        print("BUG-792 REPRODUCED: only EOF-framed + no close_notify loses the body")
        return 1
    if all(results.values()):
        print("all three laid out — BUG-792 not reproduced on this build")
        return 0
    print("mixed result — read the per-mode lines above before concluding")
    return 2


if __name__ == "__main__":
    sys.exit(main())
