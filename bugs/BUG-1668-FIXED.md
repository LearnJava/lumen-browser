# BUG-1668 — `margin: auto` не центрирует блок, ширину которого ограничил `max-width`

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), example.com)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch/dispatch_box.rs` — used width блока в потоке)

## Симптом

```html
<!doctype html><div id=d style="max-width:400px;margin:0 auto">x</div>
```

viewport 1280: Chrome `#d.getBoundingClientRect()` = `440,8,400,18`, Lumen — `8,8,400,19.4`.
`--dump-layout`: `Block rect=(8.00, 8.00, 400.00, 17.72) max-w=400.00 m=(0.00, auto, 0.00, auto)` —
ширина обрезана до 400, а свободные 864px не ушли в auto-поля.
На example.com (`body{max-width:26em;margin:auto}`) весь текст страницы был прижат влево.

## Причина

`dispatch_box.rs` применял `max-width` к черновой (`auto`) ширине, но распределение
auto-полей (§10.3.3) входило только при явном `width`. CSS 2.2 §10.4 требует: если
черновая ширина больше `max-width`, правила §10.3.3 применяются заново с
`width = max-width` — auto-поля получают остаток.

## Исправление

Кламп `max-width` запоминает, что он действительно уменьшил ширину
(`clamped_by_max`), и распределение auto-полей выполняется также в этом случае.
Без клампа (`max-width` ≥ доступной ширины) поведение прежнее: остаток нулевой.

## Проверка

- `cargo test -p lumen-layout --lib` 6015 passed, из них новый
  `flow_modes::margin_auto_centers_block_clamped_by_max_width` (800 → ширина 400, `x = 200`;
  без фикса `x = 0`);
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` чисто.
- Полный `python graphic_tests/run.py` в этой среде не прошёл калибровку захвата
  (TEST-00: magenta marker not found), но ни один фикстур не сочетает `max-width`
  с `margin: auto` — эталонные снимки эту ветку не задевают.

