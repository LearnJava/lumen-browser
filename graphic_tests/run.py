#!/usr/bin/env python3
"""Lumen graphic tests — блокирующий пайплайн.

Запуск:
    python graphic_tests/run.py                     # все тесты, стоп на первом провале
    python graphic_tests/run.py --continue-on-fail  # все тесты, собрать все результаты
    python graphic_tests/run.py --only 03           # один тест по id
    python graphic_tests/run.py --recheck           # перезапустить только FAIL из latest.json
    python graphic_tests/run.py --build             # пересобрать lumen.exe перед запуском
    LUMEN_PROFILE=dev-release python graphic_tests/run.py --build  # быстрая сборка (2–3× быстрее)
    python graphic_tests/run.py --no-cache          # принудительная пересъёмка Edge-скриншотов
    python graphic_tests/run.py --bisect 100        # юнит-зависимости interaction-теста + сам тест
    python graphic_tests/run.py --ipc               # захват Lumen по IPC (CPU-снимок), без gdigrab (TAB-7)
    python graphic_tests/run.py --live               # одно живое окно на весь прогон, gdigrab-снимок (SDC-3)
    python graphic_tests/run.py --paint-bisect 100  # DEVX-4: diff% при отключении каждого LUMEN_NO_* флага

Workflow:
  1. Снимаем Edge headless + Lumen (gdigrab) для каждого теста по порядку.
  2. TEST-00 calibration: ищем магента-маркеры → определяем crop offset.
  3. Каждый следующий тест: кропаем Lumen по offset из калибровки, считаем diff с Edge.
  4. Первый тест с diff% > threshold останавливает пайплайн (если не --continue-on-fail).

Режим --ipc (TAB-7): вместо окна Lumen + gdigrab контроллер один раз поднимает
`lumen.exe --ipc-server` (TCP-сервер таб-команд), держит одну вкладку и шлёт на каждый
тест NavigateTab + Screenshot — обратно приходит детерминированный CPU-снимок (PNG),
который уже от (0,0), поэтому магента-калибровка/crop offset не нужны. Протокол —
length-prefixed bincode (см. секцию «IPC client» ниже и crates/ipc/src/lib.rs).
Edge-эталон, ffmpeg-crop и diff-метрика — те же. NB: CPU-бэкенд снимка пока не на
паритете с femtovg по border-radius/gradients/images (BUG-221), поэтому --ipc
опционален, а gdigrab остаётся дефолтным захватом.

Режим --live (SDC-3): один процесс/окно lumen на весь прогон вместо kill+relaunch
на каждый тест (то был главный источник расхода времени и гонок фокуса — «magenta
marker not found»). Управление окном — MCP (`--mcp-live-port`, SDC-2) через
LiveWindowSession: `tools/call navigate` грузит страницу, `tools/call
wait{condition:document_ready}` даёт настоящий сигнал готовности вместо
`time.sleep(LUMEN_WAIT_SEC)`. Сам пиксельный снимок — по-прежнему gdigrab
настоящего femtovg-окна (не CPU-путь MCP `resource://screenshot`, у которого тот
же разрыв паритета, что и у --ipc), поэтому --live совместим с реальным JS
(TEST-57, 129-138 — им нужен настоящий движок, не CPU-снимок без исполнения
скриптов). TEST-00 калибрует crop offset один раз за прогон, как и раньше —
окно/процесс просто не пересоздаётся между тестами.

DEVX-1: живое окно запускается с `--deterministic --viewport 1024x720` — первое
замораживает Date.now()/Math.random()/rAF timestamp (убирает флейк в TEST-57,
129-138), второе перекрывает вызванный `--deterministic` дефолт окна 1280×800
обратно на калиброванные 1024×720 (иначе магента-маркер TEST-00 и весь crop
offset были бы недействительны). После каждого теста читается MCP
`resource://console` (буфер чистится на `navigate()`, см. `LiveWindowClient.
read_console`) — любая `console.error` на странице FAIL'ит тест независимо от
pixel diff и попадает в HTML-отчёт (класс багов, невидимый на скриншоте).

Результаты:
  graphic_tests/results/YYYYMMDD-HHMMSS.json — полные результаты прогона
  graphic_tests/results/YYYYMMDD-HHMMSS.html — визуальный отчёт (edge|lumen|diff для FAIL)
  graphic_tests/results/latest.json          — всегда указывает на последний прогон

Edge-скриншоты кэшируются: пересъёмка только если HTML новее PNG или передан --no-cache.
--recheck загружает список FAIL из latest.json и перегоняет только их + TEST-00.
"""
from __future__ import annotations
from html import escape as escape_html
import argparse
import ctypes
import ctypes.wintypes
import datetime
import io
import json
import os
import socket
import struct
import subprocess
import sys
import threading
import time
import zlib

# Force UTF-8 stdout to avoid cp1251 codec errors on Windows console
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# --- Конфиг ---

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
FFMPEG = os.path.join(REPO, 'utils', 'ffmpeg.exe')
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
LUMEN_PROFILE = os.environ.get('LUMEN_PROFILE', 'release')
LUMEN = os.path.join(REPO, 'target', LUMEN_PROFILE, 'lumen.exe')
SHOTS = os.path.join(REPO, 'graphic_tests', 'screenshots')
RESULTS_DIR = os.path.join(REPO, 'graphic_tests', 'results')
TESTS_DIR = os.path.join(REPO, 'graphic_tests')

VIEWPORT_W = 1024
VIEWPORT_H = 720
LUMEN_WAIT_SEC = 5

