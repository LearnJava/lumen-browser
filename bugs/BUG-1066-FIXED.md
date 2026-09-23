# BUG-1066 — в глобальной области dedicated `Worker` не определён `DOMException`: `new DOMException(…)` и `DOMException.NAME_ERR` бросают `ReferenceError`

**Статус:** FIXED 2026-09-23 (P1, WORKER-1 срез 1)
**Тип:** дефект реализованного кода — глобальный объект воркера собирается отдельным `rt` (`WORKER_SHIM`, `crates/js/src/worker.rs`) и не получает `DOMException`, определённый шимом главного потока.
**Заведён:** 2026-09-19 (WPT-RUN-7 срез 37, `webidl`; найден при разборе baseline `webidl/ecmascript-binding/es-exceptions/`)
**Область:** `crates/js/src/worker.rs` (`WORKER_SHIM` — бутстрап globals воркера; `SHARED_WORKER_SHIM` и `sw_worker.rs` не проверялись).
**Владелец:** P3 (P2 багов не чинит).

## Симптом

Пять файлов `webidl/ecmascript-binding/es-exceptions/DOMException-*.any.worker.html`
(`constants`, `constructor-behavior`, `custom-bindings`, `is-error`, `stack-accessor`)
падают в каждом подтесте одним и тем же сообщением:

```
FAIL new DOMException() - DOMException is not defined
ReferenceError: DOMException is not defined
FAIL Cannot construct without new - assert_throws_js: function "() => DOMException()"
  threw object "ReferenceError: DOMException is not defined" … expected instance of TypeError
```

Те же файлы в варианте `.any.html` (окно) проходят: `DOMException` в главном потоке есть
(шим `web_api_shim_mid*.js`). В `worker.rs`, `shared_worker.rs`, `sw_worker.rs` слово
`DOMException` не встречается вовсе.

Это не только тестовая проблема: любой код воркера, который ловит или создаёт
`DOMException` (`e instanceof DOMException`, `new DOMException('x', 'AbortError')`), ломается
на `ReferenceError`.

## Ожидание

`DOMException` — интерфейс, отданный в `Exposed=*` (WebIDL/DOM), то есть присутствует в
`Window` и в любом `WorkerGlobalScope`. Конструктор, `name`/`message`/`code`, все
константы (`INDEX_SIZE_ERR` … `DATA_CLONE_ERR`) и `Error.prototype` в цепочке — как в окне.

## Не проверено

- `SharedWorker` и `ServiceWorker` globals: раннер для этих файлов генерирует только
  варианты `window` и `worker`, отдельной пробы на shared/service не было.
- Совпадает ли поверхность `DOMException` в воркере с окном после того, как класс появится
  (baseline срезит 37 записывает сегодняшнее `FAIL` как правду).

## Следствие для baseline

`expected: FAIL` в `tests/wpt/metadata/webidl/ecmascript-binding/es-exceptions/DOMException-*.any.js.ini`
для `*.any.worker.html` — сегодняшняя правда движка. Починка даст unexpected-pass; baseline
регенерируется (`--update-expected` + три `--check`) в том же коммите или сразу следом.

## Исправление (2026-09-23, WORKER-1 срез 1)

Частичный дрейф: BUG-1016 уже вычислял `DOM_EXCEPTION_POLYFILL` в dedicated-воркере — но
только в нём (внутри `install_worker_globals_v8`, ради `atob`), так что shared- и
service-воркеры оставались без `DOMException`. Вычисление перенесено в
`dom::install_worker_exposed_v8`, который зовут все три вида воркеров через
`worker::install_worker_scope_globals_v8`; полифил под охраной `typeof`, повтор безвреден.
Тест: `dom::tests::v8_worker1_exposed::worker_scope_has_dom_exception`.
