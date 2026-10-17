"""runwpt.py [-j N] [-t SEC] <file-or-prefix under tests/wpt> ...
Runs testharness tests with `lumen --no-paint <url>` (no BiDi) and reads the result
from the `[JS] LUMENRESULT {json}` console line. Prints per-test summary and,
with -v, the failing subtests."""
import functools, http.server, json, os, re, socket, subprocess, sys, threading, time, concurrent.futures as cf

sys.stdout.reconfigure(encoding="utf-8")
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
WPT = os.path.join(REPO, "tests", "wpt")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # poke.py рядом
import poke
EXE = os.environ.get("LUMEN_EXE") or os.path.join(REPO, "target", "debug", "lumen.exe")

REPORT = (b"add_completion_callback(function (tests, status) { console.log('LUMENRESULT ' + JSON.stringify("
          b"{s: status.status, m: status.message, t: tests.map(function (t) { return [t.name, t.status, t.message || '']; })})); });")


class H(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        if self.path.split("?")[0] == "/resources/testharnessreport.js":
            self.send_response(200); self.send_header("Content-Type", "text/javascript")
            self.send_header("Content-Length", str(len(REPORT))); self.end_headers(); self.wfile.write(REPORT); return
        super().do_GET()
    def guess_type(self, path):
        if path.endswith(".mp4"): return "video/mp4"
        if path.endswith(".webm"): return "video/webm"
        if path.endswith(".mp3"): return "audio/mpeg"
        return super().guess_type(path)


srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(H, directory=WPT))
threading.Thread(target=srv.serve_forever, daemon=True).start()
PORT = srv.server_port


def run_one(rel, timeout):
    url = f"http://127.0.0.1:{PORT}/{rel}"
    t0 = time.time()
    p = subprocess.Popen([EXE, "--no-paint", url], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                         text=True, encoding="utf-8", errors="replace")
    res = {"s": "TIMEOUT", "t": [], "m": ""}
    errs = []
    tail = []
    def reader():
        nonlocal res
        for line in p.stderr:
            if "LUMENRESULT " in line:
                try:
                    res = json.loads(line.split("LUMENRESULT ", 1)[1])
                except Exception as e:
                    res = {"s": "BADJSON", "m": str(e), "t": []}
                return
            tail.append(line.rstrip()[:200]); del tail[:-int(os.environ.get("TAILN","12"))]
            if "JS error" in line or "script error" in line:
                errs.append(line.strip()[:200])
    th = threading.Thread(target=reader, daemon=True); th.start()
    end = time.time() + timeout
    while th.is_alive() and time.time() < end:
        th.join(1.0)
        if th.is_alive():
            try: poke.poke(p.pid)
            except Exception: pass
    subprocess.run(["taskkill", "/F", "/T", "/PID", str(p.pid)], capture_output=True)
    if os.environ.get('TAIL'): print(chr(10).join(tail))
    return rel, res, errs, time.time() - t0


def main():
    args = sys.argv[1:]
    jobs, timeout, verbose = 3, 40, False
    names = []
    while args:
        a = args.pop(0)
        if a == "-j": jobs = int(args.pop(0))
        elif a == "-t": timeout = float(args.pop(0))
        elif a == "-v": verbose = True
        else: names.append(a)
    files = []
    for n in names:
        full = os.path.join(WPT, n)
        if os.path.isdir(full):
            for f in sorted(os.listdir(full)):
                if f.endswith(".html") and not f.endswith(".https.html"):
                    files.append(f"{n.rstrip('/')}/{f}")
        else:
            files.append(n)
    total = {"pass": 0, "all": 0}
    with cf.ThreadPoolExecutor(jobs) as ex:
        for rel, res, errs, dt in ex.map(lambda f: run_one(f, timeout), files):
            t = res.get("t", [])
            npass = sum(1 for x in t if x[1] == 0)
            total["pass"] += npass; total["all"] += len(t)
            print(f"{rel}: {res.get('s')} {npass}/{len(t)} ({dt:.0f}s)" + (f" ERR={errs[0]}" if errs else ""), flush=True)
            if verbose:
                if res.get("m"): print("   harness:", str(res["m"])[:200])
                for x in t:
                    if x[1] != 0:
                        print(f"   [{x[1]}] {x[0][:80]} :: {str(x[2])[:150]}")
    print(f"TOTAL {total['pass']}/{total['all']}")


main()
