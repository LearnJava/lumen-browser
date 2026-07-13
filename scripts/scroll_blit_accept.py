#!/usr/bin/env python3
"""Blit-scroll pixel acceptance: LUMEN_SCROLL_BLIT on vs off (ADR-016 M3.2.1c).

The scroll-blit path presents a retained content band shifted by the scroll
delta instead of re-executing the display list, replaying `position:fixed`/
`sticky` overlays on top (M3.2.1c-3…c-5). Default is **on** since M3.2.1c-7;
kill-switch: `LUMEN_SCROLL_BLIT=0`. This harness compares blit-on vs blit-off
to confirm pixel equivalence (the ground truth is the direct/flag-off render,
which is byte-identical to the shipping renderer).

This harness drives one live Lumen window per flag state through an identical
sequence of scroll stops and diffs the on-vs-off desktop captures frame for
frame. The MCP `screenshot` tool re-renders on the CPU (it bypasses femtovg, so
it cannot see the blit), therefore capture is `ffmpeg` gdigrab of the real
window — the same mechanism `run.py --live` uses — cropped to the viewport via
the scroll-0 magenta-frame calibration.

Requires: a built `lumen.exe` (default `target/dev-release`, override with
`LUMEN_PROFILE`) and `utils/ffmpeg.exe`.

    python scripts/scroll_blit_accept.py                 # all fixtures
    python scripts/scroll_blit_accept.py --only 02       # one fixture
    python scripts/scroll_blit_accept.py --threshold 0.5 # per-stop FAIL bound

Exit code 0 = every stop of every fixture within threshold; 1 = a regression or
a capture/setup failure.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time

# Reuse the graphic_tests desktop-capture toolbox (side-effect-free import: its
# main() is guarded by __name__ == '__main__').
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "graphic_tests"))
import run as gt  # noqa: E402

REPO = gt.REPO
FIXTURE_DIR = os.path.join(REPO, "graphic_tests", "blit-accept")
TMP = os.path.join(REPO, ".tmp", "blit-accept")

# (id, filename). Kept in lockstep with scripts/gen_blit_fixtures.py.
FIXTURES = [
    ("01", "01-plain-tall.html"),
    ("02", "02-fixed-header.html"),
    ("03", "03-sticky-header.html"),
    ("04", "04-transform-fixed.html"),
]

# Absolute scroll-Y stops (CSS px). A mix of small one-notch steps (blit / expose)
# and large jumps (repaint), down then back up, so both scroll directions and the
# band re-seat are exercised. Content is ~3000px tall, viewport 720.
STOPS = [80, 200, 360, 600, 900, 1300, 1800, 2200, 1400, 700, 120, 0]

SETTLE = 0.55  # seconds to let a scrolled frame present before capture


def free_port() -> int:
    """Grab an ephemeral loopback port for the MCP live session."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Mcp:
    """Line-delimited JSON-RPC client to `lumen --mcp-live-port` (see run.py)."""

    def __init__(self, port: int) -> None:
        last: Exception | None = None
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError as e:
                last = e
                time.sleep(0.1)
        else:
            raise RuntimeError(f"MCP port {port} never came up: {last}")
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock.settimeout(60)
        self._reader = self.sock.makefile("r", encoding="utf-8", newline="\n")
        self._id = 0

    def call(self, name: str, arguments: dict) -> dict:
        """One `tools/call` round trip; raises on an MCP error reply."""
        self._id += 1
        req = json.dumps({
            "jsonrpc": "2.0", "id": self._id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })
        self.sock.sendall((req + "\n").encode("utf-8"))
        line = self._reader.readline()
        if not line:
            raise RuntimeError("MCP connection closed (window crashed?)")
        resp = json.loads(line)
        if resp.get("error") is not None:
            raise RuntimeError(f"{name}: {resp['error']}")
        return resp.get("result") or {}


def _gdigrab(out_png: str) -> None:
    """Grab a single desktop frame to `out_png` (whole desktop, cropped later)."""
    subprocess.run(
        [gt.FFMPEG, "-f", "gdigrab", "-i", "desktop",
         "-vframes", "1", "-update", "1", out_png, "-y"],
        capture_output=True, timeout=15,
    )


def _capture_cropped(proc, origin: tuple[int, int], raw: str, cropped: str) -> None:
    """Bring the window to front, grab the desktop, crop to the viewport."""
    gt._bring_pid_to_front(proc.pid)
    time.sleep(0.25)
    _gdigrab(raw)
    gt.ffmpeg_crop(raw, cropped, origin[0], origin[1])


