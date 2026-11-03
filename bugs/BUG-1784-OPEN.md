# BUG-1784 — `promise_test` в testharness не получает завершение: ровно один сабтест-обещание висит, файл уходит в TIMEOUT

**Статус:** OPEN
**Заведён:** 2026-11-03 (P1, разбор остатка WASM-V8-NATIVE-S1)
**Область:** js (`crates/js/src/v8_runtime/thread.rs` — прокачка очереди платформы и микрозадач; либо шим обещаний/событий)

## Симптом

В девяти самых больших сгенерированных spec-файлах `wasm/core` (`f64.wast.js.html`,
`f64_cmp`, `f32`, `simd/simd_f64x2_pmin_pmax`, …) файл доходит до последнего
сабтеста и упирается в потолок wptrunner 10 с:

```
subtests: 9840/9841 passed ... statuses: TIMEOUT=1
```

Недостающий сабтест всегда один и тот же — финальный
`promise_test(_ => chain, testNum() + "Reinitialize the default imports")`
из `tests/wpt/wasm/core/js/harness/async_index.js`.

## Что уже исключено (замеры 2026-11-03, `dev-release`, один файл, `--processes 1`)

- **Не медленность.** Счётчики в `v8_thread_main` (`LUMEN_PLATFORM_TRACE`): к 3,4 с
  очередь платформы пустеет, задачи перестают приходить, поток простаивает до
  конца; `runs` растёт (шелл шлёт команды), значит поток свободен.
- **Не зависшая компиляция.** `Promise.race` с 3-секундным сторожем вокруг каждого
  `WebAssembly.compile(buffer)` в `module()` не сработал ни разу.
- **Не количество тестов и не форма цепочки.** Синтетические страницы: 10 000
  синхронных `test()` + `promise_test` — OK; 2000 и 5000 вызовов настоящего
  `module()` из того же harness с пустым модулем и `reinitializeRegistry()` в
  конце — OK (10 002/10 002 сабтеста).
- **Не наш цикл платформы.** `timeBeginPeriod(1)` (BUG-1752/WASM-V8-NATIVE-S1) ускорил
  страницу вдвое (8,5 → 3,4 с) и вылечил `f32.wast.js.html`, но остальные девять
  висят так же.

## Что найдено

Счётчики в `resources/testharness.js` (`promise_test`: создан/начат/завершён):

```
DBG pt created=2 started=2 done=1
```

То есть оба `promise_test` страницы **начались**, а завершился только первый
(тот, что `async_index.js` регистрирует на загрузке, с пустой цепочкой).
У второго обработчик `Promise.resolve(promise).catch(step_func).then(() => test.done())`
(`testharness.js:758-769`) **никогда не вызывается**.

При этом проба `chain.then(() => { throw … })`, подцепленная к *той же* цепочке
сразу после `promise_test`, срабатывает: цепочка разрешается, и посторонний
`.then` на ней выполняется. Значит расходится не обещание, а именно цепочка
`Promise.resolve(...).catch(...).then(...)` внутри testharness — либо `.catch`/`.then`
на уже разрешённом обещании в нашем движке теряет реакцию, либо `step_func`
ломает звено молча (необработанных отказов в логе страницы нет).

## Как воспроизвести

```
python tests/wpt/run_corpus.py --binary target/dev-release/lumen.exe \
    --categories wasm --prefixes wasm/core/f64.wast.js.html --processes 1
```

Ожидание: `harness OK 1`. Факт: `TIMEOUT=1`, `9840/9841`.

## Что делать

1. Воспроизвести расхождение минимально: страница, которая делает
   `p = Promise.resolve(); p.then(() => {/*A*/}); Promise.resolve(p).catch(f).then(() => {/*B*/});`
   после разрешения `p` — проверить, что вызываются оба обработчика, затем
   усложнять до `step_func` и до цепочки из сотен `.then` (в файле их ~4900).
2. Гипотезы по убыванию вероятности: (а) обработчик, подцепленный к обещанию
   *до* того, как оно разрешилось в микрозадаче, запущенной из
   `perform_microtask_checkpoint` (`thread.rs::run_platform_tasks`), теряется при
   повторном чекпойнте; (б) `Promise.resolve(thenable)` в нашем шиме/движке
   создаёт обёртку, которая не пробрасывает разрешение; (в) `test.step_func`
   кидает, и `catch` это съедает.
3. Когда причина найдена — `run_corpus.py --prefixes wasm/core` должен дать
   258/258 `harness OK` и 166 7xx сабтестов без TIMEOUT; после этого закрывается
   критерий среза WASM-V8-NATIVE-S1.
