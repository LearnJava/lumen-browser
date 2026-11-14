# BUG-1787 — flaky-пара `lumen-js` в полном параллельном прогоне: document lock и video wake

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-11-05 (merge-гейт WASM-V8-NATIVE-S1, merge `13ce99231`)
**Область:** js (`v8_runtime/tests/dom_suspend_focus.rs:975`, `video_bindings.rs:1195`) — стык VTT-RENDER-S5 × BUG-1786 (`PLATFORM_SPIN`)

## Симптом

Два теста падают в полном параллельном прогоне `cargo test -p lumen-js --lib
--features v8-backend` (первый пост-merge прогон, 5057 passed / 2 failed) и
проходят по одному:

- `v8_runtime::tests::dom_suspend_focus::bounded_document_lock_waits_out_another_thread`
  — `assertion failed: lock_document_bounded(&doc).is_some()`
  (`dom_suspend_focus.rs:975`);
- `video_bindings::tests_v8::native_video_playback_changes_wake_the_shell`
  — «installing bindings changes nothing» (`video_bindings.rs:1195`).

## Причины — две разные, спина BUG-1786 ни при чём

1. **Тайминг теста, а не бюджета.** Держатель замка спал `sleep(4 ms)` и только
   потом отпускал guard. В полном параллельном прогоне (5000+ тестов, все ядра
   заняты) поток, вышедший из `sleep`, может ждать планировщика дольше
   `NAMED_ACCESS_LOCK_BUDGET` (20 ms). Для ожидающего это неотличимо от замка,
   который не освобождается: `lock_document_bounded` честно возвращает `None`,
   `is_some()` падает. Продакшн-бюджет не при чём — держатель в реальности
   отпускает замок за 3.9 ms, а не «просыпается когда-нибудь».

2. **Общее глобальное состояние между тестами.** `STORE_GUARD` (сериализация
   process-global стора `video_gif_store::STORE`) жил в тестовом модуле
   `video_bindings`, и `audio_element::tests_v8::
   ui_sync_mirrors_provider_state_into_playback_store` его не брал. Гонка:
   аудио-тест ставит свой стор, видео-тест перезаписывает глобальный своим,
   аудио-тест читает `get_video_gif_store()` — получает **чужой** стор и через
   `__lumen_audio_ui_sync` → `note_playback_change` поднимает в нём
   `playback_changed`. Видео-тест сразу после `install` видит
   `take_playback_change() == true` и падает на «installing bindings changes
   nothing».

## Фикс

- `dom_suspend_focus.rs`: держатель замка больше не спит — крутит `spin_loop`
  до флага `release`, после флага — короткий фиксированный спин (работа CPU, а
  не wall-clock), чтобы ожидающий успел застать замок занятым и пройти именно
  poll-путь, а не свободный. Освобождение больше не зависит от планировщика.
- `video_gif_store.rs`: `STORE_GUARD` переехал к глобалу, который он защищает
  (`#[cfg(test)] pub(crate) static STORE_GUARD`), рядом с `DECODER_TEST_LOCK`;
  `video_bindings::tests_v8` его ре-экспортирует, так что все существующие
  использования и импорт из `mse_bindings` не изменились.
- `audio_element.rs`: `ui_sync_mirrors_provider_state_into_playback_store` берёт
  `STORE_GUARD` на всё тело.

## Как проверить

`cargo test -p lumen-js --lib --features v8-backend` — пара зелёная в полном
параллельном прогоне (2026-11-15: 5004 passed / 3 failed, ни одного из этой
пары; 3 падения — pre-existing [BUG-1795](BUG-1795-OPEN.md) и
[BUG-1677](BUG-1677-OPEN.md)). Точечный прогон трёх тестов (пара + аудио-тест,
который её ронял) зелёный.
