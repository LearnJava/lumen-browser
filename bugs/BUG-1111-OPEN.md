# BUG-1111 — worker-тесты `lumen-js` ждут воркер фиксированным `sleep` и падают под нагрузкой

**Статус:** OPEN
**Заведён:** 2026-09-23 (P1, пойман `scripts/scoped-test.sh` на ветке
`p1-bug1109-select-trylock`, которая `lumen-js` не трогает).
**Область:** js (тесты `crates/js/src/dom/tests/v8_webworker.rs`,
`crates/js/src/worker.rs::tests_v8`).

## Симптом

В полном прогоне `scoped-test.sh` (16 пакетов, бинарь `lumen_js` —
4173 теста параллельно, 366 с) упало 20 worker-тестов: 10 в
`dom::tests::v8_webworker` (`worker_blob_url_script`,
`worker_external_url_fetches_and_runs_script`,
`shared_worker_external_url_connects_and_echoes`, …) и 10 в
`worker::tests_v8` (`v8_worker_end_to_end_postmessage`,
`v8_worker_import_scripts_via_data_url`, …). Все одного вида — ответ
воркера не пришёл:

```
panicked at crates\js\src\dom\tests\v8_webworker.rs:352:5:
  left: Null
 right: Number(11.0)
```

Тот же бинарь изолированно (`lumen_js-*.exe worker`) — 179/179 зелёные
за 4.6 с.

## Корень (по коду)

Тесты ждут старта V8-изолята воркера и его ответа фиксированной паузой
(`std::thread::sleep(150 мс)` — 13 мест в `v8_webworker.rs`, 300 мс в
`worker.rs`), затем один раз `pump_workers()`. Под параллельной нагрузкой
создание изолята не укладывается в паузу. Часть тестов того же файла уже
опрашивает с дедлайном 5 с (`v8_webworker.rs:138`, `:200`) — они не упали.

## Что сделать

Перевести оставшиеся тесты на опрос `pump_workers()` + проверку результата
до дедлайна (образец — `v8_webworker.rs:138-144`), без фиксированных пауз.
