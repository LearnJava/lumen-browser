# BUG-635 — `background-image` не рисуется ни одним бэкендом, хотя команда и файл на месте

**Статус:** FIXED 2026-08-05 (P3, [BUG-277](BUG-277-OPEN.md) срез 15)
**Компонент:** paint (`renderer.rs`, квады `DrawBackgroundImage` в wgpu)
**Найден:** 2026-08-05 (P3, попутно к [BUG-277](BUG-277-OPEN.md) срезу 12)
**Тест:** `graphic_tests/53-background-origin.html` (TEST-53, 7.95 % → 1.16 %)

## Симптом (как он был заявлен)

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

Из совпадения симптома на двух путях заявка вывела, что дефект сидит в общем
звене — резолве `src` в реестре изображений перед отрисовкой. Дисплей-лист при
этом был заведомо корректен:

```
$ lumen.exe --dump-display-list graphic_tests/53-background-origin.html
Загружена bg-картинка: ../samples/images/perceptron.png (852×725, Rgba8)
...
FillRect (33.00, 59.20, 240.00, 180.00) #2d3748ff
DrawBackgroundImage (33.00, 59.20, 240.00, 180.00) src="../samples/images/perceptron.png"
    size=Length(Px(80.0), Px(60.0)) pos=(Percent(0.0),Percent(0.0)) repeat=NoRepeat
DrawBorder (33.00, 59.20, 240.00, 180.00) …
```

## Что оказалось на самом деле

**Общего звена не существовало — оба наблюдения были артефактами разной
природы, и совпадение симптома оказалось случайным.**

### 1. wgpu картинку рисовал, но не там

Квады `DrawBackgroundImage` — один из двух image-путей wgpu-исполнителя
(второй — `DrawCrossFade`), которые не применяли к своим вершинам накопленную
матрицу `transform_stack`; `DrawImage`, `DrawLayerSnapshot` и все градиенты её
применяют. Под живым хромом содержимое страницы рисуется внутри
`PushTransform(0, CHROME_H)`, поэтому картинка ложилась ровно на
`CHROME_H = 69` px выше своего бокса: верхний ряд TEST-53 уезжал за верхнюю
кромку страницы (в кропе от него оставалась полоса), нижний вставал в середину
бокса вместо правого нижнего угла. На стоп-кадре 2026-07-15, по которому
писалась заявка, это читалось как «внутри рамки только background-color».

Замер по свежему кропу: белая кромка первой картинки заканчивается на строке
50 при ожидаемых 59.2…119.2 — сдвиг 69 px, то есть в точности `CHROME_H`.

Исправлено в BUG-277 срезе 15 (`renderer.rs`, `apply_affine_to_verts` для
квадов `DrawBackgroundImage`/`DrawCrossFade`). Kill-switch
`LUMEN_NO_IMG_XFORM=1` возвращает прежнее поведение и даёт A/B на одном
бинарнике: TEST-53 **7.95 % → 1.16 %**.

### 2. CPU-снимок пуст по устройству харнесса

`graphic_tests/snapshots/cpu/53-background-origin.png` генерируется
`crates/driver/tests/cases/snapshot_cpu.rs`, который **прямо документирует**,
что в нём не регистрируется декодер изображений и `DrawBackgroundImage` там
no-op («the `DrawBackgroundImage` commands are no-op on the CPU path since no
image decoder is registered»). Пустой снимок — ожидаемое поведение
снапшот-гейта, а не свидетельство о движке.

Настоящий CPU-путь картинку рисует и **без** этого фикса:

```
$ lumen.exe --screenshot .tmp/t53.png graphic_tests/53-background-origin.html
```

даёт 1.59 % расхождения с Edge-эталоном (попиксельно, порог |Δr|+|Δg|+|Δb|>60),
а пиксели внутри первого бокса содержат тело картинки, а не `background-color`.

## Урок

Два независимых пути, показавшие один симптом, не обязаны иметь общий корень:
здесь один путь был сломан, а второй просто не участвует в отрисовке картинок
по конструкции. Прежде чем выводить «общее звено», стоит проверить, что оба
наблюдения вообще измеряют одно и то же — здесь достаточно было
`lumen --screenshot` вместо закоммиченного снапшота.

## Проверка

`python graphic_tests/run.py --only 53` — картинка стоит в каждом из шести
боксов, привязанная к своему `background-origin`-боксу (border/padding/content)
в верхней строке и к его дальнему углу в нижней. Остаток 1.16 % — font-parity
меток и ресэмплинг картинки; это **ниже** headless-CPU числа той же страницы
(1.59 %), то есть расхождения бэкендов на TEST-53 больше нет.
