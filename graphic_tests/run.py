#!/usr/bin/env python3
"""Lumen graphic tests — блокирующий пайплайн.

Запуск:
    python graphic_tests/run.py                     # все тесты, стоп на первом провале
    python graphic_tests/run.py --continue-on-fail  # все тесты, собрать все результаты
    python graphic_tests/run.py --only 03           # один тест по id
    python graphic_tests/run.py --recheck           # перезапустить только FAIL из latest.json
    python graphic_tests/run.py --build             # cargo build --release перед запуском
    python graphic_tests/run.py --no-cache          # принудительная пересъёмка Edge-скриншотов

Workflow:
  1. Снимаем Edge headless + Lumen (gdigrab) для каждого теста по порядку.
  2. TEST-00 calibration: ищем магента-маркеры → определяем crop offset.
  3. Каждый следующий тест: кропаем Lumen по offset из калибровки, считаем diff с Edge.
  4. Первый тест с diff% > threshold останавливает пайплайн (если не --continue-on-fail).

Результаты:
  graphic_tests/results/YYYYMMDD-HHMMSS.json — полные результаты прогона
  graphic_tests/results/YYYYMMDD-HHMMSS.html — визуальный отчёт (edge|lumen|diff для FAIL)
  graphic_tests/results/latest.json          — всегда указывает на последний прогон

Edge-скриншоты кэшируются: пересъёмка только если HTML новее PNG или передан --no-cache.
--recheck загружает список FAIL из latest.json и перегоняет только их + TEST-00.
"""
from __future__ import annotations
import argparse
import ctypes
import ctypes.wintypes
import datetime
import io
import json
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
RESULTS_DIR = os.path.join(REPO, 'graphic_tests', 'results')
TESTS_DIR = os.path.join(REPO, 'graphic_tests')

VIEWPORT_W = 1024
VIEWPORT_H = 720
LUMEN_WAIT_SEC = 5

