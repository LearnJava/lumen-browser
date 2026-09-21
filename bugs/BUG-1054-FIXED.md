# BUG-1054 — `CharacterData.data`/`Node.nodeValue` setter не приводит `null` к `""`

**Статус:** FIXED 2026-09-21 (P3)
**Заведён:** 2026-09-15 (GAP-XMLDOC срез 25 P1, живой прогон `tests/wpt/run_report.py --all --root dom/nodes`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_character_data`, `_LUMEN_WRAPPER_CD_MEMBERS`, `_lumen_make_processing_instruction`)
**Владелец:** P3

## Симптом

`dom/nodes/Node-nodeValue.html` — все три сабтеста (`Text.nodeValue`,
`Comment.nodeValue`, `ProcessingInstruction.nodeValue`) падают на одной и
той же последней строке:

```js
the_text.nodeValue = null;
assert_equals(the_text.nodeValue, "");   // получаем "null"
```

DOM §4.4/§4.10: и `Node.nodeValue`, и `CharacterData.data` — `[LegacyNullToEmptyString]`
для `data`, а `nodeValue`'s setter algorithm явно проговаривает: «if the new
value is null, act as if it was the empty string instead». Ни один из трёх
сайтов, где эти сеттеры реализованы, этого не делает — все три просто зовут
`String(v)`, а `String(null) === "null"`:

- `_lumen_make_character_data` (детач `new Comment()`/`new Text()`) — общий
  `data`/`nodeValue`/`textContent` дескриптор.
- `_LUMEN_WRAPPER_CD_MEMBERS` (арена-backed `document.createComment`/
  `createTextNode`, живые текстовые/comment-узлы) — `data`/`nodeValue`
  зовут `_lumen_set_text_content(this.__nid__, String(v))`.
- `_lumen_make_processing_instruction` (детач
  `document.createProcessingInstruction`) — тот же паттерн, третья
  независимая копия.

## Цена

Три сабтеста в одном WPT-файле напрямую; вероятно шире — любой скрипт,
явно присваивающий `node.nodeValue = null`/`node.data = null` (нередкий
идиоматический способ «очистить» текстовый узел), получает буквальный текст
`"null"` вместо пустой строки.

## Что дальше

Не XML-специфично (общий DOM-дефект, Text/Comment/PI одинаково), поэтому не
взят в GAP-XMLDOC/BUG-786 — три сайта надо чинить вместе одним срезом, чтобы
не разъезжались: приводить `null` к `""` до `String(v)` (или сразу
`v === null ? '' : String(v)`) в каждом из трёх сеттеров выше. `undefined`
не входит в `LegacyNullToEmptyString` — `String(undefined) === "undefined"`
остаётся как есть (WPT это и проверяет отдельно для других типов, не для
`null`).

## Исправлено (2026-09-21, P3)

`v === null ? '' : String(v)` вместо голого `String(v)` — тот же паттерн, что
уже был у `HTMLElement.innerText`'а сеттера (`web_api_shim_mid.js:6930`).
Четыре сайта, не три — при разборе нашёлся четвёртый, не названный в
заявке явно, но того же класса и в том же файле: `_LUMEN_WRAPPER_PI_MEMBERS`
(живой, арена-backed `ProcessingInstruction` — `document.createProcessingInstruction`
с последующей вставкой в дерево, отдельная от детач-объекта
`_lumen_make_processing_instruction`, у которой `data`/`nodeValue` идут через
собственный литерал). Все четыре:

- `_lumen_make_character_data` (`data`/`nodeValue`/`textContent`, общий
  дескриптор для детач `new Comment()`/`new Text()`);
- `_LUMEN_WRAPPER_CD_MEMBERS` (`data`/`nodeValue`, живые арена-backed
  Text/Comment);
- `_LUMEN_WRAPPER_PI_MEMBERS` (`data`/`nodeValue`, живой arena-backed PI);
- `_lumen_make_processing_instruction` (`data`/`nodeValue`, детач PI).

`textContent` PI-объекта не входит в перечень (PI не выставляет отдельного
`textContent`-сеттера сверх `nodeValue`/`data` в этом шиме).

Проверено `run_smoke.py` на `/dom/nodes/Node-nodeValue.html`: `Text.nodeValue`/
`Comment.nodeValue`/`ProcessingInstruction.nodeValue` — все три PASS (было
FAIL). Два оставшихся FAIL того же файла (`Element.nodeValue`/
`DocumentFragment.nodeValue`, ожидают `null`, получают `undefined`) — другой
класс дефекта (отсутствующий геттер, не пропущенная null-коэрсия сеттера),
заведён отдельно как [BUG-1074](BUG-1074-OPEN.md).

`cargo test -p lumen-js --features v8-backend --lib` — 4101/4101, `cargo
clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` —
чист. Правка чисто JS-шимовая (`.js`-строки, читаются verbatim), pixel-нейтральна
по построению.
