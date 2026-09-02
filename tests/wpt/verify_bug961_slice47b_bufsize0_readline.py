#!/usr/bin/env python3
"""BUG-961 срез 47b: isolate срез 47's `anon_pipe_write.cold` /
`ProcessReaderStdout`-running-not-blocked finding down to pure CPython pipe
I/O, with ZERO lumen/wptrunner/mozprocess code in the loop — the cleanest
possible confirmation per `docs/probe-method.md`'s "your own server, not the
page and not the browser log" rule.

Hypothesis: `mozprocess.processhandler.Process.__init__` passes `bufsize=0`
literally to `subprocess.Popen` (`processhandler.py:97`, confirmed срез 47),
which gives `stdout`/`stderr` as raw, UNBUFFERED `io.FileIO` objects with no
`io.BufferedReader` wrapper. `io.RawIOBase.readline()` (the fallback used
when there is no buffer to search) reads ONE BYTE PER SYSCALL in a Python
loop until it sees `\\n` or EOF. For the console-log-large-array test's
20 000 004-byte single line (no embedded newline), that is ~20 million
individual 1-byte `read()` calls inside ONE `.readline()` call — plausibly
tens of seconds of pure interpreter+syscall overhead. `bufsize=1` in BINARY
mode (срез 43/45/46's bare-Popen shape) is silently promoted by
`subprocess.py` to `io.DEFAULT_BUFFER_SIZE` (8192) since "line buffering
(bufsize=1) isn't supported in binary mode" — giving a normal buffered
`io.BufferedReader.readline()`, which is O(n) total, not O(n) syscalls.

This script builds a plain `os.pipe()`, wraps the read end exactly the two
ways to compare, writes a 20MB no-newline line from a background thread (a
`Popen`-shaped, not a lumen-shaped, producer — this isolates the CONSUMER
side, which is where срез 47's `ProcessReaderStdout` thread lives), and
times `readline()` on each.

Usage: tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice47b_bufsize0_readline.py
Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import io
import os
import threading
import time

LINE_LEN = 20_000_004  # same size срез 39/40 measured for the real test


def _writer_thread(fd, payload):
    def run():
        with os.fdopen(fd, "wb", buffering=0) as f:
            f.write(payload)
    t = threading.Thread(target=run, daemon=True)
    t.start()
    return t


def _measure(label, buffering):
    r_fd, w_fd = os.pipe()
    payload = b"x" * LINE_LEN + b"\n"
    writer = _writer_thread(w_fd, payload)
    reader = os.fdopen(r_fd, "rb", buffering=buffering)
    t0 = time.monotonic()
    line = reader.readline()
    elapsed = time.monotonic() - t0
    reader.close()
    writer.join(timeout=5)
    print(f"[probe] {label} (buffering={buffering!r}, "
          f"type={type(reader).__mro__[0].__name__ if False else ''}): "
          f"readline() of {len(line)} bytes took {elapsed:.3f}s")
    return elapsed


def main():
    print(f"[probe] payload: {LINE_LEN} bytes + newline, no other content")
    # Buffered (default) — the shape срез 43/45/46's bare-Popen bufsize=1
    # binary-mode probe actually got (silently promoted from 1).
    t_buffered = _measure("buffered io.BufferedReader", buffering=io.DEFAULT_BUFFER_SIZE)
    # Unbuffered (bufsize=0) — the shape mozprocess.ProcessHandler actually
    # uses (`processhandler.py:97`), i.e. срез 42/46's real LumenBrowser
    # launch path.
    t_unbuffered = _measure("unbuffered io.FileIO (bufsize=0)", buffering=0)

    print(f"[probe] ratio unbuffered/buffered: {t_unbuffered / max(t_buffered, 1e-6):.1f}x")
    if t_unbuffered > 5.0 and t_unbuffered > 20 * max(t_buffered, 1e-6):
        print("[probe] CONFIRMED: unbuffered readline() of a huge no-newline "
              "line is orders of magnitude slower — matches срез 47's "
              "~31s anon_pipe_write.cold / ProcessReaderStdout-running stall "
              "shape with zero lumen/wptrunner/mozprocess code involved.")
    else:
        print("[probe] NOT confirmed at this magnitude — bufsize=0 is not "
              "(or not solely) the mechanism; needs another slice.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
