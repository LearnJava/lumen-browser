# BUG-480: `<iframe>` has no separate browsing context — `contentWindow`/`contentDocument` are absent from the JS shim entirely

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

`grep -n "contentWindow\|contentDocument" crates/js/src/dom.rs` — zero
matches. Any test that creates an `<iframe>` and reaches into it via
`iframe.contentWindow`/`iframe.contentDocument` gets `undefined`, so the next
property access throws or the test hangs waiting on a promise/event that can
never fire inside a document that doesn't exist from JS's point of view.

## Уже отмечалось походя, но не заводилось отдельно

* [BUG-311](BUG-311-FIXED.md) (fixed): `Node.isConnected` — its own test note
  says "iframes remain FAIL — nested sub-documents through `contentDocument`
  aren't modeled", but the bug itself only covers `isConnected`.
* WPT-VENDOR-focus session (2026-07-28, not filed): "~13 subtests die because
  `<iframe>` has no browsing context at all, `contentWindow`/`contentDocument`
  = `null`" — noted in passing, never turned into its own `BUG-NNN`.

This entry is the first dedicated bug for the underlying gap itself.

## Масштаб находки (в этом срезе)

`tests/wpt/css/cssom-view/resources/matchMedia.js`'s `createIFrame()` helper
(used by all `MediaQueryList-*`/`matchMedia*` tests that need a resizable
sub-document to observe media-query changes in) awaits the iframe's `load`
event and then does `iframe.contentDocument.body.offsetWidth` — six
`MediaQueryList-*`/`MediaQueryListEvent.html` files TIMEOUT outright on this.
Also implicated: `elementsFromPoint-iframes.html`,
`scrollIntoView-iframes.html`, `scroll-behavior-subframe-root.html`,
`scroll-behavior-subframe-window.html`, `matchMedia-display-none-iframe.html`
— all TIMEOUT.

## Что нужно

A real nested browsing context: a second `Document`/`Window` pair per
`<iframe>`, `contentWindow` returning that `Window`, `contentDocument`
returning that `Document` (same-origin only, per HTML LS — cross-origin must
throw/return `null` for `contentDocument` while still returning a `Window`
for `contentWindow`). Large — likely its own multi-slice task, not a single
`BUG-NNN` fix; this entry documents the gap and its WPT blast radius so a
future task doesn't have to rediscover it from scratch.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: TIMEOUT`/`FAIL`/`NOTRUN` per the
actual run.

## Срез 25 (`css/css-properties-values-api`, 2026-08-03)

`at-property-viewport-units.html` and `at-property-viewport-units-dynamic.html`
both build their entire test body inside `<iframe id=iframe srcdoc="...">` —
same gap, srcdoc-based sub-document never runs. Both file-level `TIMEOUT`
(zero subtests registered). `.ini` under
`tests/wpt/metadata/css/css-properties-values-api/` for both files.

## Срез 26 (`css/css-highlight-api`, 2026-08-03)

`HighlightRegistry-highlightsFromPoint.html`'s "returns empty array when
called on a display:none iframe document" subtest does
`iframe.contentWindow.document` — `contentWindow` is `null`, so the read
throws `Cannot read properties of null (reading 'document')` before the
test ever reaches `highlightsFromPoint()` (the other 6 subtests in the same
file fail on the unrelated [BUG-534](BUG-534-OPEN.md)). 1 subtest. `.ini`
under `tests/wpt/metadata/css/css-highlight-api/`.

## Срез 33 (`css/css-sizing/responsive-iframe`, 2026-08-03) — whole feature
directory, plus a flaky harness/subtest TIMEOUT-shape boundary

6 files, all relying on cross-frame `postMessage`/`contentWindow` for the
Responsive Iframe API (`frame-sizing`). New methodological finding: the same
file can surface as a **harness-level** TIMEOUT (0 subtests recorded) on one
run and as an **OK harness with a subtest-level TIMEOUT** on the next —
observed on 5 of the 6 files across three consecutive verify runs of the
identical `.ini`. The two shapes are not different bugs, just which side of
wptrunner's per-test timeout the process happened to land on under parallel
load; a `.ini` pinned to a single status flags spuriously as unexpected on
the other run. Fixed by using wptmanifest's list-expected syntax on both the
file-level and subtest-level line, e.g. `expected: [OK, TIMEOUT]` /
`expected: [PASS, TIMEOUT]` (confirmed the parser resolves this correctly
via `wptrunner.manifestexpected.get_manifest(...).get_test(id).get('expected')`
→ a Python list, matched against either observed status). Apply this pattern
to any future slice's iframe/postMessage-dependent TIMEOUT cluster instead
of re-diagnosing it as a regression. `responsive-iframe-request-resize-error.html`
additionally surfaces `window.requestResize is not a function` (a second,
narrower gap — the Responsive Iframe API's parent-side control method is
entirely unimplemented, not just the browsing-context container) once it
gets far enough to register subtests; folded under this bug's umbrella since
the whole `responsive-iframe/` feature area is unimplemented, not filed
separately. `.ini` under `tests/wpt/metadata/css/css-sizing/responsive-iframe/`.

## WPT-VENDOR-x-frame-options (2026-08-18) — whole category is this bug

6 files, 157 subtests, every one of them builds an `<iframe>` and awaits
either its `message` event (cross-document `postMessage` from the framed
page back to the parent) or its `load` event (the "blocked" case, checking
`iframe.contentDocument === null` from the handler). Both paths depend on
the framed document actually running as a nested browsing context — since
`<iframe>` has none, neither the message nor the load event ever fires.
`run_report.py --all --root x-frame-options --recursive` (1 min 21 s):
**0/6 harness OK, 0/157 subtests** — a uniform TIMEOUT wall, no
category-specific defect (the `X-Frame-Options`/CSP `frame-ancestors`
header logic under test is never reached). No new `BUG-NNN` filed.

## WPT-RUN-6 срез 5 (`html/semantics/embedded-content/the-iframe-element`, 2026-08-21)

161 files, `run_report.py --all --root html/semantics/embedded-content/the-iframe-element
--recursive --processes 6` (5 min): **80/149 TIMEOUT (53.7%)** — the same
uniform-TIMEOUT-wall shape as `x-frame-options` above, on a category more
than 25x its size. Sampled every subgroup (`iframe_sandbox_*` — 28 files,
`iframe-loading-lazy-*` — 21, `iframe_javascript_url_*` — 7, plus the
remainder: `move_iframe_in_dom_*`, `srcdoc_change_hash.html`,
`srcdoc_process_attributes.html`, `same_origin_parentage.html`,
`cross-origin-to-whom(-part-2).window.html`, `iframe-load-event.html`, …) —
every file checked awaits `iframe.onload` or reads
`iframe.contentWindow`/`contentDocument`, confirming `crates/shell/src/main.rs:7229`'s
own comment (`"URL-based iframe: Phase 0 — sub-document is not loaded"`) is
the operative mechanism, not just the JS-shim accessor gap. No new `BUG-NNN`
filed — folds under this bug's umbrella per the pattern above.

## Срез 24 WPT-RUN-6 (2026-08-22) — субдокумент не просто «не смоделирован в JS», его **никто не запрашивает**, и с тех пор отказ стал тихим

Замер `tests/wpt/verify_frame_load_media_gaps.py --variant nbc-iframe
--variant nbc-parser` (dev-release, Linux, коммит `c583a90b4`, `--seconds 5`,
страница жива — 9 тиков) добавляет к этому багу две вещи, которых в нём не
было.

1. **Ни одного HTTP-запроса.** Сервер пробы записывает каждый путь, который у
   него спросили; при `<iframe src="child.html">` — и созданном скриптом, и
   написанном парсером — он не получает ничего, а в stderr браузера нет ни
   строки про ребёнка. То есть речь не только о доступе из JS: документа не
   существует, потому что за ним не ходили (`main.rs:5408` «Phase 0: iframe
   sub-документы не загружаются»). Читать лог браузера как доказательство
   запроса нельзя — он умеет печатать хинт о незапрошенном ресурсе
   ([BUG-826](BUG-826-FIXED.md)); доказательство здесь только со стороны
   сервера.
2. **`contentWindow`/`contentDocument` теперь существуют.** Исходная запись
   2026-08-02 фиксировала `grep` без единого совпадения и `undefined`/`null`
   на чтении; сейчас оба отдают объект (`nbc-iframe-checked
   contentWindow=object contentDocument=object`), а `window[name]` для
   именованного фрейма — тоже объект. Для тестов это хуже, а не лучше: код
   вида `if (iframe.contentDocument) { … }` проходит проверку и работает с
   пустым документом, вместо того чтобы упасть с TypeError. Соответственно и
   симптом сместился с «бросает исключение» на «молча ждёт вечно».

