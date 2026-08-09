# BUG-571: dynamically-inserted `<script>` elements never execute

**Статус:** FIXED 2026-08-09
**Компонент:** shell (`crates/shell/src/main.rs::collect_scripts_ordered` +
`run_scripts_with_dom`), js (`crates/js/src/dom.rs` — `HTMLScriptElement`
has IDL reflection only, no execution hook)
**Найден:** P2, WPT-VENDOR-html-semantics-scripting-1, 2026-08-04
**Исправлен:** P3, 2026-08-09

## Симптом

`document.createElement('script')` (or `createElementNS` for SVG), followed
by setting `type`/`textContent`/`src` and `appendChild`-ing the element into
a live document, never runs the script — no exception, no side effect,
`window.ran` (or any variable the script would set) simply stays at its
pre-insertion value forever. Canonical repro from
`html/semantics/scripting-1/the-script-element/resources/script-type-and-language-js.js`:

```js
let script = document.createElement("script");
script.setAttribute("type", "text/javascript");
script.textContent = "window.ran = true;";
document.querySelector('#script-placeholder').appendChild(script);
assert_equals(window.ran, true);   // FAIL: got false
```

This single mechanism explains 218 of the 575 `FAIL` lines in
`script-type-and-language-js.html`/`.svg`/`.xhtml` alone (every "Script
should run with type=/language=..." case across the full legacy JavaScript
MIME-type and `language=` matrices — `is_classic_script_type()` at
`main.rs:6625` already implements the correct spec whitelist, but that
function is never consulted for a dynamically-created element because
nothing calls it after initial parse). The same root cause also accounts for
the category's `scheduler:`/ordering tests ("dynamically created external
script executes asynchronously", "Async script element execution delays the
window's load event", etc.) — anything that builds a `<script>` via the DOM
API rather than relying on the parser.

## Причина

Classic-script execution in Lumen is a **one-shot walk**, not a live
mechanism. `run_scripts_with_dom` (`main.rs:6936`) is called exactly once
per navigation (`main.rs:5228`), and internally calls
`collect_scripts_ordered` (`main.rs:6661`) which recursively walks the
already-parsed DOM tree exactly once, classifying every `<script>` node it
finds at that instant into `classic`/`modules` lists and then executing
them. There is no equivalent of the HTML "prepare the script element"
algorithm (HTML LS §8.1.3.1) hooked to node insertion — no
`MutationObserver`-style callback, no native binding fired from
`appendChild`/`insertBefore`/`replaceChild`. `HTMLScriptElement` in the JS
shim (`crates/js/src/dom.rs`, reflection installed at `dom.rs:13838`) is a
plain reflected-attribute wrapper with zero execution semantics attached to
insertion.

