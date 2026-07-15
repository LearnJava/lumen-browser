# BUG-283 — краш процесса на реальных веб-шрифтах: allocation of ~906 GB failed

**Статус:** OPEN
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

## Возможный фикс (не реализован в этой задаче)

В `rasterize()` после вычисления `x_min/x_max/y_min/y_max`:
- вернуть `None`, если `x_max < x_min` или `y_max < y_min` (испорченный
  bbox — тот же трактовка, что уже есть для `width == 0 || height == 0`);
- добавить защитный верхний предел на `width`/`height` (например, разумный
  максимум в пикселях) и возвращать `None` при превышении вместо аллокации.

## Воспроизведение

```
cargo build --release -p lumen-shell
target/release/lumen.exe https://lenta.ru
```

Крашится стабильно на первом рендере страницы (наблюдалось дважды подряд).
