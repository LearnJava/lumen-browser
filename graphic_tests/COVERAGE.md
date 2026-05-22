# Покрытие графических тестов

Все тесты — только графика (0 текста). Шум = ненулевые пиксели в diff с Edge, не связанные с тестируемым свойством.

Viewport: 1024×720. Body padding: 24px (где есть). Gap между объектами: 16px.

**Маркер.** Каждый тест начинается с 1-px магента-полоски (`#ff00ff`) шириной 1024 px как первый ребёнок body. Используется workflow для динамического определения crop offset. Из-за маркера весь существующий контент сдвинут вниз на 1 px (одинаково в Edge и Lumen — diff остаётся валидным).

**Пайплайн блокирующий.** Тест 00-calibration должен пройти первым (магента-маркеры найдены и совпадают), затем 01-sanity, потом по нумерации. Первый провал — остановка пайплайна, последующие тесты не выполняются.

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
| 18-images.html | Растровые изображения | 12 | \<img\> PNG/JPEG · CSS width/height · transparent PNG on colored bg |
| 19-object-fit.html | Вписывание изображения | 9 | object-fit: fill/contain/cover/none/scale-down · object-position |
| 20-quirks-bgcolor.html | Устаревший bgcolor (Quirks mode) | 15 | CSS hashless hex · bgcolor attr on \<td\> · legacy color parsing |
| 21-border-style.html | Стили border: dashed/dotted/double | 16 | border-style: dashed · dotted · double (2/4/8/16px) · per-side mix · double thin fallback |
| 22-transform.html | CSS transform | 30 | transform: translate · translateX/Y · rotate · scale · scaleX/Y · skewX · skewY · matrix() · combined · transform-origin (4 variants) |
| 23-pseudo-elements.html | ::before / ::after block + inline generation | 7 | ::before display:block · ::after display:block · both on one element · ::before с другой шириной · ::before inline (padding box) · ::after inline · both inline |
| 24-vertical-align.html | vertical-align | 6 | inline-block: top/middle/bottom · inline span: super/middle/sub (frag y-offset + bg) |
| 25-table-layout.html | Table layout | 19 | display:table — горизонтальный layout ячеек · auto-width distribution · явные ширины · несколько строк (вертикальное стакование) |
| 26-mask-image.html | mask-image | 3 | linear-gradient mask · radial-gradient mask · control (no mask). Phase 0: gradient masks fallback to full-opacity |
| 27-direction-rtl.html | direction | 6 | LTR start (left) · RTL start (right) · RTL end (left) · alignment gradient bands |
| 28-css-containment.html | contain | 5 | baseline (no contain) · contain:size (height=0) · contain:paint (overflow clip) · contain:layout · contain:strict |
| 29-container-queries.html | @container | 4 | wide container: min-width applies (blue) · narrow: not applies (red) · named container · max-width |
| 30-css-filter.html | CSS filter | 14 | grayscale(1) · sepia(1) · brightness(2) · invert(1) · contrast(3) · saturate(3) · opacity(0.4) · blur(8px) · hue-rotate(90deg/180deg) |
| 31-clip-path.html | clip-path | 9 | inset(1/4-value) · circle(r/at) · ellipse(rx ry/at) · polygon(triangle/rect bbox) · clip-path + overflow:hidden |
| 32-list-markers.html | list markers | 14 | display:list-item · ::marker box · list-style-type: disc/circle/square/decimal/lower-alpha/lower-roman · list-style-position: outside/inside · list-style-type:none (Порог 6%: маркеры — текст, антиалиасинг расходится с Edge) |
| 33-multi-column.html | multi-column layout + column-rule | 7 | column-count:2/3/4/5 · column-width · column-gap · column-rule: solid/dashed/dotted · rule centered in gap · rule wider than gap (clamped) |
| 34-forms.html | form controls static rendering | 18 | input[text/email/password/number/search/range/color/submit] · checkbox (unchecked/checked/disabled) · radio (unchecked/checked) · button · textarea · select · required · disabled UA styles |
| **1000000-final.html** ★ | **ФИНАЛЬНЫЙ ТЕСТ — все свойства в одном окне** | ~66 | **Ручная проверка, не для автодиффа.** Обновляется при каждом новом CSS-свойстве. background-color (все нотации) · border (width/color/per-side/currentColor/dashed/dotted/double) · border-radius (Phase 0: квадрат в Lumen, скруглён в Edge) · box-shadow (hard/blur/spread) · outline (width/offset+/-) · overflow (visible/hidden) · opacity · visibility:hidden · object-fit (5 режимов) · calc/min/clamp · padding layering · transform (translate/rotate/scale) · table layout (2×4 ячейки) |

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
| z-index / stacking order | — | ❌ требует position:absolute (не работает в Phase 0) |
| calc() / min() / max() / clamp() | 17 | — |
| sqrt() / cos() / abs() / hypot() | 17 | sin() не тестируется отдельно |
| \<img\> PNG / JPEG | 18 | — |
| \<img\> масштаб CSS width/height | 18 | — |
| transparent PNG на цветном фоне | 18 | — |
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

---

## Не покрыто (намеренно или ограничения Phase 0)

- **background-color: currentColor, transparent** — нет отдельного теста (встречается в 06-border-sides через border: solid)
- **margin-right / margin-bottom** — только left и top в 09
- **margin collapse** — требует специфичного layout-aware теста
- **border-style: dashed/dotted/double** — покрыто в тесте 21
- **display: inline** — нет смысла без текста (нулевые размеры)
- **z-index** — требует `position: absolute/relative` с offset (Phase 0: offsets не применяются)
- **box-shadow: inset** — не реализовано в paint (требует clip)
- **border-radius** — парсируется, но углы остаются прямыми (Phase 0)
- **background-image** (gradient, url) — parse only (Phase 0)
- **transform** — ✅ полностью реализован (translate/rotate/scale/skew/matrix + transform-origin), тест 22
- **filter** — ✅ реализован (grayscale/sepia/brightness/invert/contrast/saturate/opacity/hue-rotate + blur), тест 30
- **clip-path** — ✅ реализован (inset/circle/ellipse/polygon bounding-box clip), тест 31
