#!/usr/bin/env python3
"""Lumen graphic tests — блокирующий пайплайн.

Workflow:
  1. Капчуем Edge headless + Lumen (gdigrab) для каждого теста по порядку.
  2. 00-calibration: ищем магента-маркеры -> определяем crop offset.
  3. Каждый следующий тест: cropаем Lumen по offset из калибровки, считаем diff с Edge.
  4. Первый тест с diff% > threshold останавливает пайплайн.

Запуск из корня репо: `python graphic_tests/run.py`
"""
from __future__ import annotations
import argparse
import ctypes
import ctypes.wintypes
import io
import os
import struct
import subprocess
import sys
import time
import zlib

# Force UTF-8 stdout to avoid cp1251 codec errors on Windows console
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# --- Конфиг ---

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
FFMPEG = os.path.join(REPO, 'utils', 'ffmpeg.exe')
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
LUMEN = os.path.join(REPO, 'target', 'release', 'lumen.exe')
SHOTS = os.path.join(REPO, 'graphic_tests', 'screenshots')
TESTS_DIR = os.path.join(REPO, 'graphic_tests')

VIEWPORT_W = 1024
VIEWPORT_H = 720
LUMEN_WAIT_SEC = 5

# (id, html, threshold_pct, label).
# threshold — % пикселей с заметной разницей; выше = FAIL -> стоп.
TESTS: list[tuple[str, str, float, str]] = [
    ('00', '00-calibration.html', 0.5,  'calibration'),
    ('01', '01-sanity.html',      1.0,  'sanity'),
    ('02', '02-color-named.html', 1.0,  'color-named'),
    ('03', '03-color-formats.html', 1.0, 'color-formats'),
    ('04', '04-color-alpha.html', 1.0,  'color-alpha'),
    ('05', '05-border-width.html', 1.0, 'border-width'),
    ('06', '06-border-sides.html', 1.0, 'border-sides'),
    ('07', '07-box-sizing.html',  1.0,  'box-sizing'),
    ('08', '08-padding.html',     1.0,  'padding'),
    ('09', '09-margin.html',      1.0,  'margin'),
    ('10', '10-min-max-width.html', 1.0, 'min-max-width'),
    ('11', '11-min-max-height.html', 1.0, 'min-max-height'),
    ('12', '12-display.html',     1.0,  'display'),
    ('13', '13-visibility-opacity.html', 1.0, 'visibility-opacity'),
    ('14', '14-overflow.html',    1.0,  'overflow'),
    ('15', '15-box-shadow.html',  1.0,  'box-shadow'),
    ('16', '16-outline.html',     1.0,  'outline'),
    ('17', '17-calc.html',        1.0,  'calc'),
    ('18', '18-images.html',      1.0,  'images'),
    ('19', '19-object-fit.html',  1.0,  'object-fit'),
    ('20', '20-quirks-bgcolor.html', 1.0, 'quirks-bgcolor'),
    ('21', '21-border-style.html',   1.0, 'border-style dashed/dotted/double'),
    ('22', '22-transform.html',      1.5, 'CSS transform translate/rotate/scale/skew/matrix'),
    ('23', '23-pseudo-elements.html', 1.0, '::before / ::after block-level generation'),
    ('24', '24-vertical-align.html',  1.5, 'vertical-align inline y-offset + inline-block positioning'),
    ('25', '25-table-layout.html',    1.0, 'table layout: cells horizontal, rows vertical'),
    ('26', '26-mask-image.html',       2.0, 'mask-image: linear/radial gradient mask (Phase 0 fallback = no-op)'),
    ('27', '27-direction-rtl.html',    1.5, 'direction: rtl — LTR/RTL start/end alignment via colored bars'),
    ('28', '28-css-containment.html', 2.0, 'CSS Containment: contain:size (height=0) · contain:paint (clip) · contain:layout · contain:strict'),
    ('29', '29-container-queries.html', 2.0, '@container queries: min-width applies/not · named container · max-width'),
    ('30', '30-css-filter.html',        3.0, 'CSS filter: grayscale/sepia/brightness/invert/contrast/saturate/opacity/blur/hue-rotate'),
    ('31', '31-clip-path.html',          3.0, 'clip-path: inset/circle/ellipse/polygon bounding-box clip'),
    ('32', '32-list-markers.html',       6.0, 'list markers: ::marker box geometry, outside/inside, disc/decimal/alpha/roman'),
    ('33', '33-multi-column.html',       2.0, 'multi-column: column-count/width layout + column-rule solid/dashed/dotted'),
    ('34', '34-forms.html',             3.0, 'form controls: input/checkbox/radio/button/textarea/select static rendering'),
    ('35', '35-grid-named-areas.html',  2.0, 'CSS Grid named areas: grid-template-areas + grid-area: <name>'),
    ('36', '36-border-radius.html',     2.0, 'border-radius: uniform/pill/circle/asymmetric SDF rendering'),
    ('37', '37-float-clear.html',       3.0, 'float: left/right placement + clear: both clearance'),
    ('38', '38-z-index.html',           3.0, 'z-index stacking context paint order (CSS 2.1 Appendix E)'),
    ('39', '39-gradients.html',         3.0, 'linear-gradient / radial-gradient GPU pipeline'),
    ('40', '40-conic-gradients.html',   3.0, 'conic-gradient / repeating-conic-gradient (CSS Images L4 §3.7)'),
    ('41', '41-table.html',             3.0, 'display:table/row/cell with row groups (CSS 2.1 §17)'),
    ('42', '42-position-sticky.html',   3.0, 'position:sticky — flow position + BeginStickyLayer/EndStickyLayer (CSS Positioning L3 §6.3)'),
    ('43', '43-intrinsic-sizing.html',  2.0, 'CSS Intrinsic Sizing L3 — width: max-content / min-content / fit-content'),
]