Родственные элементы измерены тем же прогоном и разведены по своим багам:
`<object data>`/`<embed src>` — [BUG-798](BUG-798-OPEN.md), `<frame>` —
[BUG-854](BUG-854-FIXED.md).

## Срез 1 (P3, 2026-08-23) — шелловый конвейер под-документов

Срез 24 фиксировал «за URL фрейма никто не ходит» — срез 1 это закрывает.
`parse_and_layout` после sandbox-гейтов вызывает `load_frame_sub_documents`
(`crates/shell/src/main.rs`): для каждого `<iframe>` без `loading="lazy"`
собирается источник (`srcdoc` → inline; `src` → файл или сеть через
`fetch_subresource(RequestDestination::Document)`; нет ни того ни другого →
пустой `about:blank`; `javascript:`/`data:` отклоняются с логом), HTML парсится
в отдельный `Document`, скрипты ребёнка исполняются в собственном V8-контексте
(`run_scripts_with_dom`: те же провайдеры сети и хранилищ, что у страницы;
`sandbox` гейтится как у top-level; opaque origin — `sandbox` без
`allow-same-origin` — не получает персистентных хранилищ). Ребёнку отправляются
DOMContentLoaded + window load (последний — приближение: подресурсы фрейма ещё
не грузятся); на элементе-хосте диспектчится trusted непузырящийся `load`.
Вложенность ограничена `MAX_FRAME_DEPTH = 2`; хэндлы (`FrameHandle`: host/url/
doc/js) живут в `Lumen::frames` до замены страницы и пампятся в `about_to_wait`
(таймеры/WebSocket/SSE/workers — прямыми вызовами, хэндлы живут на UI-стороне).
`IframeInfo` (lumen-dom) дополнен полями `node` и `name`.

Не входит в срез 1 (очередь): `contentWindow`/`contentDocument` из JS родителя
(по-прежнему `null`, `iframe_element.rs`), layout/paint содержимого фрейма,
rAF фреймов, навигация/замена/удаление фрейма, `postMessage` между окнами,
X-Frame-Options/CSP `frame-ancestors` на ответе ребёнка, `window[name]`,
cross-origin ограничения доступа, bfcache не сохраняет фреймы.

Проверка: clippy `-p lumen-dom` / `-p lumen-shell` -D warnings; тесты крейтов
(dom 277 ok, shell iframe 7 ok); `dump_golden.py` 12/12 байт-в-байт; смоук —
srcdoc- и file-iframe исполняют скрипты детей в собственных контекстах.

## Перезамер после среза 1 (WPT-RUN-6, срез 28, 2026-08-23)

`tests/wpt/verify_window_history_jsurl_gaps.py --variant frame-parser`
(dev-release, Linux, `main` = `0dc60692d`) подтверждает срез 1 на парсерном
фрейме: запрос уходит, скрипт ребёнка исполняется, `load` на хосте приходит
в обеих формах регистрации (атрибут `onload` и `addEventListener`).

Что из очереди этого бага измерено и всё ещё не работает:

* `iframe.contentDocument` — пусто при том, что `contentWindow` возвращает
  объект (то есть ловушка среза 24 «`if (iframe.contentDocument)` больше не
  бросает, но и не проходит» жива);
* внутри загруженного ребёнка `window.parent === window` — идиома
  `parent.foo()`, на которой построена половина фреймовых тестов WPT,
  попадает в собственный глобал ребёнка, а не в родителя;
* `window.length` в родителе — `0` при живом загруженном фрейме, и
  `window.frames` его не видит.

Отдельно от очереди: фрейм, **вставленный скриптом**, не грузится вовсе —
[BUG-885](BUG-885-OPEN.md).

## Срез 2 (P3, 2026-08-23) — `contentWindow`/`contentDocument` из JS родителя

Срез 1 дал живой под-документ, но из JS родителя он был недостижим (геттеры в
`iframe_element.rs` возвращали `null`). Срез 2 строит мост. Каждый V8-контекст
держит собственный isolate, поэтому прямой передачи объектов между окнами нет:
в **родительском** рантайме живёт реестр биндингов «хост → под-документ»
(`crates/js/src/frame_bridge.rs`, поле `V8JsRuntime::frame_docs`). Shell после
исполнения скриптов ребёнка и строго ДО диспатча trusted `load` на хосте зовёт
новый метод трейта `PersistentJs::register_iframe_document(host_nid, doc,
url, accessible)` → `V8JsRuntime::register_frame_document`; биндинг ложится на
JS-поток, и геттеры `contentWindow`/`contentDocument` (`iframe_element.rs`
теперь читают `_lumen_frame_content_window/document(this.__nid__)`) начинают
видеть фасады без перепатчивания враппера.

Фасады строятся JS-шимом бриджа поверх нативов `_lumen_f_*` (чтение под
`Arc<Mutex<Document>>` ребёнка) и интернируются: identity постоянна
(`contentDocument === contentDocument`, `defaultView === contentWindow`,
`frameElement === хост`). Window: `document/window/self/frames/parent/top/
closed/length/frameElement/name/location/close`. Document: `body/head/
documentElement/title/URL/documentURI/readyState/defaultView/getElementById/
querySelector(All)`. Element (только чтение): tag/id/class/textContent/
attr-доступ/children/parentElement/querySelector(All)-scoped; геометрия —
честные нули (layout фрейма — будущий срез).

Доступ (`shell::frame_access_allowed`, юнит-тесты): opaque sandbox (`sandbox`
без `allow-same-origin`) отрицает всё; `about:` наследует origin родителя;
иначе origin сравнивается с нормализацией портов по умолчанию (80/443);
file↔file разрешён — задокументированное отклонение (упрощённая модель
Firefox same-directory). Cross-origin/opaque получают фасад окна БЕЗ
`.document`, `contentDocument` — `null` (спека: WindowProxy отдаётся всегда).
Фреймы без загруженного под-документа (динамические `<iframe>`, неудавшийся
fetch) биндинга не имеют — оба геттера `null`.

Не входит (очередь): динамически созданные фреймы (для них по-прежнему
`null` — а значит кластер `matchMedia.js` с `createElement('iframe')` пока не
отвечает), мутации из родителя и события через границу, layout/paint/rAF
фреймов, навигация/замена/удаление фрейма, `postMessage`, `window[name]`,
X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов.

Проверка: clippy `-p lumen-js` / `-p lumen-shell` -D warnings; тесты крейтов
(js 2964+70 ok, dom 277 ok, shell 1586+2 ok, из них 8 новых — 7 бридж + 5
правила доступа); `dump_golden.py` 12/12; смоук `--dump-layout`: srcdoc-фрейм —
`win=true doc=true p=hi frame identity=true defaultView=true parent=true
top=true frameElement=true body=BODY url=about:srcdoc`.

## Срез 3 (P3, 2026-08-23) — иерархия окон: parent/top у ребёнка, length/[i]/[name] у родителя

Срез 28 перезамерял три живых разрыва: `window.parent === window` у ребёнка,
`length = 0` и слепые `frames` в родителе. Срез 3 закрывает их поверх того же
реестра биндингов (`crates/js/src/frame_bridge.rs`).

Реестр получил слоты предков (`parent`/`top`) с псевдо-bid'ами `u32::MAX` /
`u32::MAX-1`: всё семейство нативов `_lumen_f_*` прозрачно работает и с
документом предка, отдельного семейства функций не появилось. Shell при
загрузке фрейма регистрирует ребёнку родителя (`PersistentJs::register_parent_document`,
всегда) и верх (`register_top_document`, только глубина ≥ 2 — у первого уровня
top разрешается через слот родителя, чем сохраняется identity `parent === top`).
Доступность считается отдельно для каждого направления тем же
`frame_access_allowed`.

Геттеры ставятся на `window` ЛЕНИВО, первой регистрацией, а не при установке
шима: инсталлтайм-акцессор на `parent`/`top`/`length` ломает топ-левел
`var parent = …` страниц (V8 не подменяет существующий акцессор var-биндингом
— на этом словилось 11 тестов dom.rs до переноса). Пока слотов нет, действуют
прежние константы WEB_API_SHIM; попытка переопределить неконфигурируемый
var-биндинг пользователя молча пропускается try/catch.

Что теперь работает:

* у ребёнка — `window.parent` (фасад документа родителя), `window.top`
  (корень для глубины 2), `window.frameElement` (фасад хоста из дерева
  родителя), `window.name` из атрибута хоста (сеттер хранит собственное
  значение до замены документа);
