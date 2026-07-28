# BUG-362 — `EventSource` не резолвит относительный URL: `new EventSource("resources/message.py")` падает `missing scheme`, а `.url` возвращает переданную строку вместо абсолютного URL

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8194` — конструктор `EventSource`), js (`crates/js/src/v8_runtime.rs:2646` — биндинг `_lumen_sse_connect`)
**Найден:** P2, WPT-VENDOR-eventsource (2026-07-28), `run_report.py --all --root eventsource --recursive`

## Симптом

Страница, отданная с `http://127.0.0.1:8300/eventsource/format-utf-8.any.html`, делает

```js
var source = new EventSource("resources/message.py?mime=text/event-stream&message=data%3Aok")
```

и шелл пишет в лог:

```
[JS SSE] connect error: network error: sse: invalid URL: invalid url: missing scheme: "resources/message.py?mime=text/event-stream%3bcharset=windows-1252&message=data%3Aok%E2%80%A6"
```

Относительный URL уходит в сетевой слой дословно, без резолва относительно базы
документа. Соединение не устанавливается никогда.

Вторая, отдельно наблюдаемая половина того же дефекта — атрибут `url`. По спеке
(HTML Living Standard §9.2.2) конструктор парсит URL относительно API base URL
и атрибут `url` возвращает **сериализованный абсолютный** URL. Проба
`--dump-layout` вне WPT:

```
new EventSource("")                                → .url = [] len=0
new EventSource("relative.py")                     → .url = [relative.py] len=11
new EventSource("/eventsource/resources/message.py") → .url = [/eventsource/resources/message.py]
```

Во всех случаях возвращается ровно переданная строка.

Это то же семейство отказов, что BUG-347 (`fetch()` не резолвит относительные
URL), BUG-359 (междокументная навигация не резолвит) и BUG-346 (`Url::resolve()`
не схлопывает `..`), но **четвёртый независимый сайт** — ни один из тех путей не
проходит через SSE-стек.

## Причина

Резолва нет ни на одном из двух звеньев цепочки.

1. **Шим (`dom.rs:8194`)** — первая строка конструктора:

   ```js
   function EventSource(url, opts) {
       this.url = String(url || '');
   ```

   Строка сохраняется как есть и в таком же виде уходит в
   `_lumen_sse_connect(this.url)` (`dom.rs:8216`). Базы документа
   (`_lumen_loc_href`, который уже используется в `_lumen_navigate_or_fragment`)
   конструктор не касается вовсе.

2. **Биндинг (`v8_runtime.rs:2646`)** — принимает строку и передаёт её
   провайдеру без изменений:

   ```rust
   reg!("_lumen_sse_connect", move |url: String| -> u32 {
       let Some(ref provider) = sp else { return 0 };
       match provider.connect_sse(&url) {
   ```

   `connect_sse` парсит URL как абсолютный, отсюда `missing scheme`.

Побочно: `String(url || '')` вместо `String(url)` ломает стрингификацию ложных
значений независимо от резолва — `new EventSource(null).url` даёт `""` вместо
`"null"`, `new EventSource(undefined).url` даёт `""` вместо `"undefined"`. На
этом падает `eventsource-constructor-stringify.window.html` (3 сабтеста FAIL).
Тот же `|| ''` стоит в конструкторе `Worker` (`crates/js/src/worker.rs:659`).

## Масштаб

**Доминирующая причина отказов всей категории.** Из 61 id `eventsource` почти
каждый тест начинается с `new EventSource("resources/…")`, поэтому:

- **44 id — TIMEOUT**, из них 31 в корне категории: соединение не открывается,
  `onopen`/`onmessage` не приходят никогда, `async_test` висит до 10-секундного
  таймаута гарнеса (это ~7 из 8 минут прогона категории);
- часть выполнившихся тестов падает по этой же причине через `onerror →
  assert_unreached` (например `format-utf-8.any.html`, harness OK, 0/1);
- `eventsource-constructor-empty-url.any.html` ловит вторую половину напрямую:
  `assert_equals(source.url, self.location.toString())` → `expected
  "http://127.0.0.1:8300/eventsource/eventsource-constructor-empty-url.any.html"
  but got ""`.

Отдельно стоит отметить **ложноположительный** тест: `eventsource-url.any.html`
— один из двух PASS категории — проверяет `source.url.substr(-url.length) ===
url`, то есть лишь что `url` **оканчивается** на переданный относительный путь.
Нерезолвленная строка проходит эту проверку тривиально. Тест зелёный при
полностью сломанном резолве; чинить баг, ориентируясь на него, нельзя.

За пределами WPT: `new EventSource('/api/stream')` с путём, а не полным URL —
обычная запись в реальных приложениях (SSE почти всегда ходит на свой же
origin), так что SSE в Lumen сейчас работает только если страница явно
пропишет абсолютный URL со схемой.

## Возможный фикс (не реализован в этой сессии)

- `dom.rs:8194`: резолвить в конструкторе тем же приёмом, что уже применён в
  `_lumen_navigate_or_fragment` (`dom.rs:7450`):
  `try { this.url = new URL(String(url), _lumen_loc_href).href } catch (e) { … }`.
  Заодно `String(url)` вместо `String(url || '')` чинит стрингификацию.
  На `catch` должен бросаться `SyntaxError` DOMException — см. BUG-363.
- Либо/дополнительно — резолвить один раз на Rust-стороне в
  `_lumen_sse_connect` (`v8_runtime.rs:2646`), чтобы точка была одна; тогда
  биндингу нужен URL документа. Делать в обоих местах безвредно (резолв
  идемпотентен для абсолютных URL).
- BUG-346 (`..` не схлопывается в `Url::resolve()`) лежит на Rust-овом пути
  резолва — чинить первым, если резолв переносится в Rust.
- Проверять фикс на `eventsource-constructor-empty-url.any.html`, а **не** на
  `eventsource-url.any.html` (см. выше про ложноположительный тест).

Не чинится в этой сессии — P2-wpt вендорит и обследует, фиксы кода — дорожка P3
(`CLAUDE.md`, назначения разработчиков).
