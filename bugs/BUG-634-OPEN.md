# BUG-634 — `BlobEvent` constructor doesn't validate required init dict / member, `timecode` defaults to 0 instead of NaN

**Статус:** OPEN
**Компонент:** js (`crates/js/src/media_stream_recording.rs`)
**Найден:** 2026-08-05, P2, WPT-VENDOR-mediacapture-record

## Симптом

`tests/wpt/mediacapture-record/BlobEvent-constructor.html` — единственный тест
категории, не зависящий от `HTMLCanvasElement.captureStream()` (уже
задокументированный отдельный пробел, см. `mediacapture-fromelement`/
`mediacapture-image`) — исполняется до конца (harness OK), но 3 из 5
подтестов падают:

```
FAIL The BlobEventInit dictionary is required
  - assert_throws_js: function "function() { new BlobEvent('dataavailable'); }"
    didn't throw

FAIL The BlobEventInit dictionary's data member is required.
  - assert_throws_js: function "function() { new BlobEvent('dataavailable', {}); }"
    didn't throw

FAIL The BlobEvent instance's timecode defaults to NaN when not specified.
  - assert_equals: timecode defaults to NaN expected NaN but got 0
```

## Причина (найдено чтением кода)

`crates/js/src/media_stream_recording.rs`, JS-шим `BlobEvent`:

```js
function BlobEvent(type, init) {
  ...
  init = init || {};
  this.data = (init.data instanceof Blob) ? init.data : new Blob([]);
  this.timecode = (typeof init.timecode === 'number') ? init.timecode : 0;
}
```

Спека (W3C MediaStream Recording §4.2, `BlobEventInit`) требует:
- `init` (второй аргумент конструктора) обязателен — вызов без него должен
  бросать `TypeError` (WebIDL: обязательный dictionary-аргумент).
- `init.data` обязателен внутри `BlobEventInit` — отсутствие должно бросать
  `TypeError`.
- `timecode` — необязательный член, дефолт **NaN** (double, не ограничен
  снизу/сверху), а не `0`.

Текущий шим вместо валидации подставляет пустой `Blob`/`0` — ни одна из трёх
проверок не производится.

## Как повторить

```bash
MSYS2_ARG_CONV_EXCL='*' tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_report.py --binary 'D:/RustProjects/lumen-browser/target/dev-release/lumen.exe' \
  --all --root mediacapture-record --recursive --out .tmp/wpt-mediacapture-record.html
# BlobEvent-constructor.html: Subtests passed 2/5
```

Живая проверка (не требует WPT):

```js
new BlobEvent('dataavailable');            // должно бросить TypeError, не бросает
new BlobEvent('dataavailable', {});         // должно бросить TypeError, не бросает
new BlobEvent('dataavailable', {data: new Blob([])}).timecode; // NaN ожидается, 0 получено
```

## Не путать с

Отсутствием `HTMLCanvasElement.captureStream()` — это причина падения
подавляющего большинства остальных тестов категории (`canvas.captureStream is
not a function`, уже задокументированный Phase-0 пробел, не предмет этого
бага). Этот баг — только про сам `BlobEvent`, который в остальном (событие,
`data`/строгий `Blob`-инстанс при явной передаче) реализован корректно.
