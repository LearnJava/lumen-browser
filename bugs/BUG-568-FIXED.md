# BUG-568: `document.write()`/`.open()`/`.close()` do not exist — the whole "dynamic markup insertion" family is unimplemented

**Статус:** FIXED 2026-09-26 (P6) — последняя часть (исполнение записанных `<script>` и точка вставки) закрыта; `open`/`close` — GAP-DOCWRITE
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-DOCWRITE` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Компонент:** js (`crates/js/src/dom.rs` — `document` object literal, `dom.rs:4160` onward, has no `write`/`writeln`/`open`/`close` member at all; confirmed by `grep -n "document\.write\|\"write\"\|parseHTMLUnsafe"` returning nothing for any of the four)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04; scope widened P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Симптом

`document.write(...)`/`.writeln(...)`/`.open(...)`/`.close()` all throw
`TypeError: document.<method> is not a function` — none of the four methods
exist on the `Document` JS wrapper, not a broken stub for any of them.
Originally observed in `html/semantics/embedded-content/media-elements`
(tests that build fixture markup via `document.write` before asserting on
it):

```
FAIL getting audio.muted with muted="" (document.write-created) - document.write is not a function
FAIL getting video.muted with muted="" (document.write-created) - document.write is not a function
```

`html/webappapis/dynamic-markup-insertion/` — the module built specifically
around this API family — confirms `open`/`close` are just as absent as
`write`/`writeln`:

```
TIMEOUT html/webappapis/dynamic-markup-insertion/opening-the-input-stream/011.html
TIMEOUT html/webappapis/dynamic-markup-insertion/closing-the-input-stream/document-close-with-pending-script.html
```

## Причина

`document.write`/`document.writeln`/`document.open`/`document.close` (HTML LS
§3.6, "Dynamic markup insertion") have never been implemented on the
`Document` wrapper in `dom.rs` — there is no partial stub, no guard, just
four missing members. Any WPT test (in this or any other category) that
constructs fixture elements via `document.write('<video muted></video>')`,
or re-enters the parser via `document.open()`, fails immediately with a
`TypeError`, before reaching the assertion the test actually cares about.

## Масштаб

Low direct count in the originating category (2 distinct subtests) but
`document.write`/`.open`/`.close` are foundational, spec-required `Document`
methods with broad reach across the WPT corpus wherever fixture markup is
streamed in rather than parsed via `innerHTML`. `html/webappapis` shows the
true scale: essentially the entire
`dynamic-markup-insertion/opening-the-input-stream/` (~40 files) and
`dynamic-markup-insertion/document-write/` (~15 files) subdirectories
TIMEOUT or FAIL on this alone — the single largest failure cluster in that
slice after [BUG-591](BUG-591-FIXED.md) (global error reporting).

## Перезамер 2026-08-22 (WPT-RUN-6, срез 20): что осталось после BUG-701

[BUG-701](BUG-701-FIXED.md) добавил `write`/`writeln`, поэтому исходная
формулировка «ни одного из четырёх методов нет» устарела. Живой замер
(`tests/wpt/verify_preload_script_audio_gaps.py`, коммит `79f7df91a`,
`--seconds 5`) показывает ровно два остатка:

| проба | получено |
|---|---|
| `document-write-markup` | `wrote-markup found=yes`, `later found=yes writeln=function` — разметка пишется и остаётся в дереве |
| `document-write-script` | `wrote` — и **никогда** `written-script-ran` |
| `document-open-write` | `open-threw TypeError: document.open is not a function` |

То есть: (1) `<script>`, переданный в `document.write()`, не выполняется —
разметка попадает в дерево, но скрипт не запускается и не сообщает об этом
ничем; (2) `document.open()`/`document.close()` по-прежнему отсутствуют
целиком.

Обе грани — молчаливые, поэтому дают TIMEOUT, а не FAIL. Механизм
`document-write-script-inert` в `tests/wpt/timeout_audit.py` забирает по ним
**7 id** остатка снимка WPT-RUN-5: `html/webappapis/dynamic-markup-insertion/
document-write/script_00{1,3}`, `content-security-policy/nonce-hiding/*` 2,
`html/browsers/history/the-history-interface/008`,
`html/semantics/scripting-1/…/execution-timing/068`,
`trusted-types/HTMLScriptElement-internal-slot` — все пишут `<script>` через
`document.write` и ждут его исполнения. Порядок починки, вероятно, обратный
интуитивному: исполнение записанного скрипта затрагивает больше тестов, чем
сам `document.open()`.


## Замер 2026-08-23 (WPT-RUN-6, срез 25): `document.write` с тех пор появился, но скрипт из него не выполняется; `open`/`close` по-прежнему нет

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant docwrite-script`
(dev-release, Linux, `main` = `530d0a444`, `--seconds 5`, страница жива):

| шаг | ожидалось | получено |
|---|---|---|
| `document.write("<script>…</script>")` из инлайнового скрипта | скрипт выполняется до следующей строки | `dw-after-write ran=0` — **не выполнился**, но и не бросил |
| та же запись из таймера | скрипт выполняется | `dw-late-written`, выполнения нет |
| `document.createElement('script') + textContent` (контроль) | выполняется | ✔ `dw-textcontent-ran` |
| `document.open()` | возвращает документ | `TypeError: document.open is not a function` |
| `document.close()` | есть | не проверялось — `open` бросил раньше |

То есть от исходной формулировки «не существует вся семья» осталось две
трети: `write` вызывается без ошибки и молча не исполняет записанный
`<script>`, `open`/`close` отсутствуют. Тихая половина хуже громкой — тест,
ждущий выполнения записанного скрипта, виснет вместо `FAIL`.

**Масштаб этой грани:** механизм `document-write-script-inert` в
`tests/wpt/timeout_audit.py` — 3 id остатка снимка WPT-RUN-5
(`content-security-policy/nonce-hiding/svgscript-nonces-hidden.html` и
`…-hidden-meta.sub.html`, оба с зависшим подтестом `Document-written script
executes.`, и `html/webappapis/dynamic-markup-insertion/opening-the-input-stream/document.open-03.html`).


## Реальный сайт (2026-09-24): tumblr

tumblr подключает **все** бандлы (runtime, vendor, main, …) из инлайн-скрипта в `<head>` через
`document.write('<script src=… defer>')`. В Lumen к `assets.tumblr.com/pop/js/*` не уходит ни одного
запроса: SPA не гидрируется, остаётся SSR-оболочка — 230 узлов против 5788 в Chrome, без
блокировщика. Репро `.tmp/compat/g5/docwrite.html` (+`ext.js`): Lumen `ext=0, inline=0,
log ['after:0']`, Chrome `1, 1, ['after:1']`. `write` вставляет разметку через
`body.insertAdjacentHTML` (скрипты инертны), а в `<head>` при `body === null` — no-op
(`web_api_shim_mid.js:11440-11448`). GAP-DOCWRITE закрыт без этой части; передан P6 по решению
пользователя.

## Исправление (2026-09-26, P6)

`_lumen_document_write` (`crates/js/src/shim/web_api_shim_mid.js`, рядом с «prepare the script
element») заменил вставку в конец `<body>`:

- **Точка вставки** — сразу за исполняемым парсерным скриптом (кадр на каждый скрипт, живёт вместе с
  `document.currentScript`). Из скрипта в `<head>` head-only содержимое остаётся в `<head>`, остальное
  уходит в начало `<body>` (как режим «in head» закрыл бы head). Без исполняемого скрипта (таймер,
  обработчик) — конец `<body>`, как раньше. Раньше в `<head>` при `body === null` запись терялась.
- **Разорванный тег** (`write('<i id=')` + `write("'x'>")`) и `<script>` без закрывающего тега
  придерживаются до следующей записи или возврата пишущего скрипта.
- **Записанные скрипты исполняются.** Инлайновый классический — внутри `write()`, после проверки
  `script-src` (`_lumen_check_inline_script` → `JsFetchProvider::check_inline_script` →
  `CspPolicy::inline_allows` — то же правило, что у инлайновых скриптов разметки в shell). Внешний
  классический без `async`/`defer` — парсер-блокирующий: запрос уходит сразу, исполнение — когда
  пишущий скрипт вернулся, до следующего скрипта документа (`_lumen_fetch_async_wait_text`, лимит
  20 с); записанное после него ждёт его. `defer` — в `_lumen_apply_ready_state('interactive')` до
  DOMContentLoaded. `async`/модули — путь DOM-вставленного скрипта. `check_element_src` получил
  `parser_inserted`: `'strict-dynamic'` записанный `<script src>` не пропускает (CSP3 §6.7.1.1).

Не моделируется: записанный незакрытый элемент не «поглощает» разметку, уже стоящую после скрипта;
разметка блокированной записи попадает в дерево до исполнения блокирующего скрипта (ждут только скрипты).

**Проверка.** 9 юнит-тестов `crates/js/src/dom/tests/v8_bug568_document_write.rs`. Репро
`.tmp/compat/g5/docwrite.html` в видимом окне: Lumen `ext=1, inline=1, log ['after:1']`, порядок узлов
= Chrome. tumblr: 88 запросов к `assets.tumblr.com/pop/js/*` (было 0). WPT
`document-write/` — +27 подтестов PASS, `script_001`/`script_003` OK вместо TIMEOUT;
`opening-the-input-stream/` — +2 подтеста; регрессий нет (`mutation-observer.html` под
`--processes 4` один раз дал TIMEOUT, в одиночку 3/3 OK — как и на main). baseline `.ini` обновлён.

**Остаток на tumblr (не этот баг):** после загрузки бандлов гидрация падает на
`document.body.classList is not iterable` — [BUG-1125](BUG-1125-OPEN.md) (следующая в очереди P6).
