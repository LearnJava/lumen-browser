# BUG-1849 — `lumen-paint` и `lumen-media-native` красные на main: empty-cells под border-collapse, DrawText width=0 в снапшотах, HEVC hw-vs-sw

**Статус:** OPEN
**Заведён:** 2026-11-26 (P6, попутно гейту MCP-TOTAL-S1a)
**Область:** paint (`crates/engine/paint` — модель collapse в display list; текстовые снапшоты `tests/cases/snapshot_tests.rs`), media-native (`crates/engine/media-native/src/mf/video.rs`)

## Симптом

Три кластера падают на дереве без единой правки paint/media-native (ветка
`p6-mcp-total-s1a`, код идентичен `main`) — и в полном прогоне, и по одному:

1. **`display_list::text_and_images::empty_cells_hide_ignored_under_border_collapse`**
   — «collapse model ignores empty-cells: empty cell bg stays»
   (`text_and_images.rs:477`); воспроизводится `--exact --test-threads=1`.
2. **5 снапшотов `tests/cases/snapshot_tests.rs`** (`single_paragraph`,
   `paragraph_with_background`, `line_wrap_two_lines`,
   `nested_blocks_paint_order`, `cyrillic_text`) — у `DrawText` ширина
   `0.00` против ожидаемой (`800.00`/аналогичной): `DrawText (0,0,0,19.2) "hello"`,
  golden-файл ждёт `(0,0,800,19.2)`. Похоже на нулевой text-advance в тестовом
   контексте; не локализовано.
3. **`mf::video::tests::hevc_hardware_matches_software`** — «расхождение GPU и
   CPU до 255 в канале» (`video.rs:567`); тест сверяет аппаратное декодирование
   (Media Foundation, D3D11VA, Intel Iris Plus Graphics, драйвер 27.20.100.8853)
   с программным — вероятно, чувствителен к машине/драйверу.

Проба (кластер 1):

```
cargo test -p lumen-paint --lib "display_list::text_and_images::empty_cells_hide_ignored_under_border_collapse" -- --exact --test-threads=1
```

## Причина

Не локализована, не бисектилась. Кластеры 1–2 — движковые; кластер 3 —
аппаратозависимый (возможно, должен быть `ignore`-ческим на несовпадающих
GPU, ср. `requires GPU adapter` у соседей).

## Что делать

1. Кластер 2 сначала: понять, почему advance текста 0 (шрифт/шейпер в тестовой
   сборке), чинить или признать снапшоты аппаратно-зависимыми.
2. Кластер 1: пустые ячейки таблицы под `border-collapse` — модель collapse.
3. Кластер 3: порог/окружение для hw-декода на машинах с слабым D3D11VA.

## Как проверить

`cargo test -p lumen-paint` и `cargo test -p lumen-media-native` зелёные на
машине гейта; `bash scripts/scoped-test.sh` красный вне этих кластеров и
BUG-1677/BUG-1848.
