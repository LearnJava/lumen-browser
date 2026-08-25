# BUG-909 — страничный `setTimeout(fn, delay, …args)` теряет хвостовые аргументы: обработчик получает `undefined`

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, попутно к [BUG-831](BUG-831-FIXED.md))
**Область:** `crates/js/src/dom.rs` — `setTimeout`/`setInterval` кладут в очередь
`{id, fn, deadline, interval, nesting}` и ничего больше, а `_lumen_tick_timers`
зовёт `ready[k].fn()` без аргументов
**Владелец:** P1/P3 (`lumen-js`)

## Симптом

```js
setTimeout(function (a, b) { console.log(a, b); }, 0, 'x', 'y');
// печатает "undefined undefined"
```

HTML LS §8.6 шаг 8 требует обратного: Function-обработчик вызывается
«with arguments», где arguments — всё, что шло после delay. Ошибки при этом
нет: страница получает вызов, только с пустыми параметрами, поэтому дефект
неотличим от собственной ошибки страницы.

## Прямое измерение

Временный юнит-тест на страничном рантайме (2026-08-25, dev-release, Windows):

```rust
rt.eval("var got = 'none'; setTimeout(function(a, b) { got = String(a) + String(b); }, 0, 'x', 'y');")
let r = rt.eval("_lumen_tick_timers(); got")   // → String("undefinedundefined")
```

## Почему это не половина BUG-831

Дефект соседний, но независимый: BUG-831 — про **строковый** обработчик
(он аргументов и не должен получать, §8.6 шаг 8 отдаёт их только
Function-обработчику), а здесь теряются аргументы у нормальной функции.
Воркерный шим этот шаг уже выполняет — `WORKER_TIMERS_SHIM` хранит `args` и
зовёт `task.fn.apply(globalThis, task.args)` с [BUG-815](BUG-815-FIXED.md),
и на это есть тест `v8_worker_set_timeout_passes_extra_arguments`. То есть
страничный шим отстал от воркерного на один шаг спеки.

## Направление починки (не предписание)

Скопировать форму воркерного `_schedule`: сохранять
`Array.prototype.slice.call(arguments, 2)` в записи таймера и звать
`fn.apply(window, args)` в `_lumen_tick_timers`. Внимание к двум местам
рядом: перезапись записи интервала (`_lumen_timers.push({… fn: r.fn …})`
перед вызовом колбэков) должна переносить `args`, а строковый обработчик
BUG-831 обязан получать пустой список.

## Цена

Замера в id WPT нет — заведён по прямому измерению. Вне WPT форма
`setTimeout(fn, 0, arg)` распространена в коде, транспилированном из
`Promise`-полифиллов и в старых библиотеках (jQuery-эпоха), где она заменяет
замыкание.
