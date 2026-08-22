# BUG-101

**Статус:** OPEN
**Компонент:** css-parser/paint
**Файл:** `crates/engine/css-parser/src/lib.rs`

## Описание

image-set() DPR selection / cross-fade() blend not implemented — TEST-59: 27.63%; CSS Images L4 §5/§4

## Ревизия 2026-06-23 (P1, при CPU-паритете background-image)

Описание устарело: `image-set()` DPR-выбор (`select_image_set_url`) и `cross-fade()`
эмит (`DrawCrossFade`) **уже реализованы** — резолвятся в `display_list.rs` и рисуются
femtovg-бэкендом. Попутно закрыт CPU-бэкенд (`cpu_raster.rs` рисовал обе команды пусто;
теперь паритет с femtovg — см. subsystems/paint.md).

Реальная причина остаточного расхождения TEST-59 (из Edge-эталона
`screenshots/59-image-set-cross-fade-edge.png`):
1. **Unprefixed `cross-fade(url, url, 30%)`**: Edge оставляет ячейки `cross-fade() 30%/70%`
   **пустыми** — это устаревший 3-аргументный webkit-синтаксис без префикса, невалидный по
   CSS Images L4 (валидно `cross-fade(<image> <percent>?, …)`). Lumen же эмитит `DrawCrossFade`
   для обоих `cross-fade()` и `-webkit-cross-fade()` (различие префикса теряется в css-parser →
   оба становятся `BackgroundImage::CrossFade`). Для паритета css-parser должен отвергать
   unprefixed webkit-форму как невалидную (P4/parser-грамматика, не backend).
2. **Font-parity** меток (rule 3), неустранимо.

Остаток — кандидат в KNOWN_DEBTORS после правки парсера cross-fade. Row-1 image-set/url ячейки
и `-webkit-cross-fade()` совпадают с Edge.

## Ревизия 2026-06-23 (P1, фикс грамматики cross-fade → KNOWN_DEBTOR)

Грамматика `cross-fade()` приведена к CSS Images L4 §4 в `crates/engine/layout/src/style.rs`
(`parse_cross_fade` разделён на префикс-зависимые ветки):

* **`-webkit-cross-fade(<from>, <to>, <percentage>)`** — устаревшая 3-аргументная форма,
  принимается только с `-webkit-` префиксом (`parse_webkit_cross_fade`).
* **`cross-fade( [<percentage>? && <image>]# )`** — стандартная L4-форма, 2-image
  (`parse_l4_cross_fade` + `parse_cf_image`): каждый аргумент = изображение с
  необязательным процентом непрозрачности; голый `<percentage>` без изображения невалиден.
* **Unprefixed 3-арг `cross-fade(url, url, 30%)`** теперь отвергается (`None`) — висячий
  bare `<percentage>` не является `<image>`. Совпадает с Edge/Chromium: декларация
  отбрасывается, ячейка остаётся пустой.

Проверка: TEST-59 24.18% → 17.15% (gdigrab). Центр ячеек cf-30/cf-70 = фон `#1a202c`
в обоих движках (CPU-снимок); `-webkit-cross-fade()`-ячейка по-прежнему рисуется.
4 unit-теста в `style.rs` (включая `cross_fade_unprefixed_legacy_three_arg_rejected`).

Остаток 17.15% = ресэмплинг фото-картинок (image-set/url ячейки row1 + webkit cross-fade
row2, класс BUG-219) + font-parity monospace-меток (rule 3). **KNOWN_DEBTOR** (`run.py`
`KNOWN_DEBTORS['59'] = ('BUG-101', 17.15)`). Парсер-дефект cross-fade закрыт.

## Ревизия 2026-07-29 (P3, закрытие: источники не грузились + неверный композит)

Прежний остаток («ресэмплинг фото-картинок + font-parity меток») оказался
неверен. Разложение диффа по ячейкам (headless CPU vs
`screenshots/59-image-set-cross-fade-edge.png`) дало картину, несовместимую с
«остаточным AA»: ячейка `-webkit-cross-fade()` расходилась на **99.9%** при
`std = 0` — то есть у нас там был чистый фон `#1a202c`, а не смазанная картинка.

### Дефект 1 — источники `image-set()`/`cross-fade()` не попадали в загрузчик

