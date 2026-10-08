#!/usr/bin/env python3
"""База отзывчивости Chromium для сравнения с Lumen (BUG-935): тот же сценарий,
что у `bug935_raf_relayout_census.py`, метрики — из CDP-трейса.

Сценарий на сайт и прогон: свежий профиль, окно `--start-maximized`, навигация,
ожидание `readyState == complete`, `--settle-s` секунд простоя, `--ticks`
щелчков колеса по `--delta` px с паузой 200 мс, 1 с хвоста. Границы фаз —
`performance.mark` в странице, в трейсе они лежат на той же шкале, что и
события движка.

Метрики по фазам (`load` / `settle` / `scroll`), только главный поток рендерера
вкладки (`CrRendererMain` того процесса, где стоят метки):
  * `style` — `UpdateLayoutTree` (пересчёт стилей), `layout` — `Layout`:
    число, p50/p90/max, сумма, мс. Аналог Lumen — `maybe_flush done` и
    `[engine] relayout`.
  * `long_tasks` — `RunTask` дольше 50 мс: число, сумма и TBT (сумма сверх 50 мс).
  * `latency_scroll` (фаза `scroll`) — `EventLatency` первого скролл-жеста:
    от события колеса до кадра со сдвинутой страницей, p50/p90/max, мс;
    `latency_wheel` — до обработки события `wheel` страницей.
  * `frames` (фаза `scroll`) — `PipelineReporter`: кадры показанные целиком,
    частично и сброшенные.
  * `cdp_wheel_rtt` — время ответа `Input.dispatchMouseEvent`, грубый аналог
    RTT MCP `scroll` у Lumen.

Оговорка: прокрутку Chromium ведёт поток композитора, главный поток в ней не
участвует, поэтому `latency_scroll` у него почти не зависит от занятости JS.
Работу, которую оба браузера делают на главном потоке, сравнивают
`style`/`layout`/`long_tasks`.

    python scripts/chrome_interaction_baseline.py [url ...] [--runs N]
        [--settle-s S] [--ticks N] [--delta PX] [--binary chrome.exe]
        [--json docs/perf/runs/<дата>-chrome-interaction.json]

По умолчанию бинарник — портабельный Chromium из кэша Playwright
(`%LOCALAPPDATA%/ms-playwright/chromium-*/chrome-win64/chrome.exe`, самый новый).
"""

from __future__ import annotations

import argparse
import base64
import glob
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench_chromium import WS, free_port  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

DEFAULT_URLS = ['https://ria.ru', 'https://lenta.ru', 'https://www.rbc.ru']

TRACE_CATEGORIES = ','.join([
    'devtools.timeline',
    'disabled-by-default-devtools.timeline',
    'disabled-by-default-devtools.timeline.frame',
    'blink.user_timing',
    'toplevel',
    'input',
    'latencyInfo',
    'benchmark',
    'cc',
    'viz',
])

PHASES = ('load', 'settle', 'scroll')
MARK_PREFIX = 'lumen-baseline-'
LONG_TASK_MS = 50.0
# `scroll` — от события колеса до первого кадра со сдвинутой страницей (то,
# что видит пользователь); `wheel` — до обработки `wheel`-события страницей.
LATENCY_KINDS = {
    'scroll': ('FIRST_GESTURE_SCROLL_UPDATE', 'GESTURE_SCROLL_UPDATE'),
    'wheel': ('MOUSE_WHEEL',),
}


