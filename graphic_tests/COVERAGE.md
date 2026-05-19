# Покрытие графических тестов

Все тесты — только графика (0 текста). Шум = ненулевые пиксели в diff с Edge, не связанные с тестируемым свойством.

Viewport: 1024×720. Body padding: 24px. Gap между объектами: 16px.

---

## Файлы

| Файл | Тема | Объектов | Покрываемые свойства |
|---|---|---|---|
| 00-sanity.html | Один белый квадрат на чёрном фоне | 1 | background-color, width/height, margin |
| 01-color-named.html | Именованные цвета CSS | 18 | background-color: named (red/blue/green/yellow/orange/purple/tomato/teal/coral/crimson/rebeccapurple/dodgerblue/hotpink/mediumseagreen/steelblue/goldenrod/slategray/indianred) |
| 02-color-formats.html | Нотации цвета | 9 | background-color: named · #RGB · #RRGGBB · #RGBA · #RRGGBBAA · rgb() · rgba() · hsl() · hsla() |
| 03-color-alpha.html | Прозрачность: rgba / hsla / #RRGGBBAA | 18 | background-color: alpha от 0.1 до 1.0 (red + blue + green + purple) |
| 04-border-width.html | Толщина бордера | 10 | border-width: 1/2/4/8/16px; асимметричные per-side widths |
| 05-border-sides.html | Стороны и цвета бордера | 9 | border-top/right/bottom/left; per-side colors; currentColor; border+padding layering |
| 06-box-sizing.html | content-box vs border-box | 8 | box-sizing; padding; border (наглядная разница размера) |
| 07-padding.html | Отступы внутри блока | 9 | padding: uniform · asymmetric (TB/LR) · 4-value · 0 |
| 08-margin.html | Отступы снаружи блока | 11 | margin-left (staircase) · margin-top (gap stepping) |
| 09-min-max-width.html | Ограничения по ширине | 12 | min-width · max-width · min>max edge case · calc · min() · clamp() |
| 10-min-max-height.html | Ограничения по высоте | 12 | min-height · max-height · min>max edge case |
| 11-display.html | Значения display | 17 | display: block · inline-block · none · vertical-align: top |
| 12-visibility-opacity.html | Видимость и прозрачность | 16 | visibility: hidden (space reserved) · opacity: 0.1–1.0 · opacity на group |
| 13-overflow.html | Обрезка содержимого | 4 | overflow: visible · hidden · overflow-x/overflow-y раздельно |
| 14-box-shadow.html | Тени блоков | 8 | box-shadow: offset · blur · spread · color · multiple · negative offset |
| 15-outline.html | Обводка снаружи | 9 | outline-width · outline-color · outline-offset (positive / negative) · layout не сдвигается |
| 16-calc.html | CSS math | 14 | calc() · min() · max() · clamp() · sqrt() · cos() · abs() · hypot() · nested |
| 17-images.html | Растровые изображения | 12 | \<img\> PNG/JPEG · CSS width/height · transparent PNG on colored bg |
| 18-object-fit.html | Вписывание изображения | 9 | object-fit: fill/contain/cover/none/scale-down · object-position |
| 19-quirks-bgcolor.html | Устаревший bgcolor (Quirks mode) | 15 | CSS hashless hex · bgcolor attr on \<td\> · legacy color parsing |
| **1000000-final.html** ★ | **ФИНАЛЬНЫЙ ТЕСТ — все свойства в одном окне** | ~50 | **Ручная проверка, не для автодиффа.** Обновляется при каждом новом CSS-свойстве. background-color (все нотации) · border (width/color/per-side/currentColor) · border-radius (Phase 0: квадрат в Lumen, скруглён в Edge) · box-shadow (hard/blur/spread) · outline (width/offset+/-) · overflow (visible/hidden) · opacity · visibility:hidden · object-fit (5 режимов) · calc/min/clamp · padding layering |

---

## Свойства → покрытие

| Свойство | Файл(ы) | Непокрытые аспекты |
|---|---|---|
| background-color — named | 01 | — |
| background-color — hex (#RGB/#RRGGBB/#RGBA/#RRGGBBAA) | 02, 03 | — |
| background-color — rgb()/rgba() | 02, 03 | — |
| background-color — hsl()/hsla() | 02, 03 | — |
| background-color — currentColor | — | ❌ нет отдельного теста |
| background-color — transparent | — | ❌ нет отдельного теста |
| border-width (1/2/4/8/16px) | 04 | — |
| border-width асимметричный per-side | 04 | — |
| border-color per-side | 05 | — |
| border-color currentColor | 05 | — |
| border-style: solid | 04, 05 | — |
| border-style: dashed/dotted/double | — | ❌ если реализованы — не тестируются |
| box-sizing: content-box | 06 | — |
| box-sizing: border-box | 06 | — |
| padding (4-value, asymmetric) | 07 | — |
| margin-left | 08 | — |
| margin-top | 08 | — |
| margin-right / margin-bottom | — | ❌ нет отдельного теста |
| margin collapse (вертикальный) | — | ❌ нет теста |
| width / height | 00, 06, 07, 08, 09 | — |
| min-width / max-width | 09 | — |
| min-height / max-height | 10 | — |
| display: block | 11 | — |
| display: inline-block | 11 | — |
| display: none | 11 | — |
| display: inline | — | ❌ без текста протестировать невозможно |
| visibility: hidden | 12 | — |
| opacity | 12 | — |
| overflow: hidden | 13 | — |
| overflow: visible | 13 | — |
| overflow-x / overflow-y раздельно | 13 | — |
| box-shadow (offset/blur/spread/color) | 14 | inset-тень (не реализована в paint) |
| outline (width/color/offset) | 15 | — |
| z-index / stacking order | — | ❌ требует position:absolute (не работает в Phase 0) |
| calc() / min() / max() / clamp() | 16 | — |
| sqrt() / cos() / abs() / hypot() | 16 | sin() не тестируется отдельно |
| \<img\> PNG / JPEG | 17 | — |
| \<img\> масштаб CSS width/height | 17 | — |
| transparent PNG на цветном фоне | 17 | — |
| object-fit (5 режимов) | 18 | — |
| object-position | 18 | — |
| legacy bgcolor / hashless hex | 19 | — |

---

## Не покрыто (намеренно или ограничения Phase 0)

- **background-color: currentColor, transparent** — нет отдельного теста (встречается в 05-border-sides через border: solid)
- **margin-right / margin-bottom** — только left и top в 08
- **margin collapse** — требует специфичного layout-aware теста
- **border-style: dashed/dotted/double** — добавить если/когда будет реализовано
- **display: inline** — нет смысла без текста (нулевые размеры)
- **z-index** — требует `position: absolute/relative` с offset (Phase 0: offsets не применяются)
- **box-shadow: inset** — не реализовано в paint (требует clip)
- **border-radius** — парсируется, но углы остаются прямыми (Phase 0)
- **background-image** (gradient, url) — parse only (Phase 0)
- **transform / filter / clip-path** — parse only (Phase 0)
