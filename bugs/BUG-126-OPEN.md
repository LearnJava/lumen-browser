# BUG-126

**Статус:** OPEN
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

CSS Anchor Positioning L1 (anchor-name/position-anchor/inset-area) — TEST-77: 53.45% (thr 0.5%); corner/edge/span placement around anchor wrong or missing

## Расследование и фикс (2026-06-19)

**Корень.** `resolve_inset_area` всегда растягивал anchored-элемент на всю ячейку
position-area grid, перетирая `width`/`height` размером band. Для элементов с
определённым размером (TEST-77 row 1: 60×60 боксы) это давало 8 растянутых
прямоугольников вместо аккуратной 3×3 сетки вокруг якоря.

**Фикс** (`crates/engine/layout/src/anchor.rs`):
- Введён `AxisSize { Auto, Fixed(f32) }` — used-size элемента по каждой оси.
- `band_region` возвращает `[start,end]` band (start/center/end/span-* тайлы).
- `place_axis`: `Auto` → растянуть на band; `Fixed` → сохранить размер и
  выровнять к якорю (`align_in_band`: start-side прижимается к дальнему краю band,
  end-side — к ближнему, center/span — по центру) — дефолтный position-area
  self-alignment (CSS Anchor Positioning L1 §5.1).
- Оба call-site в `box_tree.rs` (`lay_out_abs_children`, `apply_anchor_positions_rec`)
  передают `AxisSize` из `style.width/height.is_some()` + `anchor-size()` override.

Проверено `--dump-layout`: все 8 corner/edge боксов и 3 span-бара на спек-корректных
позициях. Пиксельный diff vs Edge(`position-area`): container 1 (3×3 сетка)
**совпадает пиксель-в-пиксель** (53.45% → 12.94%).

## Остаток (DEBTOR, KNOWN_DEBTORS 12.94%)

Не дефект движка — расхождение в reference-браузере:
1. Тест использует устаревшее имя `inset-area`. Текущий Edge его игнорирует
   (поддерживает только переименованное `position-area`) → в кэш-эталоне боксы
   свалены в origin. С `position-area` Edge рисует ровно то же, что Lumen.
2. Span-ряд (container 2): Lumen по спеку растягивает auto-width элемент на
   `position-area` band (видны бары); Edge не отрисовывает `span-*` вовсе. Здесь
   Lumen спек-корректнее Edge.
