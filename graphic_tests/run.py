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
KNOWN_DEBTORS: dict[str, tuple[str, float]] = {
    '54': ('BUG-173', 2.5),     # SVG <path> fill+stroke остаток (AA-швы, self-intersecting fill)
    '57': ('BUG-099', 4.14),    # <canvas> getContext("2d") — Phase 2
    '60': ('BUG-173', 1.5),     # SVG stroke advanced остаток (triangle-soup AA-швы, dash-on-curve)
    '61': ('BUG-103', 99.53),   # View Transitions API — Phase 2
    '63': ('BUG-105', 48.44),   # CSS Masonry layout — Phase 2
    '75': ('BUG-143', 16.97),   # masonry-auto-flow — Phase 2
    '119': ('BUG-173', 0.81),   # paint-order: остаток = stroke triangle-soup AA-швы (geometry фикснут BUG-174)
    '36': ('BUG-176', 1.11),    # border-radius: остаток = edge-AA + elliptical-corner kappa (квадратные рамки фикснуты BUG-175)
    '30': ('BUG-144', 7.56),    # CSS filter/backdrop-filter: row-flip (BUG-144) + gradient hard-stop row 2 (BUG-085 tail-fill, 10.5%→7.56%); остаток = filter pixel-parity (rows 1-3) + backdrop захват тёмным внутри opacity-слоя (row 4)
    '39': ('BUG-085', 1.62),    # gradients: repeating-linear/-radial теперь повторяются + hard-stop хвост дозаполняется (femtovg_stops, 12.05%→1.62%); row 1 linear совпадает пиксель-в-пиксель; остаток = 256-тексельная квантизация градиент-текстуры femtovg на repeating-границах (rows 2-3) + radial-интерполяция/AA vs Edge + gdigrab-шум
    '51': ('BUG-124', 1.09),    # scrollbar rendering: float-wrapper shrink-to-fit фикснут BUG-178 (9.91% → 1.09%); остаток = дробные layout Y-координаты vs пиксельное округление Edge
    '64': ('BUG-128', 8.99),    # table: margin-collapse таблица↔блок фикснут BUG-193 (13.89% → 8.99%); остаток = font-parity (текст в ~21 ячейках + заголовки, Inter vs Edge) + ~3px накопленный line-height сдвиг
    '18': ('BUG-219', 2.11),    # <img>: «image bottom gap» (baseline descent) фикснут BUG-180 (21.21% → 2.11%); остаток = image-resampling AA (area-avg ≠ Edge downscale kernel)
    '83': ('BUG-128', 7.88),    # scroll-behavior: text-only inline-block shrink-to-fit фикснут BUG-202 (14.02% → 7.88%); остаток = font-parity (Inter vs Edge) во всём тексте страницы + faint overlay scrollbar
    '92': ('BUG-124', 0.90),    # system colors: значения system_color() приведены к Edge BUG-210 (15.59% → 0.90%); layout/цвета идеальны (dump-layout: 164px border-box, gap 4, hex точны), остаток = gdigrab суб-пиксельный сдвиг (~+3px на 1000px) на границах ячеек vs пиксельное округление Edge
    '67': ('BUG-128', 1.36),    # attr(): ::before на flex-контейнере не генерировался — фикснут BUG-196 (16.41% → 1.36%); тёмные label-боксы и бары совпадают с Edge пиксель-в-пиксель, остаток = font-parity (white monospace label text, Inter vs Edge) + sub-pixel edge-AA по border-radius клипу
    '62': ('BUG-128', 2.32),    # scroll-snap: column flex-grow не распределял free space — фикснут BUG-104 (63.70% → 2.32%); все цветные заливки контейнеров совпадают с Edge пиксель-в-пиксель (diff), остаток = font-parity (метки секций A/1/NW/Stop, Inter vs Edge) + border-radius edge-AA (BUG-176)
    '77': ('BUG-126', 12.94),   # anchor-positioning: corner/edge placement фикснут BUG-126 (53.45% → 12.94%); 3×3 сетка container 1 совпадает с Edge(position-area) пиксель-в-пиксель (diff). Остаток = (1) тест использует устаревшее имя `inset-area`, которое текущий Edge игнорирует (Edge поддерживает только `position-area`); (2) span-ряд container 2 — Lumen по спеку растягивает auto-width элементы на position-area band, Edge не отрисовывает span-* вовсе. Lumen спек-корректнее Edge; расхождение в reference-браузере, не дефект движка
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


# Активный IPC-клиент и вкладка (заполняются в main при --ipc; иначе None).
_IPC_CLIENT: LumenIpcClient | None = None
_IPC_TAB: int = 0


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
    if _IPC_CLIENT is not None:
        # IPC-режим (TAB-7): детерминированный CPU-снимок по TCP, без gdigrab.
        try:
            ipc_capture_lumen(test_path, lumen_raw)
        except (IpcError, OSError) as e:
            print(f'TEST-{tid}: ERROR (IPC: {e})', flush=True)
            return False, crop_offset, -1.0, None
        if not os.path.exists(lumen_raw):
            print(f'TEST-{tid}: FAIL (IPC screenshot missing)')
            return False, crop_offset, -1.0, None
        # CPU-снимок уже от (0,0): магента-калибровка/crop offset не нужны.
        crop_offset = (0, 0)
    else:
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
    parser.add_argument('--bisect', metavar='ID',
                        help='Прогнать юнит-зависимости interaction-теста (DEPS), затем сам тест; '
                             'вердикт: сломано свойство или взаимодействие')
    args = parser.parse_args()

    os.makedirs(SHOTS, exist_ok=True)
    ensure_lumen(force_build=args.build)

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
        passed, crop_offset, pct, region = run_one(
            tid, html, threshold, label, crop_offset,
            no_cache=args.no_cache,
        )
        debtor_verdict, _debtor_msg = check_debtor(tid, pct)
        if pct < 0:
            status = 'ERROR'
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
        if not passed and not args.continue_on_fail:
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
