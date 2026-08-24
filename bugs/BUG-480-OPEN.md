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
   ([BUG-826](BUG-826-OPEN.md)); доказательство здесь только со стороны
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
[BUG-854](BUG-854-OPEN.md).

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