# (id, html, threshold_pct, label).
# threshold — % пикселей с заметной разницей; выше = FAIL → стоп.
TESTS: list[tuple[str, str, float, str]] = [
    ('00', '00-calibration.html',        0.5, 'calibration'),
    ('01', '01-sanity.html',             0.5, 'sanity'),
    ('02', '02-color-named.html',        0.5, 'color-named'),
    ('03', '03-color-formats.html',      0.5, 'color-formats'),
    ('04', '04-color-alpha.html',        0.5, 'color-alpha'),
    ('05', '05-border-width.html',       0.5, 'border-width'),
    ('06', '06-border-sides.html',       0.5, 'border-sides'),
    ('07', '07-box-sizing.html',         0.5, 'box-sizing'),
    ('08', '08-padding.html',            0.5, 'padding'),
    ('09', '09-margin.html',             0.5, 'margin'),
    ('10', '10-min-max-width.html',      0.5, 'min-max-width'),
    ('11', '11-min-max-height.html',     0.5, 'min-max-height'),
    ('12', '12-display.html',            0.5, 'display'),
    ('13', '13-visibility-opacity.html', 0.5, 'visibility-opacity'),
    ('14', '14-overflow.html',           0.5, 'overflow'),
    ('15', '15-box-shadow.html',         0.5, 'box-shadow'),
    ('16', '16-outline.html',            0.5, 'outline'),
    ('17', '17-calc.html',               0.5, 'calc'),
    ('18', '18-images.html',             0.5, 'images'),
    ('19', '19-object-fit.html',         0.5, 'object-fit'),
    ('20', '20-quirks-bgcolor.html',     0.5, 'quirks-bgcolor'),
    ('21', '21-border-style.html',       0.5, 'border-style dashed/dotted/double'),
    ('22', '22-transform.html',          0.5, 'CSS transform translate/rotate/scale/skew/matrix'),
    ('23', '23-pseudo-elements.html',    0.5, '::before / ::after block-level generation'),
    ('24', '24-vertical-align.html',     0.5, 'vertical-align inline y-offset + inline-block positioning'),
    ('25', '25-table-layout.html',       0.5, 'table layout: cells horizontal, rows vertical'),
    ('26', '26-mask-image.html',         0.5, 'mask-image: linear/radial gradient mask (Phase 0 fallback = no-op)'),
    ('27', '27-direction-rtl.html',      0.5, 'direction: rtl — LTR/RTL start/end alignment via colored bars'),
    ('28', '28-css-containment.html',    0.5, 'CSS Containment: contain:size (height=0) · contain:paint (clip) · contain:layout · contain:strict'),
    ('29', '29-container-queries.html',  0.5, '@container queries: min-width applies/not · named container · max-width'),
    ('30', '30-css-filter.html',         0.5, 'CSS filter: grayscale/sepia/brightness/invert/contrast/saturate/opacity/blur/hue-rotate'),
    ('31', '31-clip-path.html',          0.5, 'clip-path: inset/circle/ellipse/polygon bounding-box clip'),
    ('32', '32-list-markers.html',       0.5, 'list markers: ::marker box geometry, outside/inside, disc/decimal/alpha/roman'),
    ('33', '33-multi-column.html',       0.5, 'multi-column: column-count/width layout + column-rule solid/dashed/dotted'),
    ('34', '34-forms.html',              0.5, 'form controls: input/checkbox/radio/button/textarea/select static rendering'),
    ('35', '35-grid-named-areas.html',   0.5, 'CSS Grid named areas: grid-template-areas + grid-area: <name>'),
    ('36', '36-border-radius.html',      0.5, 'border-radius: uniform/pill/circle/asymmetric SDF rendering'),
    ('37', '37-float-clear.html',        0.5, 'float: left/right placement + clear: both clearance'),
    ('38', '38-z-index.html',            0.5, 'z-index stacking context paint order (CSS 2.1 Appendix E)'),
    ('39', '39-gradients.html',          0.5, 'linear-gradient / radial-gradient GPU pipeline'),
    ('40', '40-conic-gradients.html',    0.5, 'conic-gradient / repeating-conic-gradient (CSS Images L4 §3.7)'),
    ('41', '41-table.html',              0.5, 'display:table/row/cell with row groups (CSS 2.1 §17)'),
    ('42', '42-position-sticky.html',    0.5, 'position:sticky — flow position + BeginStickyLayer/EndStickyLayer (CSS Positioning L3 §6.3)'),
    ('43', '43-intrinsic-sizing.html',   0.5, 'CSS Intrinsic Sizing L3 — width: max-content / min-content / fit-content'),
    ('44', '44-media-queries.html',      0.5, 'Media Queries L3 — @media screen/print/min-width(em)/orientation/aspect-ratio'),
    ('45', '45-multiple-backgrounds.html', 0.5, 'CSS Backgrounds L3 §3 — multiple background layers, position/size/repeat/clip/origin'),
    ('46', '46-individual-transforms.html', 0.5, 'CSS Transforms L2 — individual translate / rotate / scale properties'),
    ('47', '47-svg-basic.html',             0.5, 'SVG basic shapes — rect/circle/ellipse/line in document flow, viewBox scale'),
    ('48', '48-line-clamp.html',            0.5, 'CSS Overflow L4 §3.2 — -webkit-line-clamp / line-clamp multi-line truncation, staircase heights 1–4 lines'),
    ('49', '49-background-blend-mode.html', 0.5, 'CSS Compositing L1 §8.3 — background-blend-mode: multiply/screen/overlay/darken/lighten/difference/exclusion/color-dodge/luminosity'),
    ('50', '50-css-variables.html', 0.5, 'CSS Variables L1 — var() basic/nested/fallback + calc(var()) + inheritance'),
    ('51', '51-scrollbar-rendering.html', 0.5, 'Scrollbar rendering — overflow:scroll/auto vertical/horizontal/both DrawScrollbar track+thumb'),
    ('52', '52-text-shadow-blur.html', 4.0, 'text-shadow blur — PushFilter{Blur(sigma)} wrapping: sharp/4px/10px/20px blur progression + multi-shadow + glow'),
    ('53', '53-background-origin.html', 0.5, 'background-origin — positioning area: border-box/padding-box/content-box vs background-clip; 0%/100% anchoring'),
    ('54', '54-svg-path-stroke.html', 0.5, 'SVG <path> stroke tessellation — open/closed stroke, fill+stroke, miter join, butt cap, widths 2-14px'),
    ('55', '55-video-placeholder.html', 0.5, '<video> replaced element — grey DrawImage placeholder; UA default 300×150; CSS/attr dimensions; border + border-radius'),
    ('56', '56-mix-blend-mode.html', 0.5, 'CSS Compositing L1 §5 — mix-blend-mode: normal/multiply/darken/color-burn/screen/lighten/color-dodge/overlay/hard-light/soft-light/difference/exclusion/hue/saturation/color/luminosity + nesting'),
    ('57', '57-canvas-2d.html', 0.5, 'HTML LS §4.12.4 — <canvas> getContext("2d"): fillRect/strokeRect/arc/path fill; UA default 300×150; attr dimensions; CSS background + border + border-radius on the canvas element box'),
    ('58', '58-first-letter-line.html', 2.0, 'CSS Pseudo-elements L4 §5.3-5.4 — ::first-letter drop-cap (large yellow, float:left) + ::first-line green bold first line'),
    ('59', '59-image-set-cross-fade.html', 2.0, 'CSS Images L4 §5/§4 — image-set() DPR selection + cross-fade() two-image blend; -webkit- vendor prefix variants'),
    ('60', '60-svg-stroke-advanced.html', 1.0, 'SVG stroke advanced: stroke-linecap (butt/round/square), stroke-linejoin (miter/bevel/round), stroke-miterlimit, stroke-dasharray, stroke-dashoffset, fill-rule (nonzero/evenodd)'),
    ('61', '61-view-transitions.html',   1.0, 'View Transitions API: document.startViewTransition(cb) — JS API, ViewTransition object, Begin/End events, 300 ms cross-fade'),
    ('62', '62-scroll-snap.html',        1.0, 'CSS Scroll Snap L1: scroll-snap-type (y/x mandatory, both proximity), scroll-snap-align (start), scroll-snap-stop (always); static layout geometry of snap containers'),
    ('63', '63-masonry.html',            1.0, 'CSS Masonry layout (Houdini) Phase 0: waterfall grid — items placed in column with min height; column-count, gap'),
    ('64', '64-table.html',              1.0, 'CSS 2.1 §17 Table layout: emit_table_box() renders table/thead/tbody/tr/td cells with separate border-spacing, cell backgrounds, borders; col_span/row_span in struct'),
    ('65', '65-flex-align-content.html', 0.5, 'CSS Flexbox align-content: flex-start/end/center/space-between/space-around/space-evenly/stretch for multi-line flex containers'),
    ('66', '66-selection-pseudo.html',  0.5, 'CSS ::selection pseudo-element: background-color + color override; swatch boxes show parsed selection colours; page renders correctly with ::selection rules present'),
    ('67', '67-attr-typed.html',         0.5, 'CSS Values L4 §7.7 attr() typed substitution: content:attr(data-label) generates ::before labels from HTML attributes; 5 labelled bar rows with distinct colours and widths'),
    ('68', '68-font-variation-settings.html', 0.5, 'CSS Fonts L4 §6.3 font-variation-settings: parsing and layout stability; coloured boxes with varied wght/wdth/slnt axes and inherited settings'),
    ('69', '69-border-spacing.html', 0.5, 'CSS 2.1 §17.6 border-spacing: equal and asymmetric gaps between table cells; three tables showing 12px equal, 8px/24px asymmetric, and 0 spacing'),
    ('70', '70-object-fit.html', 0.5, 'CSS Images L3 §5.5 object-fit / object-position: SVG viewBox scaling modes fill/contain/cover/none/scale-down and object-position alignment'),
    ('71', '71-starting-style.html', 0.5, 'CSS Transitions L2 §3.4 @starting-style: entry animation support — @starting-style rules parse without crashing; static rendering of two coloured boxes is unaffected'),
    ('72', '72-host-slotted.html', 0.5, 'CSS Scoping L1 §6.1-6.2 :host / ::slotted: shadow host gets blue background via :host; :host(.special) colours second host amber; :host(.missing) does not match third host; ::slotted children get coloured boxes'),
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
    # Use c_size_t (pointer-sized) for HWND to avoid overflow on 64-bit Windows
    EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_size_t, ctypes.c_size_t)
    found: list[int] = []

    def _cb(hwnd: int, _: int) -> bool:
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

