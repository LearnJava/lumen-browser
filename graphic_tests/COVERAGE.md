# Покрытие графических тестов

Все тесты — только графика (0 текста). Шум = ненулевые пиксели в diff с Edge, не связанные с тестируемым свойством.

Viewport: 1024×720. Body padding: 24px (где есть). Gap между объектами: 16px.

**Маркер.** Каждый тест начинается с 1-px магента-полоски (`#ff00ff`) шириной 1024 px как первый ребёнок body. Используется workflow для динамического определения crop offset. Из-за маркера весь существующий контент сдвинут вниз на 1 px (одинаково в Edge и Lumen — diff остаётся валидным).

**Пайплайн блокирующий.** Тест 00-calibration должен пройти первым (магента-маркеры найдены и совпадают), затем 01-sanity, потом по нумерации. Первый провал — остановка пайплайна, последующие тесты не выполняются.

**Диапазоны нумерации.**

| Диапазон | Слой | Назначение |
|---|---|---|
| `00–99` | Юнит-слой | Одно CSS-свойство на файл, изолированно. Падение = баг самого свойства. |
| `100–199` | Interaction-слой | Комбинации свойств, уже покрытых юнит-тестами. Падение при зелёных зависимостях (`DEPS` в `run.py`) = баг взаимодействия. Диагностика: `python graphic_tests/run.py --bisect <id>`. |
| `1000000` | Финал | Всё в одном окне, ручная проверка + CPU-снапшот. |

Все interaction-тесты используют общую сетку из 6 ячеек 300×300 (координаты в `_CELL_GRID` в `run.py`); при FAIL `run.py` пересекает diff_region с ячейками и печатает, какой сценарий разошёлся.

---

## Файлы

