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