`collect_background_image_requests` (`box_tree.rs`) собирала только
`BackgroundImage::Url`, причём **дословно**:

* `image-set(…)` хранится в слое как есть, а в display list эмиттер кладёт уже
  выбранного кандидата. Shell уходил качать текст функции как имя файла — в
  `--dump-display-list` это три подряд `Синтаксическая ошибка в имени файла …
  (os error 123)`, и по ключу-кандидату в `image_map` не оказывалось ничего.
* `BackgroundImage::CrossFade` не разбиралась вовсе — ни одна из двух сторон.

На TEST-59 это маскировалось тем, что `agi_illustration.png` и `perceptron.png`
объявлены рядом обычным `url()` и потому грузились. `sad_brain.png` встречается
только внутри `image-set()`-шортхенда и `-webkit-cross-fade()` — и не грузился
ни разу.

Фикс: `push_bg_image_urls` разворачивает `image-set()` через
`image_set::select_image_set_url` и рекурсивно обходит обе стороны
`cross-fade()` (сторона сама может быть `image-set()`).
`collect_background_image_requests` получила явный параметр `dpr`: коллектор и
`build_display_list_ordered_dpr` обязаны выбирать **одного и того же**
кандидата, иначе ключ загрузки не совпадёт с ключом поиска и картинка молча не
нарисуется. Реализаций разрешителя `image-set()` две (`lumen-layout::image_set`
и `display_list.rs`); их согласованность закреплена тестом
`image_set_resolver_agrees_with_layout_collector` в `lumen-paint` (жить он может
только там — зависимость идёт layout → paint).

### Дефект 2 — композит `cross-fade()` на CPU и femtovg

Обе реализации рисовали `a` с альфой `1−p`, затем `b` поверх с альфой `p`.
Source-over от этого даёт

```
p·b + (1−p)²·a + p(1−p)·фон
```

— вес `a` в квадрате плюс просвет подложки. Визуально: блёклое и затемнённое
изображение (замер ячейки: `mean=[157,149,154] std=[24,15,15]` против
`[180,160,164] std=[47,30,29]` у Edge). Дефолтный бэкенд wgpu считал верно —
шейдер делает `mix(a, b, t)`; CPU и femtovg приведены к нему: `a` кладётся при
альфе 1.0, `b` композитится сверху при `p`.

Существовавший тест `draw_cross_fade_respects_progress` проверял только `p=0` и
`p=1` — ровно те две точки, где дефект вырождается (при `p=0` «альфа `1−p`» = 1
случайно совпадает с верным ответом, при `p=1` `b` перекрывает всё). Добавлен
`draw_cross_fade_midpoint_is_even_mix`; на старом коде он падает
(`got (128,192,64)` вместо равных долей), на новом проходит.

### Результат

Ячейка `-webkit-cross-fade()`: 99.9% → 26.9% диффа к Edge, и — что показательнее
порога — `mean/std` сравнялись с эталоном (`[175,158,161] std=[46,28,27]` против
`[180,160,164] std=[47,30,29]`); визуально смесь совпадает. `sad_brain.png`
грузится, `os error 123` из лога ушли.

Остаток TEST-59 к BUG-101 не относится:

* **[BUG-433](BUG-433-FIXED.md)** (закрыт 2026-07-29) — основной вклад: ряд 1 сжимается ниже
  автоматического минимума flex-элементов (CSS Flexbox §4.5), шаг 198 вместо
  236, из-за чего сдвинуты все ячейки и подписи ряда;
* ресэмплинг фото при `background-size: cover` (класс BUG-219);
* font-parity monospace-меток (BUG-128).

Гейт `cases::snapshot_cpu` этот фикс на момент починки не видел и эталон `59-*.png` не
менялся: headless-путь `InProcessSession` подресурсы не качал вовсе (BUG-430), там ячейки
были пустые в обоих направлениях. Проверка шла через `--screenshot`, который грузит
подресурсы шеллом. С закрытием [BUG-430](BUG-430-FIXED.md) (2026-08-22) эталон `59-*.png`
перегенерирован и ячейки `image-set()`/`-webkit-cross-fade()` показывают реальные пиксели;
ячейки `cross-fade() 30%`/`70%` остаются пустыми — беспрефиксная 3-аргументная форма
невалидна и отбрасывается, как в Edge.