* у родителя — живой `window.length` (все дочерние контексты независимо от
  origin, как по спеке), индексный доступ `window[0]…` и именованный
  `window[имя]` к окну фрейма (ставятся при регистрации биндинга, порядок =
  tree order); `iframe.contentWindow === window[0] === window[name]`;
* фасады предков сами себе `parent`/`top`; cross-origin/opaque предки отдают
  окно без `.document`, `frameElement` — null.

Не входит (очередь): вызов функций между изолятами (`parent.foo()`, где `foo`
объявлена скриптом родителя, не работает — читать свойства документов можно),
динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)), мутации и события
через границу, layout/paint/rAF фреймов, навигация/замена/удаление фрейма,
`postMessage`, X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов;
`length`/индексные доступники у ФАСАДОВ остаются нулём (счётчик чужого
изолята недоступен, живой length есть только у настоящего window контекста).

Проверка: clippy `-p lumen-js` / `-p lumen-shell -p lumen-driver` -D warnings;
тесты js 2980 (+16: fallback констант, фасад родителя и его интернирование,
top для глубины 2, cross-origin без документов, frameElement/name из дерева
родителя, length+индексные+именованные доступники), shell 1586+2 ok, scoped
по обратным зависимостям (ai/bench/bidi/driver/knowledge/mcp/network/paint/
storage) — 28 ok без FAILED; `dump_golden.py` 12/12; смоук `--dump-layout`
(srcdoc-фрейм): `len0=1 idx0=same named=same` у родителя и
`P=object NE1 TOP1 DOCBODY FEobj` у ребёнка.

## Срез 4 (P3, 2026-08-25) — кросс-фреймовый `postMessage`

Первый канал связи между изолятами: у каждого фасада окна появился
`postMessage(message, targetOrigin)`, доставка — через процесс-глобальный
исходящий ящик ([`FRAME_OUTBOX`] в `crates/js/src/frame_bridge.rs`), ключ
адресата — указатель `Arc<Mutex<Document>>` его документа (один инстанс у
shell/реестра родителя/JS-контекста ребёнка со среза 1). Отправка валидирует
`targetOrigin` (`'*'` всегда; `'/'`/опущенный — по флагу `accessible`,
вычисленному shell'ом; явная строка — совпадение с нормализованным origin
биндинга) и кладёт конверт; получатель разбирает свой ящик на очередном тике
(`PersistentJs::pump_frame_messages`, вызывается рядом с pump_websockets и у
страницы, и у хэндлов фреймов), строит MessageEvent через новый хук
WEB_API_SHIM `_lumen_deliver_frame_message` и доставляет в
`window.onmessage` + `addEventListener('message')`. `event.source` —
интернированный фасад окна отправителя (bid разрешается реестром ПОЛУЧАТЕЛЯ),
`event.origin` считается на приёме: для `about:`-детей — origin родителя
(наследование по спеке). `install_dom` проставляет реестру ключ собственного
документа (`self_key`) и origin страницы (`self_origin`).

Отклонения среза (задокументированы в шиме и тестах): сериализация —
JSON-круготрип, т.е. подмножество structured clone (верхнеуровневый
undefined → null; вложенные функции/узлы деградируют; функции/символы
верхнего уровня бросают `DataCloneError`); рёбра только «предок ↔
непосредственный потомок» — постинг внука на `window.top` доставляется с
`source=null`; sibling↔sibling — будущий срез; доставка асинхронная на тике
пумпы, отдельного пробуждения спящего event loop нет (как у BroadcastChannel).
Попутное уточнение смоука: инлайн-скрипт ребёнка исполняется ДО регистрации
предков (лимит срезов 1–3), поэтому `window.parent.postMessage` из тела
инлайн-скрипта уходит в собственный window — постить нужно после
DOMContentLoaded.

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings; тесты js 2989 (+23: 22 юнит-бриджа на двух изолятах — оба
направления, наследование origin, `'/'`/явный origin, DataCloneError,
изоляция ящика — + 1 интеграционный install_dom→хук), shell 1586+2 ok;
`dump_golden.py` 12/12; живой смоук
`tests/wpt/verify_frame_post_message.py` (dev-release, http-сервер):
child→parent hello, parent→child ping из обработчика `load` хоста,
раунд-трип pong, `event.origin = http://127.0.0.1:<port>` и фасадный
`source` во всех доставках.

## Срез 5 (P3, 2026-08-25) — мутации под-документа из JS родителя

Фасады стали записываемыми: у Document — `createElement(tag)`/
`createTextNode(data)` и сеттер `title` (создаёт `<title>` в `<head>`, если
его нет), у Element — `setAttribute`/`removeAttribute`, рефлексии-сеттеры
`id`/`className`, `appendChild`/`insertBefore(node, ref|null)`/
`removeChild`/`remove()` и сеттер `textContent`. Всё через новое мутабельное
семейство нативов `_lumen_f_create_element/_create_text/_set_attr/
_remove_attr/_append_child/_insert_before/_remove_node/_set_text`
(`frame_bridge.rs`), пишущее в тот же общий `Arc<Mutex<Document>>`, поэтому
мутация видна контексту ребёнка немедленно — его врапперы читают живое дерево.

Границы корректности: аргументы-узлы обязаны быть фасадами того же биндинга
(на фасадах появился `__bid__`; чужой документ — тихий no-op), нативы записи
дополнительно проверяют границы арены (`checked_node`) и цикл «потомок под
собственного предка» (аналог приватного DEVX-8a `is_self_or_ancestor`;
отклонение от спеки — HierarchyRequestError заменён тихим no-op, конвенция
бриджа «невалидно = пусто»). `removeChild`, как у главного документа, снимает
узел с фактического родителя без проверки иерархии. Владение nid'ом на уровне
нативов обеспечивает именно JS-граница `__bid__`: нативы модуля приватны,
как нативы главного документа доверяют его врапперам.

Не входит (очередь): исполнение вставленных `<script>` и запрос подресурсов
вставленных элементов (загрузка подресурсов фреймов — будущий срез),
restyle/layout/paint фрейма (фреймы ещё не рендерятся), события через границу
изолятов (`facade.click()` → слушатели ребёнка) — следующий срез,
`document.open/write/close`, sibling↔sibling postMessage, вызов функций между
изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов.

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings; тесты js lib 3245 ok (+10 юнит-бриджа: запись в общее дерево с
Rust-стороной проверкой, видимость второму изоляту поверх того же `Arc`,
замена детей сеттером textContent, insertBefore/remove/removeChild, отказ
чужого биндинга, no-op при inaccessible, отказ цикла без порчи дерева, сеттер
title в head, текстовый узел); `dump_golden.py` 12/12; живой смоук
`tests/wpt/verify_frame_mutation.py` (dev-release, http-сервер): родитель в
обработчике `load` вставляет `<p id=injected>` c атрибутами и текстом, читает
обратно тем же фасадом, ставит `title`; ребёнок опросом собственного дерева
рапортует узел и родительский title (`child-sees text=from-parent attr=parent
title=mutated-by-parent`).

## Срез 6 (P3, 2026-08-25) — события через границу изолятов: `facade.click()` → слушатели ребёнка

Первый канал событий (не сообщений) между изолятами: у фасада Element появился
`click()`. Вызов ставит конверт в процесс-глобальный ящик синтетических
событий ([`frame_event_outbox`] в `crates/js/src/frame_bridge.rs`, тот же ключ
адресата — указатель `Arc<Mutex<Document>>`, что и у ящика postMessage среза
4); получатель разбирает свой ящик на том же тике пумпы, где раньше были
только сообщения (`_lumen_frame_pump_messages` теперь дренирует оба ящика;
shell не менялся — она уже вызывала пумпу и странице, и хэндлам фреймов), и
доставляет через новый хук WEB_API_SHIM `_lumen_deliver_frame_click(nid)`.

Содержательную часть делает сам ребёнок: хук зовёт общую `_lumen_perform_click`
(`crates/js/src/dom.rs`) — тело бывшего `HTMLElement.prototype.click`,
вынесенное без изменений поведения (прототипный метод теперь тонкая обёртка).
Родительский клик исполняет ПОЛНУЮ семантику click ребёнка: disabled-гейт,
re-entrancy guard, activation target до диспатча, недоверенный bubbly
`MouseEvent` через `_lumen_dispatch_rich` (слушатели, on-атрибуты, пузырьки,
preventDefault) и активационное поведение после (чекбокс/радио, submit/reset,
summary, label). Навигация `<a href>` из фрейма по-прежнему отклоняется —
дрен навигационных запросов фрейма стоит рядом с пумпой со среза 1.