class Cdp:
    """CDP поверх `bench_chromium.WS`, но события не выбрасываются, а копятся."""

    def __init__(self, ws: WS) -> None:
        self.ws = ws
        self.id = 0
        self.events: list[dict] = []

    def call(self, method: str, params: dict | None = None, timeout: float = 30) -> dict:
        self.id += 1
        mid = self.id
        self.ws.send(json.dumps({'id': mid, 'method': method, 'params': params or {}}))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            msg = json.loads(self.ws.recv())
            if msg.get('id') == mid:
                if 'error' in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get('result', {})
            if 'method' in msg:
                self.events.append(msg)
        raise TimeoutError(method)

    def wait_event(self, method: str, timeout: float = 120) -> dict:
        for i, ev in enumerate(self.events):
            if ev['method'] == method:
                return self.events.pop(i)
        self.ws.sock.settimeout(timeout)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            msg = json.loads(self.ws.recv())
            if msg.get('method') == method:
                return msg
            if 'method' in msg:
                self.events.append(msg)
        raise TimeoutError(method)

    def eval(self, expr: str, timeout: float = 30):
        r = self.call('Runtime.evaluate', {'expression': expr, 'returnByValue': True}, timeout)
        return r.get('result', {}).get('value')


def default_binary() -> str | None:
    root = os.path.join(os.environ.get('LOCALAPPDATA', ''), 'ms-playwright')
    found = glob.glob(os.path.join(root, 'chromium-*', 'chrome-win64', 'chrome.exe'))
    found.sort(key=lambda p: int(re.search(r'chromium-(\d+)', p).group(1)))
    return found[-1] if found else None


def chromium_version(binary: str) -> str:
    # `chrome.exe --version` на Windows открывает окно, а не печатает версию;
    # версия — имя `<версия>.manifest` рядом с бинарником.
    for name in os.listdir(os.path.dirname(binary)):
        if name.endswith('.manifest') and name[0].isdigit():
            return name[:-len('.manifest')]
    return 'unknown'


def page_ws_url(port: int, timeout: float = 30) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/json', timeout=2))
            for t in data:
                if t.get('type') == 'page' and t.get('webSocketDebuggerUrl'):
                    return t['webSocketDebuggerUrl']
        except OSError:
            pass
        time.sleep(0.3)
    raise RuntimeError('нет CDP-цели page')


def read_trace(cdp: Cdp, handle: str) -> list[dict]:
    chunks = []
    while True:
        r = cdp.call('IO.read', {'handle': handle, 'size': 1 << 20}, timeout=60)
        data = r.get('data', '')
        chunks.append(base64.b64decode(data) if r.get('base64Encoded') else data.encode())
        if r.get('eof'):
            break
    cdp.call('IO.close', {'handle': handle})
    doc = json.loads(b''.join(chunks).decode('utf-8', errors='replace'))
    return doc['traceEvents'] if isinstance(doc, dict) else doc


def same_site(url: str, location: str) -> bool:
    def host(u: str) -> str:
        h = urllib.parse.urlsplit(u).hostname or ''
        return h[4:] if h.startswith('www.') else h
    return host(url) == host(location)


def pct(values: list[float], p: float) -> float:
    s = sorted(values)
    return s[min(len(s) - 1, int(round(p * (len(s) - 1))))]


def dist(values: list[float]) -> dict:
    if not values:
        return {'n': 0}
    return {'n': len(values), 'p50': round(pct(values, 0.5), 2), 'p90': round(pct(values, 0.9), 2),
            'max': round(max(values), 2), 'sum': round(sum(values), 1)}