# DEVX-4: paint-optimization kill-switches (crates/engine/paint/src/renderer.rs),
# toggled one at a time by --paint-bisect to localize which optimization moves
# the pixel diff.
PAINT_BISECT_FLAGS = [
    'LUMEN_NO_FRAME_SKIP',
    'LUMEN_NO_SCROLL_COMPOSITOR',
    'LUMEN_NO_ANIM_SPLIT',
    'LUMEN_NO_BBOX_SCISSOR',
    'LUMEN_NO_BBOX_BACKDROP',
    'LUMEN_NO_IMAGE_MIPS',
    'LUMEN_NO_BAND_BIAS',
]

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
    ('52', '52-text-shadow-blur.html', 0.5, 'text-shadow blur — PushFilter{Blur(sigma)} wrapping: sharp/4px/10px/20px blur progression + multi-shadow + glow'),
    ('53', '53-background-origin.html', 0.5, 'background-origin — positioning area: border-box/padding-box/content-box vs background-clip; 0%/100% anchoring'),
    ('54', '54-svg-path-stroke.html', 0.5, 'SVG <path> stroke tessellation — open/closed stroke, fill+stroke, miter join, butt cap, widths 2-14px'),
    ('55', '55-video-placeholder.html', 0.5, '<video> replaced element — grey DrawImage placeholder; UA default 300×150; CSS/attr dimensions; border + border-radius'),
    ('56', '56-mix-blend-mode.html', 0.5, 'CSS Compositing L1 §5 — mix-blend-mode: normal/multiply/darken/color-burn/screen/lighten/color-dodge/overlay/hard-light/soft-light/difference/exclusion/hue/saturation/color/luminosity + nesting'),
    ('57', '57-canvas-2d.html', 0.5, 'HTML LS §4.12.4 — <canvas> getContext("2d"): fillRect/strokeRect/arc/path fill; UA default 300×150; attr dimensions; CSS background + border + border-radius on the canvas element box'),
    ('58', '58-first-letter-line.html', 0.5, 'CSS Pseudo-elements L4 §5.3-5.4 — ::first-letter drop-cap (large yellow, float:left) + ::first-line green bold first line'),
    ('59', '59-image-set-cross-fade.html', 0.5, 'CSS Images L4 §5/§4 — image-set() DPR selection + cross-fade() two-image blend; -webkit- vendor prefix variants'),
    ('60', '60-svg-stroke-advanced.html', 0.5, 'SVG stroke advanced: stroke-linecap (butt/round/square), stroke-linejoin (miter/bevel/round), stroke-miterlimit, stroke-dasharray, stroke-dashoffset, fill-rule (nonzero/evenodd)'),
    ('61', '61-view-transitions.html',   0.5, 'View Transitions API: document.startViewTransition(cb) — JS API, ViewTransition object, Begin/End events, 300 ms cross-fade'),
    ('62', '62-scroll-snap.html',        0.5, 'CSS Scroll Snap L1: scroll-snap-type (y/x mandatory, both proximity), scroll-snap-align (start), scroll-snap-stop (always); static layout geometry of snap containers'),
    ('63', '63-masonry.html',            0.5, 'CSS Masonry layout (Houdini) Phase 0: waterfall grid — items placed in column with min height; column-count, gap'),
    ('64', '64-table.html',              0.5, 'CSS 2.1 §17 Table layout: emit_table_box() renders table/thead/tbody/tr/td cells with separate border-spacing, cell backgrounds, borders; col_span/row_span in struct'),
    ('65', '65-flex-align-content.html', 0.5, 'CSS Flexbox align-content: flex-start/end/center/space-between/space-around/space-evenly/stretch for multi-line flex containers'),
    ('66', '66-selection-pseudo.html',  0.5, 'CSS ::selection pseudo-element: background-color + color override; swatch boxes show parsed selection colours; page renders correctly with ::selection rules present'),
    ('67', '67-attr-typed.html',         0.5, 'CSS Values L4 §7.7 attr() typed substitution: content:attr(data-label) generates ::before labels from HTML attributes; 5 labelled bar rows with distinct colours and widths'),
    ('68', '68-font-variation-settings.html', 0.5, 'CSS Fonts L4 §6.3 font-variation-settings: parsing and layout stability; coloured boxes with varied wght/wdth/slnt axes and inherited settings'),
    ('69', '69-border-spacing.html', 0.5, 'CSS 2.1 §17.6 border-spacing: equal and asymmetric gaps between table cells; three tables showing 12px equal, 8px/24px asymmetric, and 0 spacing'),
    ('70', '70-object-fit.html', 0.5, 'CSS Images L3 §5.5 object-fit / object-position: SVG viewBox scaling modes fill/contain/cover/none/scale-down and object-position alignment'),
    ('71', '71-starting-style.html', 0.5, 'CSS Transitions L2 §3.4 @starting-style: entry animation support — @starting-style rules parse without crashing; static rendering of two coloured boxes is unaffected'),
    ('72', '72-host-slotted.html', 0.5, 'CSS Scoping L1 §6.1-6.2 :host / ::slotted: shadow host gets blue background via :host; :host(.special) colours second host amber; :host(.missing) does not match third host; ::slotted children get coloured boxes'),
    ('73', '73-gap-rule.html', 0.5, 'CSS Gap Decorations L1: gap-rule-width/style/color — flex-row with solid red rules, flex-wrap with dashed cyan rules, grid with solid orange rules'),
    ('74', '74-font-stretch.html', 0.5, 'CSS Fonts L4 §5.2: font-stretch keyword + % values, cascade/inheritance, no-double wdth injection — coloured boxes'),
    ('75', '75-masonry-auto-flow.html', 0.5, 'CSS Masonry Layout §9: masonry-auto-flow: next (source order) / ordered (CSS order property) / definite-first — coloured boxes in 3 masonry grids'),
    ('76', '76-motion-path.html', 0.5, 'CSS Motion Path L1: offset-path + offset-distance + offset-rotate (auto/fixed) — boxes translated along horizontal, diagonal, and cubic-bezier paths'),
    ('77', '77-anchor-positioning.html', 0.5, 'CSS Anchor Positioning L1: anchor-name + position-anchor + inset-area corner/edge/span placement around a central anchor element'),
    ('78', '78-scroll-driven-animations.html', 0.5, 'CSS Scroll-Driven Animations L1: scroll-timeline-name/axis, view-timeline-name/axis, animation-timeline: scroll()/view()/named'),
    ('79', '79-text-underline-offset.html', 0.5, 'CSS Text Decoration L4: text-underline-offset (px/auto/negative) + text-underline-position (under) wired to push_text_decoration'),
    ('80', '80-border-collapse.html', 0.5, 'CSS Tables L2 §17.6: border-collapse separate (4px spacing) vs collapse (no spacing) + mixed border widths + cell backgrounds'),
    ('81', '81-view-transition-name.html', 0.5, 'CSS View Transitions L1 §10: view-transition-name property — named elements render identically to un-named elements (property has no visual effect outside a transition)'),
    ('82', '82-svg-use.html', 0.5, 'SVG <use> element: clone shapes/groups/symbols from <defs>, x/y offset, xlink:href, nested chains'),
    ('83', '83-scroll-behavior.html', 0.5, 'scroll-behavior: smooth/auto — overflow scroll containers + page-level scroll (CSS Scroll Behavior L1 §3)'),
    ('84', '84-text-decoration-skip-ink.html', 0.5, 'text-decoration-skip-ink: auto/none/all — underline gaps over glyph descenders (CSS Text Decoration L4 §3.5)'),
    # --- Interaction-слой (серия 100–199): взаимодействие свойств, уже покрытых юнит-тестами 00–99.
    # Падение здесь при зелёных юнит-зависимостях (см. DEPS) = баг взаимодействия, не свойства.
    ('100', '100-transform-overflow.html',   0.5, 'INTERACTION: transform × overflow:hidden — клиппинг трансформированных слоёв, поворот самого клип-контейнера'),
    ('101', '101-radius-overflow.html',      0.5, 'INTERACTION: border-radius × overflow:hidden — скруглённый клип детей (углы, круг, pill, вложенность)'),
    ('102', '102-opacity-stacking.html',     0.5, 'INTERACTION: opacity × z-index — атомарный stacking context, групповая композиция без двойного затемнения, вложенная opacity'),
    ('103', '103-filter-transform.html',     0.5, 'INTERACTION: filter × transform — фильтр поверх трансформированного слоя, filter как containing block'),
    ('104', '104-mask-gradient-radius.html', 0.5, 'INTERACTION: mask-image × gradients × border-radius — градиентная маска поверх градиентного фона и скруглений'),
    ('105', '105-float-clear-margin.html',   0.5, 'INTERACTION: float/clear × margin — отступы флоатов, clearance+margin, перенос флоатов на новую строку'),
    ('106', '106-transform-zindex.html',     0.5, 'INTERACTION: transform × z-index — transform создаёт stacking context, z-дети заперты внутри'),
    ('107', '107-shadow-radius-overflow.html', 0.5, 'INTERACTION: box-shadow × border-radius × overflow — скруглённый силуэт тени, клип тени родителем'),
    ('108', '108-nested-transforms.html',    0.5, 'INTERACTION: вложенные transform — композиция матриц (rotate∘rotate⁻¹=identity, scale×translate, 3×rotate=сумма)'),
    ('109', '109-clippath-transform.html',   0.5, 'INTERACTION: clip-path × transform × border-radius — клип в локальном боксе элемента сквозь трансформацию'),
    # --- CSS Anchor Positioning L1 (серия 85–89): Phase 0 stub тесты
    ('85', '85-anchor-name-basic.html', 0.5, 'anchor-name: --foo — базовое объявление якоря, визуализация элемента (стаб)'),
    ('86', '86-position-anchor-fallback.html', 0.5, 'position-anchor: --foo — привязка к якорю, fallback позиция без inset-area (стаб)'),
    ('87', '87-inset-area-none.html', 0.5, 'inset-area: none none — якорь не влияет на позицию при none keywords (стаб)'),
    ('88', '88-anchor-nested.html', 0.5, 'anchor-name в вложенных элементах — иерархия DOM, поиск якорей (стаб)'),
    ('89', '89-anchor-multiple-names.html', 0.5, 'несколько anchor-name элементов — регистрация множества якорей в дереве (стаб)'),
    ('90', '90-avif-image.html', 0.5, 'AVIF image decoder — <picture> с AVIF source + PNG fallback, direct <img src="...avif">'),
    ('91', '91-relative-color.html', 0.5, 'CSS Color L5 §4: relative color syntax — rgb/hsl/oklch(from <color> …) с channel keywords, calc() над каналами, channel reorder, alpha-канал над непрозрачным фоном'),
    ('92', '92-system-colors.html', 0.5, 'CSS Color 4 §6.2: system color keywords — Canvas, ButtonFace, Highlight, GrayText, AccentColor и т.д. как фон и border-color'),
    ('93', '93-field-sizing.html', 0.5, 'CSS Basic UI L4 §4.4: field-sizing: content — input/textarea подгоняют размер под текст содержимого вместо UA-дефолта'),
    ('94', '94-interpolate-size.html', 0.5, 'CSS Sizing L4 §4.5: interpolate-size: allow-keywords — наследуемый opt-in для интерполяции keyword-размеров; в покое (статичный снимок) layout не меняется'),
    ('95', '95-font-size-adjust.html', 0.5, 'CSS Fonts L5 §4: font-size-adjust — при одинаковом font-size used-размер масштабируется как adjust/aspect шрифта; видимый x-height строк уменьшается сверху вниз'),
    ('96', '96-color-function-spaces.html', 0.5, 'CSS Color 4 §10: color() предопределённые пространства — srgb-linear, a98-rgb, prophoto-rgb, xyz, xyz-d65, xyz-d50; in-gamut цвета совпадают с эталоном после маппинга в sRGB'),
    ('97', '97-counter-set.html', 0.5, 'CSS Lists L3 §4: counter-set — порядок reset→increment→set (set перекрывает increment), создание счётчика на never-reset; ::before content: counter(c) показывает 5/6/0/1/42'),
    ('98', '98-revert-layer.html', 0.5, 'CSS Cascade L5 §6.4.6: revert-layer — откат свойства к значению нижнего каскадного слоя; верхний ряд theme(red), нижний ряд revert-layer→base(green)'),
    ('99', '99-offset-path-ray.html', 0.5, 'CSS Motion Path L1 §2.2: offset-path: ray(<angle>) — восемь боксов по лучам 0/45/.../315deg формируют кольцо вокруг центра; turn-единица; offset-distance 0 держит центр'),
    ('110', '110-accent-color.html', 0.5, 'CSS UI L4 §6.1: accent-color — тинт чекбокса/радио/range/progress; пять рядов с разными accent (red/green/orange/purple/UA-blue); <meter> исключён (семантические цвета)'),
    ('111', '111-appearance-none.html', 0.5, 'CSS Basic UI L4 §4.2: appearance: none — снимает нативную отрисовку формы (тик чекбокса/радио, трек/ползунок range, бар progress); контролы внутри светлых обёрток остаются пустыми, лишний синий индикатор = регрессия'),
    ('112', '112-clip-path-fill-rule.html', 0.5, 'CSS Shapes L1 §3/§4: clip-path fill-rule — path()/polygon() с evenodd оставляют полую середину у self-intersecting пентаграммы и пересечения квадратов; nonzero (default) заливает форму целиком'),
    ('113', '113-shape-outside-path.html', 0.5, 'CSS Shapes L1 §4: shape-outside: path() — inline-block квадраты обтекают треугольный float по флэттенному SVG-контуру; колонка path() должна совпасть с эталонной колонкой polygon() (одинаковая лесенка)'),
    ('114', '114-contain-intrinsic-size.html', 0.5, 'CSS Box Sizing L4 §5: contain-intrinsic-size — боксы с contain: size берут размер из contain-intrinsic-size, игнорируя огромного ребёнка; inline-block боксы 200×120/120×200/200×100 (em), блок высотой 90px; зелёный ребёнок не должен вылезать'),
    ('115', '115-empty-cells.html', 0.5, 'CSS Tables L2 §17.6.1.1: empty-cells — в separate-модели hide прячет border+фон у пустых <td></td>; верхняя таблица (hide) показывает шахматку только из заполненных ячеек, нижняя (show) рисует рамки+фон всех ячеек'),
    ('116', '116-gradient-interpolation.html', 0.5, 'CSS Images L4 §3.1: gradient color-interpolation-method (`in <space>`) — одинаковый red→blue линейный градиент в srgb/srgb-linear/oklab/lab/hsl; Lumen дробит список стопов через color-mix-математику, рендерер лерпит плотные стопы в sRGB; середины полос заметно различаются по пространству'),
    ('117', '117-quotes.html', 0.5, 'CSS Generated Content L3 §3.2: quotes + content open-quote/close-quote — auto curly quotes, вложенные <q> (primary “”→secondary ‘’), кастомные пары « » / ‹ ›, quotes:none без знаков; глубина вложенности считается в document order через counters pre-pass'),
    ('118', '118-media-hover-pointer.html', 0.5, 'Media Queries L4 §5.3-5.6: hover/any-hover/pointer/any-pointer — на десктопе (мышь) matched-свотчи (hover:hover, any-hover:hover, pointer:fine, any-pointer:fine) зелёные, no-match (hover:none, pointer:coarse) остаются красными'),
    ('119', '119-paint-order.html', 0.5, 'CSS Fill & Stroke L3 §6 / SVG2 §13.7: paint-order — thick-stroked <path> с centred stroke; верхний ряд normal (полная ширина обводки поверх заливки), нижний paint-order:stroke (заливка поверх обводки → видимая обводка вдвое тоньше, заливка крупнее)'),
    ('120', '120-media-contrast-data.html', 0.5, 'Media Queries L5 §5.5-5.6: prefers-contrast/prefers-reduced-data — без пользовательских предпочтений matched-свотчи (no-preference) зелёные, no-match (more/less/custom/reduce) остаются красными'),
    ('121', '121-supports-selector.html', 0.5, 'CSS Conditional L4 §4.2: @supports selector() — распознаваемые селекторы (:has/:is/::slotted) применяют блок (зелёные a/b/c), неизвестные псевдо не применяют (красные d/e), not selector(<unknown>) истинно (зелёный f)'),
    ('122', '122-line-height-step.html', 0.5, 'CSS Rhythmic Sizing L1 §2: line-height-step — высота каждого line-box округляется вверх до кратного шагу; цветные inline-фоны заливают округлённый line-box, поэтому полосы у stepped-колонки (48px) вдвое выше natural-колонки (24px), одиночная строка округлена до 60px, child наследует шаг 40px'),
    ('123', '123-supports-font-tech-format.html', 0.5, 'CSS Conditional L4 §4 / Fonts L4 §4.3: @supports font-tech()/font-format() — реализованные технологии (variations/features-opentype) и декодируемые форматы (woff2/truetype) применяют блок (зелёные a/b/c/d); features-graphite и embedded-opentype не поддержаны ни Lumen, ни Edge (красные e/f); not font-tech(features-graphite) истинно (зелёный g)'),
    ('124', '124-prefers-reduced-transparency.html', 0.5, 'Media Queries L5 §5.7: prefers-reduced-transparency — без пользовательских предпочтений no-preference matched (зелёный a), reduce не матчит (красный b), невалидное low → Unsupported (красный c); Edge тоже по умолчанию no-preference'),
    ('125', '125-media-scripting.html', 0.5, 'Media Queries L5 §6.2: scripting — Lumen с QuickJS по умолчанию scripting:enabled matched (зелёный a), none/initial-only не матчат (красные b/c), невалидное sometimes → Unsupported (красный d); Edge тоже scripting enabled'),
    ('126', '126-media-inverted-colors.html', 0.5, 'Media Queries L5 §5.8: inverted-colors — без инверсии цветов none matched (зелёный a), inverted не матчит (красный b), невалидное maybe → Unsupported (красный c); Edge тоже по умолчанию inverted-colors:none'),
    ('128', '128-icc-color-management.html', 0.5, 'ICC colour management (ICC-6): Display P3 PNG (iCCP matrix-shaper). Свотчи закодированы так, что корректный P3→sRGB matrix-shaper трансформ восстанавливает известные in-gamut sRGB-цвета; Edge и Lumen, оба управляя цветом, дают идентичные пиксели. CMYK-ICC (A2B0 LUT) не в графтесте — Edge не применяет встроенный CMYK-профиль (свой SWOP), поэтому покрыт детерминированным Rust-тестом icc_color_management.rs'),
    ('129', '129-svg-dynamic-createelementns.html', 0.5, 'BUG-243: динамический SVG, построенный на клиенте через document.createElementNS + setAttribute + appendChild/append (как docs/roadmap-*.html), без текста — только solid-фигуры (rect/circle/path/g+transform). Узлы должны попасть в нативную арену и отрисоваться как в Edge. ТРЕБУЕТ JS-пути захвата (gdigrab по умолчанию), не --ipc/CPU (детерминированный снимок не исполняет скрипты)'),
    ('130', '130-svg-dynamic-nested-transforms.html', 0.5, 'BUG-243 (dynamic SVG, без текста): глубоко вложенные <g>-трансформы — кумулятивная композиция translate/rotate/scale (спираль из 16 вложенных квадратов) + каталог независимых translate()+scale() мотивов. Проверяет parse_svg_transform.compose на глубине. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('131', '131-svg-dynamic-opacity-layering.html', 0.5, 'BUG-243 (dynamic SVG, без текста): alpha-композитинг через fill-opacity/stroke-opacity — Venn из полупрозрачных дисков, рампа fill-opacity, стопка одноцветных квадратов, полупрозрачные обводки. Смешанные области должны совпасть с Edge (прямая sRGB-альфа). ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('132', '132-svg-dynamic-use-symbol.html', 0.5, 'BUG-243 (dynamic SVG, без текста): инстансинг <defs>/<symbol>/<use> — мульти-фигурный символ, инстанцированный сеткой и кольцом с per-instance translate/scale/rotate. Путь shadow-clone, построенный из JS. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('133', '133-svg-dynamic-fill-rule.html', 0.5, 'BUG-243 (dynamic SVG, без текста): fill-rule nonzero vs evenodd на самопересекающихся <path>/<polygon> (пентаграммы, концентрические квадраты-кольца) — evenodd-колонка показывает дырки, которые nonzero заливает. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('134', '134-svg-dynamic-stroke-styles.html', 0.5, 'BUG-243 (dynamic SVG, без текста): обводки — stroke-width/linecap/linejoin/dasharray/dashoffset на <rect>/<line>/<polyline>/<path> (концентрические рамки, пунктиры с разными капами, ступенчатые полилинии с разными join). ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('135', '135-svg-dynamic-paint-order.html', 0.5, 'BUG-243 (dynamic SVG, без текста): порядок отрисовки по document order (z-stack) — каскад перекрытых квадратов/кругов, плетёная сетка баров, веер повёрнутых карт. Геометрия перекрытий должна совпасть с Edge. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('136', '136-svg-dynamic-ellipse-radial.html', 0.5, 'BUG-243 (dynamic SVG, без текста): геометрия <circle>/<ellipse> — концентрические эллипсы (varying rx/ry), мишень из кругов, радиальный кластер кругов, ряд эллипсов с растущим ry. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('137', '137-svg-dynamic-transform-pinwheel.html', 0.5, 'BUG-243 (dynamic SVG, без текста): presentation-трансформы rotate(angle cx cy)/scale/matrix()/skewX — вертушки из лепестков-треугольников, кольцо повёрнутых прямоугольников, зеркальные matrix()-шевроны, skewX-сетка. Покрывает все ветки parse_svg_transform. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('138', '138-svg-dynamic-dashboard.html', 0.5, 'BUG-243 (dynamic SVG, без текста): kitchen-sink «дашборд» — вложенные <g>, <defs>/<use>, fill-opacity, <path>-«пирог» из полигональных секторов, bar-chart, area+line-chart (polyline stroke + полупрозрачная заливка), <circle>/<ellipse>. Самая сложная композиция, всё solid/детерминировано. ТРЕБУЕТ JS-захвата (gdigrab)'),
    ('139', '139-float-flow.html', 0.5, 'RP-4 общий float-поток (CSS 2.1 §9.5): (1) clear:left во вложенном НЕ-BFC обёртке клирит float родителя — синий блок падает под красный float на полную ширину; (2) overflow:hidden BFC-блок рядом с float сдвигается за его правый край, не лезет под; (3) float внутри не-BFC обёртки стыкуется справа от внешнего float (наследуется float-контекст). Только solid-боксы без перекрытий (текст игнорируется, rule 3)'),
    ('140', '140-placeholder-pseudo.html', 0.5, 'P4 ::placeholder pseudo-element (CSS Pseudo-Elements L4 §4.10): input::placeholder { color } без правила и с правилом(-ями), опционально opacity. Swatch-боксы показывают резолвленный цвет (как в 66-selection-pseudo — реальный текст не сравнивается, только solid-цвет)'),
    ('141', '141-backface-visibility.html', 0.5, 'P4 backface-visibility culling (CSS Transforms L2 §5.1): rotateY(180deg)/rotateX(180deg) + backface-visibility:hidden прячет бокс (виден фон контейнера); rotateY(0deg) или backface-visibility:visible (default) — бокс остаётся виден. Только solid-боксы, без перекрытий'),
    ('142', '142-color-profile.html', 0.5, 'P4 @color-profile + color(--name c1 c2 c3) (CSS Color L5 §4): именованный custom-профиль, реальная ICC-трансформация отложена — каналы трактуются как sRGB напрямую, каждый swatch должен совпасть с эквивалентным sRGB-цветом'),
    ('143', '143-css-function.html', 0.5, 'P4 @function (CSS Functions and Mixins L1): именованная custom-функция --name(<args>), вызванная из property value. Покрывает прямой вызов с явным аргументом, нулевой вызов (default-параметр), calc() внутри result:, вызов через custom-property цепочку (var()), локальную --x: ...; декларацию, feeding result: через clamp(). Только solid-боксы, геометрия/цвет полностью детерминированы вычислением функции'),
    ('144', '144-anchor-function.html', 0.5, 'P4 anchor()/anchor-size() (CSS Anchor Positioning L1 §3.1, §4): anchor(bottom)/anchor(left) hug the anchor edges; anchor(top)/anchor(75%) mixes a side keyword with a percentage; anchor(--missing top, 20px)/anchor(--missing left, 700px) exercises the fallback path when the referenced anchor does not exist; anchor-size(width)/anchor-size(height) sizes a box to match the anchor exactly. Только solid-боксы, геометрия полностью детерминирована resolve_anchor_func/resolve_anchor_size'),
    ('145', '145-writing-mode.html', 0.5, 'P3-vertical CSS Writing Modes L4 §3-4: writing-mode vertical-rl/vertical-lr × text-orientation mixed/upright/sideways, Latin+CJK (日本語) text. Проверяет per-glyph поворот (CPU/wgpu рендереры) — mixed держит CJK upright и поворачивает латиницу на 90°, upright не поворачивает ничего, sideways поворачивает всё. Текстовый тест — ожидаем font-parity debtor (rule 3, Inter vs Edge sans metrics)'),
    ('146', '146-imagebitmap.html', 0.5, 'P1-imagebitmap createImageBitmap + ImageBitmapRenderingContext (HTML LS §4.12.5): ImageData → createImageBitmap → getContext(\'bitmaprenderer\').transferFromImageBitmap presents an identical copy on a second <canvas>, plus a cropped (sx,sy,sw,sh) variant; createImageBitmap(OffscreenCanvas) snapshots without detaching the source (drawn again and re-presented afterwards). Только solid-боксы/векторная заливка, без текста'),
    ('147', '147-background-repeat-space.html', 0.5, 'P4-mask-repeat-space CSS Backgrounds L3 §3.4 background-repeat: space — целые плитки прижаты к обоим краям, остаток распределён равными зазорами (space_axis_geometry, общий с mask-repeat: space); рядом repeat/round для сравнения. Только картинка-плитка, без текста'),
    ('148', '148-isolation.html', 0.5, 'P4-isolation CSS Compositing & Blending L1 §2.1 isolation: isolate — изолированная группа: mix-blend-mode детей композитится с прозрачным фоном группы, а не с фоном страницы; слева блендинг с amber-фоном, справа изоляция (чистый цвет квадрата). Multiply/difference + вложенные квадраты. Только solid-боксы, без текста'),
]

# --- Известные должники (Phase 2+ фичи, baseline-храповик) ---
#
# Тесты, которые не могут достичь 0.5% пока соответствующая фича не реализована.
# Формат: test_id → (BUG-NNN, baseline_pct).
#
# Семантика (±_DEBTOR_TOL% допуск для нестабильности gdigrab):
#   actual ≤ 0.5%                      → FAIL: «удали запись — цель достигнута»
#   actual < baseline − _DEBTOR_TOL    → FAIL: «снизь baseline до X.XX» (храповик)
#   |actual − baseline| ≤ _DEBTOR_TOL  → DEBTOR: ожидаемо красный, не останавливает пайплайн
#   actual > baseline + _DEBTOR_TOL    → FAIL: регрессия на известном должнике
#
# Добавлять ТОЛЬКО Phase 2+ фичи с OPEN BUG-NNN и diff-изображением,
# подтверждающим что расхождение локализовано в области нереализованной фичи.
#
# BUG-287/BUG-277 (2026-07-16): `P1-wgpu-flip` (2026-07-13, ADR-017) сделал wgpu
# дефолтным рендер-бэкендом, но эта таблица была настроена под femtovg-baseline и
# не обновлена при флипе. wgpu-vs-Edge расхождения были ЗАРАНЕЕ измерены и
# задокументированы в BUG-277 (`LUMEN_BACKEND=wgpu` прогон 2026-07-13, commit
# e8bd5bd0+BUG-276) как гейт для Phase 3 — но флип landed без переноса этого
# базлайна сюда, из-за чего полный прогон читался как «43 FAIL регрессия»
# (заведено как BUG-287). Ревизия P3 2026-07-16 (git bisect, good=`0a767ff0`,
# bad=`c76cbeae`=`P1-wgpu-flip`) подтвердила: каждый из 37 расхождений точь-в-точь
# совпадает с рядом BUG-277 (тот же diff% на независимом прогоне 3 дня спустя) —
# не движковая регрессия, а незаполненный wgpu-baseline. Записи ниже с пометкой
# «BUG-277» — перенос этого базлайна; исходный BUG-NNN (если был) описывает
# femtovg-родословную фичи, а не текущий wgpu-специфичный остаток.
KNOWN_DEBTORS: dict[str, tuple[str, float]] = {
    '14': ('BUG-288', 1.63),   # overflow: the overflow-x:hidden/overflow-y:visible and overflow-x:visible/overflow-y:hidden columns coerce the visible axis to `auto` (BUG-020, CSS Overflow L3 §2.1) — correctly clipped, but `auto` is a scrollable overflow value, so `emit_scrollbars` (BUG-220, landed after BUG-020) now paints a static scrollbar there. Edge uses overlay scrollbars, invisible in a static headless screenshot, so it shows no bar — same class as TEST-83. No text on this page, so the whole 1.63% is this one divergence; not fixable without suppressing scrollbar rendering that is otherwise correct

    # --- BUG-277 batch (P3 2026-07-16, BUG-287 resolution): these had no prior
    #     KNOWN_DEBTORS entry under femtovg — they PASSed at ≤0.5% before
    #     `P1-wgpu-flip`. Each number below matches BUG-277's independently
    #     measured wgpu-vs-Edge row exactly (`LUMEN_BACKEND=wgpu` run,
    #     2026-07-13, 3 days apart) — not a fresh engine regression, just the
    #     already-catalogued wgpu baseline never folded into this table when
    #     wgpu became default. See BUG-277 for per-test region bounds and the
    #     "large gaps (≥10%)" note (49/59/76/101/104 likely a genuine
    #     wgpu-specific rendering gap, not just AA drift — candidates for a
    #     dedicated P1/P3 wgpu-parity pass, not this doc-sync). ---
    '24': ('BUG-277', 0.50),
    '26': ('BUG-277', 13.50),    # mask-image. DS-9 (2026-07-23, P1): ратчет 11.24%→13.50%. Постоянный тулбар удвоил высоту хрома над контентом (TAB_BAR_HEIGHT=36 → toolbar::CHROME_H=72), сдвинув абсолютную Y-позицию композитинга контента на экране; TEST-00/03 (магента-калибровка) проходят на 0.00%, т.е. геометрия viewport-а верна — сдвиг лишь меняет дробный остаток sub-pixel snapping на границах маски (тот же класс, что и BUG-124/PS-1), не новый дефект
    '49': ('BUG-277', 2.74),    # background-blend-mode. **BUG-277 срез 2 (2026-07-22, P1):** ратчет 28.15%→2.74% (10×). Root-cause: у top-level бокса (без родительского stacking-context) фоновые blend-слои композитились на from_level==1, чей «родитель» — swapchain-поверхность без TEXTURE_BINDING → wgpu-`Composite` молча падал в alpha-over, теряя blend целиком. Фикс: `emit_background_image` оборачивает стек фоновых слоёв в собственную `PushOpacity{alpha:1.0}`-изоляционную группу, когда не-нижний слой реально блендит (CSS Compositing L1 §8.3 «background = isolated group») → blend-пара получает свой 2-уровневый offscreen-стек независимо от вложенности. Плюс un-premultiply в BLEND_SHADER (offscreen-слои копят премультиплированный rgb) и отдельный uniform-буфер на каждый composite (устранён write_buffer-hazard при 2+ blend в кадре). Остаток 2.74% = AA-кромка/font-parity (rule 2/3). НЕ закрывает BUG-277: mix-blend-mode на top-level боксах (TEST-56/148) — отдельный путь, не изолируется этим срезом
    '56': ('BUG-277', 14.12),
    '68': ('BUG-277', 3.17),
    '72': ('BUG-277', 1.29),
    '74': ('BUG-277', 3.74),
    '81': ('BUG-277', 3.44),
    '100': ('BUG-277', 1.09),
    '103': ('BUG-277', 1.79),
    '104': ('BUG-277', 32.78),   # INTERACTION mask-gradient-radius (юнит-зависимости 26/39/40/36). DS-9 (2026-07-23, P1): ратчет 19.94%→32.78% — тулбар удвоил высоту хрома (CHROME_H=72), TEST-00/03 калибруются на 0.00%; каскадно наследует сдвиг sub-pixel snapping от TEST-26 (mask-image, тот же класс BUG-124/PS-1), не новый дефект
    '107': ('BUG-277', 7.27),
    '109': ('BUG-277', 7.53),
    '111': ('BUG-277', 1.27),
    '112': ('BUG-277', 7.41),
    '116': ('BUG-277', 2.40),
    '140': ('BUG-277', 2.17),
    '141': ('BUG-277', 1.59),
    '148': ('BUG-277', 5.44),   # P4-isolation: isolation:isolate blend-группа. Фича КОРРЕКТНА — CPU-снимок (cpu_raster, `lumen --screenshot`) пиксельно совпадает с Edge (изолированные ячейки = чистый цвет, неизолированные = блендинг), unit-тесты зелёные. Дивергенция целиком wgpu mix-blend (BUG-277, тот же класс, что TEST-56): в wgpu-окне неизолированный mix-blend не композитится (source-over вместо multiply), а изолирующий offscreen-слой делает multiply-против-прозрачного фоном чёрным. **BUG-277 срез 2 (2026-07-22):** un-premultiply в BLEND_SHADER (изолирующий offscreen-слой больше не чернеет при multiply-против-прозрачного) ратчет 6.30%→5.44%. Остаток = неизолированный top-level mix-blend (тот же путь, что TEST-56, не покрыт background-only изоляцией этого среза). Уйдёт в PASS с mix-blend-срезом BUG-277.
    # --- BUG-243 dynamic-SVG suite (JS-built SVG, gdigrab): tests are CORRECT; these
    #     fail until the SVG-engine gaps they uncovered are fixed. Do NOT edit the tests
    #     (user rule) — fix the engine, then delete these entries. ---
    '130': ('BUG-277', 1.00),   # nested <g rotate scale> spiral: femtovg-baseline was 0.10% (BUG-244 FIXED, CTM — history preserved). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip made wgpu the default, ratchet to live wgpu number 1.00% (matches BUG-277 row «130»)
    '132': ('BUG-246', 8.11),  # Ратчет 10.97→8.11 (P3 2026-07-16, wgpu-дефолт full-run). <use> rotated ring: rotation now applied (BUG-244 FIXED, CTM) but instances still not scaled by <symbol> viewBox→width/height → dominant residual is BUG-246; fix BUG-246 → ≤0.5%
    '133': ('BUG-247', 1.95),   # BUG-245 FIXED 2026-06-30 (3.91%→1.95%): fill-rule:evenodd now honoured (scanline even-odd tessellation) — pentagram/ring centres hollow as in Edge. Residual = diagonal/star-edge AA under gdigrab (BUG-247 class), re-pointed to that OPEN bug
    '134': ('BUG-247', 3.35),   # stroke-dasharray placement diverges from Edge under gdigrab (high-freq pattern × subpixel shift); solid strokes match. Inherent AA, not a logic defect. Ратчет 3.69→3.35 (P3 2026-07-04): нативный femtovg-штрих `DrawSvgStroke` (без внутренних triangle-soup AA-швов на пунктире) — замер подтверждён двумя чистыми gdigrab-прогонами
    '135': ('BUG-247', 0.54),   # fanned cards rotate(a cx cy): BUG-244 FIXED (15.62%→0.54%) — rotation centre now correct, cards fan out as in Edge. Residual 0.54% = rotated-rect edge AA (BUG-247 class), just over 0.5%
    '136': ('BUG-247', 1.98),   # <circle>/<ellipse> curve-edge AA vs Edge (geometry/colour correct; only ~1px edge AA differs). Inherent rasterizer-vs-Edge AA
    '137': ('BUG-247', 4.74),   # pinwheels/skew: BUG-244 FIXED (20.79%→4.74%) — rotate()/skewX() now applied via CTM, blades fan correctly. Residual 4.74% = edge AA of many thin high-frequency skewed blades (BUG-247 class, amplified by frequency like TEST-134 dash)
    # '138' removed (P3 2026-07-16, wgpu-дефолт full-run): gauge теперь 0.38% ≤ 0.5% — цель достигнута

    # '02','04','56' убраны (прогон 2026-06-29, REMOVE): BUG-250 FIXED (font-metrics descent revert) — все ≤0.5% на свежей сборке (02 0.00%, 04 0.00%, 56 0.47%)
    # '21' убран (прогон 2026-06-29, REMOVE): BUG-248 FIXED — border-style dashed/dotted/double 0.49% на свежей сборке (≤0.5%; прежний 3.13% был артефактом устаревшего gdigrab-снимка, класс BUG-240)
    '101': ('BUG-277', 20.00),   # INTERACTION border-radius × overflow:hidden: femtovg-baseline была 0.71% (sub-pixel AA скруглённых краёв, класс BUG-247 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 20.00% (совпадает с BUG-277 рядом «101»)
    '90': ('BUG-209', 1.71),    # AVIF <picture>/<img>: доминирующая причина девиации (вложенный column-flex item схлопывался по контенту вместо растяжения на cross-size ряда) исправлена BUG-241 (гейт `explicit_cross.is_some()` в lay_out_flex). TEST-90 2.27%→1.71%: рамки/фоны ячеек теперь совпадают с Edge пиксель-в-пиксель (diff чёрный). AVIF-данные в тесте обрезаны — ни Lumen, ни Edge их не декодируют; Edge рисует placeholder сломанной картинки (иконка + alt-текст), Lumen — нет. Остаток = эта chrome-иконка (не совпадёт с Edge пиксельно) + вертикальный сдвиг подписи + font-parity меток (rule 3)
    '122': ('BUG-237', 11.19),  # line-height-step: Lumen спек-корректно округляет line-box (48/60/40px фоны), Edge свойство не поддерживает (удалён из Chromium ~2018) → эталон рисует un-stepped 24px fallback. Lumen корректнее reference-браузера (класс BUG-126/TEST-77 inset-area, BUG-199/TEST-71 @starting-style). Совпасть = отключить рабочую реализацию (запрещено). Остаток + font-parity переноса natural-колонки (rule 3)
    '53': ('BUG-277', 8.17),    # background-origin: femtovg-baseline была 1.71% (font-parity, класс BUG-128 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 5.45% (совпадает с BUG-277 рядом «53»). DS-9 (2026-07-23, P1): ратчет 5.45%→8.17% — тулбар удвоил высоту хрома (CHROME_H=72), TEST-00/03 калибруются на 0.00%, сдвиг только меняет sub-pixel snapping фоновых слоёв (класс BUG-124/PS-1), не новый дефект
    '54': ('BUG-277', 2.32),    # SVG <path> stroke: femtovg-baseline была 0.26% (BUG-173 FIXED, нативный DrawSvgStroke — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 2.32% (совпадает с BUG-277 рядом «54»)
    '55': ('BUG-192', 0.54),    # <video> placeholder: фича корректна. Edge рисует `<video src="nonexistent.mp4">` БЕЗ серого placeholder (прозрачный, виден только фон) — Lumen рендерит так же (BUG-097: пустое <video> без poster/кадра не рисует ничего). Бокс с border (200×120, 3px solid #4299e1) совпадает с Edge по размеру/цвету. Декомпозиция diff (0.54%): 90% (0.48%) = font-parity 6 меток .label (11px sans, Inter vs Edge) — rule 3; остаток 0.05% = бордер-бокс на 1px ниже (y=212 vs 211), причина — line-height «normal» метки в ряду 1 (Inter ≈1.2 vs Edge sans → row1_height 171 vs 170), тоже font-parity. Реального дефекта плейсхолдера нет. Класс BUG-128
    '57': ('BUG-099', 2.96),    # <canvas> getContext("2d") — Phase 2. Ратчет вниз 4.14→2.96 (прогон 2026-06-23). IPC даёт 27.96% (режим-зависимый артефакт захвата); нормальный gdigrab = 2.96%
    '60': ('BUG-277', 0.74),    # SVG dash/curve stroke: femtovg-baseline была 0.40% (BUG-173 FIXED, нативный DrawSvgStroke — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 0.74% (совпадает с BUG-277 рядом «60»)
    '61': ('BUG-103', 10.99),   # View Transitions L1 РЕНДЕРИТСЯ (F2-4 ревизия 2026-06-22): startViewTransition + root cross-fade работают. Ратчет вниз 99.53→10.99 (прогон 2026-06-23: gdigrab дал настоящий кадр, не blank-capture). Остаток 10.99% = тайминг захвата Edge (async update-callback по спеку — Edge headless снимает кадр ДО callback → старая DOM card1 active; Lumen рендерит устоявшееся card2 active, спек-корректно; тот же класс, что TEST-71/BUG-199 и TEST-77/BUG-126) + font-parity текста (rule 3). Опц. полный L1 (named groups + ::view-transition pseudo) = XL, не валидируется этим тестом
    '65': ('BUG-277', 5.45),   # flex align-content: femtovg-baseline была 2.08% (класс BUG-127 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 5.45% (совпадает с BUG-277 рядом «65»)
    '63': ('BUG-277', 5.46),    # masonry: femtovg-baseline была 2.02% (border-radius edge-AA + font-parity, класс BUG-176 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 5.46% (совпадает с BUG-277 рядом «63»)
    # '75' убран: BUG-143 FIXED (16.97%→0.25%) — grid-masonry fallback + order + stretch + flex border (BUG-232); геометрия = Edge
    # '119' убран (прогон 2026-07-04, REMOVE): регрессия paint-order (16.52%) починена BUG-262 FIXED 2026-06-29 (svg_paint_matrix разведён от svg_transform); остаток AA-швов thick-stroke убран нативным DrawSvgStroke (BUG-173, d87dae63) → свежий full-build 0.38% (≤0.5%). Был baseline 0.81%
    '36': ('BUG-277', 7.80),    # border-radius: femtovg-baseline была 0.96% (inherent edge-AA, класс BUG-124/247 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным — wgpu рисует скруглённые углы иначе, ратчет к живому wgpu-числу 7.80% (совпадает с BUG-277 рядом «36»)
    '31': ('BUG-277', 3.99),    # clip-path: femtovg-baseline была 0.60% (inherent AA-fringe, класс BUG-176/247/173 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным — wgpu клипует иначе, ратчет к живому wgpu-числу 3.99% (совпадает с BUG-277 рядом «31»)
    '30': ('BUG-277', 10.24),    # CSS filter/backdrop-filter: femtovg-baseline была 4.27% (row-flip BUG-144 + backdrop colour-matrix BUG-085 + blur AA, класс BUG-176/247/128 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным — wgpu рендерит filter/blur иначе, ратчет к живому wgpu-числу 10.24% (совпадает с BUG-277 рядом «30» день-в-день независимо измеренным)
         '34': ('BUG-128', 3.02),    # form controls: inline-block flow (контролы шли блоками-в-столбик → теперь в строку как Edge), radio-точка стала кругом, <option> не утекает текстом, color-swatch показывает value, value-текст инпутов рисуется, placeholder серым у пустых полей, checkbox белая галочка + radio белая точка-в-центре (BUG-187 закрыт 4.78% → 3.02%); остаток = чисто font-parity лейблов/value (Inter vs Edge UI-шрифт) + вертикальный сдвиг line-height → класс BUG-128
    '39': ('BUG-277', 12.66),   # conic-gradient: femtovg-baseline была 0.48% (BUG-085 FIXED — история сохранена; '40' остаётся PASS, не тронут). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 12.66% (совпадает с BUG-277 рядом «39»)
    '51': ('BUG-124', 4.06),    # scrollbar rendering: float-wrapper shrink-to-fit фикснут BUG-178 (9.91% → 1.09%); остаток = дробные layout Y-координаты vs пиксельное округление Edge. Paint-time снэппинг ЭМПИРИЧЕСКИ исключён (2026-06-25, 3 раунда: снэп fill/border/clip в femtovg → 1.09→1.17→1.13, не помогло): бокс у Edge стоит на 1px ВЫШЕ (бордер 195 vs 196), т.е. расхождение позиции в layout, не AA-кайма от paint. Корень = PS-1 (единая политика pixel-snapping в layout, домен P1). DS-9 (2026-07-23, P1): ратчет 1.09%→4.06% — тулбар удвоил высоту хрома (CHROME_H=72), TEST-00/03 калибруются на 0.00%; на неизменённом main этот же тест уже дрейфовал 1.09%→2.52% независимо от DS-9 (PS-1 сам по себе нестабилен), DS-9 добавляет к этому дрейфу свой сдвиг sub-pixel snapping — не новый класс дефекта
    '45': ('BUG-277', 5.81),    # multiple-backgrounds: femtovg-baseline была 1.02% (font-parity + 1px y=0 row, класс BUG-128 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным — wgpu рисует layered backgrounds иначе, ратчет к живому wgpu-числу 5.81% (совпадает с BUG-277 рядом «45»)
    '64': ('BUG-128', 8.99),    # table: margin-collapse таблица↔блок фикснут BUG-193 (13.89% → 8.99%); остаток = font-parity (текст в ~21 ячейках + заголовки, Inter vs Edge) + ~3px накопленный line-height сдвиг
    '18': ('BUG-219', 2.11),    # <img>: «image bottom gap» (baseline descent) фикснут BUG-180 (21.21% → 2.11%); остаток = image-resampling AA (area-avg ≠ Edge downscale kernel). BUG-219 FIXED(DEBTOR) 2026-07-04: Lanczos-3 и Mitchell-bicubic прогнаны, ни один не выигрывает равномерно (Mitchell лучше на 18, хуже на 19) → area-avg сохранён, остаток inherent (rule 2/3)
    '19': ('BUG-219', 9.05),    # object-fit: геометрия всех 5 режимов + object-position верна (BUG-181: средние RGB совпадают, лучший сдвиг 0,0, letterbox корректен) — остаток = image-resampling AA на высокочастотном контенте (perceptron-диаграмма + agi rusty-текстура, area-avg ≠ Edge downscale kernel). BUG-219 FIXED(DEBTOR) 2026-07-04 (см. TEST-18): смена kernel отклонена, baseline не тронут
    '83': ('BUG-277', 11.91),    # scroll-behavior: femtovg-baseline была 7.88% (font-parity + overlay scrollbar, класс BUG-128 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 11.91% (совпадает с BUG-277 рядом «83»)
    '92': ('BUG-124', 0.90),    # system colors: значения system_color() приведены к Edge BUG-210 (15.59% → 0.90%); layout/цвета идеальны (dump-layout: 164px border-box, gap 4, hex точны), остаток = gdigrab суб-пиксельный сдвиг (~+3px на 1000px) на границах ячеек vs пиксельное округление Edge
    '67': ('BUG-128', 1.36),    # attr(): ::before на flex-контейнере не генерировался — фикснут BUG-196 (16.41% → 1.36%); тёмные label-боксы и бары совпадают с Edge пиксель-в-пиксель, остаток = font-parity (white monospace label text, Inter vs Edge) + sub-pixel edge-AA по border-radius клипу
    '62': ('BUG-277', 25.78),    # scroll-snap: femtovg-baseline была 8.85% (font-parity + border-radius AA, класс BUG-128/176 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 16.07% (совпадает с BUG-277 рядом «62»). DS-9 (2026-07-23, P1): ратчет 16.07%→25.78% — тулбар удвоил высоту хрома (CHROME_H=72), TEST-00/03 калибруются на 0.00%, сдвиг каскадно смещает sub-pixel snapping всех snap-остановок контейнера (класс BUG-124/PS-1), не новый дефект
    '77': ('BUG-126', 12.94),   # anchor-positioning: corner/edge placement фикснут BUG-126 (53.45% → 12.94%); 3×3 сетка container 1 совпадает с Edge(position-area) пиксель-в-пиксель (diff). Остаток = (1) тест использует устаревшее имя `inset-area`, которое текущий Edge игнорирует (Edge поддерживает только `position-area`); (2) span-ряд container 2 — Lumen по спеку растягивает auto-width элементы на position-area band, Edge не отрисовывает span-* вовсе. Lumen спек-корректнее Edge; расхождение в reference-браузере, не дефект движка
    '80': ('BUG-128', 9.91),    # border-collapse: collapse varied-width-border erasure фикснут BUG-200 (тонкая ячейка затирала толстую общую границу — границы ряда 3 теперь совпадают с Edge пиксель-в-пиксель); остаток = font-parity вертикальный дрейф (line-height «normal» Inter ≈1.2 vs Edge ≈1.06 → ячейки на ~2px выше, накапливается вниз по 4 таблицам)
    '78': ('BUG-127', 4.64),   # scroll-driven animations: feature wired (F2-2) — animation-timeline: scroll()/view()/named драйвит прогресс анимации от позиции скролла, а не от часов (shell/animation_scheduler.rs). 12.02%→10.07%→9.54%: scroll()/named-боксы садятся в from-state как у Edge, view()-бокс едет по view-прогрессу (позиция совпадает); с D2-2 (BUG-231 FIXED) фон view-бокса композится (интерполированный оранжевый вместо teal). Остаток = font-parity текста (заголовок + метки + 5-строчный .info моноблок, Inter vs Edge, класс BUG-128, доминирует) — закроется только с FP-1. --ipc 2026-06-26: 10.07→5.76. Ратчет 2026-07-04 (ревизия P3): 5.76→4.64 — стабильно на двух прогонах (full-run 2026-07-01 = 4.64, свежая сборка main 2026-07-04 gdigrab = 4.62), diff = чистый font-parity, from-state рамки совпадают
    '71': ('BUG-199', 4.53),    # Ратчет 7.03→4.53 (P3 2026-07-16, wgpu-дефолт full-run — независимо от BUG-277, чистое улучшение). @starting-style: Lumen рендерит settled-состояние (обе коробки 200×200, opacity 1 — спек-корректно после завершения 0.4s entry-перехода). Edge headless --screenshot (без virtual-time) ловит entry-transition в полёте: transform у @starting-style START-значения (box-a scale(0.5)→107px, box-b translateX(-80px)), но opacity у END-значения (1, full saturation) — взаимно несогласованный кадр для синхронных 0.4s переходов = артефакт тайминга захвата Edge, а не дефект движка. Совпасть = (1) проводка @starting-style в каскад (домен P4) + (2) полный engine entry-переходов + (3) воспроизвести невозможный кадр. Display-list геометрия/цвета settled-состояния идеальны (--dump-display-list). Тот же класс, что TEST-77/BUG-126
    '70': ('BUG-176', 1.63),    # object-fit SVG: inline <svg> игнорирует CSS object-fit/-position — viewBox маппится через preserveAspectRatio (BUG-198, 7.82% → 1.63%); все 5 режимов + эллипсы теперь совпадают по геометрии/заливке с Edge (раньше object-fit растягивал/кропал viewBox, эллипсы рисовались «стадионами»). Остаток = kappa-безье эллиптических дуг SVG <ellipse>/<circle> vs точная дуга Edge + sub-pixel AA по периметру/внутреннему rect (тот же корень, что BUG-176)
    '52': ('BUG-128', 4.25),    # text-shadow blur: blur-пайплайн корректен (BUG-191) — sigma=radius/2, GPU GaussianBlur на full-RT слое (halo не клипуется), multi-shadow и цветные glow совпадают с Edge по extent/intensity (glow-only и 20px кейсы проверены пиксельно). Остаток = font-parity: Edge рендерит serif 80px, Lumen — Inter sans; в diff доминируют два несовмещённых начертания глифов «A»/«B» (cyan/white ghosts), сами тени near-black = совпадают (rule 3)
    '84': ('BUG-128', 6.02),    # text-decoration-skip-ink: gap-геометрия фикснута BUG-203 (8.20% → 6.02%) — раньше gap занимал всю ячейку глифа + margin, поэтому ряды последовательных descender'ов («gjpqy») сливались в один огромный gap, стирая линию целиком (skip-ink:all не рисовал НИЧЕГО). Теперь gap клирит только центральную ink-зону ячейки (≈56%), линия видна сегментами между глифами как в Edge (все 4 ряда подчёркиваний совпадают). Остаток = font-parity: Edge рендерит Times serif, Lumen — Inter sans, глифы 48px расходятся по всей странице (rule 3)
    '46': ('BUG-128', 1.96),    # individual transforms: translate/rotate/scale/scale-xy + combined все совпадают с Edge пиксель-в-пиксель (BUG-188: centroid+bbox идентичны на t-translate/t-rotate/t-scale-uniform/t-scale-xy/t-all-three/t-translate-only-x). Композиция individual+transform спек-корректна (translate→rotate→scale→transform вокруг shared pivot, регресс-тест в lib.rs). Остаток = font-parity: 8 monospace-меток (Inter fallback vs Edge monospace) рисуются с другой шириной → (1) прямой diff текста + (2) косвенный сдвиг teal-бокса `individual + transform` на ~18px вправо/влево, т.к. он стоит после метки в flex-ряду и его X зависит от ширины метки (форма/scale/rotation бокса корректны, n=5064 vs 5076). Класс BUG-128
    '93': ('BUG-225', 3.54),    # field-sizing: content — контент-боксы были невидимы, т.к. appearance:none стрипал авторский border/background ПОСЛЕ каскада (BUG-211 4.11%→3.54%); стрип перенесён ПЕРЕД каскадом, авторские border/bg/padding теперь побеждают. Остаток = value-текст inputs не рисуется при appearance:none (BUG-225, emit_form_control_indicator ранний return) + font-parity textarea/labels (Inter vs Edge monospace, класс BUG-128)
    '32': ('BUG-128', 3.75),    # list ::marker: две геометрии фикснуты BUG-185 — (1) `::marker { content: "→ " }` на list-style-type:disc рисовал ДИСК вместо строки (painter приоритетил bullet-форму над непустым text; теперь непустой text всегда побеждает форму, как для counter-глифов); (2) широкий маркер (длинный @counter-style prefix/suffix «#1: » шире дефолтного em*1.5 бокса) переполнялся в первое слово контента («#1:One»), теперь бокс растёт влево и строка right-align'ится у контент-края («#1: One» как Edge). Зелёные стрелки content-marker и numeric-prefix «#1: One» совпадают с Edge по геометрии. Остаток = font-parity (Edge serif vs Inter sans по ВСЕЙ странице — ~50 строк списков/меток, rule 3) + list-style-image data-URI рисует disc вместо картинки (отдельная CSS-проводка). Класс BUG-128
    '47': ('BUG-176', 1.20),    # SVG basic shapes. BUG-189 (3.71% → 2.27%): <line> штрихуется как толстый сегмент. BUG-226 (2.27% → 1.20%): stroke на rect/circle/ellipse теперь центрирован на кромке (½ наружу + ½ внутрь, SVG 2 §13.7) — bbox надувается на stroke-width/2, внешние радиусы = r+w/2, even-odd-кольцо даёт inner = r-w/2 (центрлайн = r). Остаток 1.20% = stroke-edge/rounded-corner AA + кубическая kappa-аппроксимация эллиптических дуг vs точная дуга Edge (класс BUG-176)
    '95': ('BUG-128', 3.0),     # font-size-adjust: масштабирование used-размера спек-корректно (BUG-212) — rows a1–a4 (0.60/0.45/0.30/0.20) дают одинаковый x-height = size·z (Inter sxHeight через OS/2), прогрессия глифов совпадает с Edge. Реальный дефект был не в шрифте: `line-height:100px` хранится как ratio (×font-size), и apply_font_size_adjust, меняя used-font-size пост-каскадно, схлопывал фикс-line-box (row 0.20: 100px → 36.6px), текст уезжал вверх (baseline-сдвиг до 32px). Фикс: line_height_is_relative помечает absolute `<length>`/`<percentage>` line-height, ratio корректируется обратно (CSS2 §10.8.1) → line-box остаётся 100px (CPU-diff 3.27% → 2.86%, baseline-сдвиг 32px → 0.5px). Остаток = font-parity: глифы «xoxoxoxo»/метки рисуются Inter sans vs Edge sans, ширина/начертание расходятся (rule 3) + row none x-height естественно отличается без нормализации. Класс BUG-128
    '76': ('BUG-277', 0.96),    # motion path: femtovg-baseline была 0.64% (тонкий diag-трек `linear-gradient(... calc(50% ± 2px) ...)` рисовался корректно, BUG-230 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, диагностика P3 нашла реальный root-cause в `WgpuBackend`/`renderer.rs` (не в общей `gradient_math.rs`), ратчет 20.15%. **BUG-277 срез 1 (2026-07-21, P1):** root-cause найден — `renderer.rs`'s `DrawLinearGradient`/`DrawRadialGradient`/`PushMaskLinearGradient`/`PushMaskRadialGradient` резолвили `Px`/`Calc`-позиции стопов через `resolve_gradient_stops(stops, 1.0)` (захардкоженный line_len=1.0) вместо реальной длины градиентной линии в CSS px, как это делает `cpu_raster.rs`/femtovg. На `calc(50% ± 2px)`-хард-стопах TEST-76 это сдвигало/растягивало полосу за пределы box — 20.15% на всю диагональ. Фикс: `linear_gradient_uv_endpoints` теперь возвращает `line_len` (box-diagonal formula, идентична CPU); radial использует `radius_x.max(radius_y)`; mask-варианты — те же формулы, что `cpu_raster::render_mask`. diff-картинка подтверждает: остаток — только AA-кромка вдоль путей + edge box-обводок, тот же класс, что исходный femtovg-baseline 0.64% (rule 2/3). Не закрывает BUG-277 целиком — TEST-101/104/59/etc не грaдиентные/не Px-стопы, отдельные причины
    '82': ('BUG-128', 2.38),    # BUG-261 (REGRESS 5.66% от 2026-06-29) был ложным: замер на стало́м бинаре от 15:45, до влития BUG-262 в 20:24 (run.py без --build не пересобирает lumen.exe). На свежем HEAD gdigrab = 2.31% ≤ baseline. SVG <use> clone-функционал фикснут BUG-201 (5.00% → 2.38%). Три дефекта: (1) <polygon>/<polyline> не имели ветки рендера (звёзды-symbol не рисовались вовсе); (2) HTML5-парсер не самозакрывает <use/> → соседние <use> вкладывались друг в друга как DOM-дети, рендерился только первый клон (теперь use/polygon сканируют mis-nested siblings, как rect/circle); (3) element-transform применялся к замоканным doc-координатам, scale(0.75) масштабировал origin вьюпорта → масштабированные клоны ряда 3 уезжали с y≈347 на y≈260 (теперь трансформ применяется в user-space, потом маппинг user→doc). Все клоны/группы/symbol/polygon/nested-chain + x/y + scale совпадают с Edge по позиции/размеру/заливке (diff). Остаток = font-parity (метки .label sans 11px, Inter vs Edge, rule 3) + sub-pixel edge-AA по периметрам фигур (rule 2)
    '58': ('BUG-100', 2.47),    # ::first-letter / ::first-line РЕАЛИЗОВАНЫ (дрейф трекера, прогон 2026-06-23): drop-cap «O» флоатится (extract_first_letter_float + 7 тестов), first-line зелёная+жирная. Diff-картинка подтверждает: фича работает, остаток = font-parity тела абзаца (Inter vs Edge sans → разные метрики → перенос строк сдвигается, ghosting глифов) + edge-AA большого 48px drop-cap-глифа (rule 3). --ipc 2026-06-26: 4.92→2.47
    '59': ('BUG-277', 23.65),   # image-set() / cross-fade() РЕАЛИЗОВАНЫ, femtovg-baseline была 17.15% (грамматика приведена к CSS Images L4 §4, класс BUG-101 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 23.65% (совпадает с BUG-277 рядом «59»)
    '79': ('BUG-128', 6.76),    # text-underline-offset / text-decoration: геометрия линий корректна; остаток = font-parity (serif vs sans, Inter vs Edge) по всему тексту страницы (rule 3). BUGS.md давно помечал кандидатом в KNOWN_DEBTORS
    '110': ('BUG-214', 1.70),   # accent-color РЕАЛИЗОВАН (дрейф трекера, прогон 2026-06-23): emit_form_control_accents тинтит checkbox/radio/range/progress + юнит-тесты. Diff-картинка подтверждает: цвета-акценты применяются верно (cyan/magenta/green), остаток = расхождение нативной отрисовки form-виджетов (толщина трека слайдера, форма thumb, стиль progress-бара) vs UA-виджеты Edge — присущее кросс-браузерное расхождение (сам тест-HTML отмечает «native control sizes kept → divergence stays small»)
    '120': ('BUG-217', 3.26),   # prefers-contrast/prefers-reduced-data РЕАЛИЗОВАНЫ и спек-корректны (ревизия 2026-06-23): парсинг+матчинг в parser.rs (MediaFeature::PrefersContrast/PrefersReducedData, +unit-тесты media_query_prefers_*). Diff-картинка подтверждает: swatch .a (prefers-contrast: no-preference) зелёный в обоих движках; единственное расхождение = swatch .b (prefers-reduced-data: no-preference) — Lumen матчит (зелёный, как требует комментарий теста «correct engine → both green»), а Edge НЕ поддерживает prefers-reduced-data → query не матчится → .b остаётся красным. Lumen корректнее reference-браузера (тот же класс, что BUG-126/TEST-77 inset-area, BUG-199/TEST-71 @starting-style, BUG-237/TEST-122 line-height-step). Совпасть = отключить рабочую media-feature (запрещено rule 4)
    '113': ('BUG-277', 6.10),   # shape-outside path(): femtovg-baseline была 1.41% (AA вдоль диагональной кромки + font-parity, класс BUG-215 — история сохранена). BUG-287/BUG-277 (2026-07-16): P1-wgpu-flip сделал wgpu дефолтным, ратчет к живому wgpu-числу 6.10% (совпадает с BUG-277 рядом «113»)
    '126': ('BUG-126', 3.26),  # media inverted-colors: 3.26% FAIL на текущем main и на a7c348fa (до font-parity fix) — регрессия не от BUG-128; Edge headless ловит кадр без инверсии, Lumen рендерит инверсию через apply_inverted_colors — цветовое расхождение coverage area page, не дефект движка
    '97': ('BUG-128', 2.78),    # counter-set: порядок reset→increment→set спек-корректен (counters.rs apply_reset/apply_increment/apply_set + регресс-тест counter_set_test97_sibling_rows). Все пять значений счётчика совпадают с Edge: 5 (set на reset-0), 6 (inc), 0 (inc затем set — set перекрывает inc), 1 (inc от 0), 42 (set). Diff-картинка подтверждает: цветные боксы строк и границы совпадают пиксель-в-пиксель, весь остаток = font-parity ::before-меток counter(c) + текста строк (Inter sans vs Edge sans → разная ширина «inc+set» сдвигает глифы по X, ghosting), rule 3. Класс BUG-128
    '66': ('BUG-128', 1.07),    # ::selection: правила парсятся, но в тесте не выделяется текст — фича не видна без user-selection. Свотчи (sw1 #0078D4 / sw2 #e74c3c / sw3 #1a6ead) показывают цвета выделения и совпадают с Edge по цвету и X-позиции (41–220) пиксель-в-пиксель. Декомпозиция diff (BUG-195): 93% пикселей — текст (62% метки-лейблы «Default ::selection background…», 31% центрированный белый текст «Default»/«Highlight»/«Custom» внутри свотчей), Inter sans vs Edge sans (rule 3). Остаток = ~1–2px накопленный вертикальный line-height-дрейф верх/низ свотчей (Inter «normal» ≈1.2 vs Edge) → свотчи на 1–2px ниже к концу страницы. Реального дефекта движка нет. Класс BUG-128
    '117': ('BUG-216', 2.23),   # quotes / open-quote / close-quote: реальный дефект исправлен (BUG-216) — generated-content ::before/::after и текст склеивались через лишний inter-word пробел (`“ auto quotes ”`), теперь кавычки примыкают к тексту вплотную как в Edge (`“auto quotes”`). Корень: wrap_inline_run/one_line_fallback вставляли пробел на ЛЮБОЙ границе сегментов; теперь пробел только когда границу разделял collapsible whitespace (CSS Text L3 §4.1.1) — заодно чинит `<span>a</span><span>b</span>`→«ab» и `<em>x</em>!`→«x!». Диф-картинка: позиция кавычек верна, остаток = чистый font-parity (Edge рисует тело serif, Lumen — Inter sans → каждый глиф расходится по ширине/начертанию, построчный ghosting), rule 3. Класс BUG-128. CPU --ipc: 2.23%
    '142': ('BUG-282', 10.16),  # @color-profile + color(--name c1 c2 c3): Lumen рендерит спек-корректные swatch-цвета (каналы трактуются как sRGB, реальная ICC-трансформация отложена) — скриншот показывает ровно задуманные red/green/blue/grey/black/alpha-red. Edge не поддерживает @color-profile/custom-ident в color() вовсе → весь color() невалиден → фон элементов остаётся прозрачным (пустой белый эталон). Diff-регион (x:5-960 y:5-168) — ровно область swatch-ов, никакой другой геометрии не задето. Тот же класс, что TEST-71/BUG-199 и TEST-77/BUG-126
    '145': ('BUG-290', 3.68),   # writing-mode vertical-rl/vertical-lr × text-orientation mixed/upright/sideways: реальный дефект (BUG-289, vertical InlineRun текст никогда не рисовался на реальном DOM — emit_inline_run игнорировал writing_mode) найден и исправлен в этом же прогоне; mixed/sideways рендерят повёрнутый/upright-CJK текст корректно. Остаток 3.68% = (1) font-parity Inter vs Edge sans (rule 3, класс BUG-100/TEST-58); (2) text-orientation:upright использует пословный, не per-glyph, вертикальный аванс (Edge раскладывает каждый символ индивидуально) — сознательно отложенный пробел, см. docs/tasks/ph3-writing-mode-vertical.md. mixed/upright/sideways визуально различимы (DoD задачи выполнен)
}
_DEBTOR_TOL = 2.0  # % допуск run-to-run вариации gdigrab


def check_debtor(tid: str, pct: float) -> tuple[str | None, str]:
    """Проверяет тест против KNOWN_DEBTORS.

    Возвращает (verdict, message):
      None     — не должник, обычная логика
      'OK'     — должник в норме (не останавливает пайплайн)
      'REGRESS'— должник превысил baseline (FAIL)
      'RATCHET'— должник ниже baseline (нужно обновить запись)
      'REMOVE' — достиг 0.5% (нужно удалить запись)
    """
    if tid not in KNOWN_DEBTORS:
        return None, ''
    bug, baseline = KNOWN_DEBTORS[tid]
    if pct <= 0.5:
        return 'REMOVE', f'  → KNOWN_DEBTORS: цель 0.5% достигнута! Удали запись «{tid}» ({bug}).'
    if pct < baseline - _DEBTOR_TOL:
        return 'RATCHET', (f'  → KNOWN_DEBTORS: прогресс! Снизь baseline {baseline:.2f}% → {pct:.2f}%'
                           f' для «{tid}» ({bug}) в KNOWN_DEBTORS.')
    if pct > baseline + _DEBTOR_TOL:
        return 'REGRESS', (f'  → KNOWN_DEBTORS РЕГРЕССИЯ: {pct:.2f}% > baseline {baseline:.2f}%'
                           f' + {_DEBTOR_TOL}% допуск для «{tid}» ({bug}).')
    return 'OK', f'  ⚠ KNOWN DEBTOR {bug}: {pct:.2f}% (baseline {baseline:.2f}%)'


# --- Interaction-слой: зависимости и локализация ---

# DEPS: interaction-id → юнит-тесты, свойства которых он комбинирует.
# Используется выводом при FAIL и режимом --bisect: если юнит-зависимости зелёные,
# а interaction-тест красный — баг во взаимодействии свойств, не в самом свойстве.
DEPS: dict[str, list[str]] = {
    '100': ['22', '14'],
    '101': ['36', '14'],
    '102': ['13', '38'],
    '103': ['30', '22'],
    '104': ['26', '39', '40', '36'],
    '105': ['37', '09'],
    '106': ['22', '38'],
    '107': ['15', '36', '14'],
    '108': ['22'],
    '109': ['31', '22', '36'],
}

# Все interaction-тесты используют общую сетку из 6 ячеек 300×300.
# Координаты — в системе кропнутого вьюпорта 1024×720 (магента-рамка = 1px по краям):
# ячейка cN в .__f имеет left/top из файла + 1px рамки.
_CELL_GRID: list[tuple[str, tuple[int, int, int, int]]] = [
    ('c0', (25, 25, 325, 325)),
    ('c1', (365, 25, 665, 325)),
    ('c2', (705, 25, 1005, 325)),
    ('c3', (25, 375, 325, 675)),
    ('c4', (365, 375, 665, 675)),
    ('c5', (705, 375, 1005, 675)),
]

# REGIONS: interaction-id → подписи сценариев в ячейках c0..c5 (порядок = _CELL_GRID).
# При FAIL diff_region пересекается с ячейками → видно, какой сценарий разошёлся.
REGIONS: dict[str, list[str]] = {
    '100': ['translate clipped', 'rotate corners clipped', 'scale clipped',
            'CONTROL no clip', 'negative translate clipped', 'rotated clip container'],
    '101': ['rounded clip of bar', 'circle clip', 'CONTROL no clip',
            'nested rounded clips', 'radius+border clip', 'pill clip'],
    '102': ['z-index trapped by opacity', 'group opacity (no double-darken)',
            'CONTROL per-child opacity', 'negative z inside opacity',
            'nested opacity 0.6*0.5', 'reference opacity 0.3'],
    '103': ['grayscale on rotated', 'blur on translated', 'filter inside rotated parent',
            'filter as containing block', 'hue-rotate on scaled', 'CONTROL no filter'],
    '104': ['linear mask + gradient + radius', 'radial mask + radial bg',
            'linear mask + conic bg', 'mask + circle shape',
            'CONTROL no mask', 'mask over border'],
    '105': ['two left floats + margins', 'left+right float + middle block',
            'clear:both + margin-top', 'float wrap to next line',
            'tall float vs in-flow bg', 'CONTROL plain blocks'],
    '106': ['negative z in transformed parent', 'transformed (z:0) vs z:1 sibling',
            'z:2 over transformed z:1', 'z children in rotated parent',
            'z children in scaled parent', 'CONTROL no transform'],
    '107': ['rounded shadow', 'spread shadow on circle', 'shadow clipped by parent',
            'CONTROL shadow escapes', 'two hard shadows + radius', 'blur shadow + radius + border'],
    '108': ['rotate∘rotate⁻¹ = identity', 'REFERENCE axis-aligned', 'scale scales child translate',
            'translate then rotate', '3× rotate(10deg)', 'REFERENCE rotate(30deg)'],
    '109': ['circle clip on rotated', 'inset clip on scaled', 'triangle clip on translated',
            'clipped parent, transformed child', 'clip-path ∩ border-radius', 'CONTROL no clip'],
}


def affected_objects(tid: str, region: dict | None) -> list[str]:
    """Пересекает diff_region с сеткой ячеек interaction-теста.

    Возвращает подписи сценариев, чьи ячейки пересекаются с bounding box диффа.
    Для тестов вне interaction-слоя (нет в REGIONS) — пустой список.
    """
    labels = REGIONS.get(tid)
    if not labels or not region:
        return []
    out: list[str] = []
    for (cell, (x0, y0, x1, y1)), label in zip(_CELL_GRID, labels):
        if (region['left'] <= x1 and region['right'] >= x0
                and region['top'] <= y1 and region['bottom'] >= y0):
            out.append(f'{cell}: {label}')
    return out

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

# --- IPC client (TAB-7): lumen.exe --ipc-server bincode protocol ---
#
# Заменяет gdigrab-захват детерминированным CPU-снимком по IPC. Шелл,
# запущенный с `--ipc-server`, печатает `LUMEN_IPC_PORT=<port>` в stdout и
# становится TCP-сервером таб-команд. Кадр кодируется тем же CPU-пайплайном,
# что `--screenshot` — окно/wgpu/ffmpeg-grab не нужны, результат воспроизводим.
#
# Протокол (см. crates/ipc/src/lib.rs): каждое сообщение —
#   [u32 LE body_len][body], body — bincode payload:
#     enum variant tag = u32 LE; длины String/Vec = u64 LE; u32-поля = 4 байта LE.
#
# Индексы вариантов в объявлении enum (важно — соответствуют lumen_ipc):
#   IpcRequest:  0 Fetch · 1 Ping · 2 Shutdown · 3 CreateTab · 4 CloseTab ·
#                5 NavigateTab · 6 Screenshot
#   IpcResponse: 0 FetchOk · 1 FetchErr · 2 Pong · 3 Shutdown · 4 TabCreated ·
#                5 TabClosed · 6 Navigated · 7 Screenshot · 8 TabError

_REQ_SHUTDOWN     = 2
_REQ_CREATE_TAB   = 3
_REQ_CLOSE_TAB    = 4
_REQ_NAVIGATE_TAB = 5
_REQ_SCREENSHOT   = 6

_RESP_SHUTDOWN    = 3
_RESP_TAB_CREATED = 4
_RESP_TAB_CLOSED  = 5
_RESP_NAVIGATED   = 6
_RESP_SCREENSHOT  = 7
_RESP_TAB_ERROR   = 8


class IpcError(Exception):
    """Сбой IPC-обмена с lumen --ipc-server (протокол, соединение или TabError)."""


def _u32(v: int) -> bytes:
    return struct.pack('<I', v)


def _bstr(s: str) -> bytes:
    """bincode String/Vec<u8>: u64 LE длина + UTF-8 байты."""
    b = s.encode('utf-8')
    return struct.pack('<Q', len(b)) + b


class _Cursor:
    """Курсор для декодирования bincode-тела ответа."""

    def __init__(self, data: bytes) -> None:
        self.d = data
        self.p = 0

    def _take(self, n: int) -> bytes:
        b = self.d[self.p:self.p + n]
        if len(b) != n:
            raise IpcError('truncated IPC message body')
        self.p += n
        return b

    def u32(self) -> int:
        return struct.unpack('<I', self._take(4))[0]

    def vec(self) -> bytes:
        n = struct.unpack('<Q', self._take(8))[0]
        return self._take(n)

    def string(self) -> str:
        return self.vec().decode('utf-8', 'replace')


class LumenIpcClient:
    """Клиент к `lumen.exe --ipc-server` (TAB-7).

    Спавнит сервер, читает порт из stdout-строки `LUMEN_IPC_PORT=<port>`,
    подключается по TCP loopback и шлёт length-prefixed bincode таб-команды.
    Один фоновый поток дренирует остаток stdout, чтобы рендер-логи шелла не
    забили пайп и не заблокировали процесс (как в crates/shell/tests/ipc_server.rs).
    """

    def __init__(self, lumen_path: str, cwd: str) -> None:
        self.proc = subprocess.Popen(
            [lumen_path, '--ipc-server'], cwd=cwd,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        port: int | None = None
        assert self.proc.stdout is not None
        for _ in range(400):
            line = self.proc.stdout.readline()
            if not line:
                break
            line = line.strip()
            if line.startswith('LUMEN_IPC_PORT='):
                try:
                    port = int(line.split('=', 1)[1])
                except ValueError:
                    port = None
                break
        if port is None:
            self.proc.kill()
            raise IpcError('lumen --ipc-server не напечатал LUMEN_IPC_PORT')
        # Дренируем остаток stdout, иначе рендер-логи переполнят пайп → блок шелла.
        threading.Thread(target=self._drain_stdout, daemon=True).start()
        self.sock = socket.create_connection(('127.0.0.1', port), timeout=30)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def _drain_stdout(self) -> None:
        out = self.proc.stdout
        if out is None:
            return
        try:
            for _ in out:
                pass
        except Exception:
            pass

    def _send(self, payload: bytes) -> None:
        self.sock.sendall(_u32(len(payload)) + payload)

    def _read_exact(self, n: int) -> bytes:
        buf = bytearray()
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise IpcError('IPC соединение закрыто сервером')
            buf += chunk
        return bytes(buf)

    def _recv(self) -> _Cursor:
        body_len = struct.unpack('<I', self._read_exact(4))[0]
        return _Cursor(self._read_exact(body_len))

    def create_tab(self) -> int:
        """CreateTab → id новой headless-вкладки."""
        self._send(_u32(_REQ_CREATE_TAB))
        c = self._recv()
        tag = c.u32()
        if tag != _RESP_TAB_CREATED:
            raise IpcError(f'ожидался TabCreated, получен вариант {tag}')
        return c.u32()

    def navigate(self, tab_id: int, url: str) -> None:
        """NavigateTab(url) — load + parse + layout вкладки."""
        self._send(_u32(_REQ_NAVIGATE_TAB) + _u32(tab_id) + _bstr(url))
        c = self._recv()
        tag = c.u32()
        if tag == _RESP_NAVIGATED:
            return
        if tag == _RESP_TAB_ERROR:
            c.u32()
            raise IpcError(f'NavigateTab: {c.string()}')
        raise IpcError(f'ожидался Navigated, получен вариант {tag}')

    def screenshot(self, tab_id: int) -> bytes:
        """Screenshot → PNG-байты CPU-рендера вкладки."""
        self._send(_u32(_REQ_SCREENSHOT) + _u32(tab_id))
        c = self._recv()
        tag = c.u32()
        if tag == _RESP_SCREENSHOT:
            c.u32()
            return c.vec()
        if tag == _RESP_TAB_ERROR:
            c.u32()
            raise IpcError(f'Screenshot: {c.string()}')
        raise IpcError(f'ожидался Screenshot, получен вариант {tag}')

    def close_tab(self, tab_id: int) -> None:
        """CloseTab — best-effort, ошибки игнорируются."""
        try:
            self._send(_u32(_REQ_CLOSE_TAB) + _u32(tab_id))
            self._recv()
        except (IpcError, OSError):
            pass

    def shutdown(self) -> None:
        """Shutdown сервера + закрытие сокета + ожидание выхода процесса."""
        try:
            self._send(_u32(_REQ_SHUTDOWN))
            self._recv()
        except (IpcError, OSError):
            pass
        try:
            self.sock.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


# --- Live-window client (SDC-3: один процесс lumen на весь прогон) ---
#
# В отличие от --ipc (детерминированный CPU-снимок по TCP, без окна), этот
# режим держит ОДНО настоящее окно lumen открытым на весь прогон и управляет
# им через MCP (`--mcp-live-port`, SDC-2): `tools/call navigate` грузит
# страницу, `tools/call wait{condition:document_ready}` даёт реальный сигнал
# готовности вместо слепого `time.sleep(LUMEN_WAIT_SEC)`. Сам пиксельный
# снимок по-прежнему берётся через gdigrab (реальный femtovg-рендер) — MCP
# `resource://screenshot` рендерит через CPU-путь, который пока не на
# паритете с femtovg по border-radius/градиентам/картинкам (тот же разрыв,
# что и у --ipc, см. докстринг модуля).

# document_ready проверяет только `layout_box.is_some()` — сам GPU-рендер
# (femtovg/wgpu present) и композитинг ОС идут отдельным циклом и не
# гарантированно успевают до возврата wait(); эмпирически 0.5с было мало
# (снимок ловил ещё не отрисованное белое окно), 2.0с — надёжно. Всё ещё
# намного меньше LUMEN_WAIT_SEC=5 на тест из старого режима.
LIVE_SETTLE_SEC = 1.5


class McpLiveError(Exception):
    """Сбой MCP-обмена с lumen --mcp-live-port (протокол, соединение или ошибка инструмента)."""


class LiveWindowClient:
    """Клиент к `lumen.exe --mcp-live-port N <url>` — одно живое окно на весь прогон (SDC-3).

    Спавнит окно один раз, подключается по TCP loopback и говорит line-delimited
    JSON-RPC (MCP, см. `crates/mcp/src/protocol.rs`): один JSON-объект на строку,
    ответ — тоже одна строка. `navigate`/`wait` — единственные нужные здесь
    инструменты; `LiveWindowSession` (SDC-2) исполняет их против настоящего окна.
    """

    def __init__(self, lumen_path: str, cwd: str, port: int) -> None:
        # DEVX-1: `--deterministic` freezes Date.now()/Math.random()/rAF timestamps
        # (kills flake in JS-driven tests, e.g. TEST-57/129-138) but on its own
        # forces a 1280x800 window; `--viewport` (added alongside) overrides that
        # back to the pipeline's calibrated 1024x720 so TEST-00's magenta-marker
        # crop offset stays valid for the rest of the --live run.
        self.proc = subprocess.Popen(
            [lumen_path, '--mcp-live-port', str(port), '--no-scrollbar',
             '--deterministic', '--viewport', f'{VIEWPORT_W}x{VIEWPORT_H}',
             'about:blank'],
            cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        self.sock = self._connect_with_retry(port)
        self._reader = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._next_id = 1

    def _connect_with_retry(self, port: int, attempts: int = 200, delay: float = 0.1) -> socket.socket:
        """Poll for the MCP TCP listener — window/GPU startup takes longer than
        a single connect attempt. `attempts * delay` = 20s ceiling.

        The 5s timeout is for the *connect* attempt only — reset to a generous
        60s afterward, since `_call` reuses this same socket for `wait` requests
        whose Rust-side round trip can legitimately take up to `timeout_ms + 2s`
        (see `AutomationHandle::execute` in `crates/driver/src/live_session.rs`).
        """
        last_err: Exception | None = None
        for _ in range(attempts):
            try:
                s = socket.create_connection(('127.0.0.1', port), timeout=5)
                s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                s.settimeout(60)
                return s
            except OSError as e:
                last_err = e
                time.sleep(delay)
        self.proc.kill()
        raise McpLiveError(f'lumen --mcp-live-port {port} не поднялся за отведённое время: {last_err}')

    def _call(self, method: str, params: dict) -> dict:
        req_id = self._next_id
        self._next_id += 1
        request = json.dumps({'jsonrpc': '2.0', 'id': req_id, 'method': method, 'params': params})
        self.sock.sendall((request + '\n').encode('utf-8'))
        line = self._reader.readline()
        if not line:
            raise McpLiveError('MCP-соединение закрыто сервером (окно упало?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise McpLiveError(f'{method}: {resp["error"]}')
        return resp.get('result') or {}

    def navigate(self, url: str) -> None:
        """`navigate` — грузит страницу в живом окне; блокируется до Ack от шелла."""
        self._call('tools/call', {'name': 'navigate', 'arguments': {'url': url}})

    def wait_document_ready(self, timeout_ms: int = 10_000) -> None:
        """`wait{condition:document_ready}` — реальный сигнал готовности вместо `sleep`."""
        self._call('tools/call', {
            'name': 'wait',
            'arguments': {'condition': 'document_ready', 'timeout_ms': timeout_ms},
        })

    def read_console(self) -> list[dict]:
        """`resources/read` on `resource://console` (DEVX-1).

        Returns JS `console.log/warn/error` messages captured since the last
        `navigate()` (the live window clears its console buffer on every
        navigation — see `AutomationCommand::Navigate` in `crates/shell/src/main.rs`).
        Each entry is `{"level": "Log"|"Info"|"Warn"|"Error", "message": str}`.
        """
        result = self._call('resources/read', {'uri': 'resource://console'})
        contents = result.get('contents') or []
        if not contents:
            return []
        return json.loads(contents[0].get('text', '[]'))

    def shutdown(self) -> None:
        """Закрыть соединение и корректно (или принудительно) завершить процесс."""
        try:
            self.sock.close()
        except OSError:
            pass
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


def _free_tcp_port() -> int:
    """Выделить свободный локальный порт (bind-and-release; см. `LiveWindowClient`)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()
    return port


def capture_lumen_live(client: LiveWindowClient, test_path: str, out_png: str) -> None:
    """Живое окно (SDC-3): Navigate + wait(document_ready) вместо kill+relaunch
    процесса на каждый тест, затем gdigrab того же desktop-кадра как в
    `capture_lumen`. `test_path` — абсолютный путь (как в `ipc_capture_lumen`).
    """
    url = 'file:///' + os.path.abspath(test_path).replace('\\', '/')
    client.navigate(url)
    client.wait_document_ready()
    time.sleep(LIVE_SETTLE_SEC)
    _bring_pid_to_front(client.proc.pid)
    time.sleep(0.2)  # brief pause for window compositor to repaint
    subprocess.run(
        [FFMPEG, '-f', 'gdigrab', '-i', 'desktop',
         '-vframes', '1', '-update', '1', out_png, '-y'],
        capture_output=True, timeout=15,
    )


# Активный IPC-клиент и вкладка (заполняются в main при --ipc; иначе None).
_IPC_CLIENT: LumenIpcClient | None = None
_IPC_TAB: int = 0

# Активный live-window клиент (заполняется в main при --live; иначе None).
_LIVE_CLIENT: LiveWindowClient | None = None


def ipc_capture_lumen(test_path: str, out_png: str) -> None:
    """IPC-режим (TAB-7): навигация активной вкладки на тест + Screenshot → PNG-файл.

    Заменяет gdigrab-захват: CPU-снимок детерминирован и начинается с (0,0),
    поэтому магента-калибровка/crop offset не нужны.
    """
    assert _IPC_CLIENT is not None
    abs_path = os.path.abspath(test_path)
    _IPC_CLIENT.navigate(_IPC_TAB, abs_path)
    png = _IPC_CLIENT.screenshot(_IPC_TAB)
    with open(out_png, 'wb') as f:
        f.write(png)


# --- Window management ---

def _bring_pid_to_front(pid: int) -> None:
    """Bring the main visible window of the given PID to the foreground (Windows)."""
    user32 = ctypes.windll.user32
    # Use c_size_t (pointer-sized) for HWND to avoid overflow on 64-bit Windows
    EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_size_t, ctypes.c_size_t)
    found: list[int] = []

    def _cb(hwnd: int, _: ctypes.c_size_t) -> bool:
        proc_id = ctypes.wintypes.DWORD(0)
        user32.GetWindowThreadProcessId(ctypes.c_size_t(hwnd), ctypes.byref(proc_id))
        if proc_id.value == pid and user32.IsWindowVisible(ctypes.c_size_t(hwnd)):
            found.append(hwnd)
            return False
        return True

    user32.EnumWindows(EnumProc(_cb), 0)
    if found:
        hwnd = found[0]
        # Alt-key trick to bypass Windows foreground-lock
        ctypes.windll.user32.keybd_event(0x12, 0, 0, 0)  # VK_MENU down
        user32.SetForegroundWindow(ctypes.c_size_t(hwnd))
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

def capture_lumen(html_relpath: str, out_png: str,
                   extra_env: dict[str, str] | None = None) -> None:
    """Запускаем Lumen, ждём LUMEN_WAIT_SEC сек, грабим desktop через ffmpeg, kill-аем.

    extra_env добавляет/переопределяет переменные окружения дочернего процесса
    (DEVX-4: LUMEN_NO_* paint-бисект флаги)."""
    env = {**os.environ, **extra_env} if extra_env else None
    proc = subprocess.Popen([LUMEN, '--no-scrollbar', html_relpath], cwd=REPO,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
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
    """Собирает lumen-shell с нужным профилем. Возвращает True при успехе."""
    profile = LUMEN_PROFILE
    print(f'Сборка lumen-shell --profile {profile}...')
    env = os.environ.copy()
    env['PATH'] = r'C:\Users\konstantin\.cargo\bin' + os.pathsep + env.get('PATH', '')
    cmd = ['cargo', 'build', '-p', 'lumen-shell', '--profile', profile]
    res = subprocess.run(cmd, cwd=REPO, env=env)
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
            no_cache: bool = False,
            extra_env: dict[str, str] | None = None,
            ) -> tuple[bool, tuple[int, int] | None, float, dict | None, list[str]]:
    """Запускает один тест.

    Возвращает (passed, new_crop_offset, diff_pct, diff_region, console_errors).
    diff_pct < 0 и diff_region = None означают ошибку (ERROR). console_errors —
    тексты `console.error` со страницы (DEVX-1, только в --live режиме);
    непустой список FAIL'ит тест независимо от diff_pct (баг, невидимый на скриншоте).
    extra_env применяется только в дефолтном режиме захвата (не --ipc/--live,
    DEVX-4: LUMEN_NO_* paint-бисект).
    """
    test_path = os.path.join(TESTS_DIR, html)
    if not os.path.exists(test_path):
        print(f'TEST-{tid}: FAIL (no HTML: {test_path})')
        return False, crop_offset, -1.0, None, []

    stem = html[:-5]  # '00-calibration.html' → '00-calibration'
    edge_png   = os.path.join(SHOTS, f'{stem}-edge.png')
    lumen_raw  = os.path.join(SHOTS, f'{stem}-lumen.png')
    lumen_crop = os.path.join(SHOTS, f'{stem}-lumen-cropped.png')
    diff_png   = os.path.join(SHOTS, f'{stem}-diff.png')

    capture_edge(test_path, edge_png, force=no_cache)
    if not os.path.exists(edge_png):
        print(f'TEST-{tid}: FAIL (Edge screenshot missing)')
        return False, crop_offset, -1.0, None, []

    rel_html = os.path.relpath(test_path, REPO).replace('\\', '/')
    console_errors: list[str] = []
    if _IPC_CLIENT is not None:
        # IPC-режим (TAB-7): детерминированный CPU-снимок по TCP, без gdigrab.
        try:
            ipc_capture_lumen(test_path, lumen_raw)
        except (IpcError, OSError) as e:
            print(f'TEST-{tid}: ERROR (IPC: {e})', flush=True)
            return False, crop_offset, -1.0, None, []
        if not os.path.exists(lumen_raw):
            print(f'TEST-{tid}: FAIL (IPC screenshot missing)')
            return False, crop_offset, -1.0, None, []
        # CPU-снимок уже от (0,0): магента-калибровка/crop offset не нужны.
        crop_offset = (0, 0)
    elif _LIVE_CLIENT is not None:
        # Live-window режим (SDC-3): один процесс/окно на весь прогон, но
        # снимок всё ещё через gdigrab — та же магента-калибровка, что и в
        # process-per-test режиме ниже (offset один и тот же весь прогон).
        try:
            capture_lumen_live(_LIVE_CLIENT, test_path, lumen_raw)
        except (McpLiveError, OSError) as e:
            print(f'TEST-{tid}: ERROR (live: {e})', flush=True)
            return False, crop_offset, -1.0, None, []
        if not os.path.exists(lumen_raw):
            print(f'TEST-{tid}: FAIL (live-window screenshot missing)')
            return False, crop_offset, -1.0, None, []

        # DEVX-1: любая console.error на странице — баг, невидимый на
        # скриншоте (не влияет на pixel diff). Читаем после навигации, до
        # следующего navigate() (который чистит буфер на стороне шелла).
        try:
            console_errors = [
                e['message'] for e in _LIVE_CLIENT.read_console() if e.get('level') == 'Error'
            ]
        except McpLiveError as e:
            print(f'TEST-{tid}: WARN (console read failed: {e})', flush=True)

        if tid == '00':
            origin = find_marker_origin(lumen_raw)
            if origin is None:
                print(f'TEST-{tid}: FAIL (magenta marker not found)')
                return False, None, -1.0, None, console_errors
            crop_offset = origin
            _save_crop_offset(crop_offset)

        if crop_offset is None:
            crop_offset = _load_crop_offset()
        if crop_offset is None:
            print(f'TEST-{tid}: FAIL (no crop offset — run TEST-00 first)')
            return False, None, -1.0, None, console_errors
    else:
        capture_lumen(rel_html, lumen_raw, extra_env=extra_env)
        if not os.path.exists(lumen_raw):
            print(f'TEST-{tid}: FAIL (gdigrab screenshot missing)')
            return False, crop_offset, -1.0, None, []

        if tid == '00':
            origin = find_marker_origin(lumen_raw)
            if origin is None:
                print(f'TEST-{tid}: FAIL (magenta marker not found)')
                return False, None, -1.0, None, []
            crop_offset = origin
            _save_crop_offset(crop_offset)

        if crop_offset is None:
            crop_offset = _load_crop_offset()
        if crop_offset is None:
            print(f'TEST-{tid}: FAIL (no crop offset — run TEST-00 first)')
            return False, None, -1.0, None, []

    ffmpeg_crop(lumen_raw, lumen_crop, crop_offset[0], crop_offset[1])
    if os.path.exists(lumen_raw):
        os.remove(lumen_raw)
    if not os.path.exists(lumen_crop):
        print(f'TEST-{tid}: FAIL (ffmpeg crop failed)')
        return False, crop_offset, -1.0, None, console_errors
    ffmpeg_diff(edge_png, lumen_crop, diff_png)
    if not os.path.exists(diff_png):
        print(f'TEST-{tid}: FAIL (ffmpeg diff failed)')
        return False, crop_offset, -1.0, None, console_errors

    pct, region = diff_stats(diff_png)
    debtor_verdict, debtor_msg = check_debtor(tid, pct)
    if debtor_verdict == 'OK':
        passed = True
        region_str = _fmt_region(region) if region else ''
        print(f'TEST-{tid}: DEBTOR ({pct:.2f}%){debtor_msg}', flush=True)
    elif debtor_verdict in ('RATCHET', 'REMOVE'):
        passed = False
        print(f'TEST-{tid}: FAIL ({pct:.2f}%)', flush=True)
        print(debtor_msg, flush=True)
    elif debtor_verdict == 'REGRESS':
        passed = False
        print(f'TEST-{tid}: FAIL ({pct:.2f}%)', flush=True)
        print(debtor_msg, flush=True)
    else:
        passed = pct <= threshold
        region_str = _fmt_region(region) if region else ''
        suffix = f'  [{region_str}]' if region_str and not passed else ''
        print(f'TEST-{tid}: {"PASS" if passed else "FAIL"} ({pct:.2f}%){suffix}', flush=True)
    if console_errors:
        passed = False
        for msg in console_errors:
            print(f'TEST-{tid}: FAIL (console error: {msg})', flush=True)
    return passed, crop_offset, pct, region, console_errors


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
    debtors = sum(1 for r in results if r['status'] == 'DEBTOR')
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
            'debtors': debtors,
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
        console_errors = r.get('console_errors') or []

        css_cls = {'PASS': 'pass', 'FAIL': 'fail', 'ERROR': 'error', 'DEBTOR': 'debtor'}.get(status, 'skip')
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

        # DEVX-1: console.error текст — класс багов, невидимый на скриншоте.
        if console_errors:
            errs = ''.join(f'<div>{escape_html(msg)}</div>' for msg in console_errors)
            imgs += f'<div class="console-err">{errs}</div>'

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
tr.debtor td{{background:#1a1400}}
.tid{{width:70px;color:#777}}
.status-PASS{{color:#4b4;font-weight:bold}}
.status-FAIL{{color:#c44;font-weight:bold}}
.status-ERROR{{color:#c84;font-weight:bold}}
.status-DEBTOR{{color:#cc0;font-weight:bold}}
.pct{{width:65px}}
.thr{{width:55px;color:#666}}
.region{{width:160px;color:#888;font-size:11px}}
.label{{max-width:280px;color:#999;word-break:break-word}}
.imgs{{display:flex;gap:6px;flex-wrap:wrap;margin-top:4px}}
.console-err{{width:100%;color:#e88;font-size:11px;margin-top:4px;font-family:monospace}}
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
  <span style="color:#cc0">⚠ {s.get('debtors',0)} known-debtor</span> &nbsp;
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


def run_paint_bisect(tid: str, no_cache: bool = False) -> int:
    """DEVX-4: прогоняет один тест N+1 раз — baseline, затем по одному с каждым
    LUMEN_NO_* флагом отключённым — и печатает таблицу diff%% по флагам, чтобы
    локализовать, какая paint-оптимизация меняет картинку (см. docs/automation.md).
    """
    target = next((t for t in TESTS if t[0] == tid), None)
    if target is None:
        known = ', '.join(t[0] for t in TESTS)
        print(f'--paint-bisect: тест {tid} не найден в TESTS. Доступны: {known}')
        return 1
    _, html, threshold, label = target

    crop_offset = _load_crop_offset()
    if crop_offset is None:
        print('Калибровка отсутствует — прогоняю TEST-00...')
        cal_id, cal_html, cal_threshold, cal_label = TESTS[0]
        _, crop_offset, _, _, _ = run_one(cal_id, cal_html, cal_threshold, cal_label, None)
        if crop_offset is None:
            print('Калибровка не удалась (TEST-00) — --paint-bisect остановлен.')
            return 1

    print(f'--paint-bisect {tid}: baseline + {len(PAINT_BISECT_FLAGS)} флагов (по одному)')
    _, crop_offset, base_pct, _, _ = run_one(
        tid, html, threshold, label, crop_offset, no_cache=no_cache,
    )
    if base_pct < 0:
        print(f'--paint-bisect: baseline TEST-{tid} не дал результата (ERROR).')
        return 1
    rows: list[tuple[str, float]] = [('baseline', base_pct)]
    for flag in PAINT_BISECT_FLAGS:
        _, crop_offset, pct, _, _ = run_one(
            tid, html, threshold, label, crop_offset, extra_env={flag: '1'},
        )
        if pct < 0:
            print(f'--paint-bisect: {flag} прогон не дал результата (ERROR), пропущен.')
            continue
        rows.append((flag, pct))

    print(f'\n--paint-bisect TEST-{tid} ({label}) — diff% по флагам:')
    for name, pct in rows:
        delta_str = '' if name == 'baseline' else f'  (Δ {pct - base_pct:+.2f})'
        print(f'  {name:<26} {pct:6.2f}%{delta_str}')

    moved = [name for name, pct in rows[1:] if abs(pct - base_pct) >= 0.01]
    if moved:
        print(f'\nМеняют картинку: {", ".join(moved)}')
    else:
        print('\nНи один флаг не изменил diff% — оптимизации не влияют на этот тест.')
    return 0


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
                        help='Пересобрать lumen-shell перед запуском (профиль задаётся LUMEN_PROFILE=dev-release)')
    parser.add_argument('--no-cache', action='store_true',
                        help='Принудительная пересъёмка Edge-скриншотов (игнорировать кэш)')
    parser.add_argument('--ipc', action='store_true',
                        help='Захват Lumen через `--ipc-server` (детерминированный CPU-снимок по TCP) '
                             'вместо gdigrab — без окна/ffmpeg-grab/магента-калибровки (TAB-7)')
    parser.add_argument('--live', action='store_true',
                        help='Один процесс/окно lumen на весь прогон вместо kill+relaunch на каждый тест: '
                             'Navigate+wait(document_ready) через `--mcp-live-port` (SDC-2), '
                             'снимок по-прежнему через gdigrab (SDC-3). Совместим с реальным JS '
                             '(TEST-57, 129-138), в отличие от --ipc.')
    parser.add_argument('--bisect', metavar='ID',
                        help='Прогнать юнит-зависимости interaction-теста (DEPS), затем сам тест; '
                             'вердикт: сломано свойство или взаимодействие')
    parser.add_argument('--paint-bisect', metavar='ID',
                        help='DEVX-4: прогнать тест N+1 раз — baseline и с поочерёдным '
                             'отключением каждого LUMEN_NO_* paint-флага (renderer.rs); '
                             'таблица diff%% — какая оптимизация меняет картинку')
    args = parser.parse_args()

    if args.ipc and args.live:
        print('--ipc и --live взаимоисключающие (два разных способа захвата Lumen).')
        return 2
    if args.paint_bisect and (args.ipc or args.live):
        print('--paint-bisect несовместим с --ipc/--live: нужен свежий процесс Lumen на '
              'каждый LUMEN_NO_* флаг, чтобы переменная окружения гарантированно применилась.')
        return 2

    os.makedirs(SHOTS, exist_ok=True)
    ensure_lumen(force_build=args.build)

    if args.paint_bisect:
        return run_paint_bisect(args.paint_bisect, no_cache=args.no_cache)

    # IPC-режим (TAB-7): один раз поднимаем lumen --ipc-server, держим одну
    # вкладку, навигируем её на каждый тест. Завершение — через atexit.
    if args.ipc:
        global _IPC_CLIENT, _IPC_TAB
        print('IPC-режим: запуск lumen --ipc-server...')
        try:
            _IPC_CLIENT = LumenIpcClient(LUMEN, REPO)
            _IPC_TAB = _IPC_CLIENT.create_tab()
        except (IpcError, OSError) as e:
            print(f'Не удалось запустить IPC-сервер: {e}')
            return 2
        import atexit
        atexit.register(_IPC_CLIENT.shutdown)
        print(f'IPC-сервер готов (вкладка {_IPC_TAB}); gdigrab/магента-калибровка отключены.')

    # Live-window режим (SDC-3): один раз поднимаем настоящее окно lumen
    # (--mcp-live-port) и держим его на весь прогон вместо kill+relaunch на
    # каждый тест. gdigrab/магента-калибровка остаются (см. capture_lumen_live).
    if args.live:
        global _LIVE_CLIENT
        port = _free_tcp_port()
        print(f'Live-режим: запуск lumen --mcp-live-port {port}...')
        try:
            _LIVE_CLIENT = LiveWindowClient(LUMEN, REPO, port)
        except McpLiveError as e:
            print(f'Не удалось запустить живое окно: {e}')
            return 2
        import atexit
        atexit.register(_LIVE_CLIENT.shutdown)
        print('Живое окно готово; один процесс на весь прогон.')

    crop_offset: tuple[int, int] | None = None
    results: list[dict] = []
    halted_at: str | None = None

    # --- Определяем набор тестов для запуска ---
    run_filter: set[str] | None = None
    if args.bisect:
        if args.bisect not in DEPS:
            known = ', '.join(sorted(DEPS))
            print(f'--bisect: тест {args.bisect} не является interaction-тестом. Доступны: {known}')
            return 1
        run_filter = set(DEPS[args.bisect]) | {args.bisect}
        crop_offset = _load_crop_offset()
        if crop_offset is None:
            run_filter.add('00')
        # внутри бисекта первый провал не должен останавливать прогон
        args.continue_on_fail = True
        print(f'--bisect {args.bisect}: зависимости {", ".join(DEPS[args.bisect])} + сам тест')
    elif args.only:
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
        passed, crop_offset, pct, region, console_errors = run_one(
            tid, html, threshold, label, crop_offset,
            no_cache=args.no_cache,
        )
        debtor_verdict, debtor_msg = check_debtor(tid, pct)
        if pct < 0:
            status = 'ERROR'
        elif console_errors:
            # DEVX-1: a JS console error overrides even a known-debtor pixel
            # verdict — it's a distinct class of bug the pixel diff can't see.
            status = 'FAIL'
        elif debtor_verdict == 'OK':
            status = 'DEBTOR'
        elif passed:
            status = 'PASS'
        else:
            status = 'FAIL'
        entry = {
            'id': tid,
            'stem': html[:-5],
            'label': label,
            'html': html,
            'threshold': threshold,
            'status': status,
            'diff_pct': round(pct, 4),
            'diff_region': region,
        }
        if console_errors:
            entry['console_errors'] = console_errors
        if debtor_verdict is not None:
            bug, baseline = KNOWN_DEBTORS[tid]
            entry['debtor'] = {'bug': bug, 'baseline': baseline, 'verdict': debtor_verdict}
        if tid in DEPS:
            entry['deps'] = DEPS[tid]
            if status == 'FAIL':
                affected = affected_objects(tid, region)
                entry['affected'] = affected
                if affected:
                    print('         объекты: ' + ' · '.join(affected))
                print(f'         юнит-зависимости: {", ".join(DEPS[tid])}'
                      f'  (python graphic_tests/run.py --bisect {tid})')
        results.append(entry)
        if not passed and not args.continue_on_fail and (debtor_verdict != 'OK' or console_errors):
            halted_at = tid
            break

    # --- Сохранение результатов ---
    if not args.only and not args.bisect:
        json_path = save_results(results, crop_offset, halted_at)
        html_path = json_path.replace('.json', '.html')
        print(f'\nРезультаты: {os.path.relpath(json_path, REPO)}')
        print(f'HTML-отчёт: {os.path.relpath(html_path, REPO)}')
        prev = _load_previous()
        if prev:
            print_diff_vs_previous(results, prev)

    # --- Вердикт бисекта ---
    if args.bisect:
        target = next((r for r in results if r['id'] == args.bisect), None)
        dep_fails = [r['id'] for r in results
                     if r['id'] in DEPS[args.bisect] and r['status'] != 'PASS']
        print()
        if target is None or target['status'] == 'ERROR':
            print(f'Бисект не завершён: TEST-{args.bisect} не дал результата.')
            return 1
        if dep_fails:
            print(f'Вердикт: сломано базовое свойство — юнит-тест(ы) '
                  f'{", ".join("TEST-" + d for d in dep_fails)} FAIL. Чинить их первыми.')
            return 1
        if target['status'] == 'FAIL':
            print(f'Вердикт: баг ВЗАИМОДЕЙСТВИЯ свойств — все юнит-зависимости PASS, '
                  f'TEST-{args.bisect} FAIL.')
            return 1
        print(f'Вердикт: TEST-{args.bisect} и все его юнит-зависимости проходят.')
        return 0

    # --- Итог ---
    if halted_at:
        # порядок прогона = порядок в списке TESTS; строковое сравнение id
        # некорректно для трёхзначной interaction-серии ('100' < '22')
        ids = [t[0] for t in TESTS]
        skipped = len(ids) - ids.index(halted_at) - 1
        print(f'\nPipeline stopped at TEST-{halted_at}. {skipped} tests skipped.')
        return 1
    failed  = [r for r in results if r['status'] == 'FAIL']
    debtors = [r for r in results if r['status'] == 'DEBTOR']
    passed  = [r for r in results if r['status'] == 'PASS']
    if failed:
        debtor_note = (f'  ({len(debtors)} known-debtor: ' + ', '.join(r['id'] for r in debtors) + ')'
                       if debtors else '')
        print(f'\n{len(failed)}/{len(results)} tests FAILED: ' + ', '.join(r['id'] for r in failed))
        if debtor_note:
            print(debtor_note)
        return 1
    if debtors:
        print(f'\n{len(passed)} PASS + {len(debtors)} DEBTOR (known Phase-2): '
              + ', '.join(r['id'] for r in debtors))
        return 0
    print(f'\nAll {len(results)} tests passed.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
