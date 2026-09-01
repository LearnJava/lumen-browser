# BUG-937 — `postMessage(offscreenCanvas, [offscreenCanvas])` доезжает до воркера сырым сентинелом: половина отправителя реализована целиком, половина получателя не исполняется никогда

**Статус:** OPEN
**Тип:** дефект реализованного кода — обе половины переноса написаны, получатель отключён одной отсутствующей строкой в списке установки воркерного контекста.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — живой замер, вариант `worker-offscreen-transfer`)
**Область:** js (`crates/js/src/worker.rs:1685-1693` — воркерный поток зовёт только `install_worker_globals_v8`, без `offscreen_canvas::install_offscreen_canvas_bindings_v8`; `crates/js/src/worker.rs:542-566` — `_deserializeTransfers`, который из-за этого не срабатывает)
**Владелец:** P3.

## Симптом

Страница переносит канву в воркер ровно так, как это делает единственный
путь HTML LS §4.12.14 → §2.7:

```js
var off = canvas.transferControlToOffscreen();
w.postMessage({canvas: off}, [off]);
```

Воркер получает **не** `OffscreenCanvas`, а обычный объект с внутренним
сентинелом движка:

```
worker-said typeof=object ctor=Object keys=__lumen_sentinel__/w/h/p
            sentinel=yes getContext=undefined
            OffscreenCanvas=undefined native=undefined
```

То есть страница видит внутреннее представление транспорта, а не интерфейс:
`o.getContext('2d')` — `TypeError: o.getContext is not a function`.

## Прямое измерение

`tests/wpt/verify_replaced_content_gaps.py --variant worker-offscreen-transfer`
(2026-09-01, dev-release, Linux, `main` = `287562e61`; свой http-сервер,
улики со stderr браузера).

| что спрошено | ответ |
|---|---|
| `typeof` полученного в воркере | `object`, конструктор `Object` |
| его собственные ключи | `__lumen_sentinel__` / `w` / `h` / `p` |
| `typeof o.getContext` | `undefined` |
| `typeof OffscreenCanvas` **в воркере** | `undefined` |
| `typeof _lumen_offscreen_canvas_from_image_data` **в воркере** | `undefined` |
| `canvas.getContext('2d')` у отправителя после переноса | `null` (верно) |
| второй `transferControlToOffscreen()` | `InvalidStateError` (верно) |

Половина отправителя, таким образом, реализована **целиком и правильно**:
`_lumenSerializeWithTransfers` (`worker.rs:1208`) снимает пиксели,
нейтрализует источник, кладёт сентинел; спецификационное поведение самой
канвы после переноса — обе проверки — сходится с §4.12.5.

## Причина

`run_worker_thread_v8` (`worker.rs:1685`) устанавливает в воркерный изолят
только `install_worker_globals_v8`, и это записано в его собственном
doc-комментарии: «`OffscreenCanvas` is NOT installed here… A worker script
that references `OffscreenCanvas` sees `undefined`». Следствие — не только
отсутствие конструктора: `_deserializeTransfers` (`worker.rs:544`) вызывается
из `_lumen_worker_dispatch_message` под охраной

```js
(typeof _lumen_offscreen_canvas_from_image_data !== 'undefined')
```

а этот нейтив приходит тем же самым списком установки. Охрана честно
вырождается в «передать как есть» — и написанный рядом код восстановления
(`_lumen_offscreen_canvas_from_image_data` → `Object.create(OffscreenCanvas.prototype)`)
не исполнялся ни разу.

## Почему это дефект, а не доработка

Оба условия [docs/probe-method.md §8](../docs/probe-method.md) должны
выполняться, а здесь не выполняется ни одно: функциональность есть
(`offscreen_canvas.rs` V8-портирован и работает на главном потоке, обе
половины транспорта написаны), и объём — не семейство API, а запись в списке
установки воркерного контекста плюс проверка, что нативы `offscreen_canvas`
переживают чужой изолят.

## Что это стоит прогону

Прямых id в остатке WPT-RUN-5 у этого дефекта нет: единственный кандидат,
`html/canvas/offscreen/manual/filter/offscreencanvas.filter.w.html`, до
воркера не доходит — он открывается `createImageBitmap(patternCanvas)`,
который движок отклоняет («unsupported source type»,
[BUG-880](BUG-880-OPEN.md)), и ни одного `async_test` не регистрирует
(реплей `--variant replay-offscreen-filter-worker`: подтестов ноль).
Заведён потому, что это второй по счёту транспорт, живой у отправителя и
мёртвый у получателя, и любая проба, переносящая канву в воркер, будет молча
мерить его вместо своего предмета. Невендоренная категория
`html/canvas/offscreen/*` стоит на нём целиком.

## Направление починки

Позвать `offscreen_canvas::install_offscreen_canvas_bindings_v8` (и то, от
чего он зависит) из `run_worker_thread_v8` рядом с
`install_worker_globals_v8`, затем снять doc-комментарий на `worker.rs:1685`,
который сегодня описывает намеренный пропуск. Проверять:
`--variant worker-offscreen-transfer` должен дать `getContext=function`
и `OffscreenCanvas=function`.