def analyze(events: list[dict]) -> dict:
    marks = {}
    for e in events:
        name = e.get('name', '')
        if name.startswith(MARK_PREFIX) and e.get('ph') in ('R', 'I', 'n', 'b', 'i'):
            marks.setdefault(name[len(MARK_PREFIX):], (e['ts'], e['pid']))
    missing = [m for m in ('settle', 'scroll', 'end') if m not in marks]
    if missing:
        return {'error': f'в трейсе нет меток {missing}'}
    pid = marks['settle'][1]
    tid = next((e['tid'] for e in events if e.get('ph') == 'M' and e.get('pid') == pid
                and e.get('name') == 'thread_name'
                and e.get('args', {}).get('name') == 'CrRendererMain'), None)
    if tid is None:
        return {'error': f'нет CrRendererMain в процессе {pid}'}

    t_settle, t_scroll, t_end = marks['settle'][0], marks['scroll'][0], marks['end'][0]

    def phase_of(ts: float) -> str | None:
        if ts < t_settle:
            return 'load'
        if ts < t_scroll:
            return 'settle'
        if ts < t_end:
            return 'scroll'
        return None

    style = {p: [] for p in PHASES}
    layout = {p: [] for p in PHASES}
    tasks = {p: [] for p in PHASES}
    for e in events:
        if e.get('ph') != 'X' or e.get('pid') != pid or e.get('tid') != tid or 'dur' not in e:
            continue
        ph = phase_of(e['ts'])
        if ph is None:
            continue
        ms = e['dur'] / 1000.0
        name = e.get('name')
        if name == 'UpdateLayoutTree':
            style[ph].append(ms)
        elif name == 'Layout':
            layout[ph].append(ms)
        elif name == 'RunTask' and ms > LONG_TASK_MS:
            tasks[ph].append(ms)

    # EventLatency и PipelineReporter — асинхронные пары b/e (id2.local) в
    # браузерном процессе; к фазе относим по времени начала.
    open_async: dict[tuple, dict] = {}
    latency: dict[str, list[float]] = {k: [] for k in LATENCY_KINDS}
    frames = {'presented': 0, 'partial': 0, 'dropped': 0}
    for e in events:
        name = e.get('name')
        if name not in ('EventLatency', 'PipelineReporter') or e.get('ph') not in ('b', 'e'):
            continue
        key = (name, e.get('pid'), e.get('id') or e.get('id2', {}).get('local'))
        if e['ph'] == 'b':
            open_async[key] = e
            continue
        b = open_async.pop(key, None)
        if b is None or phase_of(b['ts']) != 'scroll':
            continue
        if name == 'EventLatency':
            etype = b.get('args', {}).get('event_latency', {}).get('event_type', '')
            for kind, types in LATENCY_KINDS.items():
                if etype in types:
                    latency[kind].append((e['ts'] - b['ts']) / 1000.0)
        else:
            state = b.get('args', {}).get('frame_reporter', {}).get('state', '')
            if state == 'STATE_PRESENTED_ALL':
                frames['presented'] += 1
            elif state == 'STATE_PRESENTED_PARTIAL':
                frames['partial'] += 1
            elif state == 'STATE_DROPPED':
                frames['dropped'] += 1

    out = {'phase_s': {'settle': round((t_scroll - t_settle) / 1e6, 2),
                       'scroll': round((t_end - t_scroll) / 1e6, 2)}}
    for ph in PHASES:
        out[ph] = {
            'style': dist(style[ph]),
            'layout': dist(layout[ph]),
            'long_tasks': {'n': len(tasks[ph]), 'sum': round(sum(tasks[ph]), 1),
                           'tbt': round(sum(t - LONG_TASK_MS for t in tasks[ph]), 1),
                           'max': round(max(tasks[ph]), 1) if tasks[ph] else 0.0},
        }
    for kind, vals in latency.items():
        out['scroll'][f'latency_{kind}'] = dist(vals)
    out['scroll']['frames'] = frames
    return out


