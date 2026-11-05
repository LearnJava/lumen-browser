# BUG-1786 — `bounded_document_lock_waits_out_another_thread` флакует от гранулярности системного таймера

**Статус:** OPEN
**Заведён:** 2026-11-05 (P1, финальный гейт среза WASM-V8-NATIVE-S1)
**Область:** js (`crates/js/src/v8_runtime/named_access.rs` — `lock_document_bounded`, `NAMED_ACCESS_LOCK_POLL`)

## Симптом

`v8_runtime::tests::dom_suspend_focus::bounded_document_lock_waits_out_another_thread`
падает эпизодически — в изоляции и в полном прогоне, на ветке и на `main` (код теста и
`lock_document_bounded` идентичен в обеих деревьях, `git diff f7dfdc2b5 main` по обоим
файлам пуст):

```
thread '...bounded_document_lock_waits_out_another_thread' panicked at
crates\js\src\v8_runtime\tests\dom_suspend_focus.rs:975:5:
assertion failed: lock_document_bounded(&doc).is_some()
```

## Причина

Держатель в тесте отпускает блокировку через 4 мс, бюджет ожидания — 20 мс
(`NAMED_ACCESS_LOCK_BUDGET`), поллинг — `thread::sleep(100 мкс)`
(`NAMED_ACCESS_LOCK_POLL`). На Windows `Sleep` округляется вверх до системного тика:
без поднятого разрешения цикл просыпается за 15,6–31,2 мс — иногда до истечения
бюджета (PASS), иногда после (FAIL). Разрешение таймера глобально по системе:
`timeBeginPeriod(1)` любого живого процесса (= в нашем коде — `ensure_v8_platform`)
делает поллинг честными 100 мкс во всех процессах.

Замер (2026-11-05, одна машина, один и тот же бинарь ветки):

```
# coarse-тик (никто не поднял разрешение): 2 из 3 прогонов FAIL, модуль 0,18–0,35 с
# fine-тик (поднят посторонним процессом): 3 из 3 PASS, модуль 0,02 с
# чистый main, тот же день: 3 из 3 PASS, модуль 0,02 с
```

Ветка была-бы только лучше: с WASM-V8-NATIVE-S1 `ensure_v8_platform` поднимает
разрешение до 1 мс в каждом процессе с рантаймом ([BUG-1784](BUG-1784-FIXED.md)),
но тест рантайм не создаёт и остаётся на милости системного тика.

## Проба

```
cargo test -p lumen-js --features v8-backend --lib bounded_document_lock   # N раз подряд
```

PASS/FAIL чередуется в зависимости от того, поднял ли кто-то в системе
`timeBeginPeriod`; надёжный воспроизводитель coarse-тика — прогон на машине без
мультимедийных процессов.

## Что делать

В `lock_document_bounded` перед сном крути короткий спин (`yield_now`, пара
миллисекунд), либо спи на `WaitableTimer`/`wait_on_address` с высоким разрешением;
либо сделай бюджет независимым от тика. Заодно переехать от `thread::sleep(100 мкс)`
во всех чувствательных к latency местах.

## Как проверить

10 подряд прогонов `cargo test -p lumen-js --features v8-backend --lib bounded_document_lock`
без падений на машине с coarse-тиком (проверять при закрытых браузерах).
