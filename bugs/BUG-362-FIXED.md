# BUG-362 — `EventSource` не резолвит относительный URL: `new EventSource("resources/message.py")` падает `missing scheme`, а `.url` возвращает переданную строку вместо абсолютного URL

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` — конструктор `EventSource`, функция `_url_resolve`)
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

Относительный URL уходил в сетевой слой дословно, без резолва относительно базы
документа. Соединение не устанавливалось никогда.

Вторая, отдельно наблюдаемая половина того же дефекта — атрибут `url`. По спеке
(HTML Living Standard §9.2.2) конструктор парсит URL относительно API base URL
и атрибут `url` возвращает **сериализованный абсолютный** URL. Проба
`--dump-layout` вне WPT (до фикса):

```
new EventSource("")                                → .url = [] len=0
new EventSource("relative.py")                     → .url = [relative.py] len=11
new EventSource("/eventsource/resources/message.py") → .url = [/eventsource/resources/message.py]
```

Во всех случаях возвращалась ровно переданная строка.

Это то же семейство отказов, что BUG-347 (`fetch()` не резолвит относительные
URL), BUG-359 (междокументная навигация не резолвит) и BUG-346 (`Url::resolve()`
не схлопывает `..`) — четвёртый независимый сайт, ни один из тех путей не
проходит через SSE-стек.

## Причина

Резолва не было ни на одном из двух звеньев цепочки.

1. **Шим (`dom.rs`, конструктор `EventSource`)** — первая строка конструктора:

   ```js
   function EventSource(url, opts) {
       this.url = String(url || '');
   ```

   Строка сохранялась как есть и в таком же виде уходила в
   `_lumen_sse_connect(this.url)`. Базы документа (`_lumen_loc_href`, уже
   использованный в `_lumen_navigate_or_fragment`) конструктор не касался
   вовсе.

2. **Биндинг (`v8_runtime.rs::_lumen_sse_connect`)** — принимал строку и
   передавал её провайдеру без изменений; `connect_sse` парсит URL как
   абсолютный, отсюда `missing scheme`.

Побочно: `String(url || '')` вместо `String(url)` ломал стрингификацию ложных
значений независимо от резолва — `new EventSource(null).url` давал `""` вместо
`"null"`, `new EventSource(undefined).url` давал `""` вместо `"undefined"`.

Отдельная находка при фиксе: `_url_resolve` (общая JS-хелпер-функция резолва
относительных URL, используемая также `fetch()`/`Request`/`URL`-конструктором)
неверно резолвила **пустую строку** — вместо base-URL целиком (RFC 3986 §4.2,
same-document reference) она отдавала директорию базы без имени файла
(`dir + '' = dir`), теряя хвост пути. Это отдельный узкий баг того же
резолвера, вскрытый именно тестом `eventsource-constructor-empty-url.any.html`
(`new EventSource("")` должен дать `.url === location.href`).

## Фикс

- `dom.rs`, конструктор `EventSource`: резолв тем же приёмом, что уже применён
  в `_lumen_navigate_or_fragment` —
  `try { this.url = new URL(_rawUrl, _lumen_loc_href).href } catch (e) { this.url = _rawUrl; }`,
  где `_rawUrl = String(url)` (не `String(url || '')` — чинит стрингификацию
  `null`/`undefined`). На `catch` фолбэк на исходную строку — бросок
  `SyntaxError` DOMException остаётся за BUG-363 (WebIDL-конформность
  интерфейса в целом).
- `_url_resolve`: добавлен ранний возврат `if (href === '') return String(base);`
  — пустая относительная ссылка резолвится в сам base URL, а не в его
  директорию.
- Резолв сделан в одном месте (JS-шим) — `_lumen_sse_connect` получает уже
  абсолютный URL, Rust-биндинг не тронут.

4 новых unit-теста в `dom::tests::v8_ws_sse` (`cargo test -p lumen-js
--features v8-backend eventsource`, 17/17 зелёных): резолв относительного пути
+ геттер `.url` абсолютный, резолв пустой строки в URL документа,
стрингификация `null`/`undefined`, и сквозной тест через мок-провайдер SSE
(`_lumen_sse_connect` реально получает разрешённый абсолютный URL). Смежный
прогон `cargo test -p lumen-js --features v8-backend -- url` (177 тестов —
location/URL/fetch/navigate/Worker/URLSearchParams) — без регрессий.

BUG-346 (`..` не схлопывается в Rust-овом `Url::resolve()`) не тронут — лежит
на другом, Rust-стороннем пути резолва, использованном сетевым/навигационным
стеком, а не этой JS-функцией.

## Как проверить (WPT)

Проверять на `eventsource-constructor-empty-url.any.html`, а **не** на
`eventsource-url.any.html` — последний ложноположителен по построению
(проверяет лишь суффикс `url`, нерезолвленная строка тривиально проходит).