def one_run(args, url: str, idx: int) -> dict:
    port = free_port()
    profile = os.path.join(REPO, '.tmp', f'chrome-baseline-profile-{idx}')
    shutil.rmtree(profile, ignore_errors=True)
    cmd = [args.binary, f'--remote-debugging-port={port}', f'--user-data-dir={profile}',
           '--no-first-run', '--no-default-browser-check', '--start-maximized',
           '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding',
           'about:blank']
    result: dict = {'url': url, 'run': idx}
    proc = None
    try:
        # Сразу после закрытия прошлого экземпляра DevTools иногда не поднимается
        # за отведённое время — один перезапуск.
        for attempt in range(2):
            proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                ws_url = page_ws_url(port, timeout=45)
                break
            except RuntimeError:
                proc.kill()
                proc.wait(timeout=10)
                time.sleep(3.0)
                if attempt == 1:
                    raise
        cdp = Cdp(WS(ws_url))
        cdp.call('Page.enable')
        cdp.call('Tracing.start', {'transferMode': 'ReturnAsStream',
                                   'traceConfig': {'includedCategories': TRACE_CATEGORIES.split(','),
                                                   'recordMode': 'recordContinuously'}})
        t0 = time.monotonic()
        cdp.call('Page.navigate', {'url': url}, timeout=60)
        deadline = time.monotonic() + args.load_timeout_s
        # Сразу после `Page.navigate` `Runtime.evaluate` ещё может попасть в
        # старый `about:blank` с `readyState == complete`.
        ready_expr = "location.href !== 'about:blank' && document.readyState === 'complete'"
        while time.monotonic() < deadline:
            if cdp.eval(ready_expr) is True:
                break
            time.sleep(0.5)
        result['ready_s'] = round(time.monotonic() - t0, 2)
        result['ready_complete'] = cdp.eval(ready_expr) is True
        result['location'] = cdp.eval('location.href')
        result['nav_timing'] = cdp.eval(
            "(() => { const n = performance.getEntriesByType('navigation')[0];"
            " return n ? {dcl: Math.round(n.domContentLoadedEventEnd),"
            " load: Math.round(n.loadEventEnd), transfer: n.transferSize} : null; })()")
        # Сайт может увести на чужой хост (lenta.ru → id.sber.ru, «aggressivelogin»):
        # такой прогон мерит другую страницу.
        if not same_site(url, result['location'] or ''):
            raise RuntimeError(f"страница ушла на другой хост: {result['location'][:80]}")
        cdp.eval(f"performance.mark('{MARK_PREFIX}settle')")
        time.sleep(args.settle_s)
        vw = cdp.eval('[innerWidth, innerHeight]') or [1600, 900]
        result['viewport'] = vw
        cdp.eval(f"performance.mark('{MARK_PREFIX}scroll')")
        rtts = []
        for _ in range(args.ticks):
            t = time.monotonic()
            cdp.call('Input.dispatchMouseEvent', {'type': 'mouseWheel', 'x': vw[0] // 2,
                                                  'y': vw[1] // 2, 'deltaX': 0,
                                                  'deltaY': args.delta})
            rtts.append((time.monotonic() - t) * 1000.0)
            time.sleep(0.2)
        time.sleep(1.0)
        cdp.eval(f"performance.mark('{MARK_PREFIX}end')")
        result['scroll_y'] = cdp.eval('scrollY')
        result['cdp_wheel_rtt'] = dist(rtts)
        cdp.call('Tracing.end')
        done = cdp.wait_event('Tracing.tracingComplete', timeout=180)
        events = read_trace(cdp, done['params']['stream'])
        if args.save_trace:
            os.makedirs(args.save_trace, exist_ok=True)
            slug = url.split('//', 1)[-1].strip('/').replace('/', '_')
            with open(os.path.join(args.save_trace, f'{slug}-{idx}.json'), 'w',
                      encoding='utf-8') as fh:
                json.dump(events, fh)
        result['trace_events'] = len(events)
        result.update(analyze(events))
    except Exception as e:  # прогон одного сайта не должен ронять остальные
        result['error'] = f'{type(e).__name__}: {e}'
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
        time.sleep(3.0)
        shutil.rmtree(profile, ignore_errors=True)
    return result


def summarize(runs: list[dict]) -> dict:
    """Медиана по прогонам для ключевых чисел сайта."""
    ok = [r for r in runs if 'error' not in r]
    if not ok:
        return {'ok_runs': 0}

    def med(path: list[str]) -> float | None:
        vals = []
        for r in ok:
            v = r
            for k in path:
                v = v.get(k, {}) if isinstance(v, dict) else {}
            if isinstance(v, (int, float)):
                vals.append(v)
        return round(statistics.median(vals), 2) if vals else None

    s = {'ok_runs': len(ok)}
    for ph in ('settle', 'scroll'):
        for m in ('style', 'layout'):
            for k in ('n', 'p50', 'p90', 'max', 'sum'):
                s[f'{ph}.{m}.{k}'] = med([ph, m, k])
        for k in ('n', 'tbt', 'max'):
            s[f'{ph}.long_tasks.{k}'] = med([ph, 'long_tasks', k])
    for kind in LATENCY_KINDS:
        for k in ('n', 'p50', 'p90', 'max'):
            s[f'scroll.latency_{kind}.{k}'] = med(['scroll', f'latency_{kind}', k])
    for k in ('presented', 'partial', 'dropped'):
        s[f'scroll.frames.{k}'] = med(['scroll', 'frames', k])
    for k in ('p50', 'p90', 'max'):
        s[f'cdp_wheel_rtt.{k}'] = med(['cdp_wheel_rtt', k])
    return s


