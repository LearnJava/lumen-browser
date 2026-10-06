#!/usr/bin/env python3
"""Прогон плавности прокрутки колесом: Lumen и Chromium на одном вводе (THREAD-5).

Колесо — настоящее, через Win32 `SendInput` в окно на переднем плане (CDP/MCP
обходят путь ОС). Сценарий один на оба браузера: окно на весь экран, загрузка,
`--settle-s` простоя, `--bursts` серий по `--ticks` щелчков с шагом
`--tick-ms`, между сериями `--burst-pause-ms`. Мышь ставится в центр окна —
пока идёт прогон, руками мышь и окно не трогать.

  * Lumen — `LUMEN_PRESENT_LOG` (crates/shell/src/present_log.rs): метка на
    каждый present и на каждый щелчок; метрики — `scroll_smoothness.analyze`.
  * Chromium — CDP-трейс на время серий, present = конец `PipelineReporter` с
    `STATE_PRESENTED_*`; задержка — `EventLatency` первого жеста прокрутки.

    python scripts/scroll_smoothness_run.py lumen|chromium URL [URL ...]
        [--runs N] [--period-ms 16.66] [--json out.json]
        [--lumen target/dev-release/lumen.exe] [--binary chrome.exe]
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
import shutil
import subprocess
import sys
import time
from ctypes import wintypes

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import scroll_smoothness as ss  # noqa: E402
import chrome_interaction_baseline as cib  # noqa: E402
from bench_chromium import WS, free_port  # noqa: E402

user32 = ctypes.windll.user32
ULONG_PTR = ctypes.c_size_t
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
user32.SetForegroundWindow.argtypes = [wintypes.HWND]
user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [('dx', wintypes.LONG), ('dy', wintypes.LONG), ('mouseData', wintypes.DWORD),
                ('dwFlags', wintypes.DWORD), ('time', wintypes.DWORD), ('dwExtraInfo', ULONG_PTR)]


class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [('mi', MOUSEINPUT)]
    _anonymous_ = ('u',)
    _fields_ = [('type', wintypes.DWORD), ('u', _U)]


MOUSEEVENTF_WHEEL = 0x0800


def wheel_click(delta: int = -120) -> None:
    inp = INPUT(type=0, mi=MOUSEINPUT(0, 0, delta & 0xFFFFFFFF, MOUSEEVENTF_WHEEL, 0, 0))
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


def find_window(pid: int, timeout: float = 30) -> int:  # noqa: D103
    found: list[int] = []
    cb_t = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(hwnd, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid and user32.IsWindowVisible(hwnd) and user32.GetWindowTextLengthW(hwnd) > 0:
            found.append(hwnd)
        return True
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        user32.EnumWindows(cb_t(cb), 0)
        if found:
            return found[0]
        time.sleep(0.3)
    raise RuntimeError(f'нет окна у pid {pid}')


def focus_center(hwnd: int) -> None:
    user32.ShowWindow(hwnd, 3)  # SW_MAXIMIZE
    user32.keybd_event(0x12, 0, 0, 0)  # Alt: разрешает SetForegroundWindow
    user32.SetForegroundWindow(hwnd)
    user32.keybd_event(0x12, 0, 2, 0)
    time.sleep(0.5)
    r = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    user32.SetCursorPos((r.left + r.right) // 2, (r.top + r.bottom) // 2 + 100)
    time.sleep(0.2)


def send_scenario(args) -> None:
    for _ in range(args.bursts):
        for _ in range(args.ticks):
            wheel_click()
            time.sleep(args.tick_ms / 1000)
        time.sleep(args.burst_pause_ms / 1000)


def run_lumen(args, url: str, idx: int) -> dict:
    log = os.path.join(REPO, '.tmp', f'present-{idx}.log')
    os.makedirs(os.path.dirname(log), exist_ok=True)
    env = dict(os.environ, LUMEN_PRESENT_LOG=log, LUMEN_NO_ADBLOCK='1')
    proc = subprocess.Popen([args.lumen, '--maximized', url], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        hwnd = find_window(proc.pid)
        time.sleep(args.load_s + args.settle_s)
        focus_center(hwnd)
        mark = os.path.getsize(log) if os.path.exists(log) else 0
        send_scenario(args)
        time.sleep(1.0)
    finally:
        proc.kill()
        proc.wait(timeout=10)
    # кадры загрузки отрезаем: берём только строки после начала серий
    with open(log, 'rb') as f:
        f.seek(mark)
        tail = f.read().decode()
    part = os.path.join(REPO, '.tmp', f'present-{idx}-scroll.log')
    with open(part, 'w') as f:
        f.write(tail)
    frames, wheels = ss.parse(part)
    return {'url': url, 'browser': 'lumen', **ss.analyze(frames, wheels, args.period_ms, 100.0)}


def run_chromium(args, url: str, idx: int) -> dict:
    port = free_port()
    profile = os.path.join(REPO, '.tmp', f'scroll-chrome-profile-{idx}')
    shutil.rmtree(profile, ignore_errors=True)
    cmd = [args.binary, f'--remote-debugging-port={port}', f'--user-data-dir={profile}',
           '--no-first-run', '--no-default-browser-check', '--start-maximized', 'about:blank']
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        cdp = cib.Cdp(WS(cib.page_ws_url(port, timeout=45)))
        cdp.call('Page.enable')
        cdp.call('Page.navigate', {'url': url}, timeout=60)
        expr = "location.href !== 'about:blank' && document.readyState === 'complete'"
        deadline = time.monotonic() + args.load_s + 60
        while time.monotonic() < deadline and cdp.eval(expr) is not True:
            time.sleep(0.5)
        time.sleep(args.settle_s)
        focus_center(find_window(proc.pid))
        cdp.call('Tracing.start', {'transferMode': 'ReturnAsStream',
                                   'traceConfig': {'includedCategories': cib.TRACE_CATEGORIES.split(','),
                                                   'recordMode': 'recordContinuously'}})
        send_scenario(args)
        time.sleep(1.0)
        cdp.call('Tracing.end')
        done = cdp.wait_event('Tracing.tracingComplete', timeout=120)
        events = cib.read_trace(cdp, done['params']['stream'])
    finally:
        proc.kill()
        proc.wait(timeout=10)
    opened: dict = {}
    frames: list[float] = []
    lat: list[float] = []
    for e in events:
        name = e.get('name')
        if name not in ('PipelineReporter', 'EventLatency') or e.get('ph') not in ('b', 'e'):
            continue
        key = (name, e.get('pid'), e.get('id') or e.get('id2', {}).get('local'))
        if e['ph'] == 'b':
            opened[key] = e
            continue
        b = opened.pop(key, None)
        if b is None:
            continue
        if name == 'PipelineReporter':
            st = b.get('args', {}).get('frame_reporter', {}).get('state', '')
            if st.startswith('STATE_PRESENTED'):
                frames.append(e['ts'] / 1000.0)
        elif b.get('args', {}).get('event_latency', {}).get('event_type') == 'FIRST_GESTURE_SCROLL_UPDATE':
            lat.append((e['ts'] - b['ts']) / 1000.0)
    # один present виден в трейсе несколькими PipelineReporter (разные
    # процессы/потоки): сводим метки ближе 2 мс в одну
    uniq: list[float] = []
    for t in sorted(frames):
        if not uniq or t - uniq[-1] > 2.0:
            uniq.append(t)
    r = ss.analyze(uniq, [], args.period_ms, 100.0)
    if lat:
        r.update(latency_ms_median=round(sorted(lat)[len(lat) // 2], 1),
                 latency_ms_max=round(max(lat), 1), bursts=len(lat))
    return {'url': url, 'browser': 'chromium', **r}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument('browser', choices=['lumen', 'chromium'])
    ap.add_argument('urls', nargs='+')
    ap.add_argument('--runs', type=int, default=3)
    ap.add_argument('--period-ms', type=float, default=16.66)
    ap.add_argument('--load-s', type=float, default=20)
    ap.add_argument('--settle-s', type=float, default=5)
    ap.add_argument('--bursts', type=int, default=5)
    ap.add_argument('--ticks', type=int, default=6)
    ap.add_argument('--tick-ms', type=float, default=40)
    ap.add_argument('--burst-pause-ms', type=float, default=800)
    ap.add_argument('--lumen', default=os.path.join(REPO, 'target', 'dev-release', 'lumen.exe'))
    ap.add_argument('--binary', default=cib.default_binary())
    ap.add_argument('--json')
    args = ap.parse_args()
    results = []
    for url in args.urls:
        for i in range(args.runs):
            fn = run_lumen if args.browser == 'lumen' else run_chromium
            r = fn(args, url, i)
            r['run'] = i
            print(json.dumps(r, ensure_ascii=False), flush=True)
            results.append(r)
    if args.json:
        with open(args.json, 'w', encoding='utf-8') as f:
            json.dump({'chromium': cib.chromium_version(args.binary) if args.binary else None,
                       'args': vars(args), 'results': results}, f, ensure_ascii=False, indent=2)


if __name__ == '__main__':
    main()