Отклонения среза (задокументированы в шиме и тестах): доставка асинхронная на
тике пумпы, не синхронная по спеке dispatch — то же отклонение, что у
postMessage среза 4; кликаются только элементы (натив режет текст/комментарий,
nid за границей арены и чужой/недоступный биндинг — «невалидно = пусто»);
ящик ограничен тем же cap'ом 256, переполнение теряет конверт молча; клик по
фасаду НЕ перезапускает загрузку подресурсов и layout ребёнка (фреймы ещё не
рендерятся). Смоучная заметка: `ev.currentTarget` при диспетче шимом не
заполняется — пробам доступен только `ev.target` (не гэп этого среза,
отмечено здесь потому, что первый живой проб на это наткнулся).

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-js` (без
фичи) -D warnings; тесты js lib 3249 ok (+4 юнит-бриджа: доставка обоих
направлений поверх пары изолятов с Rust-стороной сверкой nid, ожидание своей
пумпы получателем, отказ cross-origin биндинга, отказ текстового узла/чужого
bid/за границей арены); `dump_golden.py` 12/12; живой смоук
`tests/wpt/verify_frame_click.py` (dev-release, http-сервер): родитель в
обработчике `load` вызывает `contentDocument.getElementById('btn').click()`;
слушатель ребёнка рапортует `bubbles=true trusted=false type=click
tag=BUTTON` и отвечает кросс-фреймовым postMessage — родитель получает
`{"clicked":true,"trusted":false}` (цепочка срезов 4+6 в одном прогоне).

Не входит (очередь): focus()/blur() и произвольный dispatchEvent через фасад,
исполнение вставленных `<script>` и запрос подресурсов вставленных элементов,
restyle/layout/paint фрейма, `document.open/write/close`, sibling↔sibling
postMessage, вызов функций между изолятами, динамически созданные фреймы
([BUG-885](BUG-885-OPEN.md)), навигация/замена/удаление фрейма,
X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов.

## Срез 7 (P3, 2026-08-25) — `focus()`/`blur()` и произвольный `dispatchEvent` через фасад

Ящик событий среза 6 обобщён: [`PendingFrameEvent`] несёт [`FrameEventKind`]
(`click` | `focus` | `blur` | `dom`) вместо неявного «всегда клик»; постановка —
единый натив `_lumen_f_queue_event(bid, nid, spec_json)` (бывший
`_lumen_f_queue_click`; правила те же: нет биндинга / cross-origin / opaque /
не-элемент / неразбираемая спека — тихий «нет»), конверты всех видов
разбираются одной пумпой и доставляются хуками WEB_API_SHIM:
`_lumen_deliver_frame_focus(nid, preventScroll)` /
`_lumen_deliver_frame_blur(nid)` / `_lumen_deliver_frame_dom_event(nid, env)`;
клик — прежний `_lumen_deliver_frame_click`.

Содержательную часть исполняет сам ребёнок:

* `focus(options)` — focusability-гейт (`_lumen_is_focusable`) и
  `_lumen_focus_update(nid)`: blur/focusout на прежде сфокусированном узле
  ребёнка, focus/focusin на новом, `document.activeElement` актуален сразу
  после доставки. Два задокументированных отклонения: БЕЗ
  `_lumen_request_focus` (очередь фокус-запросов рантайма фрейма шеллом пока
  не дренируется — фреймы не рендерятся; запрос там только копился бы) и без
  scrollIntoView (`preventScroll` переносится конвертом, но игнорируется —
  layout у фреймов нулевой);
* `blur()` — no-op для не сфокусированного элемента (HTML LS §6.6.3), иначе
  `_lumen_focus_update(-1)`;
* `dispatchEvent(event)` — переносится снимок type/bubbles/cancelable/
  composed + detail (JSON-круготрип; detail-функции деградируют как у
  postMessage, символы бросают TypeError синхронно на отправке); в ребёнке
  строится заново Event/CustomEvent и диспатчится ТОЧНОЙ копией
  последовательности его собственного `el.dispatchEvent`: слушатели цели +
  on<type> (`_lumen_dispatch`) и активационное поведение для недоверенного
  'click' без preventDefault (BUG-439). Возвращаемое значение спеки
  (!defaultPrevented) при асинхронной доставке недостижимо — метод ничего не
  возвращает.

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings (прогон `-p lumen-js` без фичи красен и на чистом main — 19
предсуществующих dead-code ошибок вне этого среза); тесты js lib 3263 ok
(+14: транспорт focus/blur/dispatch обоих направлений поверх пары изолятов,
сохранение порядка конвертов, отказ чужого/cross-origin биндинга и мусорных
спек, интеграция install_dom→хуки: activeElement через фасад с пустой очередью
фокус-запросов шелла, CustomEvent с detail у локального слушателя); shell
1586+2 ok; `dump_golden.py` 12/12; живой смоук
`tests/wpt/verify_frame_actions.py` (dev-release, http-сервер): родитель в
обработчике `load` вызывает `btn.focus()` → ребёнок репортит
`child-focused active=btn` + непузырящийся focus и всплывший focusin;
`btn.dispatchEvent(new CustomEvent('hello', {detail:{n:7}}))` →
`child-hello detail={"n":7,"tag":"slice7"}`; `btn.blur()` → `child-blurred
active=BODY` + focusout, обработчик которого отвечает родителю кросс-фреймовым
postMessage (цепочка срезов 4+7 в одном прогоне).

Не входит (очередь): исполнение вставленных `<script>` и запрос подресурсов
вставленных элементов, restyle/layout/paint фрейма, `document.open/write/close`,
sibling↔sibling postMessage, вызов функций между изолятами, динамически
созданные фреймы ([BUG-885](BUG-885-OPEN.md)), навигация/замена/удаление
фрейма, X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов; уведомление
шелла о фокусе внутри фрейма (дрен очереди рантаймов фреймов) — вместе с
layout/rAF фреймов.

## Срез 8 (P3, 2026-08-25) — исполнение `<script>`, вставленных из родителя

Срез 5 вставлял скрипты молча. Теперь фасадные `appendChild`/`insertBefore`
после успешной мутации проверяют тег вставленного узла и для `<script>` ставят
в ящик событий конверт [`FrameEventKind::RunScript`] (натив `_lumen_f_queue_event`
не задействован — постановка прямая, helper `queue_run_script_if_script`);
конверт НЕ исполняется в момент вставки — доставка через границу изолятов у
моста всегда асинхронная. Ребёнок на своём тике (`_lumen_deliver_frame_run_script`)
отдаёт элемент своей ШТАТНОЙ `_lumen_script_prepare` (HTML LS §4.12.1): гейт
типа (data-блоки не исполняются), пустой `src` → `error`, инлайн-классика
синхронно с честным `document.currentScript`. «Already started» — per element,
ведёт получатель (`_lumen_frame_scripts_started`): повторная вставка
исполненного не перезапускает; конверт, адресат которого отсоединился до
доставки, теряется БЕЗ пометки — повторная вставка исполнится, как у главного
документа.

Попутно чинились два дефекта транспорта, которые этот срез вскрыл живыми
смоуками:

1. **Спящий event-loop не знал про ящики моста** (затрагивало уже слитые
   срезы 4–7): конверт, поставленный после затихания страницы, лежал до
   случайного пробуждения цикла — смоки ловили ноль доставок стабильно.
   Теперь `V8JsRuntime::frame_transport_pending()` (ключ собственного документа
   дублируется в рантайме из `frame_docs.self_key`) отвечает шеллу «есть ли
   конверт для МЕНЯ», trait-метод `PersistentJs::frame_transport_pending`
   (default false) проброшен до `about_to_wait`, и пока хоть один живой контекст
   отвечает «да», ставится poll-дедлайн +2 мс. Конверты мёртвых адресатов опрос
   не держат — латентная утечка ящика остаётся ограниченной капой, а не
   превращается в горячий цикл. Под движковым потоком страница опрашивается тем
   же `route_query_js`-каналом, что остальные чтения.
2. **Статический iframe без парсерных скриптов не получал JS-контекст вовсе**
   (`run_scripts_with_dom` выходил рано при пустом списке скриптов), то есть
   ему нечем было принимать postMessage/события/RunScript — а статические
   встраивания самые частые. Новый параметр `always_runtime` (true у фреймов)
   создаёт рантайм всегда; sandbox=SCRIPTS по-прежнему побеждает. Странице
   хватает старого поведения.

Внешний `<script src>` идёт через тот же штатный prepare (провайдеры сети у
фрейма есть со среза 1), но живая проверка упёрлась в чтение тела ответа в
контексте фрейма — это подресурсная доставка, остаётся в очереди вместе с
«загрузкой подресурсов вставленных элементов».

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings; тесты js lib 3269 ok (+6: конверты RunScript от обоих методов
вставки со своими nid и без лишнего от не-скриптов, отказ cross-origin,
интеграция install_dom→prepare: исполнение с currentScript, already-started,
detached-before-delivery, флип pending-предиката вокруг пумпы); shell 1610+2
ok; `dump_golden.py` 12/12; живые смоуки `tests/wpt/verify_frame_run_script.py`
(инлайн+currentScript, data-блок молчит; child-страница без единого скрипта —
именно тот случай, который раньше не имел рантайма) и регрессионный
`verify_frame_actions.py` среза 7 — оба зелёные на одной сборке.

Не входит (очередь): запрос подресурсов вставленных элементов и чтение тел
ответов во фрейме (внешний src скриптов — первый клиент), restyle/layout/paint
фрейма, `document.open/write/close`, sibling↔sibling postMessage, вызов функций
между изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов; уведомление шелла о фокусе внутри фрейма — вместе с
layout/rAF фреймов.

## Срез 9 (P3, 2026-08-25) — URL-рефлексии фасадов и поздний `src` скрипта

Очередной пункт «запрос подресурсов вставленных элементов … (внешний src
скриптов — первый клиент)» вскрылся живым смоуком так: сам конвейер доставки
работал со среза 8 — у фрейма свои провайдеры сети, `fetch()` ребёнка читает
тела ответов, внешний src через штатный `_lumen_script_prepare` доходит до
сети и исполняется. Ломался каноничный способ им воспользоваться:
`s.src = url` на фасаде **не писал атрибут** — присваивание заводило свойство
JS-обёртки в изоляте родителя, дерево ребёнка оставалось без src, и prepare
молча уходил в инлайн-ветку пустого тела (диагностика: сервер не видел
запроса; `started-keys` показывал, что доставка RunScript происходила).

* Фасадный элемент получил `src`/`href` (HTML LS §2.6.2): сеттер хранит
  строку дословно, геттер отдаёт '' при отсутствующем атрибуте, иначе —
  значение, разрешённое против базы ПОД-ДОКУМЕНТА (href первого `<base>`
  против URL документа — §4.2.3; helper `frameBase` над нативами бриджа,
  `_url_resolve` главного шима).
* Натив записи атрибута (`_lumen_f_set_attr`) после успешного `src` на
  `<script>` ставит тот же конверт RunScript, что и вставка (HTML LS
  §4.12.1: изменение src перезапускает prepare). Оба порядка — «src до
  appendChild» и каноничный «appendChild, потом s.src = …» — теперь
  равнозначны.
* «Already started» переведён на спековую семантику: флаг ставится только
  когда подготовка реально началась (fetch запущен / тело исполнено / пустой
  src отстрелил error) — дата-блок и элемент без src и без тела остаются
  непомеченными, и поздний src обязан получить вторую доставку. Раньше флаг
  взводился фактом доставки, что навсегда глотало бы поздний src. Предикат —
  `_lumen_frame_script_will_start` (dom.rs), зеркальный ранним выходам
  `_lumen_script_prepare`.

Отклонения среза: геттеры ставятся на каждый элемент безусловно (как id/
className фасада; безусловные члены главного шима — BUG-450, отдельная тема);
в минимальных тестовых изолятах без WEB_API_SHIM нет `_url_resolve` — геттер
тогда отдаёт сырую строку (прод-контексты всегда резолвят); load/error
события внешнего скрипта по-прежнему не доставляются обратно в изолят
родителя (обработчик `s.onload` фасада — no-op), как и `<img>`/CSS
подресурсы фреймов — следующий срез очереди.

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings; тесты js lib 3279 ok (+10: поздний src ставит второй конверт с
тем же nid, рефлексии хранят дословно/резолвят против базы c `<base>`-композицией,
пустой скрипт не помечается первой доставкой + поздний src доезжает до error
без провайдера сети, повторный src после already started — no-op, дата-блок
не помечается вовсе); shell 1610+2 ok; `dump_golden.py` 12/12; живые смоуки
новый `tests/wpt/verify_frame_external_script.py` (оба порядка src↔вставка,
запросы к серверу, исполнение обоих файлов, абсолютный URL геттера) и все
пять регрессионных срезов 4–8 (`run_script`/`actions`/`click`/`mutation`/
`post_message`) — зелёные на одной сборке.

Не входит (очередь): доставка load/error ресурсных событий через границу
изолятов (обработчики фасада), подресурсы `<img>`/`<link>` фрейма,
restyle/layout/paint фрейма, `document.open/write/close`, sibling↔sibling
postMessage, вызов функций между изолятами, динамически созданные фреймы
([BUG-885](BUG-885-OPEN.md)), навигация/замена/удаление фрейма,
X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов; уведомление шелла о
фокусе внутри фрейма — вместе с layout/rAF фреймов.

## Срез 10 (P3, 2026-08-26) — обратная доставка ресурсных событий: обработчики фасада

Срез 9 закрыл исполнение внешнего `src`, но его хвост оставался: «load/error
события внешнего скрипта не доставляются обратно в изолят родителя (обработчик
`s.onload` фасада — no-op)». Все прежние конверты шли в одну сторону
(родитель → ребёнок); ресурсное событие рождается в ребёнке, а назначивший
`s.onload = fn` код живёт в родителе, где фасад — обычный JS-объект без связи
с диспатчем ребёнка.

* Новое направление транспорта: [`FrameEventKind::Resource`] в том же ящике
  событий; постановка — натив `_lumen_f_queue_parent_resource(nid, type)`
  через глобальное зеркало `_lumen_frame_mirror_resource` (`FRAME_BRIDGE_SHIM`),
  которое зовёт `_lumen_resource_fire` (`dom.rs`) ПОСЛЕ локального диспатча.
  Гейты постановки: живой слот родителя, собственный документ
  ([`FrameDocSlots::self_doc`] — новый клон Arc рядом с `self_key`,
  ставится `install_dom`) и nid-элемент собственной арены. Топ-страница
  платит один пустой вызов натива и конвертов не ставит.
* Отправитель адресуется ключом своего документа (`source_doc` — новое поле
  конверта, у прежних видов не читается): bid биндинга разрешает реестр
  ПОЛУЧАТЕЛЯ при разборе ящика, как `event.source` у postMessage. Там же
  стоит фильтр доступности: нет биндинга или `accessible: false` — конверт
  снимается и отбрасывается (у cross-origin/opaque детей фасадов элементов
  не существует).
* Фасадный элемент получил `addEventListener`/`removeEventListener` (карта
  слушателей в WeakMap замыкания шима — фасады не настоящие EventTarget);
  ветка `'resource'` пумпы разбирается В САМОМ бридж-шиме, а не хуком
  WEB_API_SHIM: получатели (интернированные фасады и их карта) приватны для
  замыкания. Event строится заново в изоляте родителя (`target`/
  `currentTarget` — интернированный фасад, `isTrusted = true` — движковое),
  порядок вызова — слушатели, затем свойство `on<type>`, как у локального
  `_lumen_resource_fire`.

Отклонения среза: ребро только «ребёнок → непосредственный родитель» (внук
зеркалится среднему фрейму, как у postMessage среза 4); доставка асинхронная
на тике пумпы — синхронного диспатча через границу нет ни в одном направлении;
событие в изоляте получателя — новый объект, не тот же инстанс, что у ребёнка;
зеркалятся только события `_lumen_resource_fire` (script/link/style/track/
source) — медиа-шимы диспатчат мимо него.

Проверка: clippy `-p lumen-js --features v8-backend` / `-p lumen-shell`
-D warnings; тесты js lib 3288 ok (+6 на паре изолятов с общими документами:
доставка обоих видов обработчикам с порядком и полями события, removeEventListener,
гейты зеркала (топ-страница / текстовый узел / чужой nid / минимальный изолят),
потеря конверта без биндинга и при cross-origin, заголовочный сценарий — error
неудавшегося внешнего скрипта возвращается родителю в фасадный `onerror`);
shell 1610+2 ok; `dump_golden.py` 12/12; живые смоуки новый
`tests/wpt/verify_frame_resource_events.py` (200 → `a.onload` + слушатель +
исполнение тела, 404 → `b.onerror`, поля события `target===s trusted=true
bubbles=false`) и все шесть регрессионных срезов 4–9 (`run_script`/`actions`/
`click`/`mutation`/`post_message`/`external_script`) — зелёные на одной сборке.

Не входит (очередь): подресурсы `<img>`/`<link>` фрейма (парсерные элементы
ребёнка по-прежнему никем не запрашиваются), restyle/layout/paint фрейма,
`document.open/write/close`, sibling↔sibling postMessage, вызов функций между
изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов; уведомление шелла о фокусе внутри фрейма — вместе с
layout/rAF фреймов.

## Срез 11 (P3, 2026-08-26) — подресурсы парсерных элементов ребёнка: `<img src>` и `<link rel=stylesheet>`

Первый пункт очереди срезов 9–10 закрыт: парсерные `<img>` и
`<link rel=stylesheet>` под-документа теперь запрашиваются сетью (срез 24
доказал записью запросов, что за ними не ходил никто) и отчитываются перед
страницей `load`/`error`.

* Запрос — [`fetch_frame_subresources`] (`crates/shell/src/main.rs`) сразу
  после разбора HTML ребёнка и ДО его скриптов (парсерный порядок: источник
  запроса — шаг разбора). Стили идут тем же проходом страницы
  [`load_linked_stylesheets`] с media-гейтом экранного контекста страницы
  (`@import`-ы листа запрашиваются внутри); картинки — picker
  [`lumen_layout::collect_image_requests`] (`<picture>`/`srcset`, ключ URL
  совпадает с будущим layout), параллельно через [`parallel_map`]. Текст
  каскада выбрасывается (фреймы пока не рендерятся), нужны только исходы.
* Доставка — [`deliver_frame_subresource_events`] между DCL и window load
  ребёнка: стили через общий `_lumen_deliver_parser_link_events` (пер-узловой
  флаг «уже отчитался» гасит двойной отчёт для ссылок, вставленных скриптами
  самого ребёнка), картинки — `_lumen_resource_fire`, как парсерные
  `<script src>` (BUG-804). Зеркало среза 10 автоматически доставляет те же
  события обработчикам фасадов родителя. Инлайн-скрипты ребёнка исполняются
  раньше доставки, поэтому слушатели успевают встать до событий.

Отклонения среза: охват = паритет со страницей — `<video poster>`/
`<input type=image>`/SVG `<image>`/`<link rel=icon>` не запрашиваются и у
top-level (BUG-848); lazy-картинки фрейма не запрашиваются вовсе (прокси
вьюпорта отсутствует — так же срез 1 пропускает сами `loading=lazy`-iframe);
байты картинок не декодируются и в IMAGE_CACHE не попадают (пиксели некому
рисовать); print-PDF под-документов вне среза (гейт всегда экранный).

Проверка: clippy `-p lumen-shell` -D warnings; тесты shell 1611+2 ok (+1:
исходы link/img по файловой базе — оба исхода каждого вида, пропуск
rel=alternate и loading=lazy, порядок DOM); `dump_golden.py` 12/12; живой
смоук `tests/wpt/verify_frame_subresources.py` (запись запросов сервера: ok.css,
404-css, ok.png, 404-png запрошены, lazy.png — нет; события на элементах:
linkOk=load, linkBad=error, imgOk=load, imgBad=error; исходы до window load)
и все семь регрессионных срезов 4–10 — зелёные на одной сборке.

Не входит (очередь): restyle/layout/paint фрейма (и вслед за ними — применение
листов каскада, декод картинок в IMAGE_CACHE, вьюпортная прокси для lazy),
`document.open/write/close`, sibling↔sibling postMessage, вызов функций между
изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов; уведомление шелла о фокусе внутри фрейма — вместе с
layout/rAF фреймов.

## Срез 12 (P3, 2026-08-28) — layout ребёнка: контентная геометрия внутри фрейма

Первый пункт очереди «restyle/layout/paint фрейма» закрыт частично: сам
sub-документ фрейма теперь считает cascade + layout, и `getBoundingClientRect`/
`offsetWidth`/`offsetHeight` внутри фрейма отдают реальные числа вместо
«честных нулей» (`frame_bridge.rs`: «layout содержимого фрейма — отдельный
срез», срез 3). Paint остаётся серой заглушкой — компоновка display list
ребёнка в бокс `<iframe>` не входит в этот срез (см. «Не входит» ниже).

* Текст каскада ребёнка, который срез 11 запрашивал и отбрасывал
  (`fetch_frame_subresources` шла только за исходами load/error), теперь
  собирается тем же порядком, что у страницы (`parse_and_layout`):
  инлайновые `<style>` через `extract_style_blocks`/`inline_css_imports`
  (с разрешением `@import`), затем внешние `<link rel=stylesheet>` —
  и возвращается в новом поле `FrameSubresourceOutcomes::css`.
* `load_frame_sub_documents` парсит этот текст (`lumen_css_parser::parse`) и
  считает `lumen_layout::layout_measured` сразу после регистрации
  `parent`/`top` (срез 3) и до `notify_dom_content_loaded`/
  `notify_window_loaded` (срез 1) — так что скрипты, которые ребёнок
  выполняет в ответ на свои же DOMContentLoaded/load, уже видят геометрию.
  Измеритель — `page_measurer` (тот же bundled Inter + системные face-ы, что
  у страницы), но без web-шрифтов ребёнка (`web_fonts: &[]` — задел
  следующего среза, у фрейма пока нет собственного прохода `@font-face`
  url()-загрузки).
* Вьюпорт — новая константа `FRAME_UA_DEFAULT_SIZE` (300×150 CSS px,
  HTML LS §4.8.5, тот же UA-дефолт, что уже резолвит host-бокс `<iframe>` в
  родителе — `iframe_ua_default_size_300_by_150` в `lumen-layout`). Не
  реальный размер host-бокса: `load_frame_sub_documents` вызывается ДО
  layout страницы-родителя, так что фактический размер ещё не известен —
  уточнение до атрибутов `width`/`height`/CSS-переопределения родителя
  остаётся в очереди.
* Результат layout нигде не сохраняется (ни на `FrameHandle`, ни где-либо
  ещё) — используется только для одного прохода `collect_layout_rects` →
  `js.update_layout_rects(rects)` + `js.update_viewport_size(300, 150)` в
  JS-контекст ребёнка, тем же механизмом, каким страница делает это для
  себя (`page_load.rs`, «push initial layout geometry»). Геометрия
  считается один раз при загрузке; relayout ребёнка при последующих
  мутациях (его собственных или пришедших от родителя, срез 5) не входит —
  ни у страницы, ни тем более у фрейма пока нет инкрементального restyle
  для фреймов (BUG-341 стоит на паузе решением пользователя).

Отклонения среза: вьюпорт фиксированный UA-дефолт для ВСЕХ фреймов
(независимо от `width`/`height`-атрибутов и CSS) — HTML-атрибуты `<iframe>`
уже резолвятся в host-боксе родителя (`style.rs::apply_image_presentational_hints`),
но контентный вьюпорт ребёнка их пока не читает; paint (компоновка display
list ребёнка в бокс `<iframe>` вместо серой заглушки `BoxKind::Iframe`,
`display_list.rs`) и декод картинок в `IMAGE_CACHE` — следующие срезы
очереди, эта правка их не трогает и потому не требует ни графического
прогона, ни регенерации CPU-снапшотов (`dump_golden.py` подтверждает
нейтральность display list — 12/12).

Проверка: clippy `-p lumen-shell` -D warnings; тесты shell 1611+2+1 ok
(регресс: `frame_subresources_fetch_links_and_imgs_with_outcomes` и все
`frame_access_*`/`iframe_sandbox_*` — зелёные); `dump_golden.py` 12/12; живой
смоук новый `tests/wpt/verify_frame_layout.py` (явные `width`/`height` →
реальный `getBoundingClientRect`, `offsetWidth`/`offsetHeight` совпадают с
ним, `width:100%` резолвится против 300px UA-дефолтного вьюпорта — 284px с
учётом UA-дефолтного `margin: 8px` у `<body>`) и все восемь регрессионных
срезов 4–11 (`run_script`/`actions`/`click`/`mutation`/`post_message`/
`external_script`/`resource_events`/`subresources`) — зелёные на одной
сборке.

Не входит (очередь): paint ребёнка (компоновка display list в бокс
`<iframe>` вместо серой заглушки), декод картинок фрейма в `IMAGE_CACHE`,
вьюпорт ребёнка по реальному host-боксу/атрибутам `width`/`height`,
`@font-face` url()-шрифты ребёнка, relayout ребёнка при мутациях, вьюпортная
прокси для `loading=lazy` (ни для картинок ребёнка, ни для самого
`<iframe loading=lazy>` — срез 1 пропускает такие фреймы целиком),
`document.open/write/close`, sibling↔sibling postMessage, вызов функций
между изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов; уведомление шелла о фокусе внутри фрейма — вместе с
rAF фреймов.

## Срез 13 (P3, 2026-08-28) — вьюпорт ребёнка по РЕАЛЬНОМУ host-боксу

Очередь среза 12 ставила paint выше вьюпорта; срез 13 меняет их местами, и это
не вкусовщина. Компоновать display list ребёнка, посчитанного на UA-дефолтных
300×150, в бокс размером, скажем, 800×600 — значит нарисовать содержимое в
левой трети бокса и переделать это же в следующем срезе; вьюпорт — жёсткая
предпосылка paint, а не соседний пункт.

* Новый проход [`sync_frame_viewports`] (`crates/shell/src/frames.rs`) идёт
  **сразу после layout родителя**, там, где размер host-бокса впервые известен:
  ищет бокс хоста (`forms::find_layout_box`), берёт его КОНТЕНТНЫЙ бокс
  ([`host_content_size`] — `LayoutBox::rect` это border-бокс, поэтому рамки и
  padding вычитаются; та же арифметика, что у приватной `content_box_rect` в
  `display_list.rs`) и пересчитывает под него cascade + layout ребёнка тем же
  [`layout_frame_document`], которым срез 12 считал первый проход.
* Две точки вызова, а не одна: `parse_and_layout` (первая загрузка) и
  `Lumen::apply_relayout_result` — общая воронка ВСЕХ relayout, поэтому
  `width:100%`-фрейм переживает ресайз окна, зум и любое движение вёрстки над
  ним. Проход стоит ноль на странице без фреймов (ранний выход по пустому
  списку) и ноль при неизменившемся размере: `FrameHandle::viewport` хранит
  вьюпорт последнего посчитанного прохода, а relayout случается на каждый кадр
  анимации. Вызов стоит ДО заимствования `layout_source` — там берётся `&self`
  на всю функцию.
* `FrameHandle` получил три поля: `depth`, `sheet` (разобранный каскад ребёнка
  — срез 12 собирал его текст и выбрасывал, повторный разбор на каждый ресайз
  был бы чистой тратой) и `viewport`.

Отклонения среза, оба следствия того, ГДЕ известен размер:

* **Первый проход остаётся UA-дефолтным.** Скрипты ребёнка, его
  DOMContentLoaded и `load` исполняются внутри `load_frame_sub_documents`, то
  есть до layout страницы; обнулить им geometry было бы хуже, чем дать
  300×150. Живой замер: `width:100%` в собственном `load` ребёнка = 284px
  (300 − 2×8 UA-margin `<body>`), а после пересчёта — 484px (500 − 16).
  Событие о смене вьюпорта ребёнку не отправляется (у фреймов нет ни `resize`,
  ни rAF — они вместе с paint в очереди), так что страница, кэширующая размер
  в своём `load`, останется на UA-дефолте.
* **Фреймы глубины ≥ 1 пропускаются.** Их host-элемент живёт в дереве
  ПРОМЕЖУТОЧНОГО фрейма, а `NodeId` уникален только внутри своего документа —
  поиск такого узла в layout страницы дал бы либо ничего, либо чужой бокс с
  совпавшим индексом. Им нужен сохранённый layout их собственного родителя;
  сохранение layout ребёнка — ровно то, что понадобится paint, поэтому оба
  пункта уезжают в следующий срез вместе.

Что вскрыл живой замер и чего не было в постановке: **скрипт, который родитель
вставляет в `contentDocument`, не может наблюдать состояние ДО layout
страницы** — доставка через границу изолятов асинхронна (срез 8), конверт
разбирается на тике пумпы, а тик всегда позже layout. Первая версия пробы
ассертила «в фазе load вьюпорт ещё 300» и падала на двух проверках из десяти,
хотя движок был прав. Единственное место, откуда стадия среза 12 вообще видна,
— собственный `load` ребёнка; проба снимает теперь обе стадии там, где каждая
наблюдаема.

Проверка: clippy `-p lumen-shell --all-targets` -D warnings; тесты shell
1612+2 ok (+1: `host_content_size` на реальном layout —
`<iframe width=400 height=200>` с рамкой 5px и padding 3px даёт ребёнку
ровно 400×200, при том что его `rect` шире; второй ассерт не декоративный —
без него тест прошёл бы и если бы вычитание не выполнялось вовсе);
`dump_golden.py` 12/12 (display list страницы не тронут — фрейм по-прежнему
серая заглушка, поэтому ни графического прогона, ни регенерации
CPU-снапшотов срез не требует); живой смоук обновлённый
`tests/wpt/verify_frame_layout.py` (обе стадии, `matchMedia('(min-width:
400px)')` в ребёнке переключается false → true — независимый признак того,
что доехали не только прямоугольники, но и сам вьюпорт) и все восемь
регрессионных срезов 4–11 (`run_script`/`actions`/`click`/`mutation`/
`post_message`/`external_script`/`resource_events`/`subresources`) — зелёные
на одной сборке.

