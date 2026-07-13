#!/usr/bin/env python3
"""Linux scroll-benchmark runner for Lumen (counterpart of the experimental
branch's PowerShell `run_warm_frame_bench.ps1` + `proc_stats.ps1`).

Launches `target/release/lumen` with `LUMEN_BENCH=scroll:...`, samples the
process CPU and PSS memory from /proc while the harness scrolls the page
top-to-bottom and back, and merges both into one summary line per run.

Usage:
  python3 scripts/bench_scroll.py --page samples/bench-static-scroll.html \
      --backend wgpu --frames 600 --step 60 --runs 3

Notes:
  * wgpu runs get LUMEN_PRESENT=immediate by default (vsync would pin every
    frame sample at ~16.7 ms). femtovg has no present-mode switch and always
    runs vsync-bound — its frame medians are a floor, not a true cost.
  * CPU% is averaged over the whole process life (startup included); peak PSS
    is the high-water mark across samples. 100% = one core.
"""
import argparse
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time

CLK = 100  # USER_HZ


def cpu_ticks(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            parts = f.read().rsplit(")", 1)[1].split()
        return int(parts[11]) + int(parts[12])
    except OSError:
        return None


def pss_kib(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except OSError:
        return None
    return None


class Sampler(threading.Thread):
    """Samples CPU ticks and PSS of `pid` every `interval` seconds."""

    def __init__(self, pid, interval=0.5):
        super().__init__(daemon=True)
        self.pid = pid
        self.interval = interval
        self.samples = []  # (t, ticks, pss_kib)
        self.stop_flag = threading.Event()

    def run(self):
        while not self.stop_flag.is_set():
            t = time.monotonic()
            ticks = cpu_ticks(self.pid)
            pss = pss_kib(self.pid)
            if ticks is None:
                break
            self.samples.append((t, ticks, pss))
            self.stop_flag.wait(self.interval)

    def summary(self):
        if len(self.samples) < 2:
            return None
        t0, k0, _ = self.samples[0]
        t1, k1, _ = self.samples[-1]
        wall = t1 - t0
        cpu_pct = 100.0 * ((k1 - k0) / CLK) / wall if wall > 0 else 0.0
        pss_vals = [p for (_, _, p) in self.samples if p is not None]
        return {
            "wall_s": round(wall, 1),
            "cpu_pct": round(cpu_pct, 1),
            "pss_peak_mib": round(max(pss_vals) / 1024, 1) if pss_vals else None,
            "pss_last_mib": round(pss_vals[-1] / 1024, 1) if pss_vals else None,
        }


KWIN_RAISE_JS = """\
// Keep the Lumen window visible: an occluded Wayland window gets no frame
// callbacks, swapchain acquire times out and the bench measures nothing.
const wins = workspace.windowList();
for (const w of wins) {
    if (w.caption && w.caption.toLowerCase().includes("lumen")) {
        w.keepAbove = true;
        w.minimized = false;
        workspace.activeWindow = w;
    }
}
"""


def kwin_keep_above(tmpdir):
    """Best-effort: mark the Lumen window keep-above via KWin scripting (KDE).

    On a non-KDE compositor this silently does nothing; the bench then needs
    the window to be unoccluded some other way.
    """
    path = os.path.join(tmpdir, "lumen-keep-above.js")
    with open(path, "w") as fh:
        fh.write(KWIN_RAISE_JS)
    name = f"lumen-bench-{os.getpid()}"
    try:
        sid = subprocess.run(
            ["qdbus6", "org.kde.KWin", "/Scripting",
             "org.kde.kwin.Scripting.loadScript", path, name],
            capture_output=True, text=True, timeout=5,
        ).stdout.strip()
        if sid:
            subprocess.run(
                ["qdbus6", "org.kde.KWin", f"/Scripting/Script{sid}",
                 "org.kde.kwin.Script.run"],
                capture_output=True, timeout=5,
            )
            subprocess.run(
                ["qdbus6", "org.kde.KWin", "/Scripting",
                 "org.kde.kwin.Scripting.unloadScript", name],
                capture_output=True, timeout=5,
            )
    except (OSError, subprocess.SubprocessError):
        pass


BENCH_RE = re.compile(
    r"\[bench\] scroll \(full repaint\): n=(?P<n>\d+) warmup=\d+ step=(?P<step>[\d.]+) \| "
    r"median (?P<median>[\d.]+)ms p95 (?P<p95>[\d.]+)ms max (?P<max>[\d.]+)ms "
    r"min (?P<min>[\d.]+)ms mean (?P<mean>[\d.]+)ms \| total (?P<total>[\d.]+)ms \| "
    r"rendered (?P<rendered>\d+) \| passes (?P<passes>\d+) \| scroll_speed (?P<speed>\d+)px/s"
)
FIRST_FRAME_RE = re.compile(r"\[bench\] first non-empty frame: (\d+)ms")
GEOMETRY_RE = re.compile(r"\[bench\] geometry: (.+)")


FRAME_RE = re.compile(r"\[frame\] total\s+([\d.]+)ms")
SCROLL_Y_RE = re.compile(r"\(scroll_y (-?[\d.]+)")


def last_scroll_y(stderr_path):
    """Current page scroll offset, read from the newest LUMEN_FRAME_LOG line.

    Pages without <script> have no JS context (`window.scrollY` eval fails),
    but the frame log prints `scroll_y` on every repaint regardless.
    """
    try:
        with open(stderr_path, "rb") as fh:
            fh.seek(0, os.SEEK_END)
            fh.seek(max(0, fh.tell() - 4096))
            tail = fh.read().decode(errors="replace")
    except OSError:
        return None
    ms = SCROLL_Y_RE.findall(tail)
    return float(ms[-1]) if ms else None


def mcp_rpc_factory(port):
    """Connect to a lumen --mcp-live-port and return an rpc(method, params) fn."""
    import socket as socket_mod
    s = socket_mod.create_connection(("127.0.0.1", port), timeout=30)
    f = s.makefile("rw", encoding="utf-8", newline="\n")
    rid = [0]

    def rpc(method, params):
        rid[0] += 1
        f.write(json.dumps(
            {"jsonrpc": "2.0", "id": rid[0], "method": method, "params": params}
        ) + "\n")
        f.flush()
        return json.loads(f.readline())

    rpc("initialize", {})
    return rpc


def percentiles(samples):
    s = sorted(samples)
    n = len(s)
    pick = lambda q: s[round((n - 1) * q)]
    return {
        "median": round(pick(0.50), 3),
        "p95": round(pick(0.95), 3),
        "max": round(s[-1], 3),
        "min": round(s[0], 3),
        "mean": round(sum(s) / n, 3),
    }


def one_run_mcp(args, run_idx):
    """Drive full top-to-bottom-and-back passes over the page via the MCP live
    window (the interactive input path) and read per-frame costs from
    LUMEN_FRAME_LOG=1 stderr lines.

    Slower cadence than the LUMEN_BENCH harness (~7-10 scrolls/s, limited by
    the MCP round trip), but it exercises the exact code path a real user
    input takes and does not depend on winit redraw pacing.
    """
    port = 7955
    env = os.environ.copy()
    env["LUMEN_BACKEND"] = args.backend
    env["LUMEN_FRAME_LOG"] = "1"
    if args.backend == "wgpu" and args.present:
        env["LUMEN_PRESENT"] = args.present

    stderr_path = os.path.join(args.tmpdir, f"lumen-mcp-bench-{run_idx}.log")
    result = {"run": run_idx, "backend": args.backend, "page": args.page,
              "driver": "mcp"}
    with open(stderr_path, "w") as errfh:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(port), args.page],
            env=env, stdout=subprocess.DEVNULL, stderr=errfh, text=True,
        )
    sampler = Sampler(proc.pid)
    sampler.start()
    try:
        deadline = time.monotonic() + 30
        rpc = None
        while time.monotonic() < deadline:
            try:
                rpc = mcp_rpc_factory(port)
                break
            except OSError:
                time.sleep(0.5)
        if rpc is None:
            result["error"] = "MCP port never came up"
            return result
        # The MCP port opens before the window is mapped — raising too early
        # is a silent no-op and the occluded window then starves the
        # swapchain. Wait for the map, raise, and raise once more to be sure.
        time.sleep(2)
        if not args.no_raise:
            kwin_keep_above(args.tmpdir)
            time.sleep(1)
            kwin_keep_above(args.tmpdir)
        time.sleep(1)  # settle: first frames after raise

        # Everything logged before the scroll loop is startup, not scroll cost.
        skip_bytes = os.path.getsize(stderr_path)

        def scroll_y():
            return last_scroll_y(stderr_path)

        def frames_seen():
            try:
                with open(stderr_path) as fh:
                    fh.seek(skip_bytes)
                    return len(FRAME_RE.findall(fh.read()))
            except OSError:
                return 0

        t0 = time.monotonic()
        px_total = 0.0
        passes = 0
        direction = 1
        stalled = 0
        while passes < args.passes and time.monotonic() - t0 < args.timeout:
            before = scroll_y()
            frames_before = frames_seen()
            rpc("tools/call", {"name": "scroll", "arguments": {
                "target": {"selector": "body"},
                "delta": {"x": 0, "y": args.step * direction}}})
            # Pace to the renderer, like real input: wait for the repaint this
            # scroll triggered before sending the next one. Submitting scrolls
            # faster than the GPU drains them floods the queue on GPU-bound
            # pages until swapchain acquire times out (~10 s stalls).
            wait_deadline = time.monotonic() + 5
            while frames_seen() <= frames_before and time.monotonic() < wait_deadline:
                time.sleep(0.01)
            after = scroll_y()
            if before is not None and after is not None:
                moved = abs(after - before)
                px_total += moved
                # Edge reached: the page stopped moving in this direction.
                if moved < args.step / 2:
                    stalled += 1
                    if stalled >= 2:
                        direction = -direction
                        passes += 1
                        stalled = 0
                else:
                    stalled = 0
        wall = time.monotonic() - t0
        result["passes"] = passes
        result["px_total"] = round(px_total)
        result["scroll_speed_px_s"] = round(px_total / wall) if wall > 0 else 0
        result["wall_s"] = round(wall, 1)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        sampler.stop_flag.set()
        sampler.join(timeout=2)

    with open(stderr_path) as fh:
        fh.seek(skip_bytes)
        tail = fh.read()
    frames = [float(m) for m in FRAME_RE.findall(tail)]
    if frames:
        result["frame"] = percentiles(frames)
        result["rendered"] = len(frames)
    else:
        result["error"] = "no [frame] lines captured during scroll"
    if "SurfaceLost" in tail:
        result["surface_lost"] = tail.count("SurfaceLost")
    if s := sampler.summary():
        result["proc"] = s
    return result