| Файл | Тема | Объектов | Покрываемые свойства |
|---|---|---|---|
| 00-calibration.html | Магента-рамка по верху/низу viewport | 2 | Минимальный: block + width/height + background-color из class CSS. Если этот тест не проходит, всё остальное бессмысленно. |
| 01-sanity.html | Один белый квадрат на чёрном фоне | 1 | background-color, width/height, margin |
| 02-color-named.html | Именованные цвета CSS | 18 | background-color: named (red/blue/green/yellow/orange/purple/tomato/teal/coral/crimson/rebeccapurple/dodgerblue/hotpink/mediumseagreen/steelblue/goldenrod/slategray/indianred) |
| 03-color-formats.html | Нотации цвета | 9 | background-color: named · #RGB · #RRGGBB · #RGBA · #RRGGBBAA · rgb() · rgba() · hsl() · hsla() |
| 04-color-alpha.html | Прозрачность: rgba / hsla / #RRGGBBAA | 18 | background-color: alpha от 0.1 до 1.0 (red + blue + green + purple) |
| 05-border-width.html | Толщина бордера | 10 | border-width: 1/2/4/8/16px; асимметричные per-side widths |
| 06-border-sides.html | Стороны и цвета бордера | 9 | border-top/right/bottom/left; per-side colors; currentColor; border+padding layering |
| 07-box-sizing.html | content-box vs border-box | 8 | box-sizing; padding; border (наглядная разница размера) |
| 08-padding.html | Отступы внутри блока | 9 | padding: uniform · asymmetric (TB/LR) · 4-value · 0 |
| 09-margin.html | Отступы снаружи блока | 11 | margin-left (staircase) · margin-top (gap stepping) |
| 10-min-max-width.html | Ограничения по ширине | 12 | min-width · max-width · min>max edge case · calc · min() · clamp() |
| 11-min-max-height.html | Ограничения по высоте | 12 | min-height · max-height · min>max edge case |
| 12-display.html | Значения display | 17 | display: block · inline-block · none · vertical-align: top |
| 13-visibility-opacity.html | Видимость и прозрачность | 16 | visibility: hidden (space reserved) · opacity: 0.1–1.0 · opacity на group |
| 14-overflow.html | Обрезка содержимого | 4 | overflow: visible · hidden · overflow-x/overflow-y раздельно |
| 15-box-shadow.html | Тени блоков | 8 | box-shadow: offset · blur · spread · color · multiple · negative offset |
| 16-outline.html | Обводка снаружи | 9 | outline-width · outline-color · outline-offset (positive / negative) · layout не сдвигается |
| 17-calc.html | CSS math | 14 | calc() · min() · max() · clamp() · sqrt() · cos() · abs() · hypot() · nested |
| 18-images.html | Растровые изображения | 17 | \<img\> PNG/JPEG/WebP · CSS width/height · transparent PNG on colored bg · \<picture\> media/type picking · \<img srcset\> width/density descriptors · WebP VP8L |
| 19-object-fit.html | Вписывание изображения | 9 | object-fit: fill/contain/cover/none/scale-down · object-position |
| 20-quirks-bgcolor.html | Устаревший bgcolor (Quirks mode) | 15 | CSS hashless hex · bgcolor attr on \<td\> · legacy color parsing |
| 21-border-style.html | Стили border: dashed/dotted/double | 16 | border-style: dashed · dotted · double (2/4/8/16px) · per-side mix · double thin fallback |
| 22-transform.html | CSS transform | 30 | transform: translate · translateX/Y · rotate · scale · scaleX/Y · skewX · skewY · matrix() · combined · transform-origin (4 variants) |
| 23-pseudo-elements.html | ::before / ::after block + inline generation | 7 | ::before display:block · ::after display:block · both on one element · ::before с другой шириной · ::before inline (padding box) · ::after inline · both inline |
| 24-vertical-align.html | vertical-align | 6 | inline-block: top/middle/bottom · inline span: super/middle/sub (frag y-offset + bg) |
| 25-table-layout.html | Table layout | 19 | display:table — горизонтальный layout ячеек · auto-width distribution · явные ширины · несколько строк (вертикальное стакование) |
| 26-mask-image.html | mask-image, mask-mode, PushMaskLayer/PopMaskLayer | 6 | linear-gradient mask · radial-gradient mask · control (no mask) · mask-mode:alpha (radial gradient) · mask-mode:luminance (linear gradient, black→white) · control (no mask) |
| 27-direction-rtl.html | direction | 6 | LTR start (left) · RTL start (right) · RTL end (left) · alignment gradient bands |
| 28-css-containment.html | contain | 5 | baseline (no contain) · contain:size (height=0) · contain:paint (overflow clip) · contain:layout · contain:strict |
| 29-container-queries.html | @container | 4 | wide container: min-width applies (blue) · narrow: not applies (red) · named container · max-width |
| 30-css-filter.html | CSS filter + backdrop-filter | 20 | grayscale(1) · sepia(1) · brightness(2) · invert(1) · contrast(3) · saturate(3) · opacity(0.4) · blur(8px) · hue-rotate(90deg/180deg) · backdrop-filter: blur/grayscale/brightness/invert/combo |
| 31-clip-path.html | clip-path | 11 | inset(1/4-value) · circle(r/at) · ellipse(rx ry/at) · polygon(triangle/rect bbox) · path(M/L/Z + cubic Bézier) · clip-path + overflow:hidden |
| 32-list-markers.html | list markers | 20 | display:list-item · ::marker box · list-style-type: disc/circle/square/decimal/lower-alpha/lower-roman · list-style-position: outside/inside · list-style-type:none · ::marker { color } CSS override · ::marker { content } CSS override · **@counter-style** custom (alphabetic+prefix, numeric+prefix) · **list-style-type: \<custom-ident\>** lookup in CounterStyleRegistry |
| 33-multi-column.html | multi-column layout + column-rule | 7 | column-count:2/3/4/5 · column-width · column-gap · column-rule: solid/dashed/dotted · rule centered in gap · rule wider than gap (clamped) |
| 34-forms.html | form controls static rendering | 18 | input[text/email/password/number/search/range/color/submit] · checkbox (unchecked/checked/disabled) · radio (unchecked/checked) · button · textarea · select · required · disabled UA styles |
| 35-grid-named-areas.html | CSS Grid named areas | 9 | grid-template-areas · grid-area: &lt;name&gt; · named area spanning multiple rows/cols · page layout (header/sidebar/main/footer) · mini-grid with 5 named areas |
| 36-border-radius.html | border-radius: uniform, pill, circle, asymmetric, elliptical (rx≠ry) | 28 | border-radius: 0/4/8/16/24/32px · + border · pill (999px) · circle (50%) · asymmetric per-corner · large clamped · nested · elliptical shorthand (H/V) · per-corner elliptical · individual corner rx ry |
| 37-float-clear.html | float: left/right + clear: both | 11 | float: left · float: right · float left + right combined · two left floats horizontal stack · clear: both clearance |
| 38-z-index.html | z-index stacking context paint order | 6 | positive z-index (1/2/3) painted in correct order · negative z-index behind parent · z-index:auto same phase as z:0 · high z-index over zero-z sibling |
| 39-gradients.html | linear-gradient / radial-gradient | 13 | linear: to right / to bottom / 45deg / 3-stop / transparent · radial: circle center / offset / 3-stop / ellipse · repeating-linear / repeating-radial · stacked |
| 40-conic-gradients.html | conic-gradient / repeating-conic-gradient | 9 | default rainbow · 2-color · from 90deg · at 25% 25% · explicit deg-stops · pie chart (sharp) · repeating-conic 4 wedges · repeating-conic 8 wedges · wide box (box-space angles) |
| 41-table.html | display:table/row/cell layout engine | 3 | table with thead/tbody/tfoot row groups · global column width alignment across rows · native HTML table |
| 42-position-sticky.html | position:sticky | 5 | sticky-bar (top:10px) · static block (unaffected flow) · sticky-side (left:10px) · static block with border · sticky-bottom (bottom:30px) |
| 43-intrinsic-sizing.html | CSS Intrinsic Sizing L3 | 6 | width: max-content (300px/600px child) · width: min-content (250px/500px child) · width: fit-content (400px/180px child in wide container) |
| 44-media-queries.html | Media Queries L3 #12 | 6 | @media screen ✓ · @media print ✗ · @media (min-width: 48em) ✓ · @media (max-width: 50rem) ✗ · @media (orientation: landscape) ✓ · @media (min-aspect-ratio: 1/1) ✓ |
| 45-multiple-backgrounds.html | CSS Backgrounds L3 §3 — multiple layers | 9 | 2-gradient stack · 3-layer radial+stripe+solid · bg-position center 80px · bg-size top-right 60px · repeat-x stripe · two no-repeat halves · clip padding-box+border-box · origin content-box · color+gradient overlay |
| 47-svg-basic.html | SVG basic shapes + fill/stroke presentation attributes | 22 | rect · rect rx/ry · circle · ellipse · line · group `<g>` · viewBox scale 2× · viewBox with min_x/min_y offset · multiple shapes no viewBox · fill explicit color · fill:none (stroke only) · fill+stroke combo · fill-opacity · stroke-opacity · stroke-width · rounded rect stroke |
| 48-line-clamp.html | CSS Overflow L4 §3.2 — line-clamp multi-line truncation | 12 | -webkit-line-clamp 1/2/3/4 (staircase heights 40/80/120/160px) · unclamped reference 200px · explicit staircase reference · color:transparent boxes (no glyph divergence) |
| 50-css-variables.html | CSS Custom Properties (var()) | 16 | var() basic (3 colors) · nested var() (--a:var(--b)) · fallback var(--undef,color) · defined-wins-over-fallback · var() in calc() for width · custom prop inheritance via parent element |
| 51-scrollbar-rendering.html | Scrollbar rendering (DrawScrollbar) | 4 | overflow-y:scroll vertical scrollbar · overflow-x:scroll horizontal scrollbar · overflow:scroll both axes · no scrollbar when content fits |
| 52-text-shadow-blur.html | text-shadow blur (PushFilter{Blur}) | 8 | text-shadow blur=0 (sharp) · blur=4px · blur=10px · blur=20px — sharpness progression · two-color multi-shadow · drop-shadow · stacked hard shadows · glow (white text on dark bg) |
| 53-background-origin.html | background-origin (positioning area) | 6 | border-box · padding-box (default) · content-box — anchor 0%/0% (top-left) and 100%/100% (bottom-right) |
| 54-svg-path-stroke.html | SVG `<path>` stroke tessellation | 16 | open path stroke · closed path stroke · stroke-only (fill:none) · fill+stroke combo · diagonal/zigzag/curved contours · miter join · butt cap · varying stroke-width (2/5/8/10/12/14px) |
| 55-video-placeholder.html | `<video>` replaced element — grey placeholder | 6 | `<video>` UA default 300×150 · CSS `width`/`height` override · HTML `width`/`height` attr override · border · border-radius |
| 57-canvas-2d.html | `<canvas>` 2D context — JS drawing surface (HTML LS §4.12.4) | 6 | `canvas.getContext('2d')` · `fillRect` · `strokeRect` · `arc` · path fill (moveTo/lineTo/closePath/fill) · UA default 300×150 · `width`/`height` attr · CSS background + border + border-radius on the canvas element box. JS-driven: bitmap renders in the full shell / Edge nightly path; CPU snapshot shows the element box + placeholder (no JS in cpu_raster driver). |
| *(1000000-final.html)* | `<iframe>` replaced element — grey placeholder (HTML spec §4.8.5) | 3 | UA default 300×150 · CSS `width`/`height` override · HTML `width`/`height` attr override · border · empty `src`. `contentDocument`/`contentWindow` return `null` (Phase 0 — no sub-document navigation). JS `src`/`name`/`sandbox`/`width`/`height` properties reflect HTML attributes. |
| 58-first-letter-line.html | `::first-letter` drop-cap + `::first-line` color override (CSS Pseudo-elements L4 §5.3–5.4) | 2 | `::first-letter { font-size:48px; color:#f6e05e; float:left }` drop-cap · `::first-line { color:#68d391; font-weight:700 }` first-line green bold. Style applied via segment/frag style override in build_box()/lay_out() without requiring content:. |
| 60-svg-stroke-advanced.html | SVG stroke advanced: linecap / linejoin / miterlimit / dasharray / dashoffset / fill-rule | 18 | stroke-linecap butt/round/square · stroke-linejoin miter/bevel/round · stroke-miterlimit collapse · stroke-dasharray solid/dashed/offset · fill-rule nonzero/evenodd · dashed arc with round caps · combined linejoin+dasharray |
| 61-view-transitions.html | View Transitions API (CSS View Transitions L1) | 1 | `document.startViewTransition(callback)` · `ViewTransition { ready, finished, updateCallbackDone, skipTransition }` · Begin/End event queue · 300 ms opacity cross-fade (old_dl → new_dl in shell) · `::view-transition` pseudo-element stubs |
| 62-scroll-snap.html | CSS Scroll Snap L1 — scroll-snap-type/align/stop | 6 | scroll-snap-type y/x mandatory · both proximity · scroll-snap-align start · scroll-snap-stop always barrier · geometry validation in container/item |
| 63-masonry.html | CSS Masonry layout stub (Houdini) Phase 0 | 1 | display:masonry · waterfall grid algorithm — each item in column with min height · column-count · gap |
| 64-table.html | CSS Table layout §17 — Table rendering Phase 1 | 2 | **Separate mode:** `border-collapse: separate` · `border-spacing: 4px 2px` (h/v) · independent cell borders · `<thead>/<tbody>` groups · even/odd row backgrounds | **Collapse mode (pending):** `border-collapse: collapse` · border conflict resolution (wider border wins) · no spacing between cells · merged visual borders |
| 69-border-spacing.html | CSS 2.1 §17.6 `border-spacing` end-to-end wiring | 3 | `border-spacing: 12px` equal h/v · `border-spacing: 8px 24px` asymmetric · `border-spacing: 0` touching cells |
| 71-starting-style.html | CSS Transitions L2 §3.4 `@starting-style` — entry transition wiring | 2 | `@starting-style` rules parsed without crash · static rendering unaffected by entry declarations · two coloured boxes at declared CSS colours |
| 73-gap-rule.html | CSS Gap Decorations L1 `gap-rule-width/style/color` | 3 | flex-row 4 items solid red 2px rules · flex-wrap 5 items dashed cyan 3px rules · grid 3×2 solid orange 4px rules |
| 74-font-stretch.html | CSS Fonts L4 §5.2 `font-stretch` | 3 | keyword values ultra-condensed→extra-expanded · percentage form 50%–200% · inheritance + no-double wdth injection |
| 75-masonry-auto-flow.html | CSS Masonry Layout §9 `masonry-auto-flow` | 3 | masonry-auto-flow: next (source order) · ordered (CSS order property sorts items) · definite-first (explicit grid-column-start goes first) |
| 82-svg-use.html | SVG `<use>` element — shadow tree clone (SVG 2 §5.6) | 18 | clone rect/circle/ellipse from `<defs>` · x/y offset translation · `xlink:href` compat · `<g>` group clone · `<symbol>` clone · `transform=` on `<use>` · nested `<use>` chains |
| 83-scroll-behavior.html | `scroll-behavior` — smooth/auto overflow containers + page API (CSS Scroll Behavior L1 §3) | 3 | scroll-behavior: smooth overflow box · scroll-behavior: auto box · inline style · window.scrollTo({behavior:'smooth'}) API display |
| 100-transform-overflow.html | INTERACTION: transform × overflow (deps: 22, 14) | 6 | Клиппинг translate/rotate/scale-детей контейнером overflow:hidden · контроль без клипа · отрицательный translate · поворот самого клип-контейнера |
| 101-radius-overflow.html | INTERACTION: border-radius × overflow (deps: 36, 14) | 6 | Скруглённый клип детей: срез углов бара · круг из квадрата · контроль без клипа · вложенные скругления · radius+border · pill |
| 102-opacity-stacking.html | INTERACTION: opacity × z-index (deps: 13, 38) | 6 | z-index заперт в opacity-контексте · групповая композиция (нет двойного затемнения) · контроль per-child opacity · negative z внутри opacity · вложенная opacity 0.6×0.5 = эталон 0.3 |
| 103-filter-transform.html | INTERACTION: filter × transform (deps: 30, 22) | 6 | grayscale на повёрнутом градиенте · blur на translate · фильтр внутри повёрнутого родителя · filter как containing block · hue-rotate на scale · контроль |
| 104-mask-gradient-radius.html | INTERACTION: mask × gradients × radius (deps: 26, 39, 40, 36) | 6 | Линейная/радиальная маска поверх linear/radial/conic фона · маска на круге · контроль без маски · маска поверх бордера |
| 105-float-clear-margin.html | INTERACTION: float/clear × margin (deps: 37, 09) | 6 | Два флоата с margin · left+right флоат + поток между · clearance+margin-top · перенос флоатов · высокий флоат vs in-flow фон · контроль |
| 106-transform-zindex.html | INTERACTION: transform × z-index (deps: 22, 38) | 6 | negative z в transformed-родителе · transformed (z:0) vs z:1 сосед · z:2 над transformed z:1 · z-дети в rotate/scale-родителе · контроль |
| 107-shadow-radius-overflow.html | INTERACTION: shadow × radius × overflow (deps: 15, 36, 14) | 6 | Скруглённый силуэт тени · spread на круге · тень клипится родителем · контроль (тень выходит) · две тени + radius · blur-тень + radius + border |
| 108-nested-transforms.html | INTERACTION: вложенные transform (deps: 22) | 6 | rotate(15)∘rotate(−15)=identity vs эталон · scale масштабирует translate ребёнка · translate→rotate · 3×rotate(10°) vs эталон rotate(30°) |
| 109-clippath-transform.html | INTERACTION: clip-path × transform × radius (deps: 31, 22, 36) | 6 | circle-клип на rotate · inset-клип на scale · polygon на translate · клип родителя режет transformed-ребёнка · clip-path ∩ border-radius · контроль |
| **1000000-final.html** ★ | **ФИНАЛЬНЫЙ ТЕСТ — все свойства в одном окне** | ~80 | **Ручная проверка, не для автодиффа.** Обновляется при каждом новом CSS-свойстве. background-color (все нотации) · border (width/color/per-side/currentColor/dashed/dotted/double) · border-radius (SDF rendering: uniform/pill/circle/asymmetric) · box-shadow (hard/blur/spread) · outline (width/offset+/-) · overflow (visible/hidden) · opacity · visibility:hidden · object-fit (5 режимов) · calc/min/clamp · padding layering · transform (translate/rotate/scale) · table layout (2×4 ячейки) · linear/radial gradient (6 объектов) · conic gradient (5 объектов) |