Не входит (очередь): paint ребёнка (компоновка display list в бокс
`<iframe>` вместо серой заглушки) вместе с сохранением layout ребёнка на
хэндле, а с ним — вьюпорт вложенных фреймов (глубина ≥ 1); декод картинок
фрейма в `IMAGE_CACHE`, `@font-face` url()-шрифты ребёнка, relayout ребёнка
при мутациях (его собственных или пришедших от родителя, срез 5) и событие
`resize` ребёнку при смене вьюпорта, вьюпортная прокси для `loading=lazy`,
`document.open/write/close`, sibling↔sibling postMessage, вызов функций между
изолятами, динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)),
навигация/замена/удаление фрейма, X-Frame-Options/CSP `frame-ancestors`,
bfcache фреймов; уведомление шелла о фокусе внутри фрейма — вместе с rAF
фреймов.

## Срез 14 (P3, 2026-08-28) — содержимое фрейма НА ЭКРАНЕ, вложенные фреймы

Срез 13 дал ребёнку правильный вьюпорт, но его пиксели по-прежнему никуда не
шли: `<iframe>` рисовался серой заглушкой (`BoxKind::Iframe` → `DrawImage` с
незарегистрированным ключом), а `--dump-display-list` был единственным местом,
где это вообще видно. Срез 14 вклеивает display list под-документа в список
страницы.

