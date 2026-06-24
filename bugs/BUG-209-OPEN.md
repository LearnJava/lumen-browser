# BUG-209

**Статус:** OPEN (DEBTOR)
**Компонент:** layout (бывш. image)
**Тест:** TEST-90 (diff 2.27% → 1.71%, KNOWN_DEBTOR)

## Описание

TEST-90 «AVIF Image Display»: `<picture>` с AVIF source + PNG fallback и прямой
`<img src=".avif">`. Изначально классифицирован как «AVIF decoder not implemented»,
но фактическая причина девиации оказалась в layout, а не в декодере.

AVIF-данные в самом тесте **обрезаны** (≈39 байт ftyp-заголовка без полезной
нагрузки) — ни Lumen, ни Edge их не декодируют. Edge для невалидной картинки рисует
placeholder сломанного изображения (иконка + alt-текст), Lumen — нет.

## Расследование

Доминирующий вклад в diff давал **layout-баг**, не декодер: вложенный column-flex
item (`.cell-item`) внутри row-flex ряда (`.cell`), который сам — flex-item внешней
колонки, схлопывался по контенту (~40px) вместо растяжения на cross-size ряда
(~349px). Из-за этого рамки и фоны ячеек рисовались короткими полосками сверху, а не
во всю высоту — это и есть основная разница с Edge.

Причина: в `lay_out_flex` re-layout column-flex после предварительного
indefinite-прохода не гейтился на *definite* cross-size. При `explicit_cross=None`
эффективный cross падал на `line_cross` (no-op stretch), но re-layout всё равно
записывал резолвнутую px `style.height` обратно в item, затирая `height:auto`;
последующий проход с definite cross видел `is.height.is_some()` и пропускал
настоящий stretch.

## Исправление

Исправлено в **[BUG-241](BUG-241-FIXED.md)** (FIXED 2026-06-23): добавлен гейт
`&& explicit_cross.is_some()` в ветку `relayout_column_flex`
(`crates/engine/layout/src/box_tree.rs`). Регресс-тест
`flex_nested_stretch_after_indefinite_pass_fills_row`.

TEST-90: 2.27% → 1.71%. Рамки/фоны ячеек теперь совпадают с Edge пиксель-в-пиксель
(diff чёрный по всем границам).

## Остаток (DEBTOR)

1.71% — это placeholder сломанной картинки, который Edge рисует для невалидного AVIF
(иконка + alt-текст), а Lumen — нет. Браузерная chrome-иконка пиксельно с Edge не
совпала бы даже при реализации. Плюс вытекающий вертикальный сдвиг подписи и
font-parity меток (rule 3). → `KNOWN_DEBTORS['90'] = ('BUG-209', 1.71)` в `run.py`.

## Воспроизведение

`python graphic_tests/run.py --only 90` → 1.71% (KNOWN_DEBTOR, в пределах храповика).
