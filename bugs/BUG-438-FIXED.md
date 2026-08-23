# BUG-438 — `navigate` на `data:`-URL молча не загружает страницу: `success: true` и `wait document_ready` тоже успешны, но в окне остаётся предыдущий документ

**Статус:** FIXED 2026-08-23
**Компонент:** `crates/shell/src/main.rs` — `about_to_wait`'s `pending_waits` drain (BUG-308's `load_failed`/`check_wait_condition`)
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

## Скоуп шире, чем `data:` (2026-08-10, P3, при разборе [BUG-380](BUG-380-FIXED.md))

Прямая проба по BiDi против `lumen --bidi-port` (три способа завалить
навигацию, все с живого `file://`-документа) показала тот же молчаливый
no-op с `success: true` на закрытом порту, несуществующем файле и битом URL —
то есть баг не был свойством схемы `data:`, а общим поведением любой не
состоявшейся загрузки.

## Корень

`start_streaming_load` (используется на любой навигации в живом окне) уже
корректно репортит провал загрузки: любая ошибка `source.load_bytes`/
`load_bytes_streaming` шлёт `LoadEvent::LoadError`, а провал финального
рендера — `RenderDone(Err(..))`; оба обработчика выставляют
`self.load_failed = true`. BUG-308 (2026-06-xx) добавил ранний выход в
`check_wait_condition`: осевшая в ошибке навигация — это тоже «загрузка
завершена», иначе `wait{document_ready}` висел бы до дедлайна на странице без
JS-контекста. Но резолюция очереди `pending_waits`
(`about_to_wait`, `crates/shell/src/main.rs`) слепо превращала *любое*
`check_wait_condition() == true` в `AutomationReply::Ack` — не различая
«документ реально загрузился» и «навигация осела в сетевой/HTTP-ошибке».
`bc_navigate` (BiDi) вызывает `live.navigate(&url)`, затем
`live.wait(WaitCondition::DocumentReady, …)` — при `Ack` от `wait` он не видит
повода вернуть ошибку, хотя документ не сменился.

## Фикс

Новое поле `load_error_message: Option<String>` рядом с `load_failed`
(в обеих копиях состояния — активной вкладке и `PageSnapshot`) хранит текст
последней ошибки загрузки; выставляется/чистится в тех же точках, что и
`load_failed`. В `about_to_wait`'s резолюции `pending_waits`: если ожидание
(`DocumentReady`/`NetworkIdle`) удовлетворилось из-за `self.load_failed`
(а не реального `document.readyState == "complete"`), шлётся
`AutomationReply::Error("navigation failed: <message>")` вместо `Ack` — тем же
путём, каким уже отправляется ошибка таймаута ожидания чуть ниже по коду.

Ниже по цепочке ничего менять не потребовалось: `LiveWindowSession::wait`
уже превращает `AutomationReply::Error` в `Err`, `bc_navigate` уже
возвращает `unknown error` при `Err` от `live.wait(...)`, а MCP-инструмент
`wait` уже превращает `Err` в JSON-RPC ошибку `-32603`. `InProcessSession`
(headless-путь) багом не затронут — там `navigate` синхронный и уже
пробрасывает ошибку через `?`.

Тест: `navigate_with_live_window_errors_when_load_settles_in_error`
(`crates/bidi-server/src/protocol.rs`) — фейковая `LiveWindowSession`,
отвечающая на `Wait` так, как теперь отвечает реальный шелл при осевшей
ошибке, проверяет, что `browsingContext.navigate` возвращает `unknown error`
с сообщением `navigation failed: …`, а не успех.

## Остаток (не в скоупе этой правки)

`LiveWindowSession.current_url` (fallback, используется только когда у
`current_url()` ещё нет JS-контекста, чтобы прочитать `location.href`)
по-прежнему пишет *запрошенный* URL внутри `navigate()` — до того, как
известен исход загрузки, потому что `navigate()` асинхронный и
fire-and-forget по конструкции. Не тронуто: вызывающий код теперь получает
честную ошибку из `wait`, раньше, чем успеет довериться `current_url()`.
Полноценный фикс потребовал бы либо блокирующего `navigate()`, либо отдельного
канала уведомления об исходе — не оправдано текущим скоупом.