def print_summary(url: str, s: dict) -> None:
    print(f'\n== {url}  (успешных прогонов: {s.get("ok_runs", 0)})')
    if not s.get('ok_runs'):
        return
    for ph in ('settle', 'scroll'):
        for m in ('style', 'layout'):
            print(f'  {ph:6} {m:6} n={s[f"{ph}.{m}.n"]} p50={s[f"{ph}.{m}.p50"]} '
                  f'p90={s[f"{ph}.{m}.p90"]} max={s[f"{ph}.{m}.max"]} sum={s[f"{ph}.{m}.sum"]} мс')
        print(f'  {ph:6} long>50мс n={s[f"{ph}.long_tasks.n"]} tbt={s[f"{ph}.long_tasks.tbt"]} '
              f'max={s[f"{ph}.long_tasks.max"]} мс')
    for kind, label in (('scroll', 'колесо→кадр со сдвигом'), ('wheel', 'колесо→wheel в JS')):
        print(f'  {label}: n={s[f"scroll.latency_{kind}.n"]} p50={s[f"scroll.latency_{kind}.p50"]} '
              f'p90={s[f"scroll.latency_{kind}.p90"]} max={s[f"scroll.latency_{kind}.max"]} мс')
    print(f'  кадры: целиком {s["scroll.frames.presented"]}, частично {s["scroll.frames.partial"]}, '
          f'сброшено {s["scroll.frames.dropped"]}')
    print(f'  RTT Input.dispatchMouseEvent p50={s["cdp_wheel_rtt.p50"]} '
          f'p90={s["cdp_wheel_rtt.p90"]} max={s["cdp_wheel_rtt.max"]} мс')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('urls', nargs='*', default=DEFAULT_URLS)
    ap.add_argument('--runs', type=int, default=3)
    ap.add_argument('--settle-s', type=float, default=15.0)
    ap.add_argument('--ticks', type=int, default=20)
    ap.add_argument('--delta', type=float, default=400.0)
    ap.add_argument('--load-timeout-s', type=float, default=60.0)
    ap.add_argument('--binary', default=default_binary())
    ap.add_argument('--json', help='куда записать сырые прогоны и сводку')
    ap.add_argument('--save-trace', help='каталог для сырых трейсов (по файлу на прогон)')
    args = ap.parse_args()
    if not args.binary or not os.path.exists(args.binary):
        print('не найден chrome.exe: укажите --binary', file=sys.stderr)
        return 1

    version = chromium_version(args.binary)
    print(f'Chromium {version}: {args.binary}')
    report = {'date': time.strftime('%Y-%m-%d'), 'browser': f'Chromium {version}',
              'scenario': {'settle_s': args.settle_s, 'ticks': args.ticks, 'delta': args.delta,
                           'window': 'maximized'},
              'sites': {}}
    for url in args.urls:
        runs = []
        for i in range(args.runs):
            r = one_run(args, url, i)
            print(f'  {url} прогон {i + 1}/{args.runs}: '
                  + (r['error'] if 'error' in r else f"ready {r['ready_s']}с, событий трейса {r['trace_events']}"))
            runs.append(r)
        s = summarize(runs)
        print_summary(url, s)
        report['sites'][url] = {'summary': s, 'runs': runs}
    if args.json:
        with open(args.json, 'w', encoding='utf-8', newline='\n') as fh:
            json.dump(report, fh, ensure_ascii=False, indent=1)
            fh.write('\n')
        print(f'\nзаписано: {args.json}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