---

## Свойства → покрытие

| Свойство | Файл(ы) | Непокрытые аспекты |
|---|---|---|
| background-color — named | 02 | — |
| background-color — hex (#RGB/#RRGGBB/#RGBA/#RRGGBBAA) | 03, 04 | — |
| background-color — rgb()/rgba() | 03, 04 | — |
| background-color — hsl()/hsla() | 03, 04 | — |
| background-color — currentColor | — | ❌ нет отдельного теста |
| background-color — transparent | — | ❌ нет отдельного теста |
| border-width (1/2/4/8/16px) | 05 | — |
| border-width асимметричный per-side | 05 | — |
| border-color per-side | 06 | — |
| border-color currentColor | 06 | — |
| border-style: solid | 05, 06 | — |
| border-style: dashed/dotted/double | 21 | — |
| box-sizing: content-box | 07 | — |
| box-sizing: border-box | 07 | — |
| padding (4-value, asymmetric) | 08 | — |
| margin-left | 09 | — |
| margin-top | 09 | — |
| margin-right / margin-bottom | — | ❌ нет отдельного теста |
| margin collapse (вертикальный) | — | ❌ нет теста |
| width / height | 01, 07, 08, 09, 10 | — |
| min-width / max-width | 10 | — |
| min-height / max-height | 11 | — |
| display: block | 12 | — |
| display: inline-block | 12 | — |
| display: none | 12 | — |
| display: inline | — | ❌ без текста протестировать невозможно |
| visibility: hidden | 13 | — |
| opacity | 13 | — |
| overflow: hidden | 14 | — |
| overflow: visible | 14 | — |
| overflow-x / overflow-y раздельно | 14 | — |
| box-shadow (offset/blur/spread/color) | 15 | inset-тень (не реализована в paint) |
| outline (width/color/offset) | 16 | — |
| z-index / stacking order | 38 | negative-z behind viewport bg · z:auto vs z:0 same phase |
| calc() / min() / max() / clamp() | 17 | — |
| sqrt() / cos() / abs() / hypot() | 17 | sin() не тестируется отдельно |
| \<img\> PNG / JPEG | 18 | — |
| \<img\> масштаб CSS width/height | 18 | — |
| transparent PNG на цветном фоне | 18 | — |
| \<picture\> media query picking | 18 | — |
| \<picture\> type filter (skip unsupported MIME) | 18 | — |
| \<img srcset\> width descriptors (Nw) | 18 | — |
| \<img srcset\> density descriptors (Nx) | 18 | — |
| object-fit (5 режимов) | 19 | — |
| object-position | 19 | — |
| legacy bgcolor / hashless hex | 20 | — |
| display: table (table layout) | 25 | colspan/rowspan не реализованы · border-spacing не реализован |
| transform: translate(x,y) / translateX / translateY | 22 | — |
| transform: rotate(deg) | 22 | — |
| transform: scale(x,y) / scaleX / scaleY | 22 | — |
| transform: skewX / skewY | 22 | — |
| transform: matrix(a,b,c,d,e,f) | 22 | — |
| transform: combined (multiple functions) | 22 | — |
| transform-origin | 22 | % values (50% 50% default works; explicit % not tested) |
| ::before (display:block, content) | 23 | — |
| ::before (display:inline, padding box) | 23 | — |
| ::after (display:block, content) | 23 | — |
| ::after (display:inline, padding box) | 23 | — |
| vertical-align: top / middle / bottom (inline-block) | 24 | — |
| vertical-align: super / sub / middle (inline text y-offset) | 24 | length/percent (text-only, no visual without font) |
| column-count | 33 | — |
| column-width | 33 | — |
| column-gap | 33 | — |
| column-rule (shorthand) | 33 | — |
| column-rule-width | 33 | — |
| column-rule-style: solid/dashed/dotted | 33 | double не тестируется отдельно |
| column-rule-color | 33 | — |
| `var()` basic substitution | 50 | — |
| `var()` nested (--a:var(--b)) | 50 | — |
| `var()` fallback (--undef,color) | 50 | — |
| `var()` in calc() | 50 | — |
| custom property inheritance | 50 | — |

---

## Не покрыто (намеренно или ограничения Phase 0)

- **background-color: currentColor, transparent** — нет отдельного теста (встречается в 06-border-sides через border: solid)
- **margin-right / margin-bottom** — только left и top в 09
- **margin collapse** — требует специфичного layout-aware теста
- **border-style: dashed/dotted/double** — покрыто в тесте 21
- **display: inline** — нет смысла без текста (нулевые размеры)
- **z-index** — требует `position: absolute/relative` с offset (Phase 0: offsets не применяются)
- **box-shadow: inset** — не реализовано в paint (требует clip)
- **border-radius: elliptical (rx≠ry)** — ✅ supported since 2026-05-24: `border-radius: H / V` shorthand + individual `border-*-*-radius: rx ry`
- **background-image** (url) — url images ✅; gradient rendering ✅ (linear + radial GPU pipeline, тест 39; conic gradient ✅ тест 40)
- **transform** — ✅ полностью реализован (translate/rotate/scale/skew/matrix + transform-origin), тест 22
- **filter** — ✅ реализован (grayscale/sepia/brightness/invert/contrast/saturate/opacity/hue-rotate + blur), тест 30
- **backdrop-filter** — ✅ реализован (blur/grayscale/brightness/invert/combo; Phase 0: требует parent stacking context), тест 30
- **clip-path** — ✅ реализован (inset/circle/ellipse/polygon/path() clip), тест 31

- **translate / rotate / scale** (individual CSS Transforms L2 props) — ✅ реализованы как отдельные свойства, compose перед transform в matrix (translate → rotate → scale → transform), тест 46

- **background-blend-mode** — ✅ реализован (normal/multiply/screen/overlay/darken/lighten/difference/exclusion/color-dodge/color-burn/hard-light/soft-light/hue/saturation/color/luminosity/plus-lighter; comma-list cycling over background layers; wraps each non-Normal layer with PushBlendMode/PopBlendMode), тест 49

- **mix-blend-mode** — ✅ реализован (все 16 CSS-режимов + plus-lighter; элемент блендится с backdrop в своём stacking-context через PushBlendMode/PopBlendMode; CPU snapshot-путь композитит off-screen layer вниз с tiny-skia BlendMode), тест 56

- **image-set()** — ✅ реализован (CSS Images L4 §5): raw функция хранится в BackgroundImage::Url, paint выбирает лучший вариант по DPR через select_image_set_url; -webkit-image-set() тоже поддержан; тест 59
- **cross-fade()** — ✅ реализован (CSS Images L4 §4): BackgroundImage::CrossFade { a, b, t } вариант; при двух URL-sides эмитирует DrawCrossFade; -webkit-cross-fade() тоже поддержан; тест 59

- **scroll-snap-type** — ✅ shell integration реализована (CSS Scroll Snap L1 §3.1): collect_snap_containers + find_snap_target подключены к shell scroll handler; page-level snap (y/x mandatory + proximity) применяется в start_smooth_scroll/scroll_x_by с корректным viewport snap-port; snap_containers кэшируется и обновляется после каждого layout; тест 62
- **scroll-snap-align** — ✅ shell integration реализована (CSS Scroll Snap L1 §6.1): start/end/center keyword alignment на обоих осях; тест 62
- **scroll-snap-stop** — ✅ shell integration реализована (CSS Scroll Snap L1 §6.2): always barrier корректно останавливает fling-scroll; тест 62

- **display: masonry** — 🟡 Phase 0 реализована (CSS Masonry Layout L1): lay_out_masonry() алгоритм в layout/src/masonry.rs; waterfall grid с column-count колонками, items размещаются в колонку с минимальной высотой; dispatch в box_tree.rs для Display::Masonry; gap поддержана через существующие CSS свойства; align-tracks/justify-tracks отложены на Phase 1; тест 63

- **::selection** — ✅ реализован (CSS Pseudo-elements L4 §5.6): compute_pseudo_element_style() обрабатывает 'selection' без требования content; compute_selection_style() публичная обёртка; SelectionHighlight struct в lumen-layout; build_display_list_with_selection() в lumen-paint эмитирует FillRect highlight + цветовой override для текста в пределах DOM Range; frag_selection_highlight() вычисляет пиксельные границы через byte-proportional аппроксимацию; тест 66

- **attr() typed** — ✅ реализован (CSS Values L4 §7.7): expand_attr_val() в style.rs раскрывает attr(<name> <type>?) до применения декларации; unit-типы (px/em/%) конкатенируются с числом атрибута; string/default — оборачиваются в CSS-кавычки для content-парсера; color/integer/number — raw-значение атрибута; fallback при отсутствии атрибута; тест 67

- **font-variation-settings** — ✅ реализован (CSS Fonts L4 §6.3): parse_font_variation_settings() в style.rs; field в ComputedStyle + cascade inheritance; OwnedVariableFont в lumen-paint хранит fvar axes + Hvar; MultiFontMeasurer::char_width_varied() применяет HVAR advance width deltas для variable fonts; measure_text_w_varied() в box_tree.rs используется при line wrapping; тест 68

- **object-fit** — ✅ реализован (CSS Images L3 §5.5): compute_object_fit_transform() в box_tree.rs; объекты Fill/Contain/Cover/None/ScaleDown применяются к SVG viewBox; object-position (x/y PositionComponent) управляет выравниванием контента в свободном пространстве; при Fill (default) сохраняется поведение SVG preserveAspectRatio; тест 70
- **object-position** — ✅ реализован (CSS Images L3 §5.5): PositionComponent::Px/Percent; resolve(free_space) → px offset; default 50% 50% (центр); парсинг keyword/length/percent; тест 70

- **:host** — ✅ реализован (CSS Scoping L1 §6.1): matches_pseudo_class() обрабатывает PseudoClass::Host(None) → doc.is_shadow_host(); PseudoClass::Host(Some(list)) → shadow host AND matches inner selector list; парсинг уже был в css-parser; тест 72
- **::slotted** — ✅ реализован (CSS Scoping L1 §6.2): matches_slotted_complex() проверяет is_slotted_element() (DOM parent is shadow host) AND inner selector match AND outer context against shadow host; declarations применяются в compute_style() cascade когда is_slotted_element(); тест 72

- **offset-path** — ✅ реализован (CSS Motion Path L1): resolve_motion_transform() в motion_path.rs; wiring в forward_box_transform() и PropertyTrees::walk() в property_trees.rs; offset-path: path("...") и ray(&lt;angle&gt;) хранятся в ComputedStyle.offset_path; offset-distance (px/%) разрешается через diagonal bbox как percent-basis; offset-rotate: auto следует тангенту, fixed angle — фиксированный; ray(): угол в deg/grad/rad/turn, 0deg=вверх по часовой, size/contain/at-position парсятся и игнорируются (px offset-distance их не требует); 11 unit-тестов + graphic test 76 + 99
- **offset-distance** — ✅ реализован (CSS Motion Path L1): parse + resolve в property_trees.rs; тест 76
- **offset-rotate** — ✅ реализован (CSS Motion Path L1): OffsetRotate::Auto/Reverse/Angle/AutoAngle; тест 76

- **anchor-name** — ✅ реализован (CSS Anchor Positioning L1 §2): ComputedStyle.anchor_name: Option<Box<str>>; парсинг --custom-ident в apply_declaration; collect_anchors_rec регистрирует элементы в AnchorRegistry; тест 77
- **position-anchor** — ✅ реализован (CSS Anchor Positioning L1 §3): ComputedStyle.position_anchor: Option<Box<str>>; парсинг в apply_declaration; apply_anchor_positions_rec читает для выбора anchor; тест 77
- **inset-area** — ✅ реализован (CSS Anchor Positioning L1 §5): ComputedStyle.inset_area_row/col: InsetAreaKeyword; parse_inset_area_keyword (9 ключевых слов + физические алиасы); resolve_inset_area() вычисляет top/left/width/height; post-layout pass apply_anchor_positions() в box_tree.rs; тест 77
- **position-area** — ✅ реализован (alias для inset-area, CSS Anchor Positioning L1): идентичный парсинг; тест 77
- **scroll-timeline-name** — ✅ реализован (CSS Scroll-Driven Animations L1 §3.1): ComputedStyle.scroll_timeline_name: Option<String>; парсинг в apply_declaration; collect_named_scroll_timelines() обходит layout tree; тест 78
- **scroll-timeline-axis** — ✅ реализован (CSS Scroll-Driven Animations L1 §3.2): ScrollAxis enum (Block/Inline/X/Y); парсинг keyword; тест 78
- **scroll-timeline** — ✅ реализован (CSS Scroll-Driven Animations L1): shorthand name+axis; тест 78
- **view-timeline-name** — ✅ реализован (CSS Scroll-Driven Animations L1 §3.3): ComputedStyle.view_timeline_name: Option<String>; collect_named_view_timelines() обходит дерево; тест 78
- **view-timeline-axis** — ✅ реализован (CSS Scroll-Driven Animations L1 §3.4): парсинг keyword; тест 78
- **view-timeline** — ✅ реализован (CSS Scroll-Driven Animations L1): shorthand name+axis; тест 78
- **animation-timeline** — ✅ реализован (CSS Scroll-Driven Animations L1 §3.3): AnimationTimeline enum (Auto/Scroll{axis,nearest}/View{axis}/Named); parse_animation_timeline_list() разбирает scroll()/view()/ident; тест 78
- **text-underline-offset** — ✅ реализован (CSS Text Decoration L4 §5.3): ComputedStyle.text_underline_offset: Option<f32>; None=auto; parse_length_px в apply_declaration; wired в push_text_decoration(); 5 unit-тестов; тест 79
- **text-underline-position** — ✅ подключён (CSS Text Decoration L3 §6.1/L4 §5.1): TextUnderlinePosition.Under → fs*0.25 вместо fs*0.10; wired в push_text_decoration(); тест 79
- **border-collapse** — ✅ реализован (CSS Tables L2 §17.6): ComputedStyle.border_collapse: BorderCollapse; separate (default)/collapse; в collapse режиме lay_out_table/compute_table_col_widths обнуляют border-spacing; TableContext::from_box() читает реальные CSS-значения; 5 unit-тестов; тест 80

- **view-transition-name** — ✅ реализован (CSS View Transitions L1 §10): ComputedStyle.view_transition_name: Option<Box<str>>; non-inherited; None=«none»; collect_view_transition_names() обходит layout tree и возвращает [(NodeId, name)] для shell; 5 unit-тестов style.rs + 4 unit-теста lib.rs; тест 81
- **text-decoration-skip-ink** — ✅ реализован (CSS Text Decoration L4 §3.5): TextDecorationSkipInk enum (Auto/All/None); ComputedStyle.text_decoration_skip_ink: inherited, initial Auto; apply_declaration("text-decoration-skip-ink"); char_has_ink_descender() для g/j/p/q/y/Q/J; emit_decoration_line_skip_ink() делит underline на сегменты с gap margin=thickness+1px; overline под `all`; line-through без skip; 6 unit-тестов style.rs + 4 unit-тестов paint; тест 84

- **relative color syntax** — ✅ реализован (CSS Color L5 §4): `rgb/hsl/oklch/oklab/lab/lch(from <origin> c1 c2 c3 [/ a])`; parse_relative_color() в style.rs резолвит channel keywords (r/g/b, h/s/l, l/c/h, l/a/b, alpha) из origin-цвета через color_mix::relative_origin_channels(); компоненты поддерживают число/процент/угол/`calc()` с арифметикой над каналами; результат реконструируется в обычную color-функцию и переразбирается; 7 unit-тестов style.rs; тест 91

- **color() предопределённые пространства** — ✅ реализован (CSS Color 4 §10): `color(srgb-linear|a98-rgb|prophoto-rgb|xyz|xyz-d65|xyz-d50 c1 c2 c3 [/ a])` в дополнение к ранее поддержанным `srgb`/`display-p3`/`rec2020`; displayable пространства хранятся как `ColorFloat` со своим `ColorSpace` (линейная точность для GPU), остальные гамут-маппятся в sRGB при разборе (`predefined_to_srgb_linear()` + `encode_srgb_f32()` в style.rs) и хранятся как `ColorFloat { space: Srgb }`; матрицы XYZ(D65)→sRGB и Bradford D50→D65 переиспользуют константы из `lab_to_srgb`; 6 unit-тестов style.rs; тест 96

- **system color keywords** — ✅ реализован (CSS Color 4 §6.2): `Canvas`, `CanvasText`, `Field`, `ButtonFace`, `ButtonBorder`, `ButtonText`, `LinkText`, `VisitedText`, `ActiveText`, `Highlight`, `HighlightText`, `GrayText`, `Mark`, `MarkText`, `AccentColor`, `AccentColorText`, `ThreeDHighlight`, `ThreeDShadow`, `Scrollbar` и ещё 4 алиаса; `SystemColor` Copy enum + `CssColor::System(SystemColor)`; parse в `parse_css_color_legacy()`; color-scheme pre-pass в `compute_style()` + `resolve_system_colors_in_style()` post-pass; `dark_mode: bool` параметр в `apply_declaration()`; 7 unit-тестов style.rs; тест 92

- **field-sizing** — ✅ реализован (CSS Basic UI L4 §4.4): `FieldSizing` enum (Fixed/Content); `ComputedStyle.field_sizing` non-inherited, initial Fixed; parse `field-sizing: content|fixed` в `apply_declaration()`; post-cascade pass `apply_ua_form_controls_field_sizing_clear()` снимает UA-ширину/высоту text-input/textarea; `field_sizing_content_intrinsic()` меряет padding-box по тексту value (input) / text content (textarea); wiring в `lay_out` для `BoxKind::FormControl`; `FormControlKind::Input/Textarea` несут `value_text`; 5 unit-тестов style.rs + 5 unit-тестов field_sizing.rs; тест 93

- **interpolate-size** — ✅ реализован (CSS Sizing L4 §4.5): `InterpolateSizeMode` enum (NumericOnly/AllowKeywords); `ComputedStyle.interpolate_size` **inherited**, initial NumericOnly; parse `interpolate-size: numeric-only|allow-keywords` в `apply_declaration()`; gate в `TransitionScheduler::sync()` — `height: auto` интерполируется только при `allow-keywords`, иначе keyword-размер дискретен (snap); 5 unit-тестов style.rs + 2 unit-теста animation.rs; тест 94
- **font-size-adjust** — ✅ реализован (CSS Fonts L5 §4): `FontSizeAdjust` enum (None/Auto/Value), **наследуемое**, initial None; parse в `apply_declaration()`; `TextMeasurer::x_height_px()` (real OS/2 `sxHeight` в `FontMeasurer`/`MultiFontMeasurer`, fallback 0.5·size); post-build pass `apply_font_size_adjust()` в box_tree.rs переписывает `style.font_size` боксов и inline-сегментов как `size·adjust/aspect` до measurement → и layout, и paint берут масштабированный размер из одного источника; 4 unit-теста box_tree.rs + 4 style.rs; тест 95

- **counter-set** — ✅ реализован (CSS Lists L3 §4): `ComputedStyle.counter_set: Vec<(String, i32)>` non-inherited; parse `counter-set: none | (<custom-ident> <integer>?)+` через `parse_counter_list(val, 0)` (default 0); `CounterCtx::apply_set()` в counters.rs устанавливает top-of-stack (создаёт счётчик на never-reset); порядок reset→increment→set нормативен — set перекрывает increment того же элемента; 6 unit-тестов lib.rs + 4 unit-теста counters.rs; тест 97

- **revert-layer** — ✅ реализован (CSS Cascade L5 §6.4.6): значение `revert-layer` откатывает свойство к значению, которое было бы без деклараций текущего каскадного слоя (та же important-группа). Резолвится pre-pass'ом над отсортированным каскадом в `compute_style()`: для каждого свойства, чей победитель = `revert-layer`, удаляются все его декларации из слоя-победителя, затем повтор (нижний слой тоже может содержать `revert-layer`); обычный last-wins loop затем даёт откатанное значение. НЕ является `CssWideKeyword` (зависит от слоя самой декларации). Ограничение: shorthand↔longhand откаты группируются по точному имени свойства; 5 unit-тестов style.rs; тест 98

### Форматы изображений и мультимедиа

- **AVIF (AV1 Image File Format)** — ✅ реализован (ISO/IEC 23008-12 Phase 0): lumen-image::avif модуль + AvifImageDecoder trait; is_avif() проверяет ISOBMFF ftyp-бокс major brand (avif/avis); decode_avif() использует libavif через `image` крейт feature `avif` (требует cmake+nasm); поддерживает статичные AVIF, анимированные распознаются но первый кадр; ICC-профили не извлекаются (Phase 1); 14 unit-тестов в avif/mod.rs; зарегистрирован в image-decoder dispatch + supported_mime_types(); тест 90

### Формы (form controls)

- **accent-color** — ✅ реализован (CSS UI L4 §6.1): `ComputedStyle.accent_color: Option<Color>` (**наследуемое**, `None` = `auto`); parse в `apply_declaration()` (`auto` → None, цвет → Some); wiring в `emit_form_control_indicator()` (paint/display_list.rs) — резолвит `accent-color` (UA-дефолт `ACCENT_DEFAULT` = синий `rgb(21,90,192)` при `auto`) и тинтит checked checkbox/radio-индикатор, залитую часть+thumb range-слайдера (`emit_range_slider`) и value-бар `<progress>` (`emit_progress_bar`); `<meter>` исключён — сохраняет семантические green/yellow/red цвета (HTML §4.10.14); 5 unit-тестов display_list.rs; тест 110
