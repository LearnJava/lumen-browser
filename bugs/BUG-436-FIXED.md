# BUG-436 — инструмент `type` не записывает значение в поле: `input.value` остаётся `""`, при этом событие `input` диспатчится (с пустым `value`)

**Статус:** FIXED 2026-07-29 (P1)
**Компонент:** shell (`crates/shell/src/main.rs` — `inject_char`, новые `typeable_field`/`edit_focused_field`/`inject_backspace`, обработчик `AutomationCommand::Type`, ветка ввода в `handle_key`), js (`crates/js/src/dom.rs` — `_lumen_set_field_value` в `WEB_API_SHIM`)
**Найден:** 2026-07-29, внешний прогон MCP live-window (`--mcp-live-port`) против собственной тестовой страницы и против SPA-стенда

## Симптом

Вызов `tools/call {"name":"type","arguments":{"target":{"selector":"#inp"},"text":"abc"}}`
возвращал успех и эхо введённого текста:

```json
{"success": true, "text": "abc"}
```

Однако значение в DOM не менялось, а обработчик события `input` при этом **срабатывал**
и видел пустую строку:

```
input.value                     → ""
log.textContent (из oninput)    → "input:"      ← событие пришло, this.value пуст
```

Поле оставалось визуально пустым и на скриншоте (`resource://screenshot`).

Предварительный клик по полю (`click` по тому же селектору, success) ничего не менял —
поведение идентично.

## Репро

Страница (важен только `file://`):

```html
<!doctype html><html><head><meta charset="utf-8"></head><body>
<form id="f" action="result.html"><input id="inp" type="text"><button id="btn" type="submit">Go</button></form>
<textarea id="ta">seed</textarea>
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

## Причина

**У движка в живом окне вообще не было default action «вставить текст в форму».**

`Lumen::inject_char` (единственный путь ввода и для MCP/BiDi, и для инъекции по
IPC, ADR-007 §8C) диспатчил три события — `keydown` → `input` → `keyup` — через
`_lumen_dispatch_key_event` и на этом заканчивался. Сам JS-шим события только
*доставляет*; изменение значения контрола — default action движка
(HTML LS §4.10.5.5), и в шелле его не было ни в одной точке: ни в
`inject_char`, ни в `handle_key`. Единственное место в кодовой базе, где этот
шаг реализован, — headless-драйвер (`InProcessSession::dispatch_type`,
`crates/driver/src/session.rs`), поэтому те же сценарии в `--mcp` работали, а в
`--mcp-live-port` — нет.

Отсюда все три наблюдаемых следствия сразу: `input.value` пуст, обработчик
`input` видит пустую строку (событие-то настоящее), поле пусто на экране, и
форма отправляет пустое поле — сбор данных читает `value`-атрибут из DOM.

**Тем же дефектом сломан и ввод обычной клавиатурой**, а не только автоматизация:
печатный символ в `handle_key` проваливался мимо всех веток прямо в глобальную
таблицу горячих клавиш, где голая `F` открывает hint-режим, а пробел листает
страницу. Ветка для `contenteditable` там была, для `<input>`/`<textarea>` — нет.

Рядом найдены и починены два дефекта того же пути:

* `escape_js_string_char` не экранировал апостроф, хотя **все** места
  подставляют его результат в одинарные кавычки (`'{key}'`). Ввод `'` строил
  `_lumen_dispatch_key_event(3, 'keydown', ''', ''', …)` — синтаксическая
  ошибка, весь скрипт диспатча молча терялся.
* `AutomationCommand::Type` отвечал `Ack` и когда цель не резолвилась вовсе, и
  когда фокус стоял на нередактируемом элементе. Это вторая половина сигнатуры
  «успех без эффекта» — теперь это честные `Element not found` и
  `Element is not a mutable text field` (как у `Click`).

## Починка

* `Lumen::typeable_field(nid)` — классифицирует узел как изменяемый текстовый
  контрол и читает *отрисованное* значение: `value`-атрибут для `<input>`
  текстовых типов (тот же набор, что принимает `InProcessSession::type_text`),
  текстовые узлы-потомки для `<textarea>` (HTML LS §4.10.11). `disabled` и
  `readonly` отсекаются (HTML LS §4.10.19.2).
* `Lumen::edit_focused_field(edit)` — сам default action: пишет новое значение
  в DOM, обновляет `form_state` (runtime-оверлей, который читают сбор формы и
  проверка ограничений), синхронизирует JS-тень значения через новый
  `_lumen_set_field_value` и перевёрстывает (`relayout_form`). JS под локом
  документа не диспатчится — ловушка из [BUG-437](BUG-437-FIXED.md).
* `inject_char` вызывает его **между** `keydown` и `input`, поэтому обработчик
  `input` читает уже новое `this.value`; добавлен `inject_backspace`.
* `handle_key` получил ветку ввода в фокусированный `<input>`/`<textarea>` —
  перед таблицей горячих клавиш, ровно там же, где стоит ветка
  `contenteditable`.
* `_lumen_set_field_value(nid, value)` в шиме: `el.value` читает
  `_input_values[nid]` раньше атрибута, поэтому без синхронизации поле, которому
  страница когда-либо присваивала значение скриптом, продолжало бы отдавать
  старое.

## Проверено

Живое окно, `--mcp-live-port`, `target/dev-release/lumen.exe`:

```
type {"target":{"selector":"#inp"},"text":"ab'c"}   → {"success":true,"text":"ab'c"}
eval inp.value                     → "ab'c"      (в т.ч. апостроф — прежде ронял диспатч)
eval inp.getAttribute('value')     → "ab'c"      (то, что видят вёрстка и отправка формы)
eval log.textContent               → "input:ab'c"  (обработчик увидел новое значение)
type {"target":{"selector":"#ta"},"text":"XY"}     → textarea "seed" → "seedXY"
type по <div>                      → Error: Element is not a mutable text field
type по несуществующему селектору  → Error: Element not found
```

`resource://screenshot` после `type "Hello"` показывает поле с текстом `Hello`
и `#log` = `input:Hello` (прежде — пустое поле и `init`). Кадр приходит с
задержкой в несколько кадров: `relayout_form` при включённом движковом потоке
(ADR-023, дефолт) коммитит вёрстку асинхронно, поэтому скриншот сразу после
`type` может ещё показывать старый display list — это штатное поведение
`relayout_form`, а не остаток этого бага.

Юнит-тесты: `lumen-js::dom::tests::set_field_value_syncs_value_shadow`,
`lumen-shell::tests::escaped_char_is_safe_inside_single_quoted_js_literal`,
`escaped_string_escapes_every_quote`.

## Остаток

[BUG-441](BUG-441-FIXED.md) — присвоение `element.value` из скрипта по-прежнему
живёт только в JS-тени: читается обратно верно, но не доезжает ни до рендера,
ни до отправки формы.