Consequently **every dynamically-created `<script>` element on any page is
inert**, regardless of `type`, `src`, `async`/`defer`, or insertion method.
This is one of the most common real-world script-loading patterns (lazy
analytics/ads loaders, dynamic polyfill loading, most bundler runtime
chunk-loading shims that don't go through `<script type=module>`), so the
practical impact reaches far beyond WPT.

## Масштаб

At least 60 files in `html/semantics/scripting-1/the-script-element/` alone
use `createElement("script")` + insertion (`grep -rl
'createElement("script")\|createElement(\'script\')'`). Within just the one
`script-type-and-language-js` fixture (shared by 3 test files —
`.html`/`.svg`/`.xhtml`): 218 subtests. Very likely the dominant cause of
the `scheduler:`/async-ordering failure cluster observed across the rest of
the category (not separately quantified here — those tests mix this defect
with legitimate ordering-semantics gaps).

Distinguish from [BUG-446](BUG-446-OPEN.md) (network-loaded *module* import
graph) and [BUG-568](BUG-568-OPEN.md) (`document.write()`) — both are about
different script-loading paths; this one is specifically "script created via
DOM API, whether classic or module, whether inline or `src`, is never
executed at all, in a page that has already finished its initial parse".

## Блокирует BUG-703 (P3, 2026-08-09)

Диагностика [BUG-703](BUG-703-OPEN.md) (`https://www.tbank.ru/` не рендерит
React) упёрлась ровно в этот дефект: webpack-загрузчик чанков
(`a.l` в `platform.<hash>.js`) создаёт `<script>`, вешает `onload`/`onerror`,
кладёт его в `document.head` и ставит сторожевой `setTimeout(..., 24e4)`.
В Lumen элемент попадает в DOM (`document.querySelectorAll('script[data-webpack]')`
находит его с правильным `src`), но **сети по нему нет вовсе** — ни запроса,
ни `load`, ни `error`, поэтому промис `a.e()` висит вечно и асинхронный
бутстрап приложения молча останавливается. Наблюдаемый признак прямо на
живой странице: сторожевой таймаут на 240 000 мс остаётся pending.

Живой прототип фикса (проверен на tbank.ru, 2026-08-09): полифилл в чистом
JS — перехват `appendChild`/`insertBefore` на `document.head`, затем
`fetch(src).then(text => (0, eval)(text))` и диспатч `load`/`error` на
элементе — сдвинул страницу с 0 до 11 React-узлов (React начал монтировать),
т.е. механизм именно этот. Полного рендера полифилл не дал (в Edge на той же
странице 2380 React-узлов), значит за BUG-571 в этой цепочке стоит ещё
что-то — но следующий блокер BUG-703 не диагностируется, пока 571 открыт.

Практический вывод для реализации: тела скриптов не обязательно тянуть через
шелл — в шиме уже есть `fetch`, а косвенный `(0, eval)(text)` даёт ровно
семантику классического скрипта (глобальная область видимости). Порядок:
динамически созданный скрипт по спеке async, если явно не выставлен
`script.async = false`.

## Исправление (P3, 2026-08-09)

Живая половина алгоритма «prepare the script element» реализована целиком в
шиме (`crates/js/src/dom.rs`, блок `_lumen_script_*`) — шелл не тронут, его
одноразовый обход парсерных скриптов остался как был.

**Как элемент попадает под алгоритм.** `document.createElement` и
`createElementNS` вызывают `_lumen_script_track(nid, tag)`, который кладёт nid
`<script>`-элемента в `_lumen_script_pending`. Это и есть весь фильтр: скрипты
от парсера документа и от парсера фрагментов (`innerHTML` /
`insertAdjacentHTML` / `document.write`) туда не попадают никогда, а значит по
построению не могут исполниться отсюда — ровно то, что спека выражает флагом
«already started». Удаление записи при подготовке делает карту же и этим
флагом: перемещение уже отработавшего скрипта по дереву второй раз его не
запускает.

**Где ловится вставка.** Вместо правки ~30 мест шима, вставляющих узлы
(`appendChild`, `insertBefore`, `replaceChild`, `append`/`prepend`/`before`/
`after`/`replaceWith`, `select.add`, `insertAdjacentElement`, …), обёрнуты два
натива, в которые все они упираются — `_lumen_append_child` и
`_lumen_insert_before`. Когда невыполненных динамических скриптов нет (то есть
на подавляющем большинстве страниц и на всех вставках после последнего
запуска), хук стоит одно чтение свойства. Вставка узла, который сам не
отслеживается, дополнительно проверяет висящие скрипты на связность — случай
`div.appendChild(script)` до `body.appendChild(div)`.

**Что исполняется.** Тип сверяется с тем же списком JavaScript-MIME, что и
на парсерном пути (`main.rs::is_classic_script_type`), продублированным в JS;
не-JS тип (`importmap`, `application/json`, шаблонный язык) остаётся блоком
данных. Классический инлайновый скрипт выполняется синхронно прямо во вставке
(`(0, eval)(text)` — косвенный eval и есть глобальная область), потому что
каноничная форма WPT проверяет побочный эффект строкой ниже `appendChild`.
Классический внешний (`src`) и любой модульный уходят в `setTimeout(…, 0)`:
спека ставит вставленному скриптом элементу флаг force-async, и это здесь важно
дважды — `fetch` в Lumen внутри синхронный (инлайновый вызов застопорил бы саму
вставку), а почти всякий загрузчик присваивает `el.onload` уже **после**
`appendChild`. По завершении на элементе диспатчится `load`, при неудачной
загрузке — `error` (именно этого не хватало BUG-703: промис загрузчика висел
вечно).

**Модульные скрипты.** Готовятся тем же путём, но тело регистрируется в карте
ES-модулей через два новых натива (`_lumen_esm_register` /
`_lumen_esm_register_inline`, обычная запись в thread-local — в V8 они не
перевходят) и затем импортируется динамическим `import()`. Сам `import()`
компилируется лениво через `new Function`, а не пишется в шиме литералом: шим
компилируется одним классическим скриптом, и хост, отказавший в динамическом
импорте в этой позиции, уронил бы весь шим. Статические импорты **внутри**
такого модуля по-прежнему резолвятся только из заранее зарегистрированных
источников — сетевой граф модулей это [BUG-446](BUG-446-OPEN.md), здесь он не
менялся.

**Проверка.** 12 юнит-тестов (`cargo test -p lumen-js --features v8-backend`):
инлайновый запуск на `appendChild`, ровно один запуск на элемент, легаси- и
пустой `type`, инертность блока данных и `innerHTML`, отложенный запуск при
подключении предка, путь `insertBefore`, SVG-овый `<script>`, инлайновый
модуль с `load`, внешний скрипт с `fetch`+`load`, `error` на HTTP 404,
непроброс исключения наружу вставки. Живая проба (`--mcp-live-port` против
локального `http.server`): инлайновый скрипт, внешний скрипт, его `onload` и
`onerror` отсутствующего файла — все четыре сработали.

**Оговорка про `--dump-layout`.** Режим дампа не крутит таймеры вовсе
(`run_dump` не зовёт `_lumen_tick_timers`), поэтому там виден только
синхронный инлайновый путь; внешние и модульные скрипты в дампе не доходят до
исполнения — как и любой `setTimeout`. Это свойство режима дампа, не этого
исправления; проверять асинхронный путь надо живым окном.
