# Отложенные указатели очередей — 2026-10-28

Решение пользователя 2026-10-28 ([`docs/dev-roles.md`](../dev-roles.md) §Where work comes from):
очереди P1–P4 переведены на реальные сайты и скорость загрузки (`LOAD-*`, `SITES-*`).
Задачи ниже выросли из WPT и углов спецификаций; они не удалены, а отложены.
Строки ROADMAP/BUGS/CSS-SPECS не тронуты: вернуть задачу — найти её по ID
(`grep -n '| <ID> |' ROADMAP.md`, `grep -n '\[BUG-NNN\]' BUGS.md`) и дописать указатель
в конец очереди, когда отчёт триажа (`docs/perf/runs/*-compat-triage.md`) называет сайт,
которому она нужна. Номера строк в колонке «был» — на момент заморозки и дальше протухают.

## STATUS-P1 — 73

| ID | был | суть |
|---|---|---|
| AUTOMATION-KEY-DEFAULT | `ROADMAP.md:1292` | Дефолтные действия клавиш в автоматизации (кластер I разведки WPT-SHADOW-SCOUT, docs/wpt-vendor-notes/shadow-dom.md: 72 id, потолок ≈ 70 бал |
| SHADOW-ADOPTED-SS | `ROADMAP.md:1293` | Атрибут shadowrootadoptedstylesheets и IDL HTMLTemplateElement.shadowRootAdoptedStyleSheets (кластер B разведки WPT-SHADOW-SCOUT: 39 id в sh |
| SHADOW-REFTARGET | `ROADMAP.md:1294` | Reference Target (tentative, shadowrootreferencetarget / ShadowRoot.referenceTarget): кластер L разведки WPT-SHADOW-SCOUT — shadow-dom/refer |
| SHADOW-LAYOUT-2 | `ROADMAP.md:1295` | Хит-тест и раскладка через границы теневых деревьев (кластер M разведки WPT-SHADOW-SCOUT, 24 id, потолок ≈ 14 баллов): (1) DocumentOrShadowR |
| WEBCRYPTO-PQ | `ROADMAP.md:1298` | Постквантовые алгоритмы SubtleCrypto (tentative, кластер C3 разведки WPT-CRYPTO-SCOUT, docs/wpt-vendor-notes/WebCryptoAPI.md: ≈20 id, потоло |
| STREAMS-LAZY-PULL | `ROADMAP.md:1299` | start()/pull() ReadableStream, WritableStream и TransformStream — по спецификации, а не синхронно в конструкторе (кластер E разведки WPT-STR |
| STREAMS-PIPETO | `ROADMAP.md:1300` | ReadableStream.prototype.pipeTo/pipeThrough по алгоритму WHATWG Streams §4.7 ReadableStreamPipeTo (кластер P разведки WPT-STREAMS-SCOUT: pip |
| STREAMS-ITER-TEE | `ROADMAP.md:1302` | ReadableStream.from(), асинхронный итератор потока и tee() по спецификации (кластер I разведки WPT-STREAMS-SCOUT: readable-streams/{from,asy |
| SW-MODULE | `ROADMAP.md:1305` | Модульные сервис-воркеры register(url, {type: 'module'}) (кластер I разведки WPT-SW-SCOUT, docs/wpt-vendor-notes/service-workers.md: 12 id,  |
| SW-PRELOAD-ROUTER | `ROADMAP.md:1306` | Navigation preload и static routing API (кластеры H и H2 разведки WPT-SW-SCOUT, docs/wpt-vendor-notes/service-workers.md: 7 + 13 id, потолок |
| VTT-ADDTRACK | `ROADMAP.md:1308` | HTMLMediaElement.addTextTrack() и TextTrack.addCue()/removeCue() (кластер H5 разведки WPT-VTT-SCOUT, docs/wpt-vendor-notes/webvtt.md: 9 id a |
| WASM-CLONE | `ROADMAP.md:1310` | Structured clone WebAssembly.Module и WebAssembly.Memory (кластер W4 разведки WPT-WASM-SCOUT, docs/wpt-vendor-notes/wasm.md: 28 id wasm/seri |
| FETCH-LATER | `ROADMAP.md:1313` | fetchLater() и FetchLaterResult (кластер FL разведки WPT-FETCH-SCOUT, docs/wpt-vendor-notes/fetch.md: 47 id fetch/fetch-later/, OK 37 / TIME |
| FIELDSET-LEGEND-5-S1 | `ROADMAP.md:1328` | Срез 1 FIELDSET-LEGEND-5, бриф — docs/tasks/FIELDSET-LEGEND-5-slices.md §S1. Legend шире content box: legend_inline_offset (crates/engine/la |
| FIELDSET-LEGEND-5-S2 | `ROADMAP.md:1329` | Срез 2 FIELDSET-LEGEND-5, бриф — docs/tasks/FIELDSET-LEGEND-5-slices.md §S2. Интринсик-размеры вертикального flex/grid-fieldset считают lege |
| FIELDSET-LEGEND-5-S3 | `ROADMAP.md:1330` | Срез 3 FIELDSET-LEGEND-5, бриф — docs/tasks/FIELDSET-LEGEND-5-slices.md §S3. border-radius у groove/ridge-рамки не скругляется: rasterize_dr |
| NAV-API-2-S1 | `ROADMAP.md:1331` | Срез 1 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S1. Интерфейс NavigationActivation и navigation.activation: JS-класс и геттер в cra |
| NAV-API-2-S2 | `ROADMAP.md:1332` | Срез 2 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S2. Атрибуты NavigateEvent: sourceElement, downloadRequest, formData, настоящий use |
| NAV-API-2-S3 | `ROADMAP.md:1333` | Срез 3 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S3. Оболочка передаёт источник навигации в _lumen_dispatch_navigate: navigate_to_in |
| NAV-API-2-S4 | `ROADMAP.md:1334` | Срез 4 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S4. intercept({focusReset}): проверка enum (TypeError), режим manual, сброс фокуса  |
| NAV-API-2-S5 | `ROADMAP.md:1335` | Срез 5 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S5. intercept({scroll}) и NavigateEvent.scroll(): проверка enum, ошибки InvalidStat |
| NAV-API-2-S6 | `ROADMAP.md:1336` | Срез 6 NAV-API-2, бриф — docs/tasks/NAV-API-2-slices.md §S6. intercept({precommitHandler}) и NavigationPrecommitController (redirect, addHan |
| IDB-EVENTS-S1 | `ROADMAP.md:1337` | Срез 1 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S1. Новый диспетчер событий IndexedDB в crates/js/src/shim/idb_shim.js: IDBReques |
| IDB-EVENTS-S2 | `ROADMAP.md:1338` | Срез 2 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S2. Флаг активности транзакции и microtask checkpoint между событиями: _lumen_idb |
| IDB-EVENTS-S3 | `ROADMAP.md:1339` | Срез 3 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S3. Исключение в слушателе success, error или upgradeneeded откатывает транзакцию |
| IDB-EVENTS-S4 | `ROADMAP.md:1340` | Срез 4 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S4. Форма IDL: глобалы IDBFactory, IDBVersionChangeEvent, отдельный IDBCursorWith |
| IDB-EVENTS-S5 | `ROADMAP.md:1341` | Срез 5 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S5. Порядок проверок и состояние для IDBCursor (advance, continue, update, delete |
| IDB-EVENTS-S6 | `ROADMAP.md:1342` | Срез 6 IDB-EVENTS, бриф — docs/tasks/IDB-EVENTS-slices.md §S6. Порядок проверок и состояние для IDBObjectStore (add/put/delete/clear/deleteI |
| ANCHOR-SCROLL-3-S2 | `ROADMAP.md:1343` | Срез 2 ANCHOR-SCROLL-3, бриф — docs/tasks/ANCHOR-SCROLL-3-slices.md §S2. position-area в прокручиваемом содержащем блоке: сетка строится по  |
| ANCHOR-SCROLL-3-S3 | `ROADMAP.md:1344` | Срез 3 ANCHOR-SCROLL-3, бриф — docs/tasks/ANCHOR-SCROLL-3-slices.md §S3. position: fixed с якорем в потоке документа следует за прокруткой с |
| SVG-FILTER-10-S2 | `ROADMAP.md:1345` | Срез 2 SVG-FILTER-10, бриф — docs/tasks/SVG-FILTER-10-slices.md §S2. Граф фильтра читает flood-color/flood-opacity/lighting-color/color-inte |
| SVG-FILTER-10-S3 | `ROADMAP.md:1346` | Срез 3 SVG-FILTER-10, бриф — docs/tasks/SVG-FILTER-10-slices.md §S3. Интерфейсы Filter Effects в JS-шиме (crates/js/src/shim/svg_shim_head.j |
| SVG-FILTER-10-S4 | `ROADMAP.md:1347` | Срез 4 SVG-FILTER-10, бриф — docs/tasks/SVG-FILTER-10-slices.md §S4. Ряд InlineBlockRow из бокса нулевого размера (<svg width=0 height=0>) и |
| SVG-FILTER-10-S5 | `ROADMAP.md:1348` | Срез 5 SVG-FILTER-10, бриф — docs/tasks/SVG-FILTER-10-slices.md §S5. Фильтр на пустом inline-элементе без содержимого (<span class=flood></s |
| SVG-FILTER-10-S6 | `ROADMAP.md:1349` | Срез 6 SVG-FILTER-10, бриф — docs/tasks/SVG-FILTER-10-slices.md §S6. bbox HTML-бокса с filter: url(#f) для процентов области и objectBoundin |
| WEBCRYPTO-CURVES-S1 | `ROADMAP.md:1350` | Срез 1 WEBCRYPTO-CURVES, бриф — docs/tasks/WEBCRYPTO-CURVES-slices.md §S1. Заменить четыре P-256-варианта KeyMaterial (EcdsaPrivate/EcdsaPub |
| WEBCRYPTO-CURVES-S2 | `ROADMAP.md:1351` | Срез 2 WEBCRYPTO-CURVES, бриф — docs/tasks/WEBCRYPTO-CURVES-slices.md §S2. ECDSA sign/verify на трёх кривых с параметром hash (SHA-1/256/384 |
| WEBCRYPTO-CURVES-S3 | `ROADMAP.md:1352` | Срез 3 WEBCRYPTO-CURVES, бриф — docs/tasks/WEBCRYPTO-CURVES-slices.md §S3. Ed25519 в новом subtle_okp.rs: generateKey (пара, usages sign у п |
| WEBCRYPTO-CURVES-S4 | `ROADMAP.md:1353` | Срез 4 WEBCRYPTO-CURVES, бриф — docs/tasks/WEBCRYPTO-CURVES-slices.md §S4. X25519 в subtle_okp.rs: generateKey, importKey/exportKey raw/spki |
| WEBCRYPTO-CURVES-S5 | `ROADMAP.md:1354` | Срез 5 WEBCRYPTO-CURVES, бриф — docs/tasks/WEBCRYPTO-CURVES-slices.md §S5. SubtleCrypto.getPublicKey(key, keyUsages) (tentative) для EC, Ed2 |
| WEBCRYPTO-SYM-S1 | `ROADMAP.md:1355` | Срез 1 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S1. AES-192 в AES-GCM/CBC/CTR (generate_key crates/js/src/subtle_crypto.rs: |
| WEBCRYPTO-SYM-S2 | `ROADMAP.md:1356` | Срез 2 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S2. Заменить заглушки wrapKey/unwrapKey (crates/js/src/shim/crypto_shim.js: |
| WEBCRYPTO-SYM-S3 | `ROADMAP.md:1357` | Срез 3 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S3. SHA-3 (SHA3-256/384/512): новый crates/js/src/subtle_hash.rs с enum Has |
| WEBCRYPTO-SYM-S4 | `ROADMAP.md:1358` | Срез 4 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S4. digest для cSHAKE128/256 (outputLength, functionName, customization), T |
| WEBCRYPTO-SYM-S5 | `ROADMAP.md:1359` | Срез 5 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S5. ChaCha20-Poly1305 (ключ 256 бит, nonce 12 байт, тег 16) и AES-OCB (ключ |
| WEBCRYPTO-SYM-S6 | `ROADMAP.md:1360` | Срез 6 WEBCRYPTO-SYM, бриф — docs/tasks/WEBCRYPTO-SYM-slices.md §S6. KMAC128/256 (NIST SP 800-185 поверх CShake128/256 из sha3 0.10: generat |
| STREAMS-BYOB-S1 | `ROADMAP.md:1361` | Срез 1 STREAMS-BYOB, бриф — docs/tasks/STREAMS-BYOB-slices.md §S1. Очередь чанков байтового контроллера (ReadableByteStreamController, crate |
| STREAMS-BYOB-S2 | `ROADMAP.md:1362` | Срез 2 STREAMS-BYOB, бриф — docs/tasks/STREAMS-BYOB-slices.md §S2. Очередь pull-into дескрипторов и ReadableStreamBYOBReader.read(view) (str |
| STREAMS-BYOB-S3 | `ROADMAP.md:1363` | Срез 3 STREAMS-BYOB, бриф — docs/tasks/STREAMS-BYOB-slices.md §S3. read(view, {min}) (поле minimumFill дескриптора), проверки respond(bytesW |
| STREAMS-BYOB-S4 | `ROADMAP.md:1364` | Срез 4 STREAMS-BYOB, бриф — docs/tasks/STREAMS-BYOB-slices.md §S4. ReadableByteStreamTee (§4.2.x) в ReadableStream.prototype.tee (streams_sh |
| STREAMS-BYOB-S5 | `ROADMAP.md:1365` | Срез 5 STREAMS-BYOB, бриф — docs/tasks/STREAMS-BYOB-slices.md §S5. Прогон всей категории streams/readable-byte-streams, добивка красных subt |
| STREAMS-TRANSFER-S1 | `ROADMAP.md:1366` | Срез 1 STREAMS-TRANSFER, бриф — docs/tasks/STREAMS-TRANSFER-slices.md §S1. Передача ReadableStream в structuredClone(rs, {transfer: rs}): ис |
| STREAMS-TRANSFER-S2 | `ROADMAP.md:1367` | Срез 2 STREAMS-TRANSFER, бриф — docs/tasks/STREAMS-TRANSFER-slices.md §S2. Передача WritableStream и TransformStream тем же механизмом (поло |
| STREAMS-TRANSFER-S3 | `ROADMAP.md:1368` | Срез 3 STREAMS-TRANSFER, бриф — docs/tasks/STREAMS-TRANSFER-slices.md §S3. transfer в MessagePort.postMessage (crates/js/src/shim/message_ch |
| STREAMS-TRANSFER-S4 | `ROADMAP.md:1369` | Срез 4 STREAMS-TRANSFER, бриф — docs/tasks/STREAMS-TRANSFER-slices.md §S4. Поток в Worker.postMessage и postMessage воркера через мост порто |
| SW-MSG-S1 | `ROADMAP.md:1370` | Срез 1 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S1. Цикл задач изолята сервис-воркера: подключить WORKER_TIMERS_SHIM вместо синхронных за |
| SW-MSG-S2 | `ROADMAP.md:1371` | Срез 2 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S2. В изоляте SW вместо литералов-событий (_sw_fire_event sw_worker.rs:313, _sw_fire_fetc |
| SW-MSG-S3 | `ROADMAP.md:1372` | Срез 3 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S3. Страница → воркер: ServiceWorker.postMessage (crates/js/src/shim/web_api_shim_mid_b.j |
| SW-MSG-S4 | `ROADMAP.md:1373` | Срез 4 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S4. Воркер → страница: Client.postMessage из SW кладёт сообщение в исходящую очередь (нов |
| SW-MSG-S5 | `ROADMAP.md:1374` | Срез 5 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S5. MessagePort в обе стороны: передача портов в worker.postMessage(data, port) и client. |
| SW-MSG-S6 | `ROADMAP.md:1375` | Срез 6 SW-MSG, бриф — docs/tasks/SW-MSG-slices.md §S6. Clients в SW (crates/js/src/sw_worker.rs:93, сейчас заглушки): реестр окон-клиентов o |
| VTT-RENDER-S1 | `ROADMAP.md:1376` | Срез 1 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S1. Разбор текста cue по WebVTT §6.4 (cue text parsing rules) в дерево узлов: нов |
| VTT-RENDER-S2 | `ROADMAP.md:1377` | Срез 2 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S2. Настройки cue (line/lineAlign/position/positionAlign/size/align/vertical/regi |
| VTT-RENDER-S3 | `ROADMAP.md:1378` | Срез 3 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S3. Поддерево боксов cue в UA-тени <video>: новый crates/engine/dom/src/vtt_tree. |
| VTT-RENDER-S4 | `ROADMAP.md:1379` | Срез 4 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S4. Раскладка: бокс Video строит детей из UA-тени, если там есть контейнер cue (c |
| VTT-RENDER-S5 | `ROADMAP.md:1380` | Срез 5 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S5. Мост JS - DOM и удаление самодельного оверлея: video_element.js собирает акти |
| VTT-RENDER-S6 | `ROADMAP.md:1381` | Срез 6 VTT-RENDER, бриф — docs/tasks/VTT-RENDER-slices.md §S6. Обход перекрытий (§7.2, шаг «avoid overlap»): после раскладки детей контейнер |
| CSSANIM-MODEL-2-S1 | `ROADMAP.md:1385` | Срез 1 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S1. События от скриптовой перемотки: после записи startTime/currentTime |
| CSSANIM-MODEL-2-S2 | `ROADMAP.md:1386` | Срез 2 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S2. Замена эффекта у CSS-анимации: сеттер effect в _css_new (web_api_sh |
| CSSANIM-MODEL-2-S3 | `ROADMAP.md:1387` | Срез 3 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S3. Стиль «до изменения» для непрочитанных свойств: при первом наблюден |
| CSSANIM-MODEL-2-S4 | `ROADMAP.md:1388` | Срез 4 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S4. Нарисованный кадр CSS-анимации: _css_cancel (web_api_shim_tail_b.js |
| CSSANIM-MODEL-2-S5 | `ROADMAP.md:1389` | Срез 5 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S5. То же для переходов: TransitionScheduler (crates/engine/layout/src/ |
| CSSANIM-MODEL-2-S6 | `ROADMAP.md:1390` | Срез 6 CSSANIM-MODEL-2, бриф — docs/tasks/CSSANIM-MODEL-2-slices.md §S6. display: none у предка отменяет анимацию потомка, а getComputedStyl |

## STATUS-P2 — 6

| ID | был | суть |
|---|---|---|
| WPT-DROP-1027-S4 | `ROADMAP.md:1322` | Срез 4 WPT-DROP-1027, бриф §S4. Регрессии группы C (вне css/, кроме A/B; 22 id эффекта хоста из docs/wpt-vendor-notes/fetch.md §WPT-HOST-WPT |
| WPT-DROP-1027-S5 | `ROADMAP.md:1323` | Срез 5 WPT-DROP-1027, бриф §S5. Регрессии css/CSS2/ (кластер bidi-text PASS→FAIL, cascade/at-import-009.xht, text, fonts) — как S3. Готово,  |
| WPT-DROP-1027-S6 | `ROADMAP.md:1324` | Срез 6 WPT-DROP-1027, бриф §S6. Регрессии раскладки (css-grid/grid-lanes — сверить с GRID-LANES-2 550a3b935, css-break/flexbox, css-align) — |
| WPT-DROP-1027-S7 | `ROADMAP.md:1325` | Срез 7 WPT-DROP-1027, бриф §S7. Регрессии print-reftest (css/css-page, css/printing): сначала изменения исполнителя в tests/wpt/ между сборк |
| WPT-DROP-1027-S8 | `ROADMAP.md:1326` | Срез 8 WPT-DROP-1027, бриф §S8. Регрессии остального css/ (filter-effects, css-text, css-anchor-position, css-cascade, selectors, css-shapes |
| WPT-DROP-1027-S9 | `ROADMAP.md:1327` | Срез 9 WPT-DROP-1027, бриф §S9. Сводная таблица вердикт × группа в docs/wpt/drop-1027.md, абзац в docs/wpt/pass-rate.md §«Пятая полная цифра |

## STATUS-P3 — 92

| ID | был | суть |
|---|---|---|
| BUG-1563 | `BUGS.md:225` | Typed OM: new CSSMathSum(a, b), CSSMathProduct, CSSMathMin, CSSMathMax принимают массив вместо перечисления аргументов — 243 теста падают в  |
| BUG-1564 | `BUGS.md:226` | Typed OM: нет CSSNumericValue.parse, CSSColorValue.parse, CSSMathClamp, CSSImageValue, CSSNumericArray, CSS.Q/Hz/kHz/cap/svw/lvh/dvh/cqw/cqi |
| BUG-1565 | `BUGS.md:227` | Typed OM: StylePropertyMap.clear нет, get/has/delete/getAll("lemon") не бросают TypeError, set недопустимого значения не бросает, new CSSUni |
| BUG-1566 | `BUGS.md:228` | @counter-style с system: cyclic: отрицательное значение выводится со знаком (-Y), у эталона знака нет. @counter-style system: cyclic: отрица |
| BUG-1568 | `BUGS.md:230` | Счётчики: counter(list-item) в <ol><li> — 0, counter-reset: reversed(name) не реализован, display:none элемент увеличивает счётчик. Счётчики |
| BUG-1569 | `BUGS.md:231` | CSS Lists: getComputedStyle не знает counter-reset/increment/set, list-style-; канонические формы (chapter 1), недопустимое принимается. CSS |
| BUG-1571 | `BUGS.md:232` | Нет Element.scrollParent, scrollIntoViewIfNeeded, window.resizeTo/resizeBy/moveTo/moveBy. Нет Element.scrollParent, scrollIntoViewIfNeeded,  |
| BUG-1572 | `BUGS.md:233` | content: "\2022" — escape-последовательности в строках content не раскрываются: на странице печатается \2022. content: "\2022" — escape-посл |
| BUG-1573 | `BUGS.md:234` | Любая мутация el.style. скроллера, его потомка или body через ~50–150 мс после записи сбрасывает scrollTop контейнера с overflow:auto в 0: c |
| BUG-1574 | `BUGS.md:235` | У контейнера с scroll-snap-type: y mandatory вызовы scrollTo(0,100), scrollTop = 40, scrollBy(0,170) оставляют позицию как есть (100, 40, 21 |
| BUG-1575 | `BUGS.md:236` | getComputedStyle() отдаёт "" для scroll-snap-type, scroll-snap-align, scroll-snap-stop, scroll-initial-target, scroll-padding-block/-inline, |
| BUG-1576 | `BUGS.md:237` | typeof SnapEvent — undefined, onscrollsnapchange нет ни в window, ни в document, ни у элемента; события нигде не диспатчатся (grep -rn "scro |
| BUG-1582 | `BUGS.md:240` | el.style.animationDelay = "infinite" → читается infinite (ожидается ""); transitionDuration = "-1s" принимается; transitionTimingFunction =  |
| BUG-1583 | `BUGS.md:241` | new AnimationEvent() без аргумента не бросает TypeError (длина конструктора — 2, ожидается 1); "animationName" in AnimationEvent.prototype,  |
| BUG-1584 | `BUGS.md:242` | <div style="animation:o 4s linear"> + element.animate(…, {duration:4000}) (или цикл requestAnimationFrame): setTimeout(r, 1000) срабатывает  |
| BUG-1585 | `BUGS.md:243` | border: 1.5px solid → getComputedStyle(el).borderTopWidth — 1.5px (ожидается 1px), 0.5px → 0.5px (должно быть 1px), 2.7px → 2.7px (2px); out |
| BUG-1587 | `BUGS.md:245` | offset-distance: 100% у бокса 10×10 при offset-path: path("M0 0 h 200") даёт сдвиг (14.1, 0) — это диагональ бокса √(10²+10²), а не 200 (дли |
| BUG-1589 | `BUGS.md:247` | getComputedStyle(el).getPropertyValue("offset-path") и остальные шесть offset- — ""; CSS.supports("offset-position","normal") — false; el.st |
| BUG-1592 | `BUGS.md:250` | При zoom: 2 у box-shadow: 20px 0 0 полоса тени шириной 20 px (ожидается 40), text-shadow: 20px 0 0 смещён на 20 px, outline: 10px рисуется 1 |
| BUG-1593 | `BUGS.md:251` | @media (width >= 600px), (min-width: 0), (min-width: 10cm), ((min-width:0px)), (min-width:0px) or (min-height:0px), not ((min-width:99999px) |
| BUG-1594 | `BUGS.md:252` | matchMedia("(prefers-color-scheme: 0)").media — not all вместо исходного запроса; matchMedia("(min-resolution: 1x)").media — (min-resolution |
| BUG-1597 | `BUGS.md:255` | lumen --bidi-port N после UX-LOADING не отвечает на script.evaluate/navigate — все тесты WPT на main TIMEOUT; бисекция: 6d4ae2719, loading:  |
| BUG-1600 | `BUGS.md:257` | Логические свойства: margin-left:1px;margin-inline-start:5px → 1px (порядок каскада теряется), CSS.supports ложно для border-block-start-wid |
| BUG-1602 | `BUGS.md:259` | position не создаёт containing block; стекинг-контекст для opacity на inline, z-index, mask-image и др. WPT-RUN-14 S28: css-will-change 22 r |
| BUG-1603 | `BUGS.md:260` | color-scheme: dark не делает холст тёмным: (90,50) белый, color чёрный. WPT-RUN-14 S28: css-color-adjust 24 id (19 rendering/dark-color-sche |
| BUG-1604 | `BUGS.md:261` | new DOMMatrix("NONE")/"none//" не разбираются; в воркере нет DOMMatrix. WPT-RUN-14 S28: css/geometry 19 id (802 из 1 392 подтестов) |
| BUG-1606 | `BUGS.md:262` | У <iframe> нет permissionsPolicy/featurePolicy: хелпер WPT permissions-policy.js:466 получает undefined, 548 подтестов категории permissions |
| BUG-1632 | `BUGS.md:266` | Нет window.orientation и onorientationchange (Window, HTMLBodyElement): compat/idlharness.window 23/29, 6 подтестов FAIL |
| BUG-1640 | `BUGS.md:267` | perspective: inherit не даёт обёртку проекции; perspective-origin: 0 0 даёт иной результат, чем top left; перспектива контейнера доходит до  |
| BUG-1641 | `BUGS.md:268` | В живом wgpu-окне mix-blend-mode не применяется: все 16 панелей TEST-56 нарисованы как normal (gdigrab 14.13% против 1.355% на 2026-09-19; C |
| BUG-1398 | `BUGS.md:177` | 7 id contain-{body,html}-overflow- падают с расхождением 120 px у текста (x=200…221, y=2…14): абзац «Test passes if there is no red.» в widt |
| BUG-1236 | `BUGS.md:156` | cpu_snapshots_match_references падает на чистом main (9517871d0, без правок P4): 12-display 5280, 13-visibility-opacity 7740, 18-images 6590 |
| BUG-1648 | `BUGS.md:274` | Оверлей не знает каскадного происхождения значения: !important в таблице стилей должен перебивать @keyframes, а он перебивается (css-animati |
| BUG-1658 | `BUGS.md:283` | counter-reset снимается при выходе из самого элемента, а не после его следующих соседей (CSS Lists L3 §4.4: область — элемент, потомки и сле |
| BUG-1680 | `BUGS.md:302` | Корневой <svg> с padding/border получает border-box, равный его content-box: svg { width:30px; height:30px; padding:10px; border:25px solid  |
| BUG-1278 | `BUGS.md:162` | getComputedStyle() возвращает "" для caret-color, accent-color, outline-offset, шорткода outline, resize, user-select, appearance/-webkit-ap |
| BUG-1692 | `BUGS.md:310` | Вложенный custom element, созданный через shadowRoot.innerHTML другого custom element, не получает shadow tree: конструктор my-bar не выполн |
| BUG-1693 | `BUGS.md:311` | Скриптом вставленные <iframe> грузятся строго по очереди ≈140 мс каждый независимо от числа параллельных браузеров: 158 меток encoding/unsup |
| BUG-1695 | `BUGS.md:313` | API расходится со спецификацией при уже поддерживаемых кодировках: from_label вырезает все пробелы вместо обрезки по краям (504 сабтеста api |
| BUG-1696 | `BUGS.md:314` | encoding/streams: нет backpressure (backpressure.any.html 0/4), decode-utf8 выдаёт чанк на неполном вводе (10/12), invalid-realm.window (0/4 |
| BUG-1697 | `BUGS.md:315` | IPC-снимок не видит правок DOM от таймеров (display list не пересобирается без redraw_requested): TEST-61 (view-transitions) DEBTOR 3.26 % → |
| BUG-1700 | `BUGS.md:318` | navigation.navigate("#frag")/back()/traverseTo() между записями одного документа — полная навигация (повторная загрузка, новый realm): стран |
| BUG-1701 | `BUGS.md:319` | iframe.contentWindow.navigation — объект с полями Navigation, но без прототипа: navigate/entries/reload не функции, currentEntry === undefin |
| BUG-1702 | `BUGS.md:320` | window.open(url, "name") в существующий фрейм возвращает null, у cross-origin/document.domain фрейма нет navigation, navigate приходит в окн |
| BUG-1703 | `BUGS.md:321` | navigate диспатчится не синхронно внутри navigation.navigate()/back()/traverseTo(), а на следующем ходе оболочки: микрозадача обгоняет событ |
| BUG-1704 | `BUGS.md:322` | window.stop не определён (typeof === "undefined"). WPT navigation-api (navigate-intercept-stop, precommitHandler-window-stop-before-commit): |
| BUG-1705 | `BUGS.md:323` | indexedDB, IDBKeyRange, IDBDatabase не определены в DedicatedWorker и SharedWorker (в ServiceWorker шим есть). WPT IndexedDB: 398 id .any.wo |
| BUG-1706 | `BUGS.md:324` | Слушатель, добавленный внутри обработчика события IndexedDB, получает то же событие (диспетч обходит живой массив, а не копию): EventWatcher |
| BUG-1707 | `BUGS.md:325` | Нет getAllRecords(), getAll()/getAllKeys() не принимают словарь {query, count, direction} (DataError: invalid key or range). WPT IndexedDB:  |
| BUG-1708 | `BUGS.md:326` | createObjectStore/createIndex принимают невалидный keyPath ('j a', 'a..b', '.a') вместо SyntaxError. WPT IndexedDB: 3 id, 26 сабтестов |
| BUG-1709 | `BUGS.md:327` | Бинарные ключи IndexedDB (ArrayBuffer, typed array, DataView) не поддерживаются: DataError: invalid key; indexedDB.cmp с BigInt кидает не DO |
| BUG-1710 | `BUGS.md:328` | IndexedDB/structured-clone.any.html (6 вариантов) и parallel-cursors-upgrade.any.html не завершают загрузку: navigate … DocumentReady > 30 с |
| BUG-1724 | `BUGS.md:336` | Остаток BUG-1529: vertical-align не хранит first/last/alphabetic/ideographic/central/mathematical/center (в getComputedStyle нет), line-heig |
| BUG-1728 | `BUGS.md:340` | Найден WPT-SHADOW-SCOUT: содержимое <template> теряется — фрагментный парсер (innerHTML) не заполняет content, template.innerHTML отдаёт пус |
| BUG-1729 | `BUGS.md:341` | Найден WPT-SHADOW-SCOUT: путь события slotted-узла не идёт через assignedSlot и теневое дерево хоста (lt > h1 > BODY вместо lt > slot > … >  |
| BUG-1730 | `BUGS.md:342` | Найден WPT-SHADOW-SCOUT: slotchange приходит синхронно, а не в microtask; assignedNodes({flatten:true}) не отдаёт fallback пустого слота; sl |
| BUG-1731 | `BUGS.md:343` | Найден WPT-SHADOW-SCOUT: document.activeElement не ретаргетится на хост, host.focus() с delegatesFocus не делегирует, :focus/:focus-within н |
| BUG-1732 | `BUGS.md:344` | Найден WPT-SHADOW-SCOUT: у document.implementation.createHTMLDocument() нет createNodeIterator/createTreeWalker; остаток BUG-415; ownerdocum |
| BUG-1733 | `BUGS.md:345` | Остаток BUG-1539: на CPU-пути backdrop-filter теперь работает под собственными opacity/transform/clip-path/mix-blend-mode/border-radius, но  |
| BUG-1735 | `BUGS.md:347` | Найден WPT-CRYPTO-SCOUT: Crypto/SubtleCrypto/CryptoKey не WebIDL-интерфейсы — нет глобала SubtleCrypto, методы — own-свойства, Crypto перечи |
| BUG-1736 | `BUGS.md:348` | Найден WPT-CRYPTO-SCOUT: нет «normalize an algorithm» (WebCrypto §18.4) и проверок usages/типа ключа — NotSupportedError вместо SyntaxError/ |
| BUG-1737 | `BUGS.md:349` | Найден WPT-CRYPTO-SCOUT: structuredClone/postMessage теряют CryptoKey — клон простой объект, «exportKey: argument is not a CryptoKey» (класт |
| BUG-1738 | `BUGS.md:350` | Найден WPT-CRYPTO-SCOUT: SubtleCrypto считает синхронно до возврата промиса — PBKDF2 ×100 000, RSA-keygen в «Bad usages», большие данные бло |
| BUG-1739 | `BUGS.md:351` | Найден WPT-CRYPTO-SCOUT: игнорируются tagLength AES-GCM, saltLength RSA-PSS, параметры AES-CTR, length deriveBits; данные копируются после н |
| BUG-1740 | `BUGS.md:352` | Найден WPT-STREAMS-SCOUT: ReadableStream.length 2 вместо 0, перечисляемые методы и служебный _rs_do_cancel на публичном прототипе, нет Symbo |
| BUG-1741 | `BUGS.md:353` | Найден WPT-STREAMS-SCOUT: transformer.cancel не вызывается, readableType/writableType не отвергаются, terminate() не закрывает запись, исклю |
| BUG-1742 | `BUGS.md:354` | Найден WPT-SW-SCOUT: регистрация SW — таймерная имитация: updatefound до резолва register(), состояния без исполнения скрипта, scope не резо |
| BUG-1744 | `BUGS.md:356` | Найден WPT-SW-SCOUT: из respondWith доходит только тело (status всегда 200, без заголовков), preventDefault/reject не дают network error, ме |
| BUG-1743 | `BUGS.md:355` | Найден WPT-SW-SCOUT: iframe.contentWindow.<глобал> пересекает границу изолятов по значению — navigator.serviceWorker, промис fetch(), perfor |
| BUG-1228 | `BUGS.md:129` | Element.setHTMLUnsafe/ShadowRoot.setHTMLUnsafe разбирают фрагмент без Declarative Shadow DOM: <template shadowrootmode> остаётся обычным <te |
| BUG-1747 | `BUGS.md:359` | Найден WPT-SW-SCOUT: нет интерфейсных объектов ServiceWorker, ServiceWorkerContainer, ServiceWorkerRegistration, NavigationPreloadManager —  |
| BUG-1745 | `BUGS.md:357` | Найден WPT-SW-SCOUT: Cache API в окне — нет класса Cache, тело возвращается строкой «101,103,…», statusText «OK» вместо "", нет url, не рабо |
| BUG-1746 | `BUGS.md:358` | Найден WPT-SW-SCOUT: self.caches не определён в Dedicated/SharedWorker, cleanup cache_test падает → ERROR у всех cache-storage/.any.worker.h |
| BUG-1748 | `BUGS.md:360` | Найден WPT-VTT-SCOUT: оверлей cue субтитров дописывается в display list только перед GPU-кадром окна — в --screenshot/--ipc-server cue не ри |
| BUG-1749 | `BUGS.md:361` | Найден WPT-VTT-SCOUT: cuechange перепроверяется только по тику воспроизведения/seek и одним setTimeout(0) до загрузки <track> (ложное событи |
| BUG-1750 | `BUGS.md:362` | Найден WPT-VTT-SCOUT: <span style="background">a <b>bold</b> c</span> — FillRect только под «a » и « c», под вложенным <b> дыра (и GPU, и CP |
| BUG-1752 | `BUGS.md:364` | Найден WPT-WASM-SCOUT: очередь задач платформы V8 разбирается только в путях GC (run_gc_pass), цикл JS-потока не зовёт Platform::pump_messag |
| BUG-1751 | `BUGS.md:363` | Найден WPT-VTT-SCOUT (кластер H6): VTTCue — собственные data-свойства без IDL-аксессоров на прототипе, нет IndexSizeError/фильтра enum/TypeE |
| BUG-1753 | `BUGS.md:365` | Найден WPT-IDB-RUNNER: после abort() upgrade-транзакции открытия новой БД запись _idb_databasesname (version 0) остаётся, indexedDB.database |
| BUG-1754 | `BUGS.md:366` | fixed потомков (остаток BUG-1543): на display:block reftest совпадает с эталоном, на inline — нет. 1 reftest (filtered-inline-is-container). |
| BUG-1756 | `BUGS.md:368` | fetch() отвергает корректные запросы network error-ом: POST/OPTIONS/DELETE без тела и нестандартные методы («Phase 0 supports GET/HEAD only» |
| BUG-1757 | `BUGS.md:369` | Глобалы воркеров: нет atob/btoa в SharedWorker, нет URL.createObjectURL в DedicatedWorker. Кластер W WPT-FETCH-SCOUT: 10 id (http-cache, api |
| BUG-1758 | `BUGS.md:370` | 'manual' не доходит до сети; preflight не виден серверу; редирект на URL с credentials и несколько Access-Control-Allow-Origin не отвергаютс |
| BUG-1759 | `BUGS.md:371` | Отсоединённые <audio src> и <img srcset> не шлют ни load, ни error — тесты fetch/metadata/ висят до TIMEOUT. Кластер M(в) WPT-FETCH-SCOUT. |
| BUG-1760 | `BUGS.md:372` | Sec-Fetch- уходят на не потенциально доверенные URL (http://), спецификация Fetch Metadata шлёт их только на trustworthy. Кластер M(г) WPT-F |
| BUG-1761 | `BUGS.md:373` | Пробел между inline-кусками с разным кеглем/шрифтом и вокруг <img> меряется не так, как пробел внутри текста. <div><span style="font:20px Ah |
| BUG-1766 | `BUGS.md:378` | <link rel=icon> при неудачной загрузке шлёт error (и load при успехе): иконка идёт общим путём preload-подсказок, а для icon событий нет (ht |
| BUG-1767 | `BUGS.md:379` | credentials: "omit" всё равно сохраняет Set-Cookie и шлёт Cookie same-origin; нет флага host-only — cookie без Domain уходит на поддомены (f |
| BUG-1768 | `BUGS.md:380` | cookies/attributes/domain.sub.html виснет на первом сабтесте: test_driver.delete_all_cookies() с set_test_context(top.opener.top) из iframe  |
| BUG-1769 | `BUGS.md:381` | Ошибка компиляции/выполнения кросс-ориджин скрипта в importScripts пробрасывается как есть, а не глушится в DOMException NetworkError (worke |
| BUG-1770 | `BUGS.md:382` | sendBeacon с https на http возвращает true, а POST на отдельном потоке молча блокируется как mixed content (let _ =) — beacon/headers/header |
| BUG-1773 | `BUGS.md:385` | Найден WPT-RUN-19: print-reftest — lumen --print-to-pdf страницы с wptserve занимает ~20 с (file:// 0.7 с, http.server 3.9 с), 59 тестов PAS |

## STATUS-P4 — 36

| ID | был | суть |
|---|---|---|
| CSS Text L3/L4 | `CSS-SPECS.md:76` | text-align-last ✅ 2026-06-08; text-align: justify/justify-all/match-parent, text-justify ✅ 2026-10-20 (TEXT-JUSTIFY); hyphens:auto ✅ (P1 202 |
| max-lines | `CSS-SPECS.md:199` | not started (WPT-RUN-14 S16, 2026-10-07): CSS Overflow L4 §max-lines — CSS.supports('max-lines','2') is false, max-lines:2;continue:discard  |
| block-ellipsis | `CSS-SPECS.md:200` | <string>; CSS.supports('block-ellipsis','auto') is false, нет в getComputedStyle (inheritance.html: 6 из 18 сабтестов). Рисование строки-мно |
| scroll-axis-lock | `CSS-SPECS.md:201` | not started (WPT-RUN-14 S16, 2026-10-07): CSS Overflow L5 §scroll-axis-lock — 'scrollAxisLock' in getComputedStyle(el) is false; parsing/scr |
| frame-sizing | `CSS-SPECS.md:202` | content-block-size на <iframe> — CSS.supports('frame-sizing','content-width') is false. WPT css/css-sizing/responsive-iframe/ (21 id, 7 reft |
| font-kerning | `CSS-SPECS.md:252` | не разбирается (grep -rn font-kerning crates/ — ноль; WPT-RUN-14 S18, 2026-10-07): normal/auto/none — CSS Fonts L4 §6.6, тумблер фичи kern в |
| font-synthesis / -weight / -style / -small-caps / -position | `CSS-SPECS.md:253` | position (CSS Fonts L4 §2.2), запрет синтетического bold/italic/капители в box_tree (caps_synthesis и синтез жирного сейчас безусловны). WPT |
| font-variant-alternates + @font-feature-values | `CSS-SPECS.md:254` | не разбираются (grep — ноль; WPT-RUN-14 S18): stylistic()/styleset()/character-variant()/swash()/ornaments()/annotation()/historical-forms ( |
| font-variant-east-asian | `CSS-SPECS.md:255` | proportional-width, ruby (CSS Fonts L4 §6.10) → фичи jp78…, smpl, trad, fwid, pwid, ruby. WPT font-variant-east-asian- 11 reftest + parsing/ |
| font-language-override | `CSS-SPECS.md:256` | <string> (CSS Fonts L4 §6.11), тег языка для фичи locl. WPT font-language-override-0{1,2}.html, parsing/font-language-override- |
| font-width | `CSS-SPECS.md:257` | алиас font-stretch (CSS Fonts L4 §3.3): "font-width" in getComputedStyle(el) — false, element.style.fontWidth = 'condensed' принимается как  |
| mask-clip / mask-composite | `CSS-SPECS.md:483` | no-clip grammar ✅: padding-box/content-box/fill-box wrap the mask group in PushClipRect/PopClip (scissor path; fill-box = content box for CS |
| clip (deprecated rect()) | `CSS-SPECS.md:484` | not started (WPT-RUN-14 S2, 2026-10-05): clip: rect(<top>, <right>, <bottom>, <left>) on absolutely positioned boxes (CSS Masking L1 §5.1, C |
| image() / image-color | `CSS-SPECS.md:523` | не разбирается (WPT-RUN-14 S19, 2026-10-08): background-image: image(green), image(rgb(0 128 0)), image(currentcolor), image("x.png", green) |
| calc-size() / interpolate-size | `CSS-SPECS.md:567` | не разбирается (grep -rn calc-size crates/ — ноль; WPT-RUN-14 S19, 2026-10-08): width: calc-size(auto, size + 40px) отбрасывается (width: ca |
| sibling-index() / sibling-count() | `CSS-SPECS.md:568` | не разбираются (grep — ноль; WPT-RUN-14 S19): z-index: calc(sibling-index()) → auto, width: calc(sibling-index() 40px) → весь контейнер. CSS |
| random() / random-item() | `CSS-SPECS.md:569` | не разбираются (grep — только crates/core/src/auth.rs, не CSS; WPT-RUN-14 S19): width: random(100px, 100px) → весь контейнер, random-item(-- |
| if() | `CSS-SPECS.md:570` | не вычисляется (--prop: if(style(--x: 3): yes; else: no) читается как исходная строка; WPT-RUN-14 S19): style(), media(), supports() условия |
| @color-profile | `CSS-SPECS.md:761` | parsed+stored + color(--name ...) recognized; real ICC transform deferred (p4-color-profile 2026-07-15, see T3 At-Rules table above) |
| :open / :closed | `CSS-SPECS.md:445` | не разбираются (WPT-RUN-14 S20, 2026-10-08): querySelector(':open') → SyntaxError; details:open, dialog:open, select:open, input:open не раб |
| overlay | `CSS-SPECS.md:405` | auto, дискретная анимация, transition-behavior: allow-discrete для top layer). WPT css-position/overlay/ — 11 id (overlay-computed, overlay- |
| reading-flow / reading-order | `CSS-SPECS.md:406` | не разбираются (WPT-RUN-14 S21, 2026-10-08): CSS.supports('reading-flow','flex-visual') — false. CSS Display 4 §reading-flow (порядок фокуса |
| view-transition-class | `CSS-SPECS.md:407` | <custom-ident>+ разбирается с валидацией, CSS.supports, getComputedStyle (BUG-1510, P3 2026-10-23). Не сделано: сопоставление ::view-transit |
| text-decoration-inset | `CSS-SPECS.md:280` | <length>{1,2}; сдвиг начала и конца линии декорации внутрь или наружу). WPT css-text-decor/text-decoration-inset-001…030 — 29 reftest thick, |
| text-decoration-skip-spaces | `CSS-SPECS.md:281` | all; пропуск пробелов в начале/конце линии). WPT css-text-decor/text-decoration-skip-spaces-001…009 — 9 reftest thick, parsing/text-decorati |
| filter | `CSS-SPECS.md:467` | GPU pipeline: blur/brightness/contrast/grayscale/hue-rotate/invert/saturate/sepia; drop-shadow(<color>? <length>{2,3}) 🟡 (BUG-1540, 2026-10- |
| backdrop-filter | `CSS-SPECS.md:468` | backdrop GPU compositing pass with LRU cache (WQ#28); на CPU-растре (--screenshot) работает только у элемента без opacity<1/transform/clip-p |
| initial-letter | `CSS-SPECS.md:639` | <number> <integer>? parsed → non-inherited initial_letter_size/initial_letter_sink; Phase 0 layout promotes the first-letter unit to an inli |
| text-box-trim / text-box-edge / text-box | `CSS-SPECS.md:640` | не реализовано (WPT-RUN-14 S24, 2026-10-08): CSS.supports false у всех трёх, в crates/ нет; нужен трим первой/последней строки по метрикам ш |
| corner-shape / border-shape / box-shadow-{offset,color,blur,spread,position} / парные border-{top,right,bottom,left,block-,inline-}-radius / border-clip / hairline | `CSS-SPECS.md:219` | CSS Borders and Box Decorations L4 (черновик) не реализован (WPT-RUN-14 S26, 2026-10-09, BUG-491, ДОРАБОТКА → CSS-SPECS.md): CSS.supports("c |
| margin-trim | `CSS-SPECS.md:183` | inline-end (не наследуется), ComputedStyle, getComputedStyle (использованное значение поля после обрезки), обрезка у блочного контейнера (с  |
| CSS Ruby L1 | `CSS-SPECS.md:124` | ruby-position/ruby-align/ruby-merge ✅ 2026-07-04 (p4-ruby-css-props: parse → inherited ComputedStyle fields → RubyBox::from_style drives lay |
| Motion Path L1 | `CSS-SPECS.md:107` | offset-path: path() ✅ 2026-06-10 (P4: ComputedStyle fields + resolve_motion_transform wiring in property_trees); offset-distance/offset-rota |
| Media Queries L4/L5 | `CSS-SPECS.md:93` | prefers-reduced-motion ✅; hover/any-hover/pointer/any-pointer ✅ 2026-06-14 (p4-media-hover-pointer: desktop defaults hover/fine); prefers-co |
| ::cue, ::cue(<selector>), ::cue-region, ::cue-region(<selector>) | `CSS-SPECS.md:796` | Не реализовано (WPT-VTT-SCOUT 2026-10-27, docs/wpt-vendor-notes/webvtt.md, кластеры R5/R6): в crates/engine/css-parser/src/parser/selectors. |
| flood-color / flood-opacity / lighting-color / color-interpolation-filters | `CSS-SPECS.md:470` | SVG-FILTER срез 1 (+ lighting-color в SVG-FILTER-2 срез 1): flood-color/flood-opacity/lighting-color/color-interpolation-filters читаются <f |
