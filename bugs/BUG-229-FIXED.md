# BUG-229

**Статус:** FIXED 2026-06-21
**Компонент:** image (PNG decoder)
**Тест:** `lumen-image` integration `icc_color_management::display_p3_png_colour_managed_to_srgb`

## Описание

`parse_png_icc_profile` (`crates/engine/image/src/png/mod.rs`) распаковывал тело
`iCCP`-чанка сырым `flate2::read::DeflateDecoder`. Но PNG-спецификация (`iCCP`,
compression method 0) хранит профиль как **zlib**-датастрим (RFC 1950: 2-байтный
заголовок + DEFLATE + Adler-32), а не как raw DEFLATE. `DeflateDecoder` видит
zlib-заголовок (`0x78 0x9C…`) как мусорный DEFLATE-блок → `read_to_end`
завершается ошибкой → функция тихо возвращает `None`.

Эффект: **ни один реальный PNG с ICC-профилем не управлялся по цвету** — профиль
терялся на этапе декода, картинка рисовалась как сырые (некорректно
интерпретированные) пиксели. Display-P3 / Adobe-RGB / Rec.2020 PNG выглядели
пересыщенными. Баг был незаметен, потому что в репозитории не было ни одного
профильного PNG (TEST-18 использует sRGB-картинки без `iCCP`).

## Фикс

Заменён `DeflateDecoder` на `flate2::read::ZlibDecoder` — корректная распаковка
zlib-обёрнутого профиля. Колор-менеджмент matrix-shaper (ICC-3) и кэш (ICC-5)
теперь реально срабатывают на PNG.

## Воспроизведение

Сгенерировать профильный PNG (`python graphic_tests/gen_icc_images.py`) →
`lumen_image::decode("samples/images/icc_p3.png")`. До фикса swatch crimson
выдавал сырой P3-пиксель (184,63,82); после — корректный sRGB (200,50,80).

## Регресс-тест

`crates/engine/image/tests/icc_color_management.rs` — декодирует committed
`icc_p3.png` (Display P3) и `icc_cmyk.jpg` (CMYK ICC) через полный
`lumen_image::decode` и проверяет восстановленные sRGB-swatch'и.

## Связано

- Слой 6 ICC Color Management (ICC-1…ICC-6). Найден при ICC-6 (graphic_test).
