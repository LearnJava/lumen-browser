# BUG-436 — инструмент `type` не записывает значение в поле: `input.value` остаётся `""`, при этом событие `input` диспатчится (с пустым `value`)

**Статус:** OPEN
**Компонент:** driver/MCP (`crates/driver` — реализация `BrowserSession::type` для live-window сессии; связано с BUG-383 — рефлексия `value` есть, но пишет мимо неё)
**Найден:** 2026-07-29, внешний прогон MCP live-window (`--mcp-live-port`) против собственной тестовой страницы и против SPA-стенда

## Симптом

Вызов `tools/call {"name":"type","arguments":{"target":{"selector":"#inp"},"text":"abc"}}`
возвращает успех и эхо введённого текста:

```json
{"success": true, "text": "abc"}
```

Однако значение в DOM не меняется, а обработчик события `input` при этом **срабатывает**
и видит пустую строку:

```
input.value                     → ""
log.textContent (из oninput)    → "input:"      ← событие пришло, this.value пуст
```

Поле остаётся визуально пустым и на скриншоте (`resource://screenshot`).

Предварительный клик по полю (`click` по тому же селектору, success) ничего не меняет —
поведение идентично.

## Репро

`D:/Temp/lumentest/form.html` (можно положить куда угодно, важен только `file://`):

```html
<!doctype html><html><head><meta charset="utf-8"></head><body>
<form id="f" action="result.html"><input id="inp" type="text"><button id="btn" type="submit">Go</button></form>
<div id="log">init</div>
<script>
document.getElementById('inp').addEventListener('input', function(){
  document.getElementById('log').textContent = 'input:' + this.value;
});
</script></body></html>
```

```bash
target/dev-release/lumen.exe --mcp-live-port 9224 --no-scrollbar about:blank
```

```
navigate file:///.../form.html
wait     document_ready
type     {"target":{"selector":"#inp"},"text":"abc"}   → {"success":true,"text":"abc"}
eval     document.getElementById('inp').value          → ""            ← ожидалось "abc"
eval     document.getElementById('log').textContent    → "input:"      ← ожидалось "input:abc"
```

## Ожидалось

`type` вводит текст так же, как пользователь: значение попадает в `input.value`,
события `input`/`beforeinput` несут актуальное значение, поле отрисовывается с текстом.

## Почему это блокирует

`type` — один из двух инструментов, ради которых MCP предпочитают чистому BiDi
(«клик и ввод напрямую по CSS-селектору»). Сейчас любой сценарий с формой
недостижим: логин, поиск, фильтры. Вместе с [BUG-437](BUG-437-FIXED.md) (клик не
диспатчит `click`/`submit`) это делает MCP непригодным для E2E-тестирования
сайтов с формами — проверено на реальном стенде: страницу входа заполнить и
отправить не удалось ни одним способом.

Обойти из JS тоже нельзя: `eval` присваивает `value` корректно, но отправить
форму нечем — `form.submit`/`form.requestSubmit`/`element.click()` отсутствуют
([BUG-383](BUG-383-OPEN.md)).
