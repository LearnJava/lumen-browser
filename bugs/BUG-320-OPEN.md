# BUG-320: `layer_pool` full-frame layers mis-sized against the active scroll-blit band target

**Статус:** OPEN
**Дата:** 2026-07-19
**Компонент:** paint (`crates/engine/paint/src/backends/femtovg_backend.rs`, `acquire_layer`/`layer_pool`)
**Найден:** BUG-272 срез 14 (PushFilter/PushBlendMode bbox-сайзинг) — визуальная A/B-приёмка TEST-56

## Симптом

`mix-blend-mode` (все режимы кроме `normal`/`plus-lighter`, которые используют fast-path без
offscreen-слоя) не рендерится: `PopBlendMode`'s CPU-композит (`composite_blend_layer`) молча
пропускает блендинг и оставляет фон нетронутым — TEST-56 показывает голый жёлтый фон вместо
смешанного цвета в каждой из 16 не-`normal` ячеек. Воспроизведено на **немодифицированном** `main`
(commit `27b30624`, `LUMEN_BACKEND=femtovg`, окно), проверено дважды — детерминированно
(main-vs-main self-diff 0.00%, т.е. не гонка/флуктуация захвата).

## Корень (диагностировано временным `eprintln!` в `composite_blend_layer`)

```
[DEBUG] mode=Multiply src_rgba.len=3096576 backdrop_rgba.len=14581760 backdrop_w=2048 backdrop_h=1780
```

`src_rgba.len() != backdrop_rgba.len()` для **каждого** не-`normal`/`plus-lighter` блендинга →
`composite_blend_layer`'s условие `if src_rgba.len() == backdrop_rgba.len() && !src_rgba.is_empty()`
всегда ложно → блендинг и композит полностью пропускаются.

Причина размерного рассинхрона:
- `backdrop_rgba` захватывается через `self.canvas.screenshot()` **до** переключения
  render-таргета — читает пиксели текущего активного таргета (у ADR-016 M3.2.1b-сцены это band
  FBO с overscan-полями, крупнее физического окна: `backdrop_w=2048, backdrop_h=1780` в замере).
- `src_id` для offscreen-слоя выделяется через `acquire_layer()`/`layer_pool`, который сайзит
  (и валидирует переиспользование) строго по полям `self.width`/`self.height` — это физический
  размер ОКНА (`window.inner_size()`), **не** размер активного render-таргета. Пока рендерится band
  (viewport + overscan margin, см. `band_geometry`), `self.width`/`self.height` НЕ отражают его
  реальные (обычно бо́льшие) device-px размеры.
- `femtovg::Canvas::screenshot()` внутри читает `self.view[0]/[1]`, которые `set_target`
  выставляет в размеры **текущего** GL-таргета — т.е. `src_id`'s screenshot возвращает его
  собственный (меньший, `self.width×self.height`) размер, не совпадающий с band-сайзингом
  `backdrop_rgba`.

## Область поражения

Любой Push-опенер, чей fallback/full-frame путь всё ещё идёт через `acquire_layer()`/`layer_pool`
**во время активного band-рендера** (страница выше вьюпорта, ADR-016 M3.2.1b scroll-blit), сравнимо
уязвим: `PushFilter`'s blur-цепочка (осталась full-frame намеренно, BUG-272 срез 14), полноэкранный
fallback `PushOpacity`/`PushClipRoundedRect`/`PushClipPath`/`PushMask*` (когда `bounds` — `None`
или bbox-путь промахнулся), `PushBackdropFilter`'s более старые full-frame сайты. Bbox-сайзинг
(срезы 11–14, `acquire_bbox_layer`) на практике избегает бага для затронутых Push-типов, поскольку
bbox-размер почти всегда заметно меньше `self.width`/`self.height` и никогда не завязан на band-FBO
размер напрямую — но это побочный эффект, не намеренный фикс.

TEST-56 (`mix-blend-mode`) физически невысокий (не переполняет вьюпорт по высоте) — потребует
проверки, действительно ли band-рендер активен для этой страницы, либо `self.width`/`self.height`
рассинхронизированы с активным таргетом по какой-то другой причине (не только band); диагностика
через `eprintln!` подтвердила лишь факт и числа, не полный causal chain до вызывающего кода
(`run_content_pass`/band-переключения).

## Влияние на BUG-272 срез 14

Срез 14 (bbox-сайзинг видимого слоя `PushFilter`/`PushBlendMode`) **не вызывает** этот баг и
**не регрессирует** его — баг воспроизводится идентично на чистом `merge-base` (до среза 14).
Срез 14's bbox-путь для `PushBlendMode` **обходит** баг как побочный эффект (слой сайзится по
`screen_bbox_device_px`, не по `self.width/height`), поэтому визуальная A/B-приёмка (branch vs
merge-base) показывает диф TEST-56 (~12.8%) — это branch **чинящий** плохой baseline, не
регрессирующий его. Полный фикс требует отдельного среза: `acquire_layer()` должен сайзиться по
размеру ТЕКУЩЕГО активного render-таргета (band или экран), а не по `self.width`/`self.height`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release LUMEN_BACKEND=femtovg cargo run -p lumen-shell -- graphic_tests/56-mix-blend-mode.html
```
Визуально: только первая ячейка (`normal`) показывает синий квадрат поверх жёлтого фона; остальные
16 ячеек показывают голый жёлтый фон без блендинга.
