# BUG-1723 — wgpu с `LUMEN_NO_IMAGE_MIPS=1`: повторная регистрация `src` оставляет устаревшие уменьшенные копии `"src@WxH"`

**Статус:** OPEN
**Заведён:** 2026-10-26 (по чтению кода, main d10fb6b5c; найден при исправлении того же дефекта в femtovg)
**Область:** paint (`crates/engine/paint/src/renderer/texture_pool.rs` — `register_image`, `ensure_image_gpu_key`, `unregister_image`)

## Симптом (ожидаемый, вживую не воспроизводился)

Запуск с `LUMEN_NO_IMAGE_MIPS=1` (kill-switch старого пути без mip-цепочки). Картинка, которая
перерегистрируется под тем же ключом — кадры `video:{nid}`, `canvas:{nid}`, кадры GIF, — при
показе в размере меньше натурального (`object-fit`, `width`/`height` меньше intrinsic) застывает на
первом кадре, который был уменьшен. В натуральном размере кадры идут.

По умолчанию (mip-путь) дефекта нет: ключ всегда `src`, копии `"src@WxH"` не создаются.

## Причина

`Renderer::register_image` перезаписывает `raw_images[src]` и `images[src]`, но не трогает уже
созданные `images["src@WxH"]`. `ensure_image_gpu_key` создаёт копию только при
`!self.images.contains_key(&gpu_key)`, поэтому после первого кадра копия больше не пересоздаётся, а
`compute_image_gpu_key` продолжает возвращать её ключ.

Удаление копий уже есть в `unregister_image` (`texture_pool.rs:397`, `retain` по префиксу
`"{src}@"`), но `register_image` его не вызывает.

## Как исправить

1. В `register_image`, только на пути `image_mips_disabled()`, перед вставкой удалить из `images`
   копии этого `src` — так же, как делает `unregister_image`. Старые `wgpu::Texture` дропаются при
   удалении, wgpu сам откладывает освобождение до конца работы GPU с ними.
2. Попутно: префикс `"{src}@"` в `unregister_image` задевает и чужой `src`, который начинается с
   `"{src}@"` (например, URL с `@`). Проверять, что остаток после префикса имеет вид `WxH` из
   двух `u32`, — как `LruMap::remove_variants_of` в
   `crates/engine/paint/src/backends/femtovg_backend.rs` (исправление того же дефекта в femtovg,
   коммит 25bf34d4a).

## Готово, когда

- Юнит-тест на `Renderer` (или на вынесенный хелпер выбора ключей): после повторного
  `register_image` того же `src` с другими пикселями ключ `"src@WxH"` отсутствует в `images`,
  а копия другого `src` с префиксом `"{src}@"` остаётся.
- Вживую: `LUMEN_NO_IMAGE_MIPS=1 LUMEN_NO_ADBLOCK=1 lumen.exe --maximized` на странице с
  `<video autoplay muted loop style="width:50%">` — кадры меняются.
- `cargo clippy -p lumen-paint --all-targets -- -D warnings` и `cargo test -p lumen-paint --lib` проходят.