def run_flag(exe: str, fixture: str, blit_on: bool, tag: str) -> list[str] | None:
    """Drive one live window through STOPS; return the list of cropped PNGs
    (one per stop) or None on a setup/capture failure."""
    port = free_port()
    env = dict(os.environ)
    if blit_on:
        env.pop("LUMEN_SCROLL_BLIT", None)  # default on; explicit pop keeps env clean
    else:
        env["LUMEN_SCROLL_BLIT"] = "0"  # kill-switch: disable the default-on path
    log_path = os.path.join(TMP, f"{tag}.stderr.log")
    log_f = open(log_path, "w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(
        [exe, "--mcp-live-port", str(port), "--no-scrollbar", "about:blank"],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f,
    )
    crops: list[str] = []
    try:
        c = Mcp(port)
        url = "file:///" + os.path.abspath(fixture).replace("\\", "/")
        c.call("navigate", {"url": url})
        c.call("wait", {"condition": "document_ready", "timeout_ms": 20000})
        time.sleep(1.0)  # let the page paint / settle at scroll 0

        # Calibrate the crop from the magenta frame at scroll 0. Retry once on a
        # blank / mis-focused grab (known gdigrab flakiness).
        origin = None
        for _ in range(3):
            raw0 = os.path.join(TMP, f"{tag}_cal_raw.png")
            gt._bring_pid_to_front(proc.pid)
            time.sleep(0.3)
            _gdigrab(raw0)
            origin = gt.find_marker_origin(raw0)
            if origin is not None:
                break
            time.sleep(0.4)
        if origin is None:
            print(f"    [{tag}] magenta calibration failed — no crop origin", flush=True)
            return None

        for i, target in enumerate(STOPS):
            # STOPS are absolute; scroll_by_delta is relative, so send the delta
            # from the previous target (0 is the calibrated start).
            prev = STOPS[i - 1] if i > 0 else 0
            c.call("scroll", {"target": {"css": "body"}, "delta": {"x": 0, "y": target - prev}})
            time.sleep(SETTLE)
            cropped = os.path.join(TMP, f"{tag}_stop{i:02d}.png")
            raw = os.path.join(TMP, f"{tag}_stop{i:02d}_raw.png")
            _capture_cropped(proc, origin, raw, cropped)
            crops.append(cropped)
        return crops
    except Exception as e:  # noqa: BLE001 — harness: report and fail the fixture
        print(f"    [{tag}] ERROR: {e}", flush=True)
        return None
    finally:
        log_f.close()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


def compare(off: list[str], on: list[str], threshold: float) -> tuple[float, list[float]]:
    """Diff on-vs-off crops per stop; return (worst pct, per-stop pcts)."""
    pcts: list[float] = []
    for i, (a, b) in enumerate(zip(off, on)):
        diff_png = os.path.join(TMP, f"diff_stop{i:02d}.png")
        gt.ffmpeg_diff(a, b, diff_png)
        pct, _region = gt.diff_stats(diff_png)
        pcts.append(pct)
    return (max(pcts) if pcts else 0.0), pcts


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--only", help="run a single fixture id (e.g. 02)")
    ap.add_argument("--threshold", type=float, default=0.5,
                    help="per-stop diff %% that counts as a regression (default 0.5)")
    args = ap.parse_args()

    profile = os.environ.get("LUMEN_PROFILE", "dev-release")
    exe = os.path.join(REPO, "target", profile, "lumen.exe")
    if not os.path.exists(exe):
        print(f"no binary {exe} — build lumen-shell (--profile {profile})", file=sys.stderr)
        return 1
    os.makedirs(TMP, exist_ok=True)

    fixtures = [f for f in FIXTURES if args.only is None or f[0] == args.only]
    if not fixtures:
        print(f"no fixture matches --only {args.only}", file=sys.stderr)
        return 1

    any_fail = False
    print(f"blit-scroll acceptance (threshold {args.threshold}%), {len(STOPS)} stops/fixture")
    for fid, fname in fixtures:
        path = os.path.join(FIXTURE_DIR, fname)
        print(f"\n[{fid}] {fname}")
        off = run_flag(exe, path, blit_on=False, tag=f"{fid}_off")
        on = run_flag(exe, path, blit_on=True, tag=f"{fid}_on")
        if off is None or on is None or len(off) != len(on):
            print(f"  [{fid}] FAIL (capture error)")
            any_fail = True
            continue
        worst, pcts = compare(off, on, args.threshold)
        stop_fail = [i for i, p in enumerate(pcts) if p > args.threshold]
        status = "FAIL" if stop_fail else "PASS"
        if stop_fail:
            any_fail = True
        print(f"  [{fid}] {status}  worst {worst:.3f}%  "
              f"stops=" + " ".join(f"{p:.2f}" for p in pcts))
        if stop_fail:
            print(f"        over-threshold stops (scroll_y): "
                  + ", ".join(f"{STOPS[i]}px={pcts[i]:.2f}%" for i in stop_fail))

    print("\n" + ("RESULT: FAIL" if any_fail else "RESULT: PASS"))
    return 1 if any_fail else 0


if __name__ == "__main__":
    sys.exit(main())
