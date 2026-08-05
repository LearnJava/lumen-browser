# BUG-631 — градиентный фон не обрезается по `border-radius` (все бэкенды)

**Статус:** OPEN
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

## Как чинить (предположительно)

`PushClipRoundedRect { rect: clip, radii }` вместо `PushClipRect { rect: clip }`,
когда радиусы бокса ненулевые. Клип по скруглению уже умеют все три пути:
`cpu_raster` (BUG-249), femtovg и wgpu (BUG-277 срез 5 — offscreen + SDF-маска).
Учесть: при `needs_clip == false` клип сейчас не эмитится вовсе — со скруглением
он нужен и в этом случае.

## Влияние

Остаток TEST-104 после BUG-277 среза 6 — **0.33 %** (две ячейки из шести с
градиентным фоном под `border-radius`; их квадратные углы и есть весь остаток).
Порог 0.5 % тест проходит, то есть графический гейт этот дефект не ловит — но он
живой и виден на любой странице с «карточкой» из градиента со скруглением.

## Связанные

- [BUG-277](BUG-277-OPEN.md) — срез 6 нашёл этот дефект как остаток TEST-104
- [BUG-249](BUG-249-FIXED.md) — скруглённый клип на CPU-пути
