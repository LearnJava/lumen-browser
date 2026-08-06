# BUG-684 — `ReadableStream`/`WritableStream`/`TransformStream` callable without `new` silently succeed and pollute `globalThis`; `ReadableStream` is not async-iterable

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`, §"WHATWG Streams", ~line 5630)
**Найден:** P2, WPT-VENDOR-streams, 2026-08-06

## Симптом

Категория `streams` (`tests/wpt/streams/`, 98 значимых файлов, ни одного
`.https.` кроме трёх, ни одного `variant`/`testdriver`) — вендорена и
прогнана целиком (`run_report.py --all --root streams --recursive`, 5:50,
88 отобранных id): **55/88 harness OK, 154/1076 сабтестов**. Подавляющее
большинство неожиданных результатов объясняется без находок движка — уже
открытым [BUG-346](BUG-346-OPEN.md) (`Url::resolve()` не схлопывает
`..`-сегменты: `streams/piping/../resources/recording-streams.js`,
`streams/writable-streams/../resources/test-utils.js` и т.п. 404-ят,
объясняя ~200 FAIL с текстом `recordingReadableStream is not defined` /
`recordingWritableStream is not defined` / `delay is not defined` /
`readableStreamToArray is not defined`) и уже задокументированным
пробелом обзора — невендоренным `/common/gc.js` (`garbageCollect is not
defined`, тот же класс, что `FileAPI`/`custom-elements`/`js`). Остальной
объём FAIL (byobRequest не определён, `pull()`/`flush()` не вызываются
повторно, транзитные VideoFrame-сериализации) соответствует
собственному doc-комментарию модуля (`dom.rs:5630-5634`,
«synchronous-friendly model», «async pull callbacks are not
re-invoked») — задокументированное упрощение Phase 0, не находка.

Живая проба (`--mcp-live-port`) на форме самих конструкторов нашла
независимый WebIDL-дефект, не покрытый ни одной из причин выше:

```json
"ReadableStream() без new"    -> undefined, не бросает
"globalThis._rs_state и т.п." -> появляются на globalThis после вызова:
  ["_rs_do_close","_rs_make_body_stream","_rs_state","_rs_error",
   "_rs_reader","_rs_cancel_fn","_rs_pull_fn","_rs_ctrl"]
"WritableStream() без new"    -> тот же паттерн (globalThis._ws_* появляются)
"TransformStream() без new"   -> globalThis.readable / globalThis.writable
                                  становятся объектами (!)
"ReadableStream.prototype[Symbol.asyncIterator]" -> undefined
"ReadableStream.prototype.values"                -> undefined
```

Все три конструктора — обычные ES5 `function X(...) { this._foo = ...; }`
без проверки `new.target`. Вызов без `new` в нестрогом режиме биндит
`this` на `globalThis`, поэтому каждое поле, которое конструктор
устанавливает, становится одноимённым глобалом. Особенно опасны
`TransformStream()`: она пишет `this.readable`/`this.writable` —
предельно общие имена, с высокой вероятностью коллизии с переменными
любой сторонней библиотеки или страницы. Спека (WHATWG Streams §3-5)
определяет публичные конструкторы для всех трёх интерфейсов, но по
WebIDL операция-конструктор, вызванная без `new`, обязана бросать
`TypeError` — здесь она вместо этого молча выполняется и портит глобал.

Отдельно: `ReadableStream` по спеке асинхронно итерируема
(`ReadableStream.prototype[Symbol.asyncIterator]`, alias `values()`,
WHATWG Streams §4.1) — метод отсутствует целиком, что и объясняет
кластер FAIL `s[Symbol.asyncIterator] is not a function` /
`rs.values is not a function` в самом прогоне (независимо от
BUG-346/gc.js, эти тесты используют `for await` на исполнившихся,
не-404-нутых стримах).

## Масштаб

Тот же класс дефекта, что уже открыт для `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)), `FileSystemFileHandle`
([BUG-374](BUG-374-OPEN.md)), `Serial`/`SerialPort`
([BUG-672](BUG-672-OPEN.md)) и `StorageManager`/`StorageBucket`/
`StorageBucketManager` ([BUG-681](BUG-681-OPEN.md)) — седьмая-девятая
независимая поверхность одного системного паттерна (ни один шим не
ставит guard на `new.target`), на этот раз в `dom.rs`'s собственной
реализации Streams, а не в отдельном модуле. Здесь риск выше обычного:
`readable`/`writable` — не редкие внутренние имена вроде `_rs_state`, а
общеупотребимые идентификаторы, которые случайный вызов
`TransformStream()` без `new` (например, из багованного полифилла
стороннего сайта) молча превратит в глобалы.

## Причина

Не установлена детально (вне скоупа WPT-VENDOR-задачи). `ReadableStream`
(`dom.rs:5681`), `WritableStream` (аналогичная функция чуть ниже) и
`TransformStream` (`dom.rs:5927`) объявлены как обычные функции без
проверки `new.target !== undefined`. Отсутствие async-итератора —
отдельная, не связанная с guard'ом находка: `ReadableStream.prototype`
просто не содержит ни `Symbol.asyncIterator`, ни `values`.

## Дальше

Fix scope: (1) добавить `new.target`-guard во все три конструктора —
тот же паттерн фикса, что предстоит для BUG-629/374/672/681 (общий
guard-механизм имеет смысл вводить один раз для всех публично
конструируемых WebIDL-интерфейсов сразу, а не по одному на баг); (2)
реализовать `ReadableStream.prototype[Symbol.asyncIterator]`
(тривиально поверх существующего `getReader()`/`read()`, aliased как
`values()`) — независимый от guard'а, самостоятельно ценный фикс,
который сразу закроет заметную долю сигнала следующего прогона той же
категории. Не требует HTTPS/TLS для воспроизведения/фикса — живой
`--mcp-live-port`-пробы достаточно для верификации.
