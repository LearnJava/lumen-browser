# WPT status — готовность Web Platform Tests

Живой документ готовности: какие категории WPT-корпуса вендорены/прогнаны в Lumen, какие ещё нет,
и кто назначен разбирать конкретный тест или категорию. Дополняет [`BUGS.md`](../BUGS.md) —
провалы тестов не заводятся как отдельная задача на каждый тест, а группируются по первопричине
(`BUG-NNN`, см. методологию ниже), но *назначение разработчика* делается здесь, на уровне
теста/категории, по желанию того, кто ведёт этот файл.

**Владелец инфраструктуры:** P2 (`docs/tasks/p2-wpt-integration.md`, `tests/wpt/`). Назначения
конкретных тестов/категорий проставляет тот, кто ведёт этот файл (колонка «Владелец» ниже) — не
обязательно P2; провал в `css` может уйти P4, в `dom` — P1/P3 и т.д.

## Охват

Апстрим [`web-platform-tests/wpt`](https://github.com/web-platform-tests/wpt) на закреплённом
коммите `35be3b44f3111c4d614b5b201e399493d20e7b38` (см. [`tests/wpt/VENDOR.md`](../tests/wpt/VENDOR.md))
содержит **277 категорий верхнего уровня** (после исключения служебных директорий вроде `tools/`,
`resources/`, `common/` — они не тестовые категории, а инфраструктура самого WPT). Из них сейчас
**вендорены и гоняются две** — `dom/nodes/` (168 файлов) и `FileAPI/` (115 файлов, добавлена
2026-07-21 в рамках дорожки WPT-VENDOR) — движок исполнения обеих один и тот же минимальный
executor `tests/wpt/browsers/lumen.py` поверх `wptrunner` + WebDriver BiDi: одно окно, без
`test_driver.*`, без iframes/multi-window — см. `docs/tasks/p2-wpt-integration.md`. Остальные
275 категорий **не вендорены** — таблица ниже перечисляет их все, чтобы было видно полное поле
задачи, а не только то, что уже потрогали.

## Легенда

**Скоуп** (относится ли категория к архитектуре Lumen как приватного лёгкого браузера-читалки):
- ⬜ кандидат — вероятно в скоупе, вендорить/гонять когда дойдут руки
- 🚫 вне скоупа (первый черновой проход, см. заметку в колонке) — нет медиа-конвейера/аппаратной
  интеграции/платёжного стека/ad-tech-слоя и т.п.; можно оспорить и передвинуть в ⬜, если появится
  причина (например, видео вернёт в скоуп `media-source`/`mediacapture-*`)

**Вендорено:**
- ✅ вся категория вендорена и гоняется
- 🟡 вендорена частично (см. заметку)
- — не вендорена вовсе

**Статус теста (в таблице `dom/nodes` ниже):** `OK`/`PASS` — гарнес отработал (не значит, что все
сабтесты прошли, см. колонку «Сабтесты»); `ERROR`/`TIMEOUT`/`CRASH` — гарнес не долетел до конца;
`NOT RUN` — тест выбран, но результата нет вовсе (упал раньше `test_start`).

## Методология: не одна задача на тест

4802 сабтеста в одной только `dom/nodes` — заводить BUG на каждый упавший бессмысленно
(не масштабируется, и почти всегда 10-100 упавших тестов — симптом одной первопричины в движке;
пример: [BUG-324](../bugs/BUG-324-FIXED.md) — отсутствие `document.implementation` закрывает ~75%
провалов в `dom/nodes`). Рабочий цикл:

1. Прогнать `tests/wpt/run_report.py --all` (см. `tests/wpt/README.md`), найти кластеры провалов.
2. Завести один `BUG-NNN` на первопричину (не на тест), с симптомами и списком задетых тестов.
3. Здесь, в этом файле, назначить владельца — на конкретный тест (колонка «Владелец» в таблице
   `dom/nodes`) или на категорию целиком (колонка «Владелец» в категорийном индексе), в колонке
   «Баг» — ссылка на заведённый `BUG-NNN`.
4. После фикса — расширить курированный `.ini`-сабсет (`tests/wpt/metadata/`) этим тестом с
   `expected: PASS`, чтобы он попал под гейт `run_suite.py` и не регрессировал молча.

## Как обновить этот файл

**Таблица `dom/nodes` ниже — генерируется**, между HTML-комментариями-маркерами в конце этого
файла (см. исходник `docs/wpt-status.md` — не пересказываю их здесь буквально, чтобы текстовый
поиск маркера в скрипте синхронизации не цеплялся за это упоминание вместо настоящего блока).
Колонки «Владелец»/«Баг»/«Заметка» **сохраняются** между синхронизациями (скрипт мёржит по id
теста, не затирает руками проставленное). Два шага — прогон (медленный, минуты) и синхронизация
(быстрая, парсит уже готовый HTML, тесты не гоняет):

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --out .tmp/wpt-report-all.html --all
tests/wpt/.venv/Scripts/python.exe tests/wpt/gen_status_md.py
```

**Категорийный индекс** ниже (все 277 категорий) — ведётся руками: при вендоринге новой категории
поменять «Вендорено» на ✅/🟡 и по возможности добавить детальную таблицу по образцу `dom/nodes`
(для этого нужно обобщить `gen_status_md.py` на произвольную категорию, а не хардкодить `dom/nodes` —
пока не сделано; для `FileAPI` (добавлена 2026-07-21) вместо детальной таблицы — агрегированная
сводка прямо в колонке «Заметка» категорийного индекса, `run_report.py --root`/`--recursive`
уже обобщены и пригодны для любой категории организованной в подкаталоги). Скоуп/заметку в категорийном индексе можно и нужно
пересматривать — это первый черновой проход одного разработчика, не итог обсуждения.

---

## Категорийный индекс (277 категорий верхнего уровня)

| Категория | Скоуп | Вендорено | Владелец | Баг | Заметка |
|---|---|---|---|---|---|
| `FileAPI` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-21 (коммит `35be3b44`, `tests/wpt/FileAPI/`, 115 файлов; `common/`/`html/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон `run_report.py --all --root FileAPI --recursive`: 66/70 id получили результат (4 — `.https.html`-тесты не добежали), 35/66 harness OK, 115/305 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `Blob.prototype.bytes()`/`.textStream()` отсутствуют; конструктор `Blob`/`File` не поддерживает опцию `endings`; `File-constructor-endings.html` возвращает пустое содержимое (0/34, хуже симметричного Blob-теста); `FileReader.readyState`-трекинг в ряде сабтестов не совпадает с ожиданиями; 4 теста `*.https.html` в `BlobURL/` не добежали (`invalid url: invalid port: "None"` — минимальный исполнитель не поднимает HTTPS-порт, тот же класс ограничения, что и отсутствие iframes/multi-window) |
| `IndexedDB` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `c8cad69f`, `tests/wpt/IndexedDB/`, 245 файлов; `common/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон `run_report.py --all --root IndexedDB --recursive`: 230 id получили результат, 197/230 harness OK (33 не добежали — почти все TIMEOUT/внешний hang, 1 `SKIP` из-за отсутствующей поддержки `testdriver.js` в минимальном исполнителе), 393/1225 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `IDBCursor.continuePrimaryKey` отсутствует (`TypeError: cursor.continuePrimaryKey is not a function`); брошенные ошибки в ряде путей курсора/ключей (`DataError`/`ReadOnlyError`/`TransactionInactiveError`) не всегда настоящие `DOMException` — у части `.code`/`.name` не совпадает с ожидаемым (напр. `indexedDB.cmp()` с BigInt-ключом кидает объект без `code`); транзакции: «Expected success event, but got upgradeneeded event» в ряде сценариев после `abort()` — расхождение в жизненном цикле версий; BigInt как ключ не поддержан (`TypeError: Do not know how to serialize a BigInt` во внутреннем JSON-based персисте) и структурное клонирование с циклами тоже падает на persist (`Converting circular structure to JSON`) — намекает, что внутренний слой персистентности IndexedDB сериализует значения через JSON, а не через настоящий structured-clone |
| `WebCryptoAPI` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `629dbeb5`, `tests/wpt/WebCryptoAPI/`, 185 файлов; ссылается только на уже вендоренные `/resources/testharness.js`+`testharnessreport.js`, внекатегорийных хелперов нет). 122/125 тестовых файлов — `.https.`-only (спека требует secure context для `crypto.subtle`); прогон `run_report.py --all --root WebCryptoAPI --recursive` подтвердил на 73 добежавших id, что все они падают одинаково (`invalid url: invalid port: "None"` — минимальный исполнитель не поднимает HTTPS-порт, тот же класс ограничения, что и `FileAPI`/`BlobURL`, здесь просто доминирует почти всю категорию); полный прогон дважды не добрался до конца (зависание раннера на многочасовой отметке — не сбой браузера, `WebSocket connection closed` со стороны wptrunner), поэтому по `.https.`-подмножеству детальной разбивки по сабтестам нет. 2 не-`.https.` файла (`getRandomValues.any.js`, `historical.any.js`) прогнаны изолированно: 2/2 harness OK, 28/42 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `crypto.getRandomValues()` не бросает `TypeMismatchError` на `Float16/32/64Array`/`DataView` (спека требует отказа для не-целочисленных TypedArray); брошенный при превышении 65536 байт `QuotaExceededError` не соответствует спеке по форме (нет ожидаемых `requested`/`quota`); `crypto.subtle`/`SubtleCrypto`/`CryptoKey` доступны из небезопасного (http, не secure context) контекста — secure-context gate для Web Crypto отсутствует вовсе |
| `accelerometer` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `0f7f0157`, `tests/wpt/accelerometer/`, 19 файлов), включена несмотря на скоуп 🚫 по прямому запросу пользователя 2026-07-21. Почти все тесты ссылаются на `/generic-sensor/generic-sensor-tests.js` (общая логика для всего семейства Sensor API) плюс `/page-visibility/`, `/permissions-policy/resources/` — намеренно НЕ вендорены (та же методология, что и в FileAPI/IndexedDB: внекатегорийные хелперы не тянутся, гэп документируется; здесь пришлось бы затянуть ещё ~950 КБ ради уже вне-скоуп категории). Прогон `run_report.py --all --root accelerometer --recursive`: 12 отобранных id (4 пропущены — `Executor does not support testdriver.js`), 1/12 harness OK, 1 сабтест из 1 passed=0. 6 `.https.` тестов — TIMEOUT (`invalid url: invalid port: "None"`, тот же класс ограничения — минимальный исполнитель не поднимает HTTPS-порт); `idlharness.https.window.html` — ERROR (harness вернул результат с URL предыдущего теста, похоже на артефакт переиспользования сессии/WebSocket между тестами, не отдельный движковый баг). Единственный содержательный результат: `Accelerometer-supported-by-permissions-policy.html` — FAIL (`document.permissionsPolicy.features` не содержит `"accelerometer"` — Permissions Policy не объявляет эту фичу, при том что `crates/js/src/generic_sensor.rs` реализует сами конструкторы `Accelerometer`/`GravitySensor`/`LinearAccelerationSensor` как Phase-0-заглушку без реального чтения датчиков). Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `accessibility` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (коммит `344c7afb`, `tests/wpt/accessibility/`, 59 файлов; ReadMe.md + `crashtests/` + один testdriver.js-тест). Внекатегорийных хелперов не обнаружено. Категория почти целиком (58/59) — не-testharness `crashtests/`: манифест WPT классифицирует их как тип `crashtest`, для которого у минимального исполнителя `browsers/lumen.py` вовсе нет реализации (`Unsupported test type crashtest for product lumen`) — они даже не попадают в очередь прогона, а не проваливаются как тест. Прогон `run_report.py --all --root accessibility --recursive`: выбран 1 реально исполнимый id (`svg-mouse-listener.html`, тип `testharness`), результат — `SKIP` (`Executor does not support testdriver.js`, тот же класс ограничения, что в `accelerometer`/`IndexedDB`). Не заводился отдельный BUG-NNN — первый проход, см. методологию выше; для полноценного покрытия этой категории потребовалась бы отдельная реализация `crashtest`-executor'а (проверка "страница загрузилась и не крашнула браузер" без `testharness.js`), это отдельная задача инфраструктуры, не движковый баг |
| `accname` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (коммит `69ab520d`, `tests/wpt/accname/`, 183 файла: `manual/`-подкаталог — тесты для ручной проверки, не рассчитаны на автоматизацию через testharness.js; `name/` — `comp_*.html`, вычисление accessible name/description; корневые `aria-owns.html`/`basic.html`). Внекатегорийных хелперов не обнаружено. Прогон `run_report.py --all --root accname --recursive`: 19 отобранных id (`manual/`-подкаталог не попал в выборку — не `testharness`), все 19 — `SKIP` (`Executor does not support testdriver.js`, тот же класс ограничения, что в `accelerometer`/`accessibility`/`IndexedDB`), 0/19 harness OK, 0/0 сабтестов. Категория целиком зависит от `testdriver.js` (симуляция пользовательских действий/фокуса при проверке accessible name) — без отдельного testdriver-исполнителя недостижима для автоматизации. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `acid` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/acid/`, 30 файлов: `acid2/`, `acid3/`), включена несмотря на скоуп 🚫 (исторические Acid1/2/3, не актуальный спек) по тому же постоянному решению пользователя, что и `accelerometer`. Внекатегорийных хелперов не обнаружено. Прогон `run_report.py --all --root acid --recursive`: 3 отобранных id. 2 — WPT-манифест тип `reftest` (`acid2/reftest.html` matches `px-reference.html`; `acid3/test.html` matches `reference.sub.html`) — исполнитель вообще не реализует reftest (`Unsupported test type reftest for product lumen`, S8 в `docs/tasks/p2-wpt-integration.md` всё ещё будущий срез), тот же класс гэпа, что `crashtest` в `accessibility`. 1 — `acid3/numbered-tests.html` (тип `testharness`, собственный скоринг-харнесс Acid3 на несколько окон/фреймов) — TIMEOUT, вне однооконной модели минимального исполнителя. 0/1 harness OK. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `ai` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/ai/`, 180 файлов: `classifier/`, `language-model/`, `language_detection/`, `proofreader/`, `rewriter/`, `semantic_embedder/`, `summarizer/`, `translator/`, `writer/`), включена несмотря на скоуп 🚫 (AI/Writer API, нет LLM-интеграции) по тому же постоянному решению пользователя, что и `accelerometer`/`acid`. Внекатегорийный хелпер `/common/get-host-info.sub.js` не довендорен (та же методология, что `FileAPI`). Категория на 100% `.https.`-only (163 `*.window.js` + 6 прямых `.html`, 0 не-https тестов — впервые ни одного изолированно прогоняемого файла, хуже `WebCryptoAPI` 122/125). Полный `run_report.py --all --root ai --recursive` не запускался до конца: пробный прогон (10 мин) обработал единицы id, каждый — `TIMEOUT` (`invalid url: invalid port: "None"`, минимальный исполнитель не поднимает HTTPS-порт), тот же класс ограничения, что `WebCryptoAPI`/`FileAPI`/`BlobURL` (тот прогон дважды зависал, не дойдя до конца). Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `ambient-light` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/ambient-light/`, 16 файлов: `AmbientLightSensor*.html`, `idlharness.https.window.js`, `resources/sensor-data.js`), включена несмотря на скоуп 🚫 (датчик устройства) по тому же постоянному решению пользователя, что и `accelerometer`/`acid`/`ai`. Внекатегорийных хелперов не обнаружено. Прогон `run_report.py --all --root ambient-light --recursive` добежал целиком (10 отобранных id, 1:36): 2 `SKIP` (`Executor does not support testdriver.js`, тот же класс, что `accelerometer`/`accname`), 6 `.https.`-тестов — `TIMEOUT` (`invalid url: invalid port: "None"`, минимальный исполнитель не поднимает HTTPS-порт, тот же класс, что `WebCryptoAPI`/`ai`), 1 — `idlharness.https.window.html` — `ERROR` (результат пришёл с URL предыдущего теста, тот же артефакт переиспользования сессии, что в `accelerometer`). Единственный содержательный результат: `AmbientLightSensor-supported-by-permissions-policy.html` — FAIL (`document.permissionsPolicy.features` не содержит `"ambient-light-sensor"` — Permissions Policy не объявляет эту фичу, тот же паттерн, что `accelerometer`). 0/10 subtests passed. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `animation-worklet` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/animation-worklet/`, 50 файлов: 43 корневых `.html`, `META.yml`, `common.js`, `idlharness.any.js`, `references/` (2 файла), `resources/` (2 iframe-хелпера)). Внекатегорийные хелперы `/web-animations/testcommon.js` (используется почти всеми тестами) и `/common/reftest-wait.js` (используется reftest-тестами) не довендорены, та же методология, что `FileAPI`/`ai`; `idlharness.any.js` дополнительно ссылается на невендоренные `/resources/WebIDLParser.js`+`/resources/idlharness.js` (в `tests/wpt/resources/` лежат только `testharness.js`+`testharnessreport.js`). Прогон `run_report.py --all --root animation-worklet --recursive` добежал целиком (46 отобранных id, 7:11): 15 из отобранных `.https.html` — WPT-манифест тип `reftest` (`<link rel="match">` на `references/*-ref.html` или соседние `*-ref.html`) — исполнитель не реализует reftest (`Unsupported test type reftest`, тот же класс гэпа, что `acid`/`accessibility` crashtest, S8 в `docs/tasks/p2-wpt-integration.md` всё ещё будущий срез); вместе с 8 их reference-файлами (не выбираются как отдельные тесты) это даёт 23 «not run». Остальные 23 id — тип `testharness`, все `.https.` — реально прогнаны: 21 `TIMEOUT` (`invalid url: invalid port: "None"`, минимальный исполнитель не поднимает HTTPS-порт, тот же класс, что `WebCryptoAPI`/`ai`/`ambient-light`) + 2 `ERROR` (оба — session-reuse артефакты того же класса, что `accelerometer`/`ambient-light`: один — харнесс вернул результат с URL `idlharness.any.html` вместо ожидаемого теста, второй — «`eval: live window closed before replying`»). 0/23 harness OK, 0/0 сабтестов. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше |
| `annotation-model` | ⬜ | — |  |  |  |
| `annotation-protocol` | ⬜ | — |  |  |  |
| `annotation-vocab` | ⬜ | — |  |  |  |
| `apng` | ⬜ | — |  |  |  |
| `appmanifest` | 🚫 | — |  |  | PWA-инсталляция |
| `attribution-reporting` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `audio-output` | 🚫 | — |  |  | медиа-конвейер |
| `audio-session` | 🚫 | — |  |  | медиа-конвейер |
| `autoplay-policy-detection` | 🚫 | — |  |  | медиа-конвейер |
| `avif` | ⬜ | — |  |  |  |
| `background-fetch` | 🚫 | — |  |  | Service Worker расширение — фоновая ОС-интеграция |
| `background-sync` | 🚫 | — |  |  | Service Worker расширение — фоновая ОС-интеграция |
| `badging` | 🚫 | — |  |  | PWA/ОС-интеграция |
| `battery-status` | 🚫 | — |  |  | датчик устройства |
| `beacon` | ⬜ | — |  |  |  |
| `bluetooth` | 🚫 | — |  |  | аппаратный API (Bluetooth) — нет слоя интеграции с устройствами |
| `browsing-topics` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `captured-mouse-events` | ⬜ | — |  |  |  |
| `clear-site-data` | ⬜ | — |  |  |  |
| `client-hints` | ⬜ | — |  |  |  |
| `clipboard-apis` | ⬜ | — |  |  |  |
| `close-watcher` | ⬜ | — |  |  |  |
| `compat` | ⬜ | — |  |  |  |
| `compression` | ⬜ | — |  |  |  |
| `compute-pressure` | 🚫 | — |  |  | датчик устройства (нагрузка CPU/GPU) |
| `connection-allowlist` | 🚫 | — |  |  | нишевый корпоративный API |
| `console` | ⬜ | — |  |  |  |
| `contacts` | 🚫 | — |  |  | Contact Picker API |
| `container-timing` | ⬜ | — |  |  |  |
| `content-dpr` | ⬜ | — |  |  |  |
| `content-index` | ⬜ | — |  |  |  |
| `content-security-policy` | ⬜ | — |  |  |  |
| `contenteditable` | ⬜ | — |  |  |  |
| `cookies` | ⬜ | — |  |  |  |
| `cookiestore` | ⬜ | — |  |  |  |
| `core-aam` | ⬜ | — |  |  |  |
| `cors` | ⬜ | — |  |  |  |
| `cpu-performance` | ⬜ | — |  |  |  |
| `credential-management` | 🚫 | — |  |  | Credential Management API |
| `css` | ⬜ | — |  |  |  |
| `cssom` | ⬜ | — |  |  |  |
| `custom-elements` | ⬜ | — |  |  |  |
| `delegated-ink` | 🚫 | — |  |  | нишевый Ink API (стилус) |
| `density-size-correction` | ⬜ | — |  |  |  |
| `deprecation-reporting` | ⬜ | — |  |  |  |
| `device-bound-session-credentials` | 🚫 | — |  |  | нишевый auth API |
| `device-memory` | 🚫 | — |  |  | датчик устройства |
| `device-posture` | 🚫 | — |  |  | датчик устройства (форм-фактор) |
| `digital-credentials` | 🚫 | — |  |  | Digital Credentials API |
| `direct-sockets` | 🚫 | — |  |  | аппаратный/сетевой низкоуровневый API |
| `document-picture-in-picture` | 🚫 | — |  |  | PiP — нет медиа-конвейера видео |
| `document-policy` | ⬜ | — |  |  |  |
| `dom` | ⬜ | 🟡 (`dom/nodes/` только) |  | [BUG-324](../bugs/BUG-324-FIXED.md) | таблица ниже |
| `domparsing` | ⬜ | — |  |  |  |
| `domxpath` | ⬜ | — |  |  |  |
| `dpub-aam` | ⬜ | — |  |  |  |
| `dpub-aria` | ⬜ | — |  |  |  |
| `ecmascript` | ⬜ | — |  |  |  |
| `editing` | ⬜ | — |  |  |  |
| `element-timing` | ⬜ | — |  |  |  |
| `encoding` | ⬜ | — |  |  |  |
| `encoding-detection` | ⬜ | — |  |  |  |
| `encrypted-media` | ⬜ | — |  |  |  |
| `entries-api` | ⬜ | — |  |  |  |
| `event-timing` | ⬜ | — |  |  |  |
| `eventsource` | ⬜ | — |  |  |  |
| `eyedropper` | 🚫 | — |  |  | нишевый EyeDropper API |
| `fedcm` | 🚫 | — |  |  | Federated Credential Management |
| `fenced-frame` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `fetch` | ⬜ | — |  |  |  |
| `file-system-access` | ⬜ | — |  |  |  |
| `fledge` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `focus` | ⬜ | — |  |  |  |
| `font-access` | ⬜ | — |  |  |  |
| `fonts` | ⬜ | — |  |  |  |
| `forced-colors-mode` | ⬜ | — |  |  |  |
| `fs` | ⬜ | — |  |  |  |
| `fullscreen` | ⬜ | — |  |  |  |
| `gamepad` | 🚫 | — |  |  | аппаратный API (геймпады) |
| `generic-sensor` | 🚫 | — |  |  | датчик устройства (базовый API) |
| `geolocation` | ⬜ | — |  |  |  |
| `geolocation-sensor` | 🚫 | — |  |  | датчик устройства |
| `gif` | ⬜ | — |  |  |  |
| `gpc` | ⬜ | — |  |  |  |
| `graphics-aam` | ⬜ | — |  |  |  |
| `graphics-aria` | ⬜ | — |  |  |  |
| `gyroscope` | 🚫 | — |  |  | датчик устройства |
| `hr-time` | ⬜ | — |  |  |  |
| `hsts` | ⬜ | — |  |  |  |
| `html` | ⬜ | — |  |  |  |
| `html-aam` | ⬜ | — |  |  |  |
| `html-longdesc` | ⬜ | — |  |  |  |
| `html-media-capture` | 🚫 | — |  |  | медиа-конвейер |
| `html-ruby-extensions` | ⬜ | — |  |  |  |
| `https-upgrades` | ⬜ | — |  |  |  |
| `idle-detection` | ⬜ | — |  |  |  |
| `imagebitmap-renderingcontext` | ⬜ | — |  |  |  |
| `images` | ⬜ | — |  |  |  |
| `import-maps` | ⬜ | — |  |  |  |
| `inert` | ⬜ | — |  |  |  |
| `input-device-capabilities` | ⬜ | — |  |  |  |
| `input-events` | ⬜ | — |  |  |  |
| `installedapp` | 🚫 | — |  |  | ОС-интеграция |
| `intersection-observer` | ⬜ | — |  |  |  |
| `intervention-reporting` | ⬜ | — |  |  |  |
| `is-input-pending` | ⬜ | — |  |  |  |
| `jpegxl` | ⬜ | — |  |  |  |
| `js` | ⬜ | — |  |  |  |
| `js-self-profiling` | ⬜ | — |  |  |  |
| `keyboard-lock` | ⬜ | — |  |  |  |
| `keyboard-map` | ⬜ | — |  |  |  |
| `largest-contentful-paint` | ⬜ | — |  |  |  |
| `layout-instability` | ⬜ | — |  |  |  |
| `loading` | ⬜ | — |  |  |  |
| `long-animation-frame` | ⬜ | — |  |  |  |
| `longtask-timing` | ⬜ | — |  |  |  |
| `magnetometer` | 🚫 | — |  |  | датчик устройства |
| `managed` | 🚫 | — |  |  | корпоративное управление устройством |
| `mathml` | ⬜ | — |  |  |  |
| `measure-memory` | ⬜ | — |  |  |  |
| `media` | ⬜ | — |  |  |  |
| `media-capabilities` | 🚫 | — |  |  | медиа-конвейер |
| `media-playback-quality` | 🚫 | — |  |  | медиа-конвейер |
| `media-source` | 🚫 | — |  |  | медиа-конвейер (MSE) |
| `mediacapture-extensions` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-fromelement` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-handle` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-image` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-insertable-streams` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-record` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-region` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediacapture-streams` | 🚫 | — |  |  | медиазахват — нет конвейера |
| `mediasession` | 🚫 | — |  |  | медиа-конвейер |
| `merchant-validation` | 🚫 | — |  |  | Payment Request API |
| `mimesniff` | ⬜ | — |  |  |  |
| `mixed-content` | ⬜ | — |  |  |  |
| `mst-content-hint` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `nav-tracking-mitigations` | ⬜ | — |  |  |  |
| `navigation-api` | ⬜ | — |  |  |  |
| `navigation-timing` | ⬜ | — |  |  |  |
| `netinfo` | ⬜ | — |  |  |  |
| `network-error-logging` | ⬜ | — |  |  |  |
| `notifications` | ⬜ | — |  |  |  |
| `orientation-event` | 🚫 | — |  |  | датчик устройства (legacy DeviceOrientation) |
| `orientation-sensor` | 🚫 | — |  |  | датчик устройства |
| `page-lifecycle` | ⬜ | — |  |  |  |
| `page-visibility` | ⬜ | — |  |  |  |
| `paint-timing` | ⬜ | — |  |  |  |
| `parakeet` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `payment-method-basic-card` | 🚫 | — |  |  | Payment Request API |
| `payment-method-id` | 🚫 | — |  |  | Payment Request API |
| `payment-request` | 🚫 | — |  |  | Payment Request API |
| `performance-timeline` | ⬜ | — |  |  |  |
| `periodic-background-sync` | 🚫 | — |  |  | Service Worker расширение — фоновая ОС-интеграция |
| `permissions` | ⬜ | — |  |  |  |
| `permissions-policy` | ⬜ | — |  |  |  |
| `permissions-request` | ⬜ | — |  |  |  |
| `permissions-revoke` | ⬜ | — |  |  |  |
| `picture-in-picture` | 🚫 | — |  |  | PiP — нет медиа-конвейера видео |
| `png` | ⬜ | — |  |  |  |
| `pointerevents` | ⬜ | — |  |  |  |
| `pointerlock` | ⬜ | — |  |  |  |
| `preload` | ⬜ | — |  |  |  |
| `presentation-api` | 🚫 | — |  |  | медиа/casting API |
| `print` | ⬜ | — |  |  |  |
| `private-aggregation` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `private-click-measurement` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `proximity` | 🚫 | — |  |  | датчик устройства |
| `push-api` | 🚫 | — |  |  | Push-уведомления — нужен пуш-сервис |
| `quirks` | ⬜ | — |  |  |  |
| `referrer-policy` | ⬜ | — |  |  |  |
| `remote-playback` | 🚫 | — |  |  | медиа-конвейер |
| `reporting` | ⬜ | — |  |  |  |
| `requestidlecallback` | ⬜ | — |  |  |  |
| `resize-observer` | ⬜ | — |  |  |  |
| `resource-timing` | ⬜ | — |  |  |  |
| `sanitizer-api` | ⬜ | — |  |  |  |
| `savedata` | ⬜ | — |  |  |  |
| `scheduler` | ⬜ | — |  |  |  |
| `screen-capture` | 🚫 | — |  |  | медиа-конвейер (getDisplayMedia) |
| `screen-details` | 🚫 | — |  |  | мульти-монитор ОС-интеграция |
| `screen-orientation` | ⬜ | — |  |  |  |
| `screen-wake-lock` | ⬜ | — |  |  |  |
| `scroll-animations` | ⬜ | — |  |  |  |
| `scroll-performance-timing` | ⬜ | — |  |  |  |
| `scroll-to-text-fragment` | ⬜ | — |  |  |  |
| `secure-contexts` | ⬜ | — |  |  |  |
| `secure-payment-confirmation` | 🚫 | — |  |  | Payment Request API |
| `selection` | ⬜ | — |  |  |  |
| `serial` | 🚫 | — |  |  | аппаратный API (Serial) |
| `server-timing` | ⬜ | — |  |  |  |
| `service-workers` | ⬜ | — |  |  |  |
| `shadow-dom` | ⬜ | — |  |  |  |
| `shape-detection` | ⬜ | — |  |  |  |
| `shared-storage` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `shared-storage-selecturl-limit` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `signed-exchange` | 🚫 | — |  |  | нишевый формат доставки (SXG) |
| `soft-navigation-heuristics` | ⬜ | — |  |  |  |
| `speculation-rules` | ⬜ | — |  |  |  |
| `speech-api` | 🚫 | — |  |  | нет речевого движка |
| `storage` | ⬜ | — |  |  |  |
| `storage-access-api` | ⬜ | — |  |  |  |
| `streams` | ⬜ | — |  |  |  |
| `subapps` | 🚫 | — |  |  | PWA-инсталляция |
| `subresource-integrity` | ⬜ | — |  |  |  |
| `svg` | ⬜ | — |  |  |  |
| `svg-aam` | ⬜ | — |  |  |  |
| `timing-entrytypes-registry` | ⬜ | — |  |  |  |
| `top-level-storage-access-api` | ⬜ | — |  |  |  |
| `touch-events` | ⬜ | — |  |  |  |
| `trust-tokens` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `trusted-types` | ⬜ | — |  |  |  |
| `ua-client-hints` | ⬜ | — |  |  |  |
| `uievents` | ⬜ | — |  |  |  |
| `upgrade-insecure-requests` | ⬜ | — |  |  |  |
| `url` | ⬜ | — |  |  |  |
| `urlpattern` | ⬜ | — |  |  |  |
| `user-timing` | ⬜ | — |  |  |  |
| `vibration` | 🚫 | — |  |  | аппаратный API (вибро) |
| `video-rvfc` | 🚫 | — |  |  | медиа-конвейер |
| `viewport` | ⬜ | — |  |  |  |
| `viewport-segments` | 🚫 | — |  |  | складные устройства |
| `virtual-keyboard` | 🚫 | — |  |  | мобильная ОС-интеграция |
| `visual-viewport` | ⬜ | — |  |  |  |
| `wai-aria` | ⬜ | — |  |  |  |
| `wasm` | ⬜ | — |  |  |  |
| `web-animations` | ⬜ | — |  |  |  |
| `web-based-payment-handler` | 🚫 | — |  |  | Payment Request API |
| `web-bundle` | ⬜ | — |  |  |  |
| `web-extensions` | 🚫 | — |  |  | модель расширений — отдельная архитектура |
| `web-install` | 🚫 | — |  |  | PWA-инсталляция |
| `web-locks` | ⬜ | — |  |  |  |
| `web-nfc` | 🚫 | — |  |  | аппаратный API (NFC) |
| `web-otp` | 🚫 | — |  |  | WebOTP (SMS) |
| `web-share` | ⬜ | — |  |  |  |
| `webaudio` | ⬜ | — |  |  |  |
| `webauthn` | 🚫 | — |  |  | WebAuthn — отдельная крипто/платформенная интеграция |
| `webcodecs` | 🚫 | — |  |  | нет аппаратного/софт кодек-конвейера |
| `webdriver` | 🚫 | — |  |  | тестовая инфраструктура самого WPT/WebDriver, не веб-фича сайта |
| `webgl` | ⬜ | — |  |  |  |
| `webgpu` | 🚫 | — |  |  | нет compute-конвейера GPU (растеризация — своя) |
| `webhid` | 🚫 | — |  |  | аппаратный API (HID) |
| `webidl` | ⬜ | — |  |  |  |
| `webmcp` | 🚫 | — |  |  | экспериментальный, вне текущего скоупа |
| `webmessaging` | ⬜ | — |  |  |  |
| `webmidi` | 🚫 | — |  |  | аппаратный API (MIDI) |
| `webnn` | 🚫 | — |  |  | нет ML-инференс рантайма |
| `webrtc` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-encoded-transform` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-extensions` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-ice` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-identity` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-priority` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-stats` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `webrtc-svc` | 🚫 | — |  |  | WebRTC — нет конвейера |
| `websockets` | ⬜ | — |  |  |  |
| `webstorage` | ⬜ | — |  |  |  |
| `webtransport` | 🚫 | — |  |  | нет транспортного стека |
| `webusb` | 🚫 | — |  |  | аппаратный API (USB) |
| `webvtt` | ⬜ | — |  |  |  |
| `webxr` | 🚫 | — |  |  | XR — нет рантайма |
| `window-management` | 🚫 | — |  |  | мульти-монитор ОС-интеграция |
| `workers` | ⬜ | — |  |  |  |
| `worklets` | ⬜ | — |  |  |  |
| `x-frame-options` | ⬜ | — |  |  |  |
| `xhr` | ⬜ | — |  |  |  |
| `xml` | ⬜ | — |  |  |  |

---

## `dom/nodes` — детально (168 вендоренных тестов)

Генерируется `tests/wpt/gen_status_md.py` (см. «Как обновить этот файл» выше). «Сабтесты» —
`пройдено/всего` по данным последнего прогона. Пустой «Владелец»/«Баг» — тест ещё не разобран.

<!-- gen:dom/nodes:start -->

| Тест | Статус | Сабтесты | Владелец | Баг | Заметка |
|---|---|---|---|---|---|
| `/dom/nodes/CharacterData-appendChild.html` | OK | 9/9 |  | [BUG-325](../bugs/BUG-325-FIXED.md) | Фикс влит 2026-07-20; сабтесты/статус обновит следующий `run_report.py --all` |
| `/dom/nodes/CharacterData-appendData.html` | OK | 12/14 |  |  |  |
| `/dom/nodes/CharacterData-data.html` | OK | 14/16 |  |  |  |
| `/dom/nodes/CharacterData-deleteData.html` | OK | 18/18 |  |  |  |
| `/dom/nodes/CharacterData-insertData.html` | OK | 18/18 |  |  |  |
| `/dom/nodes/CharacterData-remove.html` | OK | 4/12 |  |  |  |
| `/dom/nodes/CharacterData-replaceData.html` | OK | 34/34 |  |  |  |
| `/dom/nodes/CharacterData-substringData.html` | OK | 26/28 |  |  |  |
| `/dom/nodes/CharacterData-surrogates.html` | OK | 2/8 |  |  |  |
| `/dom/nodes/ChildNode-after.html` | OK | 6/45 |  |  |  |
| `/dom/nodes/ChildNode-before.html` | OK | 5/45 |  |  |  |
| `/dom/nodes/ChildNode-replaceWith.html` | OK | 9/33 |  |  |  |
| `/dom/nodes/Comment-constructor.html` | TIMEOUT | 15/16 |  |  |  |
| `/dom/nodes/DOMImplementation-createDocument-with-null-browsing-context-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/DOMImplementation-createDocument.html` | OK | 111/434 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/DOMImplementation-createDocumentType.html` | OK | 82/82 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/DOMImplementation-createHTMLDocument-with-null-browsing-context-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/DOMImplementation-createHTMLDocument-with-saved-implementation.html` | OK | 0/1 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/DOMImplementation-createHTMLDocument.html` | OK | 2/13 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/DOMImplementation-hasFeature.html` | OK | 137/137 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-URL.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/Document-adoptNode.html` | OK | 0/4 |  |  |  |
| `/dom/nodes/Document-characterSet-normalization-1.html` | TIMEOUT | 0/315 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-characterSet-normalization-2.html` | TIMEOUT | 0/339 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-constructor.html` | OK | 2/5 |  |  |  |
| `/dom/nodes/Document-createAttribute.html` | OK | 0/36 |  |  |  |
| `/dom/nodes/Document-createCDATASection.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/Document-createComment.html` | OK | 0/6 |  |  |  |
| `/dom/nodes/Document-createElement-namespace.html` | TIMEOUT | 3/51 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-createElement.html` | OK | 0/147 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-createElementNS.html` | OK | 0/596 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-createEvent.https.html` | ERROR | 0/0 |  |  |  |
| `/dom/nodes/Document-createProcessingInstruction.html` | OK | 12/12 |  |  |  |
| `/dom/nodes/Document-createTextNode.html` | OK | 0/6 |  | [BUG-327](../bugs/BUG-327-FIXED.md) | Фикс влит 2026-07-21 (реально 6/6), сабтесты/статус обновит следующий `run_report.py --all` |
| `/dom/nodes/Document-createTreeWalker.html` | OK | 4/5 |  |  |  |
| `/dom/nodes/Document-doctype.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/Document-getElementById.html` | OK | 13/18 |  |  |  |
| `/dom/nodes/Document-getElementsByClassName.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/Document-getElementsByTagName.html` | OK | 6/18 |  |  |  |
| `/dom/nodes/Document-getElementsByTagNameNS.html` | OK | 0/14 |  |  |  |
| `/dom/nodes/Document-implementation.html` | OK | 2/2 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Document-importNode.html` | OK | 0/5 |  |  |  |
| `/dom/nodes/DocumentFragment-constructor.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/DocumentFragment-getElementById.html` | OK | 0/5 |  |  |  |
| `/dom/nodes/DocumentFragment-querySelectorAll-after-modification.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/DocumentType-literal.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/DocumentType-remove.html` | OK | 0/4 |  |  |  |
| `/dom/nodes/Element-childElement-null.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-childElementCount-dynamic-add.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-childElementCount-dynamic-remove.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-childElementCount-nochild.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-childElementCount.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-children.html` | OK | 0/2 |  | [BUG-322](../bugs/BUG-322-FIXED.md), [BUG-323](../bugs/BUG-323-FIXED.md) | Оба фикса влиты 2026-07-21; сабтесты/статус обновит следующий `run_report.py --all` |
| `/dom/nodes/Element-classlist.html` | OK | 765/1420 |  |  | XML-namespace validation gap, см. BUG-324 заметку |
| `/dom/nodes/Element-closest.html` | OK | 25/29 |  |  |  |
| `/dom/nodes/Element-firstElementChild-namespace.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/Element-firstElementChild.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-getElementsByClassName.html` | OK | 1/3 |  |  |  |
| `/dom/nodes/Element-getElementsByTagName-change-document-HTMLNess.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/Element-getElementsByTagName.html` | OK | 0/19 |  |  |  |
| `/dom/nodes/Element-getElementsByTagNameNS.html` | OK | 0/16 |  |  |  |
| `/dom/nodes/Element-hasAttribute.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/Element-hasAttributes.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/Element-insertAdjacentElement.html` | OK | 3/6 |  |  |  |
| `/dom/nodes/Element-insertAdjacentText.html` | OK | 3/6 |  |  |  |
| `/dom/nodes/Element-lastElementChild.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-matches-namespaced-elements.html` | OK | 3/6 |  |  |  |
| `/dom/nodes/Element-matches.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/Element-nextElementSibling.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-previousElementSibling.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-remove.html` | OK | 2/4 |  |  |  |
| `/dom/nodes/Element-removeAttribute.html` | OK | 0/2 |  |  |  |
| `/dom/nodes/Element-removeAttributeNS.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/Element-setAttribute-crbug-1138487.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-setAttribute.html` | OK | 0/2 |  |  |  |
| `/dom/nodes/Element-siblingElement-null.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/Element-tagName.html` | OK | 0/6 |  |  |  |
| `/dom/nodes/Element-webkitMatchesSelector.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/MutationObserver-attributes.html` | TIMEOUT | 32/42 |  |  |  |
| `/dom/nodes/MutationObserver-callback-arguments.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/MutationObserver-characterData.html` | TIMEOUT | 17/23 |  |  |  |
| `/dom/nodes/MutationObserver-childList.html` | TIMEOUT | 13/38 |  |  |  |
| `/dom/nodes/MutationObserver-cross-realm-callback-report-exception.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/MutationObserver-disconnect.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/MutationObserver-document.html` | OK | 1/4 |  |  |  |
| `/dom/nodes/MutationObserver-inner-outer.html` | TIMEOUT | 0/3 |  |  |  |
| `/dom/nodes/MutationObserver-nested-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/MutationObserver-sanity.html` | TIMEOUT | 11/16 |  |  |  |
| `/dom/nodes/MutationObserver-takeRecords.html` | OK | 3/3 |  |  |  |
| `/dom/nodes/MutationObserver-textContent.html` | TIMEOUT | 0/4 |  |  |  |
| `/dom/nodes/Node-appendChild.html` | OK | 4/11 |  |  |  |
| `/dom/nodes/Node-baseURI.html` | OK | 4/9 |  |  |  |
| `/dom/nodes/Node-childNodes-cache-2.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/Node-childNodes-cache.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/Node-childNodes.html` | OK | 1/6 |  |  |  |
| `/dom/nodes/Node-cloneNode-XMLDocument.html` | OK | 0/1 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Node-cloneNode-document-with-doctype.html` | OK | 0/3 |  |  |  |
| `/dom/nodes/Node-cloneNode-external-stylesheet-no-bc.sub.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/Node-cloneNode-on-inactive-document-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/Node-cloneNode-svg.html` | OK | 0/4 |  |  |  |
| `/dom/nodes/Node-cloneNode.html` | OK | 97/135 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Node-compareDocumentPosition.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/Node-constants.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/Node-contains.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/Node-insertBefore.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/Node-isConnected-shadow-dom.html` | OK | 0/2 |  |  |  |
| `/dom/nodes/Node-isConnected.html` | OK | 1/2 |  |  |  |
| `/dom/nodes/Node-isEqualNode.html` | OK | 0/9 |  |  |  |
| `/dom/nodes/Node-isSameNode.html` | OK | 0/9 |  |  |  |
| `/dom/nodes/Node-lookupNamespaceURI.html` | OK | 0/70 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/Node-mutation-adoptNode.html` | OK | 0/2 |  |  |  |
| `/dom/nodes/Node-nodeName.html` | OK | 5/6 |  |  |  |
| `/dom/nodes/Node-nodeValue.html` | OK | 0/7 |  |  |  |
| `/dom/nodes/Node-normalize.html` | OK | 0/4 |  |  |  |
| `/dom/nodes/Node-parentElement.html` | OK | 6/12 |  |  |  |
| `/dom/nodes/Node-parentNode-iframe.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/Node-parentNode.html` | TIMEOUT | 2/5 |  |  |  |
| `/dom/nodes/Node-properties.html` | TIMEOUT | 0/0 |  |  |  |
| `/dom/nodes/Node-removeChild.html` | OK | 0/28 |  |  |  |
| `/dom/nodes/Node-replaceChild.html` | OK | 1/29 |  |  |  |
| `/dom/nodes/Node-textContent.html` | OK | 33/81 |  |  |  |
| `/dom/nodes/NodeList-Iterable.html` | OK | 7/8 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-1.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-2.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-3.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-indexOf-1.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-indexOf-2.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/NodeList-static-length-getter-tampered-indexOf-3.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/ParentNode-append.html` | OK | 0/25 |  |  |  |
| `/dom/nodes/ParentNode-children.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/ParentNode-prepend.html` | OK | 0/22 |  |  |  |
| `/dom/nodes/ParentNode-querySelector-All-content.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/ParentNode-querySelector-All.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/ParentNode-querySelector-case-insensitive.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/ParentNode-querySelector-escapes.html` | OK | 20/68 |  |  |  |
| `/dom/nodes/ParentNode-querySelector-scope.html` | OK | 2/4 |  |  |  |
| `/dom/nodes/ParentNode-querySelectorAll-removed-elements.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/ParentNode-querySelectors-exclusive.html` | OK | 1/1 |  |  |  |
| `/dom/nodes/ParentNode-querySelectors-namespaces.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/ParentNode-querySelectors-space-and-dash-attribute-value.html` | OK | 2/2 |  |  |  |
| `/dom/nodes/ParentNode-replaceChildren.html` | OK | 0/31 |  |  |  |
| `/dom/nodes/Text-constructor.html` | TIMEOUT | 15/16 |  |  |  |
| `/dom/nodes/Text-splitText.html` | OK | 0/6 |  |  |  |
| `/dom/nodes/Text-wholeText.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/append-on-Document.html` | OK | 0/5 |  |  |  |
| `/dom/nodes/attributes-namednodemap.html` | OK | 0/8 |  |  |  |
| `/dom/nodes/attributes.html` | OK | 6/67 |  |  |  |
| `/dom/nodes/case.html` | OK | 8/285 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/getElementsByClassName-32.html` | OK | 4/4 |  |  |  |
| `/dom/nodes/getElementsByClassName-empty-set.html` | OK | 3/3 |  |  |  |
| `/dom/nodes/getElementsByClassName-whitespace-class-names.html` | OK | 5/26 |  |  |  |
| `/dom/nodes/insert-adjacent.html` | OK | 6/14 |  |  |  |
| `/dom/nodes/insertBefore-iframe-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/name-validation.html` | ERROR | 0/0 |  |  |  |
| `/dom/nodes/node-appendchild-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/prepend-on-Document.html` | OK | 0/5 |  |  |  |
| `/dom/nodes/processing-instruction-attributes.html` | OK | 6/140 |  | [BUG-324](../bugs/BUG-324-FIXED.md) |  |
| `/dom/nodes/query-target-in-load-event.html` | TIMEOUT | 0/1 |  |  |  |
| `/dom/nodes/query-target-in-load-event.part.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/querySelector-mixed-case.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/remove-and-adopt-thcrash.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/remove-from-shadow-host-and-adopt-into-iframe-ref.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/remove-from-shadow-host-and-adopt-into-iframe.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/remove-next-sibling-during-replace-with.html` | OK | 0/1 |  |  |  |
| `/dom/nodes/remove-unscopable.html` | OK | 0/6 |  |  |  |
| `/dom/nodes/replaceWith-document-element-crash.html` | NOT RUN | 0/0 |  |  |  |
| `/dom/nodes/rootNode.html` | OK | 0/5 |  |  |  |
| `/dom/nodes/svg-template-querySelector.html` | OK | 3/3 |  |  |  |

<!-- gen:dom/nodes:end -->
