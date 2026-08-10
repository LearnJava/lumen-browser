# BUG-438 — `navigate` на `data:`-URL молча не загружает страницу: `success: true` и `wait document_ready` тоже успешны, но в окне остаётся предыдущий документ

**Статус:** OPEN
**Компонент:** driver/MCP (`crates/driver` — `BrowserSession::navigate` для live-window сессии) либо шелл-навигация по `data:`-схеме
**Найден:** 2026-07-29, внешний прогон MCP live-window (`--mcp-live-port`)

## Симптом

```
navigate {"url":"data:text/html;charset=utf-8,%3C%21doctype%20html%3E%3Chtml%3E%3Cbody%3E%3Cdiv%20id%3Dx%3Ehello%3C/div%3E%3C/body%3E%3C/html%3E"}
  → {"success": true, "url": "data:text/html;charset=utf-8,%3C%21doctype…"}
wait     {"condition":"document_ready"}
  → {"success": true}

query    "#x"    → 0 узлов          ← ожидался 1
query    "body"  → 1 узел
eval     document.body.textContent
  → содержимое ПРЕДЫДУЩЕЙ страницы (form.html), а не "hello"
```

То есть навигация не состоялась вовсе, но об этом никак не сообщается: и
`navigate`, и `wait` рапортуют успех, а `query "body"` находит body — от старого
документа.

Если до этого не открывалось ничего (`about:blank` как стартовая страница), то
симптом выглядит иначе и ещё запутаннее: `type` по селектору с несуществующей
страницы отвечает `{"success": true}`, `click` по тому же селектору — уже
`{"code": -32603, "message": "Click error: Element not found"}`, а `eval` —
`{"code": -32603, "message": "Eval error: JS context not available"}`. Именно
эта комбинация ошибок и вывела на баг.

## Ожидалось

Либо `data:`-URL загружается (в `docs/testing-your-site-with-lumen.md` он
упомянут как поддерживаемая схема в `navigate`, и `data:`/`blob:` — единственные
рабочие URL для `new Worker`, то есть схема в движке живая), либо `navigate`
возвращает ошибку. Молчаливый no-op с `success: true` — худший из вариантов:
тест продолжает выполняться поверх чужого документа и падает позже и не там.

## Побочная находка (в доке — неточность)

`docs/testing-your-site-with-lumen.md`, раздел «Известные ограничения MCP»,
утверждает:

> **`eval` — то же ограничение, что и в BiDi**: работает только до первого
> `navigate`. После него `eval` возвращает ошибку `"JS context not available"`.

Замерено обратное: после `navigate` на `file:///…/form.html` и на
`http://my-stand.local/dashboard` (реальный сайт, редирект на Keycloak) `eval`
работает — `typeof document` → `"object"`, чтение и запись `input.value`,
`document.getElementById(...).textContent` возвращают верные значения.
Ошибка `"JS context not available"` воспроизводится только там, где документ
фактически не загрузился, — то есть это симптом настоящего бага (данного),
а не самостоятельное ограничение `eval`. Формулировку в доке стоит поправить,
иначе она уводит от диагностики.

## Скоуп шире, чем `data:` (2026-08-10, P3, при разборе [BUG-380](BUG-380-FIXED.md))

Прямая проба по BiDi против `lumen --bidi-port` (три способа завалить
навигацию, все с живого `file://`-документа):

| URL | ответ `browsingContext.navigate` | что в окне после |
|---|---|---|
| `http://127.0.0.1:<закрытый порт>/nope.html` | `{navigation, url}` — **успех** | прежний документ, `location.href` прежний |
| `D:\…\does-not-exist.html` | `{navigation, url}` — **успех** | то же |
| `http://127.0.0.1:None/x.html` (битый порт) | `{navigation, url}` — **успех** | то же |

То есть молчаливый no-op с `success: true` — не свойство схемы `data:`, а общее
поведение любой не состоявшейся загрузки: ни BiDi-ошибки, ни страницы ошибки,
ни смены `location.href`. `bc_navigate`
(`crates/bidi-server/src/protocol.rs`) поднимает ошибку только когда сорвались
сами `LiveWindowSession::navigate` или ожидание `DocumentReady`; провал самой
загрузки сообщается асинхронно и до BiDi-ответа не доезжает, а
`LiveWindowSession::navigate` (`crates/driver/src/live_session.rs`) вдобавок
пишет запрошенный URL в `current_url` ещё до попытки загрузки — так что и
`current_url` после провала врёт.

Починка `data:`-случая, не покрывающая этот общий путь, закроет заявку лишь
частично.
