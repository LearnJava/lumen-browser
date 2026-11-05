# BUG-1787 — flaky-пара `lumen-js` в полном параллельном прогоне: document lock и video wake

**Статус:** OPEN
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

Каждый упал по одному разу. Изолированный однопоточный прогон пары — ok
(0,52 с); повторный полный параллельный — 5059 passed / 0 failed.

## Контекст

Обе — тайминговые тесты на стыке S5 (мост video → wake шелла) и спин-фикса
BUG-1786: `PLATFORM_SPIN` (500 мкс `yield_now` после последней задачи очереди)
добавляет CPU-активность JS-потока, под полным параллельным прогоном это
лишняя конкуренция за ЦП. Причинная связь со спином не доказана — на чистом
S5-main пара под полным параллельным прогоном не прогонялась. Прецедент
реестра — [BUG-1677](BUG-1677-OPEN.md) (тот же паттерн: в полной параллельной
пачке падают, по одному проходят).

## Проба

```
cargo test -p lumen-js --lib --features v8-backend
# при воспроизведении: ровно эти два FAILED

cargo test -p lumen-js --lib --features v8-backend -- --test-threads=1 \
  v8_runtime::tests::dom_suspend_focus::bounded_document_lock_waits_out_another_thread \
  video_bindings::tests_v8::native_video_playback_changes_wake_the_shell
# по одному — зелёные
```

## Что делать

Найти разделяемый ресурс или окно тайминга: bounded-ожидание лока документа
(`lock_document_bounded`, стrophe 975) и условие «установка нативных
video-биндингов меняет wake шелла» (1195). Если виноват спин — сузить
`PLATFORM_SPIN` или выходить из спина при ожидании лока документа. Если нет —
общий источник конкуренции, как в BUG-1677.

## Как проверить

Несколько полных параллельных `cargo test -p lumen-js --lib --features
v8-backend` подряд без падений этой пары; изолированный прогон пары
стабильно зелёный.
