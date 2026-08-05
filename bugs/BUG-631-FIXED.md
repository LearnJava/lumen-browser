# BUG-631 — градиентный фон не обрезается по `border-radius` (все бэкенды)

**Статус:** FIXED 2026-08-05
**Компонент:** paint (`display_list.rs::emit_background_image`) — дефект дисплей-листа, не бэкенда
**Найден:** 2026-08-05, P3, при разборе остатка TEST-104 (BUG-277 срез 6)

## Симптом

Бокс со скруглением и градиентным фоном рисуется с **квадратными углами**:
градиент заливает всю коробку целиком, скругление игнорируется. Сплошной
(`background: <color>`) фон того же бокса скругляется правильно.

Воспроизведение — `graphic_tests/104-mask-gradient-radius.html`, ячейка `c4`
(«CONTROL», `background: linear-gradient(...)` + `border-radius: 40px`):

```
пиксель (368, 378) — внутри угла, который скругление обязано вырезать
  Edge:  (26, 32, 44)  — фон страницы
  Lumen: (229, 64, 62) — цвет градиента
```

## Не зависит от бэкенда

Проверено headless-CPU-путём (`lumen --screenshot`, `cpu_raster`) — угол так же
залит градиентом. Дефект в дисплей-листе, а не в исполнителе:

```
$ lumen --dump-display-list graphic_tests/104-mask-gradient-radius.html
DrawLinearGradient (365.00, 375.00, 300.00, 300.00) angle=180.0deg stops=2 repeating=false
FillRoundedRect   (705.00, 375.00, 300.00, 300.00) #2f855aff r=[30.00,30.00,30.00,30.00]
```

Скруглённый бокс со сплошным фоном получает `FillRoundedRect` с радиусами;
градиентный — голый `DrawLinearGradient` без радиусов и без скруглённого клипа.

## Корень

`display_list.rs::emit_background_image`, ветки
`BackgroundImage::Gradient(ParsedGradient::{Linear,Radial,Conic})`: они берут
`gradient_paint_rects(layer, origin, clip)` и при `needs_clip` оборачивают
рисование в **`PushClipRect`** — прямоугольный клип. `border-radius` бокса в эту
ветку не доходит вовсе: ни отдельной команды с радиусами, ни
`PushClipRoundedRect` не эмитится.

CSS Backgrounds L3 §4.3: фон обрезается по `background-clip`-боксу, а его углы
скруглены по `border-radius` — это относится к любому `background-image`,
включая градиенты, а не только к `background-color`.

## Фикс (2026-08-05, P3)

`emit_background_image` теперь считает `radii = CornerRadii::from_style_and_box(&b.style, b.rect.width, b.rect.height)`
один раз (те же border-box радиусы, что уже использует соседний
`background-color`-путь на этом же боксе) и передаёт их в `emit_background_layer`.
Все три ветки градиента (`Linear`/`Radial`/`Conic`) эмитят
`PushClipRoundedRect { rect: clip, radii: [tl, tr, br, bl] }` вместо
`PushClipRect { rect: clip }`, когда радиусы ненулевые — `PushClipRoundedRect`
несёт по одному радиусу на угол (`[f32; 4]`), а не отдельные x/y, как
`CornerRadii`, поэтому эллиптические углы здесь так же приближаются к
круглым, как и в существующем `overflow:hidden`-пути (BUG-132). Условие
клипа расширено с `needs_clip` на `needs_clip || has_radii` — раньше при
`needs_clip == false` (одна команда на всю painting area, auto/cover/contain
размер) клип не эмитился вовсе, и скруглённый бокс без тайлинга тоже тёк в
углы. Клип по скруглению уже умели все три бэкенда: `cpu_raster` (BUG-249),
femtovg и wgpu (BUG-277 срез 5 — offscreen + SDF-маска) — фикс только в
дисплей-листе.

Регресс-тест `background_image_linear_gradient_with_border_radius_clips_rounded`
(`crates/engine/paint/src/display_list.rs`) проверяет, что градиент на
скруглённом боксе эмитит парный `PushClipRoundedRect`/`PopClip` с верными
радиусами. `graphic_tests/snapshots/cpu/104-mask-gradient-radius.png`
регенерирован (единственная страница корпуса с затронутой геометрией).

## Влияние

Пиксель (368, 378) на `graphic_tests/104-mask-gradient-radius.html`:
`(229, 64, 62)` (цвет градиента) → `(26, 32, 44)` (фон страницы, как у Edge).
TEST-104: **0.33% → 0.05%** (PASS). Остаток TEST-104 после BUG-277 среза 6 был
**0.33 %** (две ячейки из шести с градиентным фоном под `border-radius`; их
квадратные углы были всем остатком). Порог 0.5 % тест проходил и до фикса, то
есть графический гейт этот дефект не ловил — но он был живым и виден на любой
странице с «карточкой» из градиента со скруглением. Полный прогон 152 тестов
чанками (`--only`) — 97 PASS · 52 DEBTOR · 3 FAIL (TEST-147/150/151, все
пред-существующие — BUG-330/font-variant-caps/unicode-bidi), регрессий нет.

## Связанные

- [BUG-277](BUG-277-FIXED.md) — срез 6 нашёл этот дефект как остаток TEST-104
- [BUG-249](BUG-249-FIXED.md) — скруглённый клип на CPU-пути