# --- PNG reader (stdlib only) ---

def read_png(path: str) -> tuple[int, int, int, bytes]:
    """Возвращает (width, height, bytes_per_pixel, raw_pixels)."""
    with open(path, 'rb') as f:
        data = f.read()
    assert data[:8] == b'\x89PNG\r\n\x1a\n', f'Not a PNG: {path}'
    pos = 8
    width = height = color_type = None
    idat = b''
    while pos < len(data):
        cl = struct.unpack('>I', data[pos:pos+4])[0]
        ct = data[pos+4:pos+8]
        cd = data[pos+8:pos+8+cl]
        if ct == b'IHDR':
            width, height, _bd, color_type = struct.unpack('>IIBB', cd[:10])
        elif ct == b'IDAT':
            idat += cd
        elif ct == b'IEND':
            break
        pos += 8 + cl + 4
    raw = zlib.decompress(idat)
    bpp = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color_type]
    stride = width * bpp
    pixels = bytearray(width * height * bpp)
    prev = bytearray(stride)
    for y in range(height):
        f_byte = raw[y * (stride + 1)]
        row = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        out = bytearray(stride)
        if f_byte == 0:
            out = row
        elif f_byte == 1:
            for i in range(stride):
                left = out[i-bpp] if i >= bpp else 0
                out[i] = (row[i] + left) & 0xFF
        elif f_byte == 2:
            for i in range(stride):
                out[i] = (row[i] + prev[i]) & 0xFF
        elif f_byte == 3:
            for i in range(stride):
                left = out[i-bpp] if i >= bpp else 0
                out[i] = (row[i] + (left + prev[i]) // 2) & 0xFF
        elif f_byte == 4:
            for i in range(stride):
                a = out[i-bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i-bpp] if i >= bpp else 0
                p = a + b - c
                pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                out[i] = (row[i] + pr) & 0xFF
        pixels[y*stride:(y+1)*stride] = out
        prev = out
    return width, height, bpp, bytes(pixels)

# --- Window management ---

def _bring_pid_to_front(pid: int) -> None:
    """Bring the main visible window of the given PID to the foreground (Windows)."""
    user32 = ctypes.windll.user32
    EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.wintypes.HWND, ctypes.wintypes.LPARAM)
    found: list[ctypes.wintypes.HWND] = []

    def _cb(hwnd: ctypes.wintypes.HWND, _: ctypes.wintypes.LPARAM) -> bool:
        proc_id = ctypes.wintypes.DWORD(0)
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(proc_id))
        if proc_id.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
            return False
        return True

    user32.EnumWindows(EnumProc(_cb), 0)
    if found:
        hwnd = found[0]
        # Alt-key trick to bypass Windows foreground-lock
        ctypes.windll.user32.keybd_event(0x12, 0, 0, 0)  # VK_MENU down
        user32.SetForegroundWindow(hwnd)
        ctypes.windll.user32.keybd_event(0x12, 0, 2, 0)  # VK_MENU up

# --- Capture helpers ---

def capture_edge(html_path: str, out_png: str) -> None:
    url = 'file:///' + os.path.abspath(html_path).replace('\\', '/')
    subprocess.run(
        [EDGE, '--headless', f'--screenshot={out_png}',
         f'--window-size={VIEWPORT_W},{VIEWPORT_H}', url],
        capture_output=True, timeout=60,
    )