def capture_edge(html_path: str, out_png: str, force: bool = False) -> None:
    """Снимает страницу в Edge headless.

    Кэш: если out_png существует и новее html_path — пропускает захват.
    force=True принудительно пересоздаёт PNG.
    """
    if not force and os.path.exists(out_png):
        if os.path.getmtime(out_png) >= os.path.getmtime(html_path):
            return  # cache hit
    url = 'file:///' + os.path.abspath(html_path).replace('\\', '/')
    subprocess.run(
        [EDGE, '--headless', f'--screenshot={out_png}',
         f'--window-size={VIEWPORT_W},{VIEWPORT_H}',
         '--hide-scrollbars', url],
        capture_output=True, timeout=60,
    )

def capture_lumen(html_relpath: str, out_png: str) -> None:
    """Запускаем Lumen, ждём LUMEN_WAIT_SEC сек, грабим desktop через ffmpeg, kill-аем."""
    proc = subprocess.Popen([LUMEN, '--no-scrollbar', html_relpath], cwd=REPO,
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

# --- Diff metric + bounding box ---

def diff_stats(png_path: str, channel_threshold: int = 16) -> tuple[float, dict | None]:
    """Считает diff-пиксели в изображении.

    Возвращает:
        pct        — % пикселей, у которых хотя бы один канал > channel_threshold
        diff_region — {top, left, bottom, right} bounding box плохих пикселей,
                      или None если diff == 0
    """
    w, h, bpp, p = read_png(png_path)
    total = w * h
    bad = 0
    min_x = w;  max_x = -1
    min_y = h;  max_y = -1
    for yi in range(h):
        row_has_bad = False
        for xi in range(w):
            i = (yi * w + xi) * bpp
            if p[i] > channel_threshold or p[i+1] > channel_threshold or p[i+2] > channel_threshold:
                bad += 1
                row_has_bad = True
                if xi < min_x: min_x = xi
                if xi > max_x: max_x = xi
        if row_has_bad:
            if yi < min_y: min_y = yi
            if yi > max_y: max_y = yi
    pct = 100.0 * bad / total
    region = {'top': min_y, 'left': min_x, 'bottom': max_y, 'right': max_x} if bad > 0 else None
    return pct, region

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

def _build_lumen() -> bool:
    """Собирает lumen-shell --release. Возвращает True при успехе."""
    print('Сборка lumen-shell --release...')
    env = os.environ.copy()
    env['PATH'] = r'C:\Users\konstantin\.cargo\bin' + os.pathsep + env.get('PATH', '')
    res = subprocess.run(['cargo', 'build', '-p', 'lumen-shell', '--release'],
                         cwd=REPO, env=env)
    if res.returncode != 0:
        print('Сборка Lumen упала.')
        return False
    print('Сборка завершена.')
    return True


def ensure_lumen(force_build: bool = False) -> None:
    """Гарантирует наличие release-бинаря. force_build=True — пересобрать в любом случае."""
    if force_build or not os.path.exists(LUMEN):
        if not _build_lumen():
            sys.exit(2)


def run_one(tid: str, html: str, threshold: float, label: str,
            crop_offset: tuple[int, int] | None,
            no_cache: bool = False) -> tuple[bool, tuple[int, int] | None, float, dict | None]:
    """Запускает один тест.

    Возвращает (passed, new_crop_offset, diff_pct, diff_region).
    diff_pct < 0 и diff_region = None означают ошибку (ERROR).
    """
    test_path = os.path.join(TESTS_DIR, html)
    if not os.path.exists(test_path):
        print(f'TEST-{tid}: FAIL (no HTML: {test_path})')
        return False, crop_offset, -1.0, None

    stem = html[:-5]  # '00-calibration.html' → '00-calibration'
    edge_png   = os.path.join(SHOTS, f'{stem}-edge.png')
    lumen_raw  = os.path.join(SHOTS, f'{stem}-lumen.png')
    lumen_crop = os.path.join(SHOTS, f'{stem}-lumen-cropped.png')
    diff_png   = os.path.join(SHOTS, f'{stem}-diff.png')

    capture_edge(test_path, edge_png, force=no_cache)
    if not os.path.exists(edge_png):
        print(f'TEST-{tid}: FAIL (Edge screenshot missing)')
        return False, crop_offset, -1.0, None

    rel_html = os.path.relpath(test_path, REPO).replace('\\', '/')
    capture_lumen(rel_html, lumen_raw)
    if not os.path.exists(lumen_raw):
        print(f'TEST-{tid}: FAIL (gdigrab screenshot missing)')
        return False, crop_offset, -1.0, None

    if tid == '00':
        origin = find_marker_origin(lumen_raw)
        if origin is None:
            print(f'TEST-{tid}: FAIL (magenta marker not found)')
            return False, None, -1.0, None
        crop_offset = origin
        _save_crop_offset(crop_offset)

    if crop_offset is None:
        crop_offset = _load_crop_offset()
    if crop_offset is None:
        print(f'TEST-{tid}: FAIL (no crop offset — run TEST-00 first)')
        return False, None, -1.0, None

    ffmpeg_crop(lumen_raw, lumen_crop, crop_offset[0], crop_offset[1])
    if os.path.exists(lumen_raw):
        os.remove(lumen_raw)
    if not os.path.exists(lumen_crop):
        print(f'TEST-{tid}: FAIL (ffmpeg crop failed)')
        return False, crop_offset, -1.0, None
    ffmpeg_diff(edge_png, lumen_crop, diff_png)
    if not os.path.exists(diff_png):
        print(f'TEST-{tid}: FAIL (ffmpeg diff failed)')
        return False, crop_offset, -1.0, None

    pct, region = diff_stats(diff_png)
    passed = pct <= threshold
    region_str = _fmt_region(region) if region else ''
    suffix = f'  [{region_str}]' if region_str and not passed else ''
    print(f'TEST-{tid}: {"PASS" if passed else "FAIL"} ({pct:.2f}%){suffix}', flush=True)
    return passed, crop_offset, pct, region


def _fmt_region(r: dict) -> str:
    """Форматирует bounding box для вывода: 'x:10–50 y:683–720'."""
    return f'x:{r["left"]}–{r["right"]} y:{r["top"]}–{r["bottom"]}'

# --- Result persistence ---

def _git_info() -> dict[str, str]:
    def _run(cmd: list[str]) -> str:
        try:
            return subprocess.check_output(cmd, cwd=REPO, stderr=subprocess.DEVNULL,
                                           text=True).strip()
        except Exception:
            return ''
    return {
        'commit': _run(['git', 'rev-parse', '--short', 'HEAD']),
        'branch': _run(['git', 'rev-parse', '--abbrev-ref', 'HEAD']),
        'subject': _run(['git', 'log', '-1', '--format=%s']),
    }


def save_results(results: list[dict], crop_offset: tuple[int, int] | None,
                 halted_at: str | None) -> str:
    """Сохраняет results в RESULTS_DIR/<timestamp>.json и latest.json.
    Генерирует HTML-отчёт рядом. Возвращает путь к JSON-файлу."""
    os.makedirs(RESULTS_DIR, exist_ok=True)
    ts = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    ts_iso = datetime.datetime.now().isoformat(timespec='seconds')
    git = _git_info()
    passed  = sum(1 for r in results if r['status'] == 'PASS')
    failed  = sum(1 for r in results if r['status'] == 'FAIL')
    errors  = sum(1 for r in results if r['status'] == 'ERROR')
    skipped = len(TESTS) - len(results)
    data = {
        'timestamp': ts_iso,
        'git': git,
        'crop_offset': list(crop_offset) if crop_offset else None,
        'halted_at': halted_at,
        'summary': {
            'total': len(TESTS),
            'passed': passed,
            'failed': failed,
            'errors': errors,
            'skipped': skipped,
        },
        'tests': results,
    }
    json_path = os.path.join(RESULTS_DIR, f'{ts}.json')
    with open(json_path, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    latest = os.path.join(RESULTS_DIR, 'latest.json')
    with open(latest, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)

    html_path = os.path.join(RESULTS_DIR, f'{ts}.html')
    _write_html_report(html_path, data)

    return json_path


def _write_html_report(path: str, data: dict) -> None:
    """Генерирует HTML-отчёт с edge|lumen|diff изображениями для каждого FAIL."""
    git = data.get('git', {})
    s = data.get('summary', {})
    ts = data.get('timestamp', '')

    rows_html: list[str] = []
    for r in data.get('tests', []):
        tid     = r['id']
        status  = r['status']
        pct     = r.get('diff_pct', -1.0)
        thr     = r.get('threshold', 0.5)
        label   = r.get('label', '')
        region  = r.get('diff_region')

        css_cls = {'PASS': 'pass', 'FAIL': 'fail', 'ERROR': 'error'}.get(status, 'skip')
        pct_str = f'{pct:.2f}%' if pct >= 0 else '—'
        region_str = _fmt_region(region) if region else '—'

        # показываем скриншоты только для FAIL и ERROR
        if status in ('FAIL', 'ERROR'):
            stem = r.get('stem', tid)
            ep = f'../screenshots/{stem}-edge.png'
            lp = f'../screenshots/{stem}-lumen-cropped.png'
            dp = f'../screenshots/{stem}-diff.png'
            imgs = (
                f'<div class="imgs">'
                f'<figure><img src="{ep}" loading="lazy"><figcaption>Edge</figcaption></figure>'
                f'<figure><img src="{lp}" loading="lazy"><figcaption>Lumen</figcaption></figure>'
                f'<figure><img src="{dp}" loading="lazy"><figcaption>Diff</figcaption></figure>'
                f'</div>'
            )
        else:
            imgs = ''

        rows_html.append(
            f'<tr class="{css_cls}">'
            f'<td class="tid">TEST-{tid}</td>'
            f'<td class="status-{status}">{status}</td>'
            f'<td class="pct">{pct_str}</td>'
            f'<td class="thr">{thr}%</td>'
            f'<td class="region">{region_str}</td>'
            f'<td class="label">{label}</td>'
            f'<td>{imgs}</td>'
            f'</tr>'
        )

    rows = '\n'.join(rows_html)
    html = f"""<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>Lumen tests {ts}</title>
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font:13px/1.5 monospace;background:#111;color:#ccc;padding:20px}}
h1{{font-size:15px;color:#fff;margin-bottom:6px}}
.meta{{color:#666;font-size:11px;margin-bottom:4px}}
.summary{{margin-bottom:20px;font-size:13px}}
.summary .p{{color:#4b4}}
.summary .f{{color:#c44}}
.summary .s{{color:#666}}
table{{width:100%;border-collapse:collapse;font-size:12px}}
th{{text-align:left;padding:4px 8px;background:#1e1e1e;color:#888;position:sticky;top:0}}
td{{padding:5px 8px;border-bottom:1px solid #1e1e1e;vertical-align:top}}
tr.pass td{{background:#0b1a0b}}
tr.fail td{{background:#1a0b0b}}
tr.error td{{background:#1a150b}}
.tid{{width:70px;color:#777}}
.status-PASS{{color:#4b4;font-weight:bold}}
.status-FAIL{{color:#c44;font-weight:bold}}
.status-ERROR{{color:#c84;font-weight:bold}}
.pct{{width:65px}}
.thr{{width:55px;color:#666}}
.region{{width:160px;color:#888;font-size:11px}}
.label{{max-width:280px;color:#999;word-break:break-word}}
.imgs{{display:flex;gap:6px;flex-wrap:wrap;margin-top:4px}}
figure{{margin:0}}
figcaption{{font-size:10px;color:#555;text-align:center;margin-top:2px}}
img{{display:block;width:310px;border:1px solid #2a2a2a}}
</style>
</head>
<body>
<h1>Lumen graphic tests — {ts}</h1>
<div class="meta">commit {git.get('commit','?')} · {git.get('branch','?')} · {git.get('subject','')}</div>
<div class="summary">
  <span class="p">✓ {s.get('passed',0)} passed</span> &nbsp;
  <span class="f">✗ {s.get('failed',0)} failed</span> &nbsp;
  <span class="s">{s.get('errors',0)} errors &nbsp; {s.get('skipped',0)} skipped</span>
</div>
<table>
<tr>
  <th>ID</th><th>Status</th><th>Diff%</th><th>Thr.</th>
  <th>Diff region</th><th>Label</th><th>Screenshots (Edge | Lumen | Diff)</th>
</tr>
{rows}
</table>
</body>
</html>"""
    with open(path, 'w', encoding='utf-8') as f:
        f.write(html)


def _load_latest() -> dict | None:
    """Загружает latest.json если существует."""
    latest = os.path.join(RESULTS_DIR, 'latest.json')
    if not os.path.exists(latest):
        return None
    try:
        with open(latest, encoding='utf-8') as f:
            return json.load(f)
    except Exception:
        return None


def _load_previous() -> dict | None:
    """Загружает предыдущий результат (второй по дате файл, не latest.json)."""
    try:
        files = sorted(
            [f for f in os.listdir(RESULTS_DIR)
             if f.endswith('.json') and f != 'latest.json'],
            reverse=True,
        )
        # files[0] — только что записанный, files[1] — предыдущий
        if len(files) < 2:
            return None
        with open(os.path.join(RESULTS_DIR, files[1]), encoding='utf-8') as f:
            return json.load(f)
    except Exception:
        return None


def print_diff_vs_previous(current: list[dict], prev_data: dict) -> None:
    """Выводит регрессии и улучшения относительно предыдущего прогона."""
    prev_by_id = {r['id']: r for r in prev_data.get('tests', [])}
    regressions: list[str] = []
    improvements: list[str] = []
    for r in current:
        tid = r['id']
        prev = prev_by_id.get(tid)
        if prev is None:
            continue
        cur_pass  = r['status'] == 'PASS'
        prev_pass = prev['status'] == 'PASS'
        if prev_pass and not cur_pass:
            delta = r['diff_pct'] - prev['diff_pct']
            regressions.append(
                f'  TEST-{tid}  PASS→FAIL  {prev["diff_pct"]:.2f}% → {r["diff_pct"]:.2f}%'
                f'  (+{delta:.2f}%)  {r["label"]}'
            )
        elif not prev_pass and cur_pass:
            delta = prev['diff_pct'] - r['diff_pct']
            improvements.append(
                f'  TEST-{tid}  FAIL→PASS  {prev["diff_pct"]:.2f}% → {r["diff_pct"]:.2f}%'
                f'  (-{delta:.2f}%)  {r["label"]}'
            )

    prev_ts     = prev_data.get('timestamp', '?')
    prev_commit = prev_data.get('git', {}).get('commit', '?')
    print(f'\nДельта vs предыдущий прогон ({prev_ts}  commit {prev_commit}):')
    if regressions:
        print(f'  Регрессии ({len(regressions)}):')
        for line in regressions:
            print(line)
    if improvements:
        print(f'  Улучшения ({len(improvements)}):')
        for line in improvements:
            print(line)
    if not regressions and not improvements:
        print('  Изменений нет.')


def main() -> int:
    parser = argparse.ArgumentParser(
        description='Lumen graphic tests pipeline',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Примеры:
  python run.py                          # полный прогон, стоп на первом провале
  python run.py --continue-on-fail       # собрать все результаты
  python run.py --only 22                # только TEST-22 (transform)
  python run.py --recheck                # перезапустить только FAIL из latest.json
  python run.py --build --continue-on-fail  # пересобрать + полный прогон
  python run.py --no-cache --only 05     # пересъёмка Edge для TEST-05""",
    )
    parser.add_argument('--only',
                        help='Запустить только указанный тест-id (e.g. 03)')
    parser.add_argument('--continue-on-fail', action='store_true',
                        help='Не останавливаться при первом провале')
    parser.add_argument('--recheck', action='store_true',
                        help='Перезапустить только тесты, упавшие в последнем прогоне (latest.json)')
    parser.add_argument('--build', action='store_true',
                        help='Пересобрать lumen-shell --release перед запуском')
    parser.add_argument('--no-cache', action='store_true',
                        help='Принудительная пересъёмка Edge-скриншотов (игнорировать кэш)')
    args = parser.parse_args()

    os.makedirs(SHOTS, exist_ok=True)
    ensure_lumen(force_build=args.build)

    crop_offset: tuple[int, int] | None = None
    results: list[dict] = []
    halted_at: str | None = None

    # --- Определяем набор тестов для запуска ---
    run_filter: set[str] | None = None
    if args.only:
        run_filter = {args.only}
    elif args.recheck:
        latest = _load_latest()
        if latest is None:
            print('Нет предыдущих результатов в latest.json. Сначала запустите полный прогон.')
            return 1
        fail_ids = {r['id'] for r in latest.get('tests', []) if r['status'] != 'PASS'}
        if not fail_ids:
            print('В последнем прогоне нет провалившихся тестов — нечего перепроверять.')
            return 0
        # Включаем TEST-00 для определения crop_offset; если он не упал — берём
        # сохранённый offset и не тратим время на перепрогон калибровки.
        if '00' not in fail_ids:
            crop_offset = _load_crop_offset()
            if crop_offset:
                print(f'Калибровка пропущена (crop_offset={crop_offset} из кэша).')
            else:
                fail_ids.add('00')  # нет кэша — нужно перекалибровать
        run_filter = fail_ids
        print(f'--recheck: {len(fail_ids)} тест(ов) из последнего прогона')

    # --- Прогон ---
    for tid, html, threshold, label in TESTS:
        if run_filter is not None and tid not in run_filter:
            continue
        passed, crop_offset, pct, region = run_one(
            tid, html, threshold, label, crop_offset,
            no_cache=args.no_cache,
        )
        if pct < 0:
            status = 'ERROR'
        elif passed:
            status = 'PASS'
        else:
            status = 'FAIL'
        results.append({
            'id': tid,
            'stem': html[:-5],
            'label': label,
            'html': html,
            'threshold': threshold,
            'status': status,
            'diff_pct': round(pct, 4),
            'diff_region': region,
        })
        if not passed and not args.continue_on_fail:
            halted_at = tid
            break

    # --- Сохранение результатов ---
    if not args.only:
        json_path = save_results(results, crop_offset, halted_at)
        html_path = json_path.replace('.json', '.html')
        print(f'\nРезультаты: {os.path.relpath(json_path, REPO)}')
        print(f'HTML-отчёт: {os.path.relpath(html_path, REPO)}')
        prev = _load_previous()
        if prev:
            print_diff_vs_previous(results, prev)

    # --- Итог ---
    if halted_at:
        skipped = len([t for t in TESTS if t[0] > halted_at])
        print(f'\nPipeline stopped at TEST-{halted_at}. {skipped} tests skipped.')
        return 1
    failed = [r for r in results if r['status'] != 'PASS']
    if failed:
        print(f'\n{len(failed)}/{len(results)} tests FAILED: ' + ', '.join(r['id'] for r in failed))
        return 1
    print(f'\nAll {len(results)} tests passed.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
