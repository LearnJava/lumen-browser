# BUG-218

**Статус:** OPEN
**Компонент:** css-parser / paint (домен P4 — CSS property)
**Тест:** TEST-26 (остаток 5.02% после BUG-183)

## Описание

`mask-mode: luminance` не парсится и не применяется. Свойство `mask-mode`
есть в `SUPPORTED_PROPERTIES` (css-parser), но не имеет поля в `ComputedStyle`
и ветки в `apply_declaration`, поэтому маска всегда обрабатывается как
`mask-mode: alpha` (значение по умолчанию).

## Воспроизведение

`python graphic_tests/run.py --only 26` → FAIL 5.02%. Расходится единственная
ячейка row2 (x≈271, mask-mode-luma): `mask-image: linear-gradient(to right,
black, white)` + `mask-mode: luminance`. Edge гасит левую (тёмную, luma≈0)
половину; Lumen показывает бокс целиком (оба стопа непрозрачные → alpha-маска
= 1).

## Как чинить (P4 + paint)

1. P4: добавить `mask_mode: MaskMode` в `ComputedStyle` + ветку
   `"mask-mode"` в `apply_declaration` (alpha | luminance).
2. Протянуть `MaskMode` в gradient mask-команды дисплей-листа
   (`PushMaskLinearGradient`/`Radial`/`Conic` — сейчас без поля `mode`) из
   `emit_push_mask` (`display_list.rs`).
3. paint (femtovg `composite_mask_layer`, cpu_raster `MaskSpec`): при
   `Luminance` строить альфу стопов как `luma(rgb)·a`
   (`0.2126·R+0.7152·G+0.0722·B`) перед `DestinationIn` — DestinationIn
   умножает на alpha источника, поэтому luma кодируется в alpha стопа.

После фикса убрать TEST-26 из `KNOWN_DEBTORS`.