def capture_lumen(html_relpath: str, out_png: str) -> None:
    """Запускаем Lumen, ждём LUMEN_WAIT_SEC сек, грабим desktop через ffmpeg, kill-аем."""
    proc = subprocess.Popen([LUMEN, html_relpath], cwd=REPO,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(LUMEN_WAIT_SEC)
    _bring_pid_to_front(proc.pid)
    time.sleep(0.2)  # brief pause for window compositor to repaint
    subprocess.run(
        [FFMPEG, '-f', 'gdigrab', '-i', 'desktop',
         '-vframes', '1', '-update', '1', out_png, '-y'],
        capture_output=True, timeout=15,
    )
    proc.kill()
    proc.wait(timeout=5)

def ffmpeg_crop(in_png: str, out_png: str, x: int, y: int) -> None:
    subprocess.run(
        [FFMPEG, '-i', in_png,
         '-vf', f'crop={VIEWPORT_W}:{VIEWPORT_H}:{x}:{y}',
         out_png, '-y'],
        capture_output=True, timeout=15,
    )

def ffmpeg_diff(edge_png: str, lumen_png: str, out_png: str) -> None:
    subprocess.run(
        [FFMPEG, '-i', edge_png, '-i', lumen_png,
         '-filter_complex', 'blend=all_mode=difference',
         out_png, '-y'],
        capture_output=True, timeout=15,
    )

# --- Magenta marker detection ---

MAG = (240, 20, 240)  # порог: R>240, G<20, B>240 — pure magenta с допуском

def is_magenta(p: bytes, idx: int) -> bool:
    return p[idx] > MAG[0] and p[idx+1] < MAG[1] and p[idx+2] > MAG[2]

def _longest_run(seq: list[bool]) -> tuple[int, int]:
    """Return (run_length, run_start) of the longest True-run in seq."""
    best_len = best_start = 0
    run_start = -1
    run_len = 0
    for i, v in enumerate(seq):
        if v:
            if run_start == -1:
                run_start = i
            run_len += 1
            if run_len > best_len:
                best_len = run_len
                best_start = run_start
        else:
            run_start = -1
            run_len = 0
    return best_len, best_start


def find_marker_origin(png_path: str) -> tuple[int, int] | None:
    """Ищем верхний-левый угол магента-рамки в desktop snapshot.

    Возвращает (origin_x, origin_y) — точка кропа.

    Primary: горизонтальное сканирование (верхняя граница, run >= 1000 px),
    затем верификация нижней строки.
    Fallback: вертикальное сканирование (левый столбец, run >= VIEWPORT_H/2),
    затем верификация правого столбца.
    """
    w, h, bpp, p = read_png(png_path)

    # --- Primary: horizontal scan (top border row) ---
    for y in range(h):
        row_mag = [is_magenta(p, y*w*bpp + x*bpp) for x in range(w)]
        best_len, best_start = _longest_run(row_mag)
        if best_len >= 1000:
            bot_y = y + VIEWPORT_H - 1
            if bot_y >= h:
                continue
            bot_count = sum(1 for x in range(best_start, best_start + best_len)
                            if is_magenta(p, bot_y*w*bpp + x*bpp))
            if bot_count >= best_len * 0.9:
                return (best_start, y)

    # --- Fallback: vertical scan (left border column) ---
    for x in range(w):
        col_mag = [is_magenta(p, y*w*bpp + x*bpp) for y in range(h)]
        best_len, best_start = _longest_run(col_mag)
        if best_len >= VIEWPORT_H // 2:
            right_x = x + VIEWPORT_W - 1
            if right_x >= w:
                continue
            right_count = sum(1 for y in range(best_start, best_start + best_len)
                              if is_magenta(p, y*w*bpp + right_x*bpp))
            if right_count >= best_len * 0.9:
                return (x, best_start)

    return None

# --- Diff metric ---

def diff_percent(png_path: str, channel_threshold: int = 16) -> float:
    """Возвращает % пикселей, у которых хотя бы один канал > channel_threshold в diff-изображении."""
    w, h, bpp, p = read_png(png_path)
    total = w * h
    bad = 0
    for i in range(0, len(p), bpp):
        if p[i] > channel_threshold or p[i+1] > channel_threshold or p[i+2] > channel_threshold:
            bad += 1
    return 100.0 * bad / total

# --- Crop offset persistence ---

_CROP_OFFSET_FILE = os.path.join(os.path.dirname(__file__), 'screenshots', 'crop_offset.txt')

def _save_crop_offset(offset: tuple[int, int]) -> None:
    os.makedirs(os.path.dirname(_CROP_OFFSET_FILE), exist_ok=True)
    with open(_CROP_OFFSET_FILE, 'w') as f:
        f.write(f'{offset[0]},{offset[1]}\n')

def _load_crop_offset() -> tuple[int, int] | None:
    if not os.path.exists(_CROP_OFFSET_FILE):
        return None
    with open(_CROP_OFFSET_FILE) as f:
        parts = f.read().strip().split(',')
    if len(parts) != 2:
        return None
    try:
        return (int(parts[0]), int(parts[1]))
    except ValueError:
        return None

# --- Pipeline ---

def ensure_lumen() -> None:
    if os.path.exists(LUMEN):
        return
    print('Lumen release-бинарь не найден. Билдим...')
    env = os.environ.copy()
    env['PATH'] = r'C:\Users\konstantin\.cargo\bin' + os.pathsep + env.get('PATH', '')
    res = subprocess.run(['cargo', 'build', '-p', 'lumen-shell', '--release'],
                         cwd=REPO, env=env)
    if res.returncode != 0:
        print('Сборка Lumen упала.')
        sys.exit(2)

def run_one(tid: str, html: str, threshold: float, label: str,
            crop_offset: tuple[int, int] | None) -> tuple[bool, tuple[int, int] | None, float]:
    """Возвращает (passed, new_crop_offset, diff_pct)."""
    test_path = os.path.join(TESTS_DIR, html)
    if not os.path.exists(test_path):
        print(f'TEST-{tid}: FAIL (no HTML: {test_path})')
        return False, crop_offset, -1.0

    edge_png   = os.path.join(SHOTS, f'{tid}-edge.png')
    lumen_raw  = os.path.join(SHOTS, f'{tid}-lumen.png')
    lumen_crop = os.path.join(SHOTS, f'{tid}-lumen-cropped.png')
    diff_png   = os.path.join(SHOTS, f'{tid}-diff.png')

    capture_edge(test_path, edge_png)
    if not os.path.exists(edge_png):
        print(f'TEST-{tid}: FAIL (Edge screenshot missing)')
        return False, crop_offset, -1.0

    rel_html = os.path.relpath(test_path, REPO).replace('\\', '/')
    capture_lumen(rel_html, lumen_raw)
    if not os.path.exists(lumen_raw):
        print(f'TEST-{tid}: FAIL (gdigrab screenshot missing)')
        return False, crop_offset, -1.0

    if tid == '00':
        origin = find_marker_origin(lumen_raw)
        if origin is None:
            print(f'TEST-{tid}: FAIL (magenta marker not found)')
            return False, None, -1.0
        crop_offset = origin
        _save_crop_offset(crop_offset)

    if crop_offset is None:
        crop_offset = _load_crop_offset()
    if crop_offset is None:
        print(f'TEST-{tid}: FAIL (no crop offset — run TEST-00 first)')
        return False, None, -1.0

    ffmpeg_crop(lumen_raw, lumen_crop, crop_offset[0], crop_offset[1])
    if not os.path.exists(lumen_crop):
        print(f'TEST-{tid}: FAIL (ffmpeg crop failed)')
        return False, crop_offset, -1.0
    ffmpeg_diff(edge_png, lumen_crop, diff_png)
    if not os.path.exists(diff_png):
        print(f'TEST-{tid}: FAIL (ffmpeg diff failed)')
        return False, crop_offset, -1.0

    pct = diff_percent(diff_png)
    passed = pct <= threshold
    print(f'TEST-{tid}: {"PASS" if passed else "FAIL"} ({pct:.2f}%)', flush=True)
    return passed, crop_offset, pct

def main() -> int:
    parser = argparse.ArgumentParser(description='Lumen graphic tests pipeline')
    parser.add_argument('--only', help='Запустить только указанный тест-id (e.g. 03)')
    parser.add_argument('--continue-on-fail', action='store_true',
                        help='Не останавливаться при первом fail-е (для диагностики)')
    args = parser.parse_args()

    os.makedirs(SHOTS, exist_ok=True)
    ensure_lumen()

    crop_offset: tuple[int, int] | None = None
    results: list[tuple[str, str, bool, float]] = []
    halted_at: str | None = None

    for tid, html, threshold, label in TESTS:
        if args.only and tid != args.only:
            continue
        passed, crop_offset, pct = run_one(tid, html, threshold, label, crop_offset)
        results.append((tid, label, passed, pct))
        if not passed and not args.continue_on_fail:
            halted_at = tid
            break

    if halted_at:
        skipped = len([t for t in TESTS if t[0] > halted_at])
        print(f'Pipeline stopped at TEST-{halted_at}. {skipped} tests skipped.')
        return 1
    failed = [r for r in results if not r[2]]
    if failed:
        print(f'{len(failed)}/{len(results)} tests FAILED: ' + ', '.join(r[0] for r in failed))
        return 1
    print(f'All {len(results)} tests passed.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
