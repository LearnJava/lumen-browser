# BUG-634 — `background-image` не рисуется ни одним бэкендом, хотя команда и файл на месте

**Статус:** OPEN
**Компонент:** paint (`renderer.rs` / `cpu_raster.rs`, обработка `DrawBackgroundImage`)
**Найден:** 2026-08-05 (P3, попутно к [BUG-277](BUG-277-OPEN.md) срез 12)
**Тест:** `graphic_tests/53-background-origin.html` (TEST-53, 7.95 %)

## Симптом

На TEST-53 шесть боксов задают

```css
background-image: url(../samples/images/perceptron.png);
background-size: 80px 60px;
background-repeat: no-repeat;
```

Edge рисует картинку в каждом боксе. Lumen не рисует её **ни в живом
wgpu-окне** (`graphic_tests/screenshots/53-…-lumen-cropped.png`), **ни в
детерминированном CPU-снимке** (`graphic_tests/snapshots/cpu/53-…png`) — в
обоих внутри рамки только сплошной `background-color`.

## Что уже установлено

Дефект **не** в загрузке ресурса и **не** в дисплей-листе:

```
$ lumen.exe --dump-display-list graphic_tests/53-background-origin.html
Загружена bg-картинка: ../samples/images/perceptron.png (852×725, Rgba8)
...
FillRect (33.00, 59.20, 240.00, 180.00) #2d3748ff
DrawBackgroundImage (33.00, 59.20, 240.00, 180.00) src="../samples/images/perceptron.png"
    size=Length(Px(80.0), Px(60.0)) pos=(Percent(0.0),Percent(0.0)) repeat=NoRepeat
DrawBorder (33.00, 59.20, 240.00, 180.00) …
```

Порядок команд правильный (заливка → картинка → рамка), файл прочитан и
декодирован (852×725, Rgba8), команд ровно шесть — по одной на бокс. Теряется
именно исполнение `DrawBackgroundImage`: оба бэкенда, судя по всему, не
находят картинку по ключу `src` в своём реестре изображений (у wgpu —
`register_image`/`ensure_image_gpu_key`) и молча пропускают команду.

Поскольку симптом одинаков у GPU- и CPU-пути, разойтись они могут только в
общем звене — регистрации/резолве `src` перед отрисовкой, а не в самих
растеризаторах.

## Почему это важно

TEST-53 числится в `KNOWN_DEBTORS` за BUG-277 («долг wgpu»), но wgpu тут ни
при чём: femtovg-базлайн страницы был 1.71 %, а картинки нет и на CPU-пути.
Запись стоит перецелить на этот баг, когда он будет разобран. Шесть картинок
80×60 — около 3.9 % площади вьюпорта, то есть половина текущего дифа
страницы.

## Проверка после фикса

`python graphic_tests/run.py --only 53` — картинка должна появиться в каждом
из шести боксов, привязанная к своему `background-origin`-боксу
(border/padding/content) в верхней строке и к его дальнему углу в нижней.
`background-origin` в дисплей-лист сейчас вообще не передаётся — проверить
заодно, что позиционирование считается от правильного бокса.