def one_run(args, run_idx):
    env = os.environ.copy()
    env["LUMEN_BENCH"] = f"scroll:{args.frames}:{args.warmup}:{args.step}"
    env["LUMEN_BACKEND"] = args.backend
    if args.backend == "wgpu" and args.present:
        env["LUMEN_PRESENT"] = args.present

    proc = subprocess.Popen(
        [args.binary, args.page],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    sampler = Sampler(proc.pid)
    sampler.start()
    if not args.no_raise:
        time.sleep(2.5)
        kwin_keep_above(args.tmpdir)

    result = {"run": run_idx, "backend": args.backend, "page": args.page}
    try:
        stderr = ""
        try:
            _, stderr = proc.communicate(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            proc.send_signal(signal.SIGKILL)
            _, stderr = proc.communicate()
            result["error"] = f"timeout after {args.timeout}s"
    finally:
        sampler.stop_flag.set()
        sampler.join(timeout=2)

    m = BENCH_RE.search(stderr)
    if m:
        result["frame"] = {
            k: float(m.group(k))
            for k in ("median", "p95", "max", "min", "mean", "total")
        }
        result["rendered"] = int(m.group("rendered"))
        result["passes"] = int(m.group("passes"))
        result["scroll_speed_px_s"] = int(m.group("speed"))
    else:
        result["error"] = result.get("error") or "no [bench] report in stderr"
        result["stderr_tail"] = stderr.strip().splitlines()[-8:]
    if fm := FIRST_FRAME_RE.search(stderr):
        result["first_frame_ms"] = int(fm.group(1))
    if gm := GEOMETRY_RE.search(stderr):
        result["geometry"] = gm.group(1)
    if "INVALID" in stderr:
        result["invalid"] = True
    if s := sampler.summary():
        result["proc"] = s
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--page", required=True)
    ap.add_argument("--backend", default="wgpu", choices=["wgpu", "femtovg"])
    ap.add_argument("--frames", type=int, default=600)
    ap.add_argument("--warmup", type=int, default=30)
    ap.add_argument("--step", type=float, default=60)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--present", default="immediate",
                    help="wgpu present mode (immediate|mailbox|fifo); "
                         "'fifo' keeps vsync")
    ap.add_argument("--binary", default="target/release/lumen")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--json", help="append results (one JSON object per line)")
    ap.add_argument("--no-raise", action="store_true",
                    help="do not keep-above the window via KWin scripting")
    ap.add_argument("--tmpdir", default=os.environ.get("TMPDIR", "/tmp"))
    ap.add_argument("--driver", default="mcp", choices=["mcp", "bench"],
                    help="mcp: drive scroll through the MCP live window "
                         "(interactive path, robust); bench: LUMEN_BENCH "
                         "harness (stalls on Wayland acquire as of 2026-07-12)")
    ap.add_argument("--passes", type=int, default=2,
                    help="mcp driver: full edge-to-edge passes to scroll")
    args = ap.parse_args()
    if args.present == "fifo":
        args.present = None

    runs = []
    for i in range(1, args.runs + 1):
        r = one_run_mcp(args, i) if args.driver == "mcp" else one_run(args, i)
        runs.append(r)
        if "frame" in r:
            f, p = r["frame"], r.get("proc", {})
            print(
                f"run {i}: median {f['median']:.2f}ms p95 {f['p95']:.2f}ms "
                f"mean {f['mean']:.2f}ms | passes {r.get('passes', '?')} "
                f"speed {r.get('scroll_speed_px_s', '?')}px/s | "
                f"cpu {p.get('cpu_pct', '?')}% pss_peak {p.get('pss_peak_mib', '?')}MiB"
                + (" | INVALID" if r.get("invalid") else "")
                + (f" | SurfaceLost×{r['surface_lost']}" if r.get("surface_lost") else "")
            )
        else:
            print(f"run {i}: FAILED — {r.get('error')}")
            for line in r.get("stderr_tail", []):
                print(f"    {line}")
        time.sleep(1)

    ok = [r for r in runs if "frame" in r and not r.get("invalid")]
    if ok:
        medians = sorted(r["frame"]["median"] for r in ok)
        print(
            f"== {args.backend} {os.path.basename(args.page)} step={args.step}: "
            f"median-of-medians {medians[len(medians) // 2]:.2f}ms "
            f"({len(ok)}/{len(runs)} valid runs)"
        )
    if args.json:
        with open(args.json, "a") as fh:
            for r in runs:
                fh.write(json.dumps(r, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
