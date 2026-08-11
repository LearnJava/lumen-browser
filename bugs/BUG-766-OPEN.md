# BUG-766 — `isSecureContext` отсутствует в `WorkerGlobalScope`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs:227` — `worker_global_shim`)
**Найден:** P3, при закрытии [BUG-399](BUG-399-FIXED.md), 2026-08-11

## Симптом

`worker_global_shim` (глобал воркер-потока) не заводит `isSecureContext` вовсе:
`'isSecureContext' in self === false`, чтение даёт `undefined`. Свойства нет ни
в каком виде — это не «всегда `true`», как было в окне до
[BUG-399](BUG-399-FIXED.md), а дыра в поверхности.

По HTML LS `isSecureContext` объявлен в миксине `WindowOrWorkerGlobalScope`
(`[Exposed=(Window,Worker)]`), то есть обязателен и в воркере.

## Причина

`worker_global_shim` — сокращённый стаб: он заводит `self`/`name`/`postMessage`/
`addEventListener`/`console`/`importScripts`/таймеры и на этом заканчивается.
Ни `location`, ни `navigator`, ни `performance`
([BUG-401](BUG-401-FIXED.md)), ни `isSecureContext` в нём нет. Отдельного
источника URL у воркер-глобала тоже нет — считать флаг ему сейчас не из чего;
по спеке он наследуется от environment settings object создателя, то есть
значение нужно прокидывать с главного потока при старте воркера (там оно уже
посчитано — BUG-399).

## Последствия

Скрипт воркера, ветвящийся по `isSecureContext` (обычная форма для кода,
который делит путь между окном и воркером), получает `undefined` → falsy, то
есть ведёт себя как в небезопасном контексте даже на https-странице. Тихое
расхождение двух глобалов: в окне флаг верен, в воркере его нет.

## Что нужно сделать

Прокинуть уже посчитанное на главном потоке значение (BUG-399, замыкание рядом
с `window.isSecureContext`) в `worker_global_shim` тем же способом, каким туда
попадает `worker_id` (интерполяция в шаблон шима), и выставить его
getter-only-аксессором. Гейт — юнит-тест на воркере с https- и http-страницы.

## Связанные

* [BUG-399](BUG-399-FIXED.md) — окно; источник значения для воркера.
* [BUG-401](BUG-401-FIXED.md) — `performance` отсутствует в том же глобале;
  тот же класс «сокращённый стаб воркера».
* [BUG-765](BUG-765-OPEN.md) — гейт `[SecureContext]` в воркере будет нечем
  питать, пока этот флаг не появится.
