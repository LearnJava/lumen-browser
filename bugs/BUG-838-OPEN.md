# BUG-838 — `<script src="">` (пустой `src`) не даёт ни `error`, ни `load`: элемент молча трактуется как пустой инлайн-скрипт

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:5844`–`5849` (`_lumen_script_prepare`: пустой `src` после `trim()` уходит в ветку инлайн-тела и выходит по `return`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var s = document.createElement("script");
s.onerror = () => console.log("error");   // не приходит
s.onload  = () => console.log("load");    // тоже не приходит
s.setAttribute("src", "");
document.body.appendChild(s);
```

Ни события, ни сообщения в stderr, ни сетевого запроса. Страница, которая
ждёт `error` (а это ровно то, что предписывает спека), висит.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant script-src-empty`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 6`,
страница жива — 11 тиков):

| ожидалось | получено |
|---|---|
| `script-error type=error` | `appended-empty-src` — и всё |

## Причина (локализована чтением кода)

```js
var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
src = (src === null) ? '' : String(src).trim();          // dom.rs:5845
if (src !== '') { _lumen_script_load_external(nid, src, isModule); return; }
var body = _lumen_u2n(_lumen_get_text_content(nid));
if (body === null || String(body).trim() === '') return;  // dom.rs:5849 — тихий выход
```

Отсутствующий и пустой `src` сведены в одно значение `''`, после чего
элемент идёт по инлайн-ветке и выходит без событий, потому что тела нет.
HTML LS §4.12.1 «prepare the script» различает эти случаи: если атрибут
`src` присутствует и его значение — пустая строка, шаги требуют
**поставить задачу, которая выстрелит `error`** (то же самое, что при
неудачной загрузке).

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: на этой форме в остатке
WPT-RUN-5 стоят два id —
`html/semantics/scripting-1/the-script-element/fetch-src/empty.html` и
`empty-with-base.html`, — и обоих мало для отдельного правила, которое
надёжно отделяло бы `src=""` от обычного внешнего скрипта по исходнику.
Заводится по прямому замеру, как [BUG-825](BUG-825-OPEN.md).

Родственный (но отдельный) дефект — [BUG-804](BUG-804-OPEN.md): парсерная
вставка `<script src>` вообще не доставляет `load`. Здесь речь о другом
входе (элемент создан скриптом) и о другом событии.

## Направление починки (не предписание)

Разделить «атрибута нет» и «атрибут пустой»: сохранить результат
`_lumen_get_attr` до нормализации и, если атрибут присутствует, но пуст,
вызвать `_lumen_resource_fire(nid, 'error')` через `setTimeout(…, 0)` (тем
же способом, каким уже отправляется `error` при провале внешней загрузки,
`dom.rs:5905`).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant script-src-empty` — ожидается `script-error type=error`.
2. WPT: `run_report.py --all --root html/semantics/scripting-1/the-script-element/fetch-src --recursive`.
