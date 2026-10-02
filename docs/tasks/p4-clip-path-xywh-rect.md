# Задача: `clip-path: xywh()` / `rect()` + `<geometry-box>`

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:106` · **Размер:** S–M · **Крейты:** `lumen-layout`

## Цель
`parse_clip_path` не знает `xywh`/`rect`, хвостовой `content-box`/`padding-box` молча игнорируется.

## Точка входа
`crates/engine/layout/src/style/parse/shape.rs:27` (`parse_clip_path`).

## Что сделать
Свести `xywh()`/`rect()` к `ClipPath::Inset` (с `round`); опорный бокс — по `<geometry-box>` (по умолчанию border-box).

## Готово, когда
Юнит: `xywh(10px 20px 50% 30px)` на боксе 200×100 → Inset(top 20, right 90, bottom 50, left 10); `--dump-display-list` показывает соответствующий `PushClip*`.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings` + `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.