* `FrameHandle` получил четыре поля: `layout` (layout ребёнка на текущем
  вьюпорте — по нему рисуется содержимое И в нём ищется host-бокс ВЛОЖЕННОГО
  фрейма), `content_dl` (готовый display list ребёнка в его собственных
  координатах, с уже вклеенным содержимым его вложенных фреймов), `host_rect`
  (контентный бокс хоста — адрес вклейки) и `host_src` (вторая половина ключа
  поиска заглушки). `host_content_size` стала `host_content_rect`: срезу нужен
  не только размер, но и НАЧАЛО бокса.
* `splice_frame_content` (`crates/shell/src/frames.rs`) заменяет команду-
  заглушку на `PushClipRect` + `PushTransform(translation_2d(x, y))` +
  содержимое + `PopTransform` + `PopClip`. Идемпотентна: после вклейки
  заглушки в списке нет, повторный проход ничего не находит — а он бывает,
  список страницы пишется не в одном месте.
* Заглушка ищется по **паре** «`src` + прямоугольник». Одного `src` мало (два
  `<iframe src="">` на странице — обычное дело), одного прямоугольника мало для
  гарантии, что это заглушка, а не совпавшая по геометрии картинка. Порядок
  слагаемых в `host_content_rect` побитово повторяет приватную
  `content_box_rect` из `display_list.rs`: сравнение float переживает
  перестановку слагаемых не всегда, а расхождение хоть на рамку означало бы,
  что вклейка молча не находит ничего.
