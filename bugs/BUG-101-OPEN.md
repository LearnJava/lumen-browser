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
