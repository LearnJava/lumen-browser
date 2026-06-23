# BUG-236

**Статус:** FIXED 2026-06-23
**Компонент:** layout (`crates/engine/layout/src/motion_path.rs`, `property_trees.rs`)
**Тест:** TEST-99 (`offset-path: ray()`) — регрессия PASS→FAIL (0.00% → 3.28%)

## Описание

Регрессия, внесённая фиксом BUG-125 (Motion Path L1, 2026-06-22). BUG-125
унифицировал motion-матрицу в `forward_box_transform`/`walk` как

```
final = T(rect + path) · R(θ) · m · T(-(rect + anchor))
```

Это корректно для `offset-path: path(...)`: `point_at_distance` возвращает
**абсолютную** точку пути в координатах бокса, на которую садится `offset-anchor`,
поэтому вычитание anchor нужно. Но для `offset-path: ray(...)` функция
`resolve_ray` возвращала **относительное** смещение `offset_distance · dir`
(anchor-агностичное: луч начинается в `offset-position`, т.е. в нормальной
позиции бокса). При относительном смещении вычитание anchor в матрице добавляло
лишний `−anchor` (для бокса 40×40 это `−20px` по обеим осям) → каждый ray-бокс
сдвигался на пол-бокса. TEST-99 (восемь боксов по лучам + центр + turn) ушёл в
3.28%.

## Воспроизведение

`graphic_tests/99-offset-path-ray.html` — пока `offset-rotate: 0deg` (чистая
трансляция, без AA повёрнутых краёв). До фикса CPU `--screenshot` vs Edge = 3.27%.

## Как починено

`resolve_motion_transform` теперь принимает `anchor: (f32, f32)`. Ветка `ray()`
возвращает **абсолютную** точку пути `anchor + displacement` — ту же семантику,
что и `path()`. Тогда в матрице `T(rect + path) · … · T(-(rect + anchor))` член
`−anchor` сворачивается, оставляя чистое смещение луча независимо от anchor:

```
origin → −anchor (·R·m=I) → + (anchor + disp) = disp   ✓
```

`path()` не тронут (anchor игнорируется, координаты уже абсолютные). Оба
call-site в `property_trees.rs` (`forward_box_transform`, `walk`) считают anchor
перед вызовом и передают его. Регресс-тест `ray_path_point_is_anchor_plus_displacement`
фиксирует `anchor + displacement`.

**Результат:** TEST-99 3.27% → 0.02% (CPU `--screenshot` vs Edge). Все 2971
теста `lumen-layout` зелёные, clippy чисто.