* Вложенные фреймы (глубина ≥ 1), которые срез 13 пропускал целиком, работают:
  `sync_frame_viewports` идёт по ВОЗРАСТАНИЮ глубины (host-бокс фрейма глубины
  `d` живёт в layout фрейма `d-1`, а тот готов ровно после предыдущего
  прохода), а display list собирается по УБЫВАНИЮ (в список фрейма вклеено
  содержимое его собственных детей). Родитель адресуется `Arc::ptr_eq` по
  документу, а не индексом в плоском списке: вложенные хэндлы попадают в него
  РАНЬШЕ своего родителя, а `NodeId` уникален лишь внутри своего документа.
* `host_rect` пишется ВСЕГДА, а не только при смене размера: фрейм может уехать
  вниз, не изменив габаритов (что-то над ним выросло), и тогда вклеивать надо
  по новому адресу.

**Чего не было в постановке и что нашлось измерением — порядок записи списка.**
Сама вклейка была правильной, а на экране не появлялось ничего: в
`Lumen::apply_loaded_page` `set_display_list(page.display_list)` стоял ВЫШЕ
`self.frames = page.frames`, то есть первый кадр новой страницы склеивался с
фреймами ПРЕДЫДУЩЕЙ (а на первой загрузке — ни с чем), и содержимое проявлялось
лишь после первого relayout. Рядом нашёлся второй экземпляр той же формы, уже
не про порядок: резервный (безоконный) путь `reload()` не заменял `self.frames`
ВОВСЕ — хэндлы предыдущего документа переживали навигацию, что до этого среза
было невидимо, а с ним означало бы содержимое старой страницы, нарисованное
внутри новой.

