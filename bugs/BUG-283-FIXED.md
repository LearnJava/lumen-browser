# BUG-283 — краш процесса на реальных веб-шрифтах: allocation of ~906 GB failed

**Статус:** FIXED 2026-07-15
**Компонент:** font (`Rasterizer::rasterize`)
**Найден:** 2026-07-15, ручной прогон релизной сборки на https://lenta.ru

## Симптом

Процесс `lumen.exe` (release) вылетает через ~7 секунд после старта на
https://lenta.ru, сразу после первого непустого кадра:

```
[bench] first non-empty frame: 7009ms since process start
memory allocation of 906238016744 bytes failed
```

Страница использует кастомные `@font-face` (`Lato Lenta`, `Source Serif Pro
Lenta`, `SB Sans Text`), которые к моменту краша уже успели асинхронно
загрузиться (`@font-face async загружен: ...` в логе непосредственно перед
крашем).

## Расследование

`RUST_BACKTRACE=full` указывает на аллокацию внутри
`lumen_font::rasterizer::Rasterizer::rasterize`
(`crates/engine/font/src/rasterizer.rs:81`, `vec![0u8; width * height]`);
остальная часть бэктрейса зашумлена смешением инлайнов в release-сборке и не
надёжна выше этого фрейма.

Корень — в `rasterize()` (`crates/engine/font/src/rasterizer.rs:66-71`):

```rust
let x_min = (glyph.bbox.x_min as f32 * scale - pad).floor() as i32;
let x_max = (glyph.bbox.x_max as f32 * scale + pad).ceil() as i32;
let width = (x_max - x_min) as u32;   // нет проверки x_max >= x_min
let height = (y_max - y_min) as u32;  // нет проверки y_max >= y_min
```

Если `glyph.bbox` у конкретного глифа в шрифте инвертирован или испорчен
(`x_max < x_min` либо `y_max < y_min`), разность `i32` отрицательна, а каст в
`u32` заворачивает её в число, близкое к `u32::MAX`. `width * height as usize`
затем даёт огромный размер (наблюдалось 906 238 016 744 байт), и
`vec![0u8; ...]` падает через `handle_alloc_error` вместо контролируемой
ошибки.

Нет также верхней защитной границы на разумный размер битмапа глифа (не
связанной с переполнением) — даже корректный, но экстремальный
`pixel_size`/`units_per_em` может запросить неограниченно большой буфер.

## Фикс (2026-07-15)

`rasterize()` теперь считает `width_i32`/`height_i32` как `i32` (без каста в
`u32`) и возвращает `None`, если:
- `width_i32 <= 0` или `height_i32 <= 0` — испорченный/инвертированный bbox
  (тот же случай, что раньше заворачивался в `u32::MAX`);
- `width_i32 > MAX_GLYPH_DIM` или `height_i32 > MAX_GLYPH_DIM` (8192 px) —
  корректный, но экстремальный bbox/pixel_size, который иначе запросил бы
  неограниченно большой буфер.

Только после этой проверки диапазоны кастуются в `u32` для аллокации
`pixels`. 3 новых юнит-теста: `inverted_bbox_returns_none_instead_of_crashing`,
`inverted_bbox_y_axis_returns_none`, `oversized_bbox_returns_none_instead_of_huge_allocation`
(`crates/engine/font/src/rasterizer.rs`). `cargo test -p lumen-font` — 362/362
зелёных, `cargo clippy -p lumen-font --all-targets -- -D warnings` чист.

## Воспроизведение

```
cargo build --release -p lumen-shell
target/release/lumen.exe https://lenta.ru
```

Крашится стабильно на первом рендере страницы (наблюдалось дважды подряд).