Три точки вклейки, а не одна, и все три обязательны: `set_display_list` (общая
воронка записи), `apply_relayout_result` (там список пишется напрямую — мимо
метода, потому что `layout_source` уже заимствован) и путь программного скролла
контейнера в `about_to_wait` (`paint_ordered` пересобирает список из layout и о
фреймах не знает — без вклейки содержимое исчезало бы на первом же скролле).
В `apply_relayout_result` вклейка стоит ПОСЛЕ `content_height_of`/
`content_width_of`: обе складывают плоский список прямоугольников и клипов не
видят, поэтому содержимое фрейма растянуло бы прокрутку страницы.

`--dump-display-list` тоже вклеивает (`dump_mode.rs`) — иначе дамп остался бы
единственным местом, где фрейм серый, и проверить срез headless было бы нечем.

Проверка: clippy `-p lumen-shell --all-targets` -D warnings; тесты shell
1615+2 ok (+3 новых: замена заглушки с проверкой координат клипа и сдвига,
идемпотентность, отказ вклеить при несовпавшем прямоугольнике — заглушку во
всех трёх рисует НАСТОЯЩИЙ эмиттер через `paint_ordered`, так что тест ловит и
расхождение `host_content_rect` с `content_box_rect`); `dump_golden.py` 12/12.
Живые дампы: одиночный фрейм (содержимое ребёнка в клипе по контентному боксу,
сдвиг на его начало), два уровня вложенности (содержимое внука внутри
содержимого сына), несуществующий `src` (заглушка на месте), `srcdoc`
(вклеивается), `<iframe>` без `src` (заглушка — хэндл для такого не заводится
вовсе). Ни графического прогона, ни регенерации CPU-снимков срез не требует, и
это структурный факт, а не наблюдение: `lumen-driver` (владелец
`cases::snapshot_cpu`) не зависит от `lumen-shell` и строит display list сам,
поэтому шелловая вклейка до него не доходит; в графнаборе `run.py` страниц с
`<iframe>` нет вовсе. Обратная сторона того же факта: **пиксельного гейта у
рисования фреймов теперь нет** — единственная страница с `<iframe>` во всём
наборе, `1000000-final.html`, живёт в CPU-снимках, то есть на пути, который
вклейку не видит.

Не входит (очередь): картинки фрейма (его `<img>` не декодируются в
`IMAGE_CACHE`, поэтому внутри фрейма они — заглушки), hit-test и скролл внутри
фрейма, `@font-face` url()-шрифты ребёнка, relayout ребёнка при мутациях (его
собственных или пришедших от родителя, срез 5) и событие `resize` ребёнку при
смене вьюпорта, вьюпортная прокси для `loading=lazy`, `document.open/write/
close`, sibling↔sibling postMessage, вызов функций между изолятами,
динамически созданные фреймы ([BUG-885](BUG-885-OPEN.md)), навигация/замена/
удаление фрейма, X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов;
уведомление шелла о фокусе внутри фрейма — вместе с rAF фреймов.

## Срез 15 (P3, 2026-08-28) — картинки внутри фрейма

Срез 11 ходил за `<img src>` ребёнка, но брал только БАЙТЫ
([`fetch_image_bytes`]) и выбрасывал их: декодировать было незачем, пока
содержимое фрейма не попадало на экран. Срез 14 его туда привёл — и картинки
остались единственным, что внутри правильно нарисованного фрейма рисуется серым.
Измерено ДО правки (`tests/wpt/verify_frame_images.py`, `--screenshot` + журнал
запросов сервера): все четыре файла запрошены, картинка страницы нарисована,
картинка фрейма — **0 из 20 000 пикселей**.

* [`fetch_frame_subresources`] проходит теперь весь путь страницы:
  [`decode_image`] через `IMAGE_CACHE`, `apply_intrinsic_size` в дерево ребёнка
  (без него `<img>` без атрибутов лёг бы нулевым боксом — вторая фаза, как у
  `fetch_and_decode_images`: сеть и декод параллельно, мутация документа
  последовательно) и пиксели наружу в `FrameHandle::images`.
* Пиксели едут в **общий** `LoadedPage::images` (`page_pipeline.rs`), а не в
  новый список: регистрацию картинок делают четыре разных места
  (`apply_loaded_page`, `reload`, `pending_images`, CPU-кэш снимков), и ни одно
  из них срез не трогает — они подхватывают чужие строки как свои.

**Ключ регистрации пришлось развести, и это не украшение.** Ключ картинки в
`IMAGE_CACHE`, в `Renderer::register_image` и в `DrawImage.src` — сырое значение
атрибута, уникальное лишь внутри ОДНОГО документа: страница и фрейм из другого
каталога легко держат каждый свой `<img src="pic.png">`, и с общим ключом во
фрейме молча оказалась бы картинка страницы. Ключом стал разрешённый относительно
базы РЕБЁНКА адрес ([`frame_image_key`]) — он же, наоборот, схлопывает
действительно один и тот же файл. Расплата: `paint_ordered` кладёт в список
сырой `src`, поэтому [`rekey_frame_images`] переписывает ключи в display list
ребёнка. Два правила этого прохода не выводятся из его названия:

* в карте лежат и **не загрузившиеся** картинки — иначе их сырой ключ остался бы
  в списке и совпал бы с чужим зарегистрированным;
* заглушка ВЛОЖЕННОГО фрейма пропускается по `src`: вклейка ищет её именно по
  нему, и переписанный ключ дал бы серый прямоугольник вместо содержимого внука
  — то есть дефект выглядел бы как «вложенные фреймы сломались», хотя картинка
  при этом нарисовалась бы верно. Отсюда и порядок: переписать ключи ДО
  `splice_children_of`.

**Второй дефект, которого не было в постановке и который нашёлся только пробой:**
`--screenshot` содержимое фреймов не вклеивал вовсе. Срез 14 завёл три точки
вклейки на живом пути и одну в `--dump-display-list`, а снимок собирает список
сам (`dump_mode.rs`, `paint_ordered(&parsed.layout)`) — то есть на нём весь фрейм
оставался серой заглушкой, картинки там или нет. Это ровно та форма, которую
срез 14 назвал риском («страница, чей список пишет какой-то ЧЕТВЁРТЫЙ путь,
молча покажет заглушку»), и она уже существовала. `--print-to-pdf` — не тот же
случай и в срез не входит: он строит список через `paginate` +
`build_print_display_list`, куда содержимое фрейма надо не вклеивать, а
разбивать по страницам.

Проверка: clippy `-p lumen-shell --all-targets` -D warnings; тесты shell
1616+2 ok (+1 новый на [`rekey_frame_images`] — свой ключ переписан, заглушка
вложенного фрейма цела, чужого не тронули; тест среза 11 расширен: фикстура
стала НАСТОЯЩИМ PNG и относительным URL, потому что исход `<img>` теперь
означает «пиксели есть», а не «байты прочитались», а на абсолютном пути
разрешённый ключ совпал бы с сырым и ничего не доказывал); `dump_golden.py`
12/12 (у страниц набора фреймов нет); живая проба `verify_frame_images.py` в
двух вариантах — плоском и вложенном (картинка ВНУКА, глубина 1: она проходит
через два уровня вклейки и переписывание ключей): 19 200 из 20 000 пикселей
красного и ноль синего, то есть картинка страницы во фрейм не протекла.
Пиксельного гейта у рисования фреймов по-прежнему нет (срез 14 объяснил
почему), так что проба — единственная защита от регрессии.

Не входит (очередь): анимированный GIF во фрейме (первый кадр рисуется,
анимация — нет: `Lumen::animated_gifs` тикает только карту страницы),
`background-image` ребёнка (за ними никто не ходит), `loading=lazy` внутри
фрейма (нужна вьюпортная прокси), дедуп одного и того же файла между страницей
и фреймом при РАЗНОМ написании URL (сырой `src` страницы против разрешённого
ключа ребёнка — декод дважды), плюс всё, что перечислил срез 14: hit-test и
скролл внутри фрейма, `@font-face` url()-шрифты ребёнка, relayout ребёнка при
мутациях и `resize` ему при смене вьюпорта, `document.open/write/close`,
sibling↔sibling postMessage, вызов функций между изолятами, динамически
созданные фреймы ([BUG-885](BUG-885-OPEN.md)), навигация/замена/удаление
фрейма, X-Frame-Options/CSP `frame-ancestors`, bfcache фреймов, содержимое
фрейма в `--print-to-pdf`.
