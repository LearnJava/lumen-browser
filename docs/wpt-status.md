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
iframes/multi-window — см. `docs/tasks/p2-wpt-integration.md`. `test_driver.*` (WPT-RUN-2,
2026-08-02) больше не блокирует тесты блочным SKIP — из действий реально исполняется только
`click` (`input.performActions`), остальные `test_driver_internal.*`-вызовы отклоняют промис
теста явной ошибкой вместо зависания. HTTPS-порт (WPT-RUN-2) тоже больше не `invalid port:
"None"` — `.https.`-тесты доезжают до настоящего TLS-хендшейка и получают `UnknownIssuer`
(тестовый сертификат самоподписан и не в доверенных корнях Lumen, см.
`tests/wpt/certs/README.md`), не хендшейк вовсе. Остальные
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
пока не сделано; `run_report.py --root`/`--recursive` уже обобщены и пригодны для любой категории
организованной в подкаталоги). Скоуп/заметку в категорийном индексе можно и нужно пересматривать —
это первый черновой проход одного разработчика, не итог обсуждения.

**Колонку «Заметка» держать короткой.** До 2026-08-09 сюда писалась вся сводка прогона целиком —
файл разросся до 319 КБ строк, за лимит чтения инструментами. Вместо этого: одно предложение
(дата, число файлов, headline-результат прогона), заканчивающееся ссылкой
`[Подробности](wpt-vendor-notes/<slug>.md)`, а полный текст — в этот файл (создать, если его ещё
нет; `<slug>` = имя категории, `/` заменить на `-`).

---

## Категорийный индекс (277 категорий верхнего уровня)

| Категория | Скоуп | Вендорено | Владелец | Баг | Заметка |
|---|---|---|---|---|---|
| `FileAPI` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-21 (коммит `35be3b44`, `tests/wpt/FileAPI/`, 115 файлов; `common/`/`html/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон… [Подробности](wpt-vendor-notes/FileAPI.md). |
| `IndexedDB` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `c8cad69f`, `tests/wpt/IndexedDB/`, 245 файлов; `common/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон `run_report.py… [Подробности](wpt-vendor-notes/IndexedDB.md). |
| `WebCryptoAPI` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `629dbeb5`, `tests/wpt/WebCryptoAPI/`, 185 файлов; ссылается только на уже вендоренные `/resources/testharness.js`+`testharnessreport.js`, внекатегорийных… [Подробности](wpt-vendor-notes/WebCryptoAPI.md). |
| `accelerometer` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-22 (коммит `0f7f0157`, `tests/wpt/accelerometer/`, 19 файлов), включена несмотря на скоуп 🚫 по прямому запросу пользователя 2026-07-21. Почти все тесты ссылаются на… [Подробности](wpt-vendor-notes/accelerometer.md). |
| `accessibility` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (коммит `344c7afb`, `tests/wpt/accessibility/`, 59 файлов; ReadMe.md + `crashtests/` + один testdriver.js-тест). Внекатегорийных хелперов не обнаружено. Категория почти… [Подробности](wpt-vendor-notes/accessibility.md). |
| `accname` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (коммит `69ab520d`, `tests/wpt/accname/`, 183 файла: `manual/`-подкаталог — тесты для ручной проверки, не рассчитаны на автоматизацию через testharness.js; `name/` —… [Подробности](wpt-vendor-notes/accname.md). |
| `acid` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/acid/`, 30 файлов: `acid2/`, `acid3/`), включена несмотря на скоуп 🚫 (исторические Acid1/2/3, не актуальный спек) по тому же постоянному решению пользователя,… [Подробности](wpt-vendor-notes/acid.md). |
| `ai` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/ai/`, 180 файлов: `classifier/`, `language-model/`, `language_detection/`, `proofreader/`, `rewriter/`, `semantic_embedder/`, `summarizer/`, `translator/`,… [Подробности](wpt-vendor-notes/ai.md). |
| `ambient-light` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/ambient-light/`, 16 файлов: `AmbientLightSensor*.html`, `idlharness.https.window.js`, `resources/sensor-data.js`), включена несмотря на скоуп 🚫 (датчик… [Подробности](wpt-vendor-notes/ambient-light.md). |
| `animation-worklet` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/animation-worklet/`, 50 файлов: 43 корневых `.html`, `META.yml`, `common.js`, `idlharness.any.js`, `references/` (2 файла), `resources/` (2 iframe-хелпера)).… [Подробности](wpt-vendor-notes/animation-worklet.md). |
| `annotation-model` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/annotation-model/`, 290 файлов: `definitions/`+`examples/`+`tools/samples/`+`tools/tests/` — 259 JSON-фикстур, `scripts/` —… [Подробности](wpt-vendor-notes/annotation-model.md). |
| `annotation-protocol` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/annotation-protocol/`, 58 файлов: `files/annotations/` — 43 JSON/JSON-LD аннотации-фикстуры + `*.headers`, `files/index.html` — HTML-страница-цель для ручного… [Подробности](wpt-vendor-notes/annotation-protocol.md). |
| `annotation-vocab` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/annotation-vocab/`, 10 файлов: 6 `NN-*-manual.html`, `META.yml`, `tools/anno.jsonld`, `tools/README.md`, `tools/vocab_tester.py`). Все 6 HTML ссылаются только… [Подробности](wpt-vendor-notes/annotation-vocab.md). |
| `apng` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-23 (`tests/wpt/apng/`, 4 файла: `META.yml`, reftest-пара `animated-png-timeout.html`+`animated-png-timeout-ref.html`, `supported-in-source-type.html`). Внекатегорийные… [Подробности](wpt-vendor-notes/apng.md). |
| `appmanifest` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (`tests/wpt/appmanifest/`, 253 файла: `META.yml`, `WEB_FEATURES.yml`, 19 `*-member/` подкатегорий — `display`, `display-override`, `file_handlers`, `icons`(`_localized`),… [Подробности](wpt-vendor-notes/appmanifest.md). |
| `attribution-reporting` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (`tests/wpt/attribution-reporting/`, 13 файлов: `WEB_FEATURES.yml`, `resources/` — `helpers.js` + Python-хендлеры `wptserve`, `aggregatable-debug/` — 2 тестовых файла,… [Подробности](wpt-vendor-notes/attribution-reporting.md). |
| `audio-output` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (`tests/wpt/audio-output/`, 12 файлов: `META.yml`, `idlharness.https.window.js`, 9 `*.https(.sub).html`, `setSinkId-manual.https.html`), включена несмотря на скоуп 🚫… [Подробности](wpt-vendor-notes/audio-output.md). |
| `audio-session` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (`tests/wpt/audio-session/`, 3 файла: `audiosession-default-values.https.html`, `audiosession-type-setter.https.html`, `idlharness.window.js`), включена несмотря на скоуп… [Подробности](wpt-vendor-notes/audio-session.md). |
| `autoplay-policy-detection` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (`tests/wpt/autoplay-policy-detection/`, 4 файла: `META.yml`, `autoplaypolicy.html`, `autoplaypolicy_media_element.html`, `idlharness.window.js`), включена несмотря на… [Подробности](wpt-vendor-notes/autoplay-policy-detection.md). |
| `avif` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/avif/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, reftest-пара `animated-avif-timeout.html`+`animated-avif-timeout-ref.html`). Два… [Подробности](wpt-vendor-notes/avif.md). |
| `background-fetch` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/background-fetch/`, 20 файлов: `META.yml`, `WEB_FEATURES.yml`, 11 тестовых файлов, `resources/` —… [Подробности](wpt-vendor-notes/background-fetch.md). |
| `background-sync` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/background-sync/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, `idlharness.https.any.js`, `service_workers/sw.js`), включена несмотря на скоуп… [Подробности](wpt-vendor-notes/background-sync.md). |
| `badging` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/badging/`, 9 файлов: `META.yml`, `WEB_FEATURES.yml`, `badge-error.https.any.js`, `badge-success.https.any.js`, `idlharness.https.any.js`,… [Подробности](wpt-vendor-notes/badging.md). |
| `battery-status` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/battery-status/`, 30 файлов: `META.yml`, `WEB_FEATURES.yml`, 22 тестовых/header-файла, `resources/` —… [Подробности](wpt-vendor-notes/battery-status.md). |
| `beacon` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-24 (коммит `35be3b44`, `tests/wpt/beacon/`, 22 файла: `META.yml`, `WEB_FEATURES.yml`, 5 корневых `.https.` тестовых файлов, `beacon-common.sub.js`, `idlharness.any.js`,… [Подробности](wpt-vendor-notes/beacon.md). |
| `bluetooth` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/bluetooth/`, 187 файлов: `META.yml`, `WEB_FEATURES.yml`, `README.md`, `resources/` — 4 хелпера, `bidi/` — 9 подкатегорий… [Подробности](wpt-vendor-notes/bluetooth.md). |
| `browsing-topics` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/browsing-topics/`, 18 файлов: `WEB_FEATURES.yml`, 13 тестовых файлов, `resources/` — 3 Python-хендлера + `pixel.png` + 4 JS-хелпера),… [Подробности](wpt-vendor-notes/browsing-topics.md). |
| `captured-mouse-events` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/captured-mouse-events/`, 6 файлов: `META.yml`, `capture-controller-oncapturedmousechange.https.html`,… [Подробности](wpt-vendor-notes/captured-mouse-events.md). |
| `clear-site-data` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/clear-site-data/`, 28 файлов: `META.yml`, `WEB_FEATURES.yml`, 13 тестовых файлов, `support/` — 5 Python-хендлеров +… [Подробности](wpt-vendor-notes/clear-site-data.md). |
| `client-hints` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/client-hints/`, 276 файлов: `META.yml`, подкатегории… [Подробности](wpt-vendor-notes/client-hints.md). |
| `clipboard-apis` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/clipboard-apis/`, 76 файлов: `META.yml`, корневые тесты, `detached-iframe/`+`events/`+`permissions/`+`permissions-policy/` подкатегории,… [Подробности](wpt-vendor-notes/clipboard-apis.md). |
| `close-watcher` | ⬜ | ✅ |  | [BUG-340](../bugs/BUG-340-OPEN.md) | Вендорена целиком 2026-07-25 (`tests/wpt/close-watcher/`, 54 файла: `META.yml`, `WEB_FEATURES.yml`, root-level tests, `esc-key/`+`iframes/`+`user-activation/` подкатегории, `resources/helpers.js`).… [Подробности](wpt-vendor-notes/close-watcher.md). |
| `compat` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `ed90cf49`, `tests/wpt/compat/`, 56 файлов; в основном webkit-совместимостные reftest-пары, 11 testharness-тестов + 2 не довендорены гэпом ниже). 2 из 11… [Подробности](wpt-vendor-notes/compat.md). |
| `compression` | ⬜ | ✅ |  | [BUG-342](../bugs/BUG-342-FIXED.md) | Вендорена целиком 2026-07-25 (`tests/wpt/compression/`, 28 файлов: `META.yml`, `WEB_FEATURES.yml`, 18 `.any.js` compression/decompression тестов, `idlharness.https.any.js`, `resources/` —… [Подробности](wpt-vendor-notes/compression.md). |
| `compute-pressure` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-25 (`tests/wpt/compute-pressure/`, 37 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, 19 корневых `.https.` тестов, `permissions-policy/` подкаталог (9 файлов + 2… [Подробности](wpt-vendor-notes/compute-pressure.md). |
| `connection-allowlist` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/connection-allowlist/`, 183 файла, вся категория живёт под `tentative/`, без category-level `META.yml`/`WEB_FEATURES.yml`), включена… [Подробности](wpt-vendor-notes/connection-allowlist.md). |
| `console` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/console/`, 16 файлов: `META.yml`, `WEB_FEATURES.yml`, `helper.js`, `idlharness.any.js`, 11 тестовых файлов), Console API уже частично… [Подробности](wpt-vendor-notes/console.md). |
| `contacts` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-25 (`tests/wpt/contacts/`, 5 файлов: `META.yml`, `WEB_FEATURES.yml`, `contacts-select.https.window.js`, `resources/` — `helpers.js`+`non-main-frame-select.html`), включена… [Подробности](wpt-vendor-notes/contacts.md). |
| `container-timing` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (коммит `35be3b44`, `tests/wpt/container-timing/`, 22 файла: `META.yml`, `resources/` — `container-timing-helpers.js`+`square100.png`, `tentative/` — 19 тестовых файлов).… [Подробности](wpt-vendor-notes/container-timing.md). |
| `content-dpr` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (`tests/wpt/content-dpr/`, 14 файлов: 7 тестов + 7 ресурсов, без `META.yml`/`WEB_FEATURES.yml`), скоуп ⬜ (легаси WebKit-специфичный HTTP-заголовок `Content-DPR`,… [Подробности](wpt-vendor-notes/content-dpr.md). |
| `content-index` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (`tests/wpt/content-index/`, 6 файлов: `META.yml`, `WEB_FEATURES.yml`, `content-index.https.window.js`, `idlharness.https.any.js`, `resources.js`, `resources/sw.js`),… [Подробности](wpt-vendor-notes/content-index.md). |
| `content-security-policy` | ⬜ | — |  |  |  |
| `contenteditable` | ⬜ | ✅ |  | [BUG-344](../bugs/BUG-344-OPEN.md), [BUG-345](../bugs/BUG-345-OPEN.md) | Вендорена целиком 2026-07-25 (`tests/wpt/contenteditable/`, 6 файлов: `META.yml`, `WEB_FEATURES.yml`, `designmode-iscontenteditable.html`, `plaintext-only.html`, `select-text-change-crash.html`,… [Подробности](wpt-vendor-notes/contenteditable.md). |
| `cookies` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-25 (`tests/wpt/cookies/`, 171 файл: `attributes/`, `domain/`, `encoding/`, `name/`, `ordering/`, `origin-bound-cookies/`, `partitioned-cookies/`, `path/`, `prefix/`,… [Подробности](wpt-vendor-notes/cookies.md). |
| `cookiestore` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/cookiestore/`, 65 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, `resources/` — 8 хелперов, 54 тестовых файла). Внекатегорийные… [Подробности](wpt-vendor-notes/cookiestore.md). |
| `core-aam` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (`tests/wpt/core-aam/`, 289 файлов: `META.yml`, `aamtests/` — 104 Python-файла (платформенные accessibility-API тесты через AT-SPI/IAccessible2/AXAPI-обёртки,… [Подробности](wpt-vendor-notes/core-aam.md). |
| `cors` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (`tests/wpt/cors/`, 45 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, `support.js`, 21 корневых тестовых файла (в основном `.htm`, легаси-расширение), `resources/`… [Подробности](wpt-vendor-notes/cors.md). |
| `cpu-performance` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/cpu-performance/`, 2 файла: `META.yml`, `cpu-performance.tentative.https.window.js`), скоуп ⬜ (`navigator.cpuPerformance` — предложение… [Подробности](wpt-vendor-notes/cpu-performance.md). |
| `credential-management` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/credential-management/`, 21 файл: `META.yml`, `WEB_FEATURES.yml`, 13 корневых тестовых файлов, `support/` — 5 хелперов + `README.md`),… [Подробности](wpt-vendor-notes/credential-management.md). |
| `css` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/css/`, 54350 файлов, ~209 МБ — весь верхнеуровневый каталог `css/` апстрима: `CSS1`/`CSS2`, `WOFF2`, `compositing`, все модули `css-*`… [Подробности](wpt-vendor-notes/css.md). |
| `cssom` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/cssom/`, 2 файла: `crashtests/csstext-with-all.html`, `crashtests/delete-rule.html`), скоуп ⬜ — верхнеуровневая категория `cssom/`,… [Подробности](wpt-vendor-notes/cssom.md). |
| `custom-elements` | ⬜ | ✅ |  | [BUG-346](../bugs/BUG-346-OPEN.md), [BUG-347](../bugs/BUG-347-OPEN.md) | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/custom-elements/`, 208 файлов: `META.yml`, `WEB_FEATURES.yml`,… [Подробности](wpt-vendor-notes/custom-elements.md). |
| `delegated-ink` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/delegated-ink/`, 6 файлов: `WEB_FEATURES.yml`, `delete-presentation-area.html`, `exception-thrown-bad-color.tentative.html`,… [Подробности](wpt-vendor-notes/delegated-ink.md). |
| `density-size-correction` | ⬜ | ✅ |  | [BUG-348](../bugs/BUG-348-OPEN.md) | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/density-size-correction/`, 44 файла: `WEB_FEATURES.yml`, `resources/` — EXIF-фикстуры `.jpg` + `exify.js` + `third_party/piexif/`, ~22… [Подробности](wpt-vendor-notes/density-size-correction.md). |
| `deprecation-reporting` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/deprecation-reporting/`, 3 файла: `META.yml`, `__dir__.ini`, `idlharness.any.js`). Апстримный `__dir__.ini` сам помечает категорию… [Подробности](wpt-vendor-notes/deprecation-reporting.md). |
| `device-bound-session-credentials` | 🚫 | ✅ |  | [BUG-350](../bugs/BUG-350-OPEN.md) | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/device-bound-session-credentials/`, 49 файлов: 27 `.https.html`/`.html` тестов, `helper.js`, `url_fetcher.html` (fixture-страница, не… [Подробности](wpt-vendor-notes/device-bound-session-credentials.md). |
| `device-memory` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/device-memory/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, `device-memory.https.any.js`, `idlharness.https.any.js`), скоуп 🚫 (датчик… [Подробности](wpt-vendor-notes/device-memory.md). |
| `device-posture` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/device-posture/`, 8 файлов: `META.yml`, `WEB_FEATURES.yml`, `README.md`, `idlharness.https.window.js`, 4 `device-posture-*.https.html`… [Подробности](wpt-vendor-notes/device-posture.md). |
| `digital-credentials` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/digital-credentials/`, 32 файла: `META.yml`, `dc-types.ts`, `tsconfig.json`, 21 `.https(.sub)?.html`/`.http.html` тестов,… [Подробности](wpt-vendor-notes/digital-credentials.md). |
| `direct-sockets` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/direct-sockets/`, 8 файлов: `META.yml`, `README.md`, `disabled-by-permissions-policy.https.sub.html`+`.headers`,… [Подробности](wpt-vendor-notes/direct-sockets.md). |
| `document-picture-in-picture` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/document-picture-in-picture/`, 27 файлов: `META.yml`, `WEB_FEATURES.yml`, 24 `.https.html`/`.html` тестов, `support/focus-opener.js`,… [Подробности](wpt-vendor-notes/document-picture-in-picture.md). |
| `document-policy` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/document-policy/`, 27 файлов: `META.yml`, `echo-policy.py`+`echo-policy-nested.html` — рекурсивная iframe-echo-фикстура,… [Подробности](wpt-vendor-notes/document-policy.md). |
| `dom` | ⬜ | 🟡 (`dom/nodes/` только) |  | [BUG-324](../bugs/BUG-324-FIXED.md) | таблица ниже |
| `domparsing` | ⬜ | ✅ |  | [BUG-351](../bugs/BUG-351-OPEN.md) | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/domparsing/`, 71 файл: `META.yml`, `WEB_FEATURES.yml`, `DOMParser-*`/`XMLSerializer-*`/`innerhtml-*`/`outerhtml-*`/`insert*` root tests,… [Подробности](wpt-vendor-notes/domparsing.md). |
| `domxpath` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/domxpath/`, 39 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, 34 корневых теста, `resources/` —… [Подробности](wpt-vendor-notes/domxpath.md). |
| `dpub-aam` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/dpub-aam/`, 43 файла: `META.yml`, `role/roles.html`, `manual/` — 41 `doc-*-manual.html` фикстура + `META.yml` + `README.md`), в скоупе (⬜,… [Подробности](wpt-vendor-notes/dpub-aam.md). |
| `dpub-aria` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/dpub-aria/`, 4 файла: `META.yml`, `README.md`, `.editorconfig`, `inuse-manual.html`), в скоупе (⬜, DPUB-ARIA Recommendation — словарь ролей… [Подробности](wpt-vendor-notes/dpub-aria.md). |
| `ecmascript` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-26 (коммит `35be3b44`, `tests/wpt/ecmascript/`, 3 файла: `README.md`, `locale-compat.html`, `regexp-lookbehind.html`), в скоупе (⬜, WPT-покрытие… [Подробности](wpt-vendor-notes/ecmascript.md). |
| `editing` | ⬜ | ✅ |  | [BUG-353](../bugs/BUG-353-OPEN.md), [BUG-346](../bugs/BUG-346-OPEN.md) | Вендорена целиком 2026-07-27 (коммит `35be3b44`, `tests/wpt/editing/`, 379 файлов: `META.yml`, `README`, `crashtests/` — 144, `other/` — 97, `run/` — 43, `data/` — 36, `whitespaces/` — 19,… [Подробности](wpt-vendor-notes/editing.md). |
| `element-timing` | ⬜ | ✅ |  | [BUG-354](../bugs/BUG-354-OPEN.md) | Вендорена целиком 2026-07-27 (коммит `35be3b44`, `tests/wpt/element-timing/`, 70 файлов: `META.yml`, `WEB_FEATURES.yml`, 49 тестовых `.html`, `idlharness.window.js`, `resources/` —… [Подробности](wpt-vendor-notes/element-timing.md). |
| `encoding` | ⬜ | ✅ |  | [BUG-356](../bugs/BUG-356-OPEN.md), [BUG-357](../bugs/BUG-357-OPEN.md) | Вендорена целиком 2026-07-27 (коммит `35be3b44`, `tests/wpt/encoding/`, 341 файл: `META.yml`, `WEB_FEATURES.yml`, 54 корневых теста/хелпера, `resources/` —… [Подробности](wpt-vendor-notes/encoding.md). |
| `encoding-detection` | ⬜ | ✅ |  | [BUG-358](../bugs/BUG-358-OPEN.md), [BUG-359](../bugs/BUG-359-FIXED.md) | Вендорена целиком 2026-07-27 (коммит `35be3b44`, `tests/wpt/encoding-detection/`, 109 файлов: 75 корневых `*.tentative.html`/`.html`-тестов, `support/` — 31 `*-late.sub.html`-фикстура, два… [Подробности](wpt-vendor-notes/encoding-detection.md). |
| `encrypted-media` | ⬜ | ✅ |  | [BUG-360](../bugs/BUG-360-FIXED.md), [BUG-361](../bugs/BUG-361-FIXED.md), [BUG-347](../bugs/BUG-347-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/encrypted-media/`, 173 файла: 96 `.https.`-тестов в корне (`clearkey-*`/`drm-*` — генерируемые пары «один сценарий × два key system»),… [Подробности](wpt-vendor-notes/encrypted-media.md). |
| `entries-api` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/entries-api/`, 29 файлов: 14 `*-manual.html`, `idlharness.window.js`, `idlharness-manual.window.js`, `support.js`, `support/` — 8… [Подробности](wpt-vendor-notes/entries-api.md). |
| `event-timing` | ⬜ | ✅ |  | [BUG-354](../bugs/BUG-354-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/event-timing/`, 78 файлов: 69 корневых `.html` (в т.ч. один `-manual` и два `.tentative.`), `idlharness.any.js`, `idlharness.window.js`,… [Подробности](wpt-vendor-notes/event-timing.md). |
| `eventsource` | ⬜ | ✅ |  | [BUG-362](../bugs/BUG-362-FIXED.md), [BUG-363](../bugs/BUG-363-FIXED.md), [BUG-364](../bugs/BUG-364-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/eventsource/`, 95 файлов: 33 корневых `.any.js`, 12 корневых `.window.js`, 2 корневых `.htm`, `dedicated-worker/` — 7 `.htm` + 9 скриптов… [Подробности](wpt-vendor-notes/eventsource.md). |
| `eyedropper` | 🚫 | ✅ |  | [BUG-365](../bugs/BUG-365-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/eyedropper/`, 7 файлов: 2 корневых теста `eye-dropper-abort-signal.tentative.https.html` и `idlharness.https.window.js`, `manual/` — 2… [Подробности](wpt-vendor-notes/eyedropper.md). |
| `fedcm` | 🚫 | ✅ |  | [BUG-366](../bugs/BUG-366-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fedcm/`, 192 файла: `META.yml`/`WEB_FEATURES.yml`, 39 корневых `.https.`-тестов плюс их `.headers`-файлы, 12 подкатегорий… [Подробности](wpt-vendor-notes/fedcm.md). |
| `fenced-frame` | 🚫 | ✅ |  | [BUG-367](../bugs/BUG-367-FIXED.md), [BUG-368](../bugs/BUG-368-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fenced-frame/`, 400 файлов: `META.yml`/`WEB_FEATURES.yml`/`README.md`, 198 корневых `.html`-тестов, из них 197 `.https.` и ровно один… [Подробности](wpt-vendor-notes/fenced-frame.md). |
| `fetch` | ⬜ | ✅ |  | [BUG-369](../bugs/BUG-369-FIXED.md), [BUG-370](../bugs/BUG-370-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fetch/`, 985 файлов: `META.yml`/`README.md`, 23 подкатегории (`api/` — крупнейшая, далее `metadata/`, `fetch-later/`, `h1-parsing/`,… [Подробности](wpt-vendor-notes/fetch.md). |
| `file-system-access` | ⬜ | ✅ |  | [BUG-371](../bugs/BUG-371-FIXED.md), [BUG-372](../bugs/BUG-372-FIXED.md), [BUG-373](../bugs/BUG-373-FIXED.md), [BUG-374](../bugs/BUG-374-FIXED.md), [BUG-750](../bugs/BUG-750-FIXED.md), [BUG-751](../bugs/BUG-751-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/file-system-access/`, 43 файла: `META.yml`/`WEB_FEATURES.yml`/`README.md`, **5** исполняемых тестов (`getDirectory.https.any.js`,… [Подробности](wpt-vendor-notes/file-system-access.md). |
| `fledge` | 🚫 | ✅ |  | [BUG-375](../bugs/BUG-375-FIXED.md), [BUG-376](../bugs/BUG-376-FIXED.md), [BUG-377](../bugs/BUG-377-FIXED.md), [BUG-378](../bugs/BUG-378-FIXED.md), [BUG-379](../bugs/BUG-379-FIXED.md), [BUG-380](../bugs/BUG-380-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fledge/`, 77 файлов: всё дерево лежит под `tentative/` — у категории нет ни `META.yml`, ни `WEB_FEATURES.yml` на верхнем уровне; 34… [Подробности](wpt-vendor-notes/fledge.md). |
| `focus` | ⬜ | ✅ |  | [BUG-381](../bugs/BUG-381-OPEN.md), [BUG-382](../bugs/BUG-382-OPEN.md), [BUG-383](../bugs/BUG-383-OPEN.md), [BUG-384](../bugs/BUG-384-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/focus/`, 116 файлов: `WEB_FEATURES.yml`, 45 файлов верхнего уровня, `support/` из 69 хелперов, `LICENSE-WPT.md`; `META.yml` у категории… [Подробности](wpt-vendor-notes/focus.md). |
| `font-access` | ⬜ | ✅ |  | [BUG-385](../bugs/BUG-385-FIXED.md), [BUG-386](../bugs/BUG-386-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/font-access/`, 20 файлов: `META.yml`, `WEB_FEATURES.yml`, `README.md`, 6 `font_access_*.tentative.https.window.js`, `permissions-policy/` —… [Подробности](wpt-vendor-notes/font-access.md). |
| `fonts` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fonts/`, 205 файлов + `LICENSE-WPT.md`), скоуп ⬜ (кандидат). **Это не тестовая категория, а разделяемый ресурсный каталог**: `README.md`… [Подробности](wpt-vendor-notes/fonts.md). |
| `forced-colors-mode` | ⬜ | ✅ |  | [BUG-387](../bugs/BUG-387-FIXED.md), [BUG-388](../bugs/BUG-388-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/forced-colors-mode/`, 118 файлов: `README.txt`, `WEB_FEATURES.yml` — `META.yml` у категории нет, 92 файла верхнего уровня, в основном… [Подробности](wpt-vendor-notes/forced-colors-mode.md). |
| `fs` | ⬜ | ✅ |  | [BUG-389](../bugs/BUG-389-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fs/`, 88 файлов: `META.yml`, `README.md` — `WEB_FEATURES.yml` у категории нет, 42 исполняемых файла верхнего уровня, `resources/` из 17… [Подробности](wpt-vendor-notes/fs.md). |
| `fullscreen` | ⬜ | ✅ |  | [BUG-390](../bugs/BUG-390-FIXED.md), [BUG-391](../bugs/BUG-391-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/fullscreen/`, 111 файлов: `META.yml`, `WEB_FEATURES.yml`, `api/` — 64 файла, `rendering/` — 19 файлов (6 `rel=match` reftest-пар), `model/`… [Подробности](wpt-vendor-notes/fullscreen.md). |
| `gamepad` | 🚫 | ✅ |  | [BUG-392](../bugs/BUG-392-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/gamepad/`, 15 файлов: `META.yml`, `WEB_FEATURES.yml`, 6 `*-manual(.https).html`, 7 не-manual тестовых файлов, `LICENSE-WPT.md`), включена… [Подробности](wpt-vendor-notes/gamepad.md). |
| `generic-sensor` | 🚫 | ✅ |  | [BUG-393](../bugs/BUG-393-FIXED.md), [BUG-394](../bugs/BUG-394-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/generic-sensor/`, 11 файлов: `META.yml`, `WEB_FEATURES.yml`, 3 тестовых файла верхнего уровня, 3 общих `.js`-хелпера для всего семейства… [Подробности](wpt-vendor-notes/generic-sensor.md). |
| `geolocation` | ⬜ | ✅ |  | [BUG-395](../bugs/BUG-395-FIXED.md), [BUG-762](../bugs/BUG-762-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/geolocation/`, 28 файлов: `META.yml`, `WEB_FEATURES.yml`, 22 `*.https(.sub).html`/`*.https.window.js`, 1 `.http.html`, `resources/`,… [Подробности](wpt-vendor-notes/geolocation.md). |
| `geolocation-sensor` | 🚫 | ✅ |  |  | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/geolocation-sensor/`, 3 файла: `META.yml`, `historical.https.html`, `LICENSE-WPT.md` — `WEB_FEATURES.yml` у категории нет), включена… [Подробности](wpt-vendor-notes/geolocation-sensor.md). |
| `gif` | ⬜ | ✅ |  |  | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/gif/`, 4 файла: `META.yml` — `WEB_FEATURES.yml` у категории нет, `reset-no-gce-1.html`+`reset-no-gce-ref.html` — reftest-пара `rel=match`,… [Подробности](wpt-vendor-notes/gif.md). |
| `gpc` | ⬜ | ✅ |  | [BUG-397](../bugs/BUG-397-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/gpc/`, 10 файлов: `META.yml`, `WEB_FEATURES.yml`, `global_privacy_control.testdriver.html`, `idlharness.any.js`,… [Подробности](wpt-vendor-notes/gpc.md). |
| `graphics-aam` | ⬜ | ✅ |  | [BUG-398](../bugs/BUG-398-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/graphics-aam/`, 7 файлов: `META.yml`, 6 `*-manual.html` — ATTAcomm-тесты для ролей `graphics-document`/`graphics-object`/`graphics-symbol`… [Подробности](wpt-vendor-notes/graphics-aam.md). |
| `graphics-aria` | ⬜ | ✅ |  | [BUG-398](../bugs/BUG-398-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/graphics-aria/`, 3 файла: `META.yml`, `graphics-roles.html`, `LICENSE-WPT.md` — `WEB_FEATURES.yml` у категории нет), скоуп ⬜ (кандидат),… [Подробности](wpt-vendor-notes/graphics-aria.md). |
| `gyroscope` | 🚫 | ✅ |  | [BUG-361](../bugs/BUG-361-FIXED.md), [BUG-399](../bugs/BUG-399-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/gyroscope/`, 16 файлов: `META.yml`, `WEB_FEATURES.yml`, 9 `*.https(.html)`/plain `.html` root-тестов, 2 `.headers`-сайдкара,… [Подробности](wpt-vendor-notes/gyroscope.md). |
| `hr-time` | ⬜ | ✅ |  | [BUG-400](../bugs/BUG-400-FIXED.md), [BUG-401](../bugs/BUG-401-FIXED.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/hr-time/`, 19 файлов: `META.yml`, `WEB_FEATURES.yml`, 13 тестовых файлов верхнего уровня (3 `.https.` + 2 `.https.*.headers`), `resources/`… [Подробности](wpt-vendor-notes/hr-time.md). |
| `hsts` | ⬜ | ✅ |  | [BUG-402](../bugs/BUG-402-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/hsts/`, 4 файла: `WEB_FEATURES.yml`, единственный тест `third-party-subframe-hsts-upgrade.tentative.sub.html`,… [Подробности](wpt-vendor-notes/hsts.md). |
| `html` | ⬜ | ✅ (обследована частично) | | [BUG-418](../bugs/BUG-418-FIXED.md), [BUG-412](../bugs/BUG-412-OPEN.md), [BUG-413](../bugs/BUG-413-OPEN.md), [BUG-414](../bugs/BUG-414-FIXED.md), [BUG-415](../bugs/BUG-415-OPEN.md), [BUG-416](../bugs/BUG-416-OPEN.md), [BUG-417](../bugs/BUG-417-OPEN.md), [BUG-568](../bugs/BUG-568-OPEN.md), [BUG-569](../bugs/BUG-569-OPEN.md), [BUG-570](../bugs/BUG-570-OPEN.md), [BUG-571](../bugs/BUG-571-FIXED.md), [BUG-572](../bugs/BUG-572-OPEN.md), [BUG-573](../bugs/BUG-573-OPEN.md), [BUG-574](../bugs/BUG-574-OPEN.md), [BUG-575](../bugs/BUG-575-OPEN.md), [BUG-576](../bugs/BUG-576-OPEN.md), [BUG-585](../bugs/BUG-585-OPEN.md), [BUG-586](../bugs/BUG-586-OPEN.md), [BUG-587](../bugs/BUG-587-OPEN.md), [BUG-588](../bugs/BUG-588-OPEN.md), [BUG-589](../bugs/BUG-589-OPEN.md), [BUG-590](../bugs/BUG-590-OPEN.md) | Вендорена целиком 2026-07-28 (коммит `35be3b44`, `tests/wpt/html/`, **14161 файл** — крупнейшая вендоренная категория, следующая по размеру `fetch` — 985; 26 подкаталогов верхнего уровня, 10263 `.html`, 1813 `.js`, 523 `.xhtml`, 298 `.png`, 241 `.headers`, 207 `.htm`, 97 Python-эндпоинтов `wptserve`, 61 `.dat` — фикстуры парсера), скоуп ⬜ (кандидат). По `MANIFEST.json` категория даёт **8399 `testharness`-id** (701 с флагом `testdriver`), плюс 1382 `reftest`, 183 `crashtest`, 357 `manual`, для которых у минимального исполнителя пути нет. **Первая категория, где вендоринг и обследование пришлось разделить:** по замеренному темпу срезов ниже полный `--all --root html --recursive` — 40+ часов, в одну сессию не помещается; дерево зафиксировано одним коммитом, обследование идёт по подкаталогам, остаток вынесен явными строками `WPT-VENDOR-html-*` в `ROADMAP.md` (`canvas` 3308 id, `semantics` 2223, `browsers` 759, `webappapis` 353, `editing` 216, `interaction` 192, `rendering` 150, `misc` 556) — молча не выброшен ни один подкаталог. Невендоренные внекатегорийные хелперы — 136 различных путей, самый широкий разрыв в backlog-е (`/resources/testdriver.js`+`testdriver-vendor.js` по 641 ссылке, `/images/green-100x50.png` 460, `/resources/testdriver-actions.js` 443, `/common/slow.py` 202, `/common/utils.js` 197, `/common/get-host-info.sub.js` 197, `/shadow-dom/focus-navigation/resources/focus-utils.js` 130, `/common/dispatcher/dispatcher.js` 130, `/common/media.js` 119, `/common/reftest-wait.js` 113, `/common/blank.html` 93), тот же задокументированный класс, что `beacon`/`fenced-frame`/`fetch`. **Срез `html/dom`** — `run_report.py --all --root html/dom --recursive`, **12 мин 39 с, 192/249 harness OK, 253/4784 сабтестов**: лучшая доля harness OK во всём backlog-е (77 % против 37 % у `fetch` и 1/199 у `fenced-frame`) — подкаталог почти самодостаточен (2 `testdriver`-файла и 2 `.https.` на 478 файлов), так что тесты доходят до собственных утверждений вместо того, чтобы гаснуть на разрывах исполнителя; при этом зелёных целиком тестов всего 13 из 249 — сигнал не в «harness OK», а в 4531 упавшем сабтесте. Раскладка исходов (242 записи `TEST_END`): 185 `Test OK`, 17 `Test TIMEOUT` с частичными сабтестами, 24 TIMEOUT, 12 ERROR, 2 SKIP. Главная находка среза — [BUG-418](../bugs/BUG-418-FIXED.md): все 9 файлов `html/dom/reflection-*.html` (канонический набор рефлексии IDL-атрибутов HTML, ~8000 сабтестов каждый — вместе ~72 000, на порядок больше всего остального среза) отдали `ERROR — WebSocket connection closed` прямо на `browsingContext.navigate`, и pid браузера в логе сменился ровно 9 раз; воспроизведено вне WPT standalone-скриптом против `python -m http.server` и сведено к шести строкам (`for (i=0;i<60000;i++) document.createElement('div')` → `EXIT=127`). Причина — не BiDi-сервер, а паника продакшн-кода: сторож переполнения DOM-арены в шиме проверяет `nid < 0` с явным комментарием «QuickJS converts the Rust u32::MAX sentinel to -1», а дефолтный V8 конвертирует `u32` беззнаково (проба со страницы: `calls=49987 last=4294967295 isNeg=false`), поэтому `QuotaExceededError` не бросается, `4294967295` уходит дальше как NodeId и роняет `self.nodes[4294967295]` внутри `extern "C"`-колбэка V8 → `panic in a function that cannot unwind` → abort всего процесса. Остальные четыре бага — отсутствующие API, каждый подтверждён отдельной пробой `--dump-layout` вне WPT: [BUG-412](../bugs/BUG-412-OPEN.md) `document.getElementsByName` (31 FAIL-сабтест в 4 файлах), [BUG-413](../bugs/BUG-413-OPEN.md) `innerText`/`outerText` (169 сабтестов — `innertext-setter.html` 0/126, `outertext-setter.html` 0/43), [BUG-414](../bugs/BUG-414-FIXED.md) `dataset`/`DOMStringMap` (9 сабтестов; SVG-заглушка `get dataset() { return {} }` не спасает — тест проверяет `instanceof`), [BUG-415](../bugs/BUG-415-OPEN.md) отсоединённый документ без методов `Node` и без `head`/`body` (`Document.body.html` 2/26, где первый же `doc.removeChild` валит 17 сабтестов **до** их собственного утверждения — то есть предмет теста в срезе фактически не проверен). Уже заведённые баги, переподтверждённые срезом: [BUG-351](../bugs/BUG-351-OPEN.md) `insertAdjacentHTML`/`outerHTML` (35 сообщений), [BUG-384](../bugs/BUG-384-FIXED.md) именованный доступ на Window, [BUG-358](../bugs/BUG-358-OPEN.md) `document.compatMode` и соседи. **Срез `html/syntax`** — `run_report.py --all --root html/syntax --recursive`, **53 мин 16 с, 69/379 harness OK, 30/675 сабтестов**, зелёных целиком 2. Раскладка исходов: 176 чистых TIMEOUT, 131 `Test TIMEOUT` с частичными сабтестами, 68 `Test OK`, 2 ERROR (тот же abort из BUG-418), 1 SKIP. Контраст с `html/dom` (18 % против 77 % harness OK) держится не на разрывах исполнителя, а на движке: 61 сообщение `Cannot read properties of null (reading 'document')` — задокументированный предел «`<iframe>` без browsing context» (тот же класс, что `focus`/`hr-time`), 76 — `Cannot set properties of undefined (setting 'innerHTML')`, где `doc` приходит из `createHTMLDocument()` и не имеет `body`, то есть ровно [BUG-415](../bugs/BUG-415-OPEN.md), найденный в предыдущем срезе. Две новых находки: [BUG-416](../bugs/BUG-416-OPEN.md) (`Element.prototype.getElementsByTagName` и `getElementsByTagNameNS` отсутствуют — незакрытый остаток закрытого [BUG-279](../bugs/BUG-279-FIXED.md), 14 сообщений в `parsing/`+`parsing-html-fragments/`) и [BUG-417](../bugs/BUG-417-OPEN.md) — найден **пробой, а не прогоном**: `<template>`, встреченный до появления `<body>`, не даёт парсеру создать `<body>` вовсе (`document.body` === `null`, страница может отрендериться пустой), потому что `</template>` безусловно переводит парсер в `InBody` вместо спекового reset insertion mode; существующий юнит-тест `template_in_head` зелёный, так как про `<body>` не спрашивает. Итог по двум срезам: **261/628 harness OK, 283/5459 сабтестов** на 641 из 8399 id категории (7,6 %); остальные 7758 id вынесены строками `WPT-VENDOR-html-*` в `ROADMAP.md`. **Срез `html/semantics/embedded-content`** (2026-08-04, `WPT-VENDOR-html-semantics-embedded-content`) — крупнейший подсрез `html/semantics` (708 id), прогнан целиком по 14 под-`--root`ам (`media-elements`/`the-iframe-element`/`the-img-element`/`the-canvas-element`/`the-video-element`/`the-object-element`/`the-embed-element`/`the-audio-element`/`bfcache`/`image-maps`/`the-area-element`/`the-frame-element`/`crashtests`/`resources`), суммарно **351/708 harness OK, 333/1487 сабтестов**. Три новых бага (глобалы/методы отсутствуют целиком, не сломанные стабы): [BUG-568](../bugs/BUG-568-OPEN.md) `document.write()`, [BUG-569](../bugs/BUG-569-OPEN.md) `HTMLImageElement.prototype.decode()` (57 сабтестов, весь `the-img-element/`), [BUG-570](../bugs/BUG-570-OPEN.md) `VTTCue`/`TextTrackCue`/`TrackEvent` глобальные конструкторы отсутствуют (22 сабтеста, `media-elements/`) — cue-данные и `TextTrack` JS API при этом реально работают, отсутствует только сам конструктор/интерфейс. Переподтверждены: [BUG-464](../bugs/BUG-464-OPEN.md)/[BUG-477](../bugs/BUG-477-OPEN.md) (`elementFromPoint`), [BUG-478](../bugs/BUG-478-OPEN.md)/[BUG-551](../bugs/BUG-551-OPEN.md) (`getClientRects`), [BUG-449](../bugs/BUG-449-OPEN.md) (`ImageData` не глобал). Доминирующий источник шума в медиа-подкаталогах — НЕ движковый баг: `/common/media.js`+`/common/stringifiers.js` (внекатегорийные хелперы) не вендорены, тот же задокументированный survey-gap, что `FileAPI`/`custom-elements` выше — 404 → `getVideoURI`/`getAudioURI`/`token is not defined` на ~416 сабтестах. `test_driver.Actions is not a constructor` (16 сабтестов, `image-maps`) — известное ограничение минимального executor'а (только `click`, см. §выше "`test_driver.*`"). Гипотеза «упрётся в `<iframe>` без browsing context» (BUG-381/383) подтвердилась на `the-iframe-element` (59/144 harness OK) и заметной доле `the-embed-element`/`the-object-element`. `bfcache/` — 0/6, все 6 TIMEOUT (bfcache для встроенного контента не реализован, ожидаемо на первом проходе). **Срез `html/semantics/scripting-1`** (2026-08-04, `WPT-VENDOR-html-semantics-scripting-1`) — прогнан целиком по 3 под-`--root`ам (`the-noscript-element`/`the-template-element`/`the-script-element`), суммарно **302/455 harness OK, 745/2225 сабтестов**: `the-noscript-element` 1/1 (1/1), `the-template-element` 21/21 (330/660, остаток на пределе «`<iframe>` без browsing context»), `the-script-element` 280/433 (414/1564, 47 TIMEOUT, 177 сообщений `module ... not found` — известный [BUG-446](../bugs/BUG-446-OPEN.md), сетевой ESM-граф). Крупнейшая новая находка — [BUG-571](../bugs/BUG-571-FIXED.md): `<script>`, вставленный в живой документ через `createElement`+`appendChild` (классический или модульный, инлайновый или `src`), никогда не исполняется — классический скрипт запускается одноразовым обходом DOM ровно один раз за навигацию, без аналога алгоритма «prepare a script element», привязанного к вставке узла; объясняет 218 из 575 `FAIL` в одной фикстуре (`script-type-and-language-js`, полный матрикс легаси-MIME-типов и `language=`) и, по всей видимости, большинство `scheduler:`/async-ordering отказов среза; минимум 60 файлов подсреза используют динамическое создание скрипта. Ещё два новых бага: [BUG-572](../bugs/BUG-572-OPEN.md) (`HTMLScriptElement.supports()` отсутствует целиком, 4 сабтеста) и [BUG-573](../bugs/BUG-573-OPEN.md) (`Range.prototype.createContextualFragment` отсутствует, 3 сабтеста). Переподтверждены: [BUG-446](../bugs/BUG-446-OPEN.md), [BUG-568](../bugs/BUG-568-OPEN.md) (`document.write`), класс сессионного повторного использования результатов на `.https.`-тестах (BUG-380), предел `<iframe>` без browsing context (BUG-381/383). **Срез `html/semantics/forms`** (2026-08-04, `WPT-VENDOR-html-semantics-forms`) — прогнан целиком одним `run_report.py --all --root html/semantics/forms --recursive --processes 6` (**3 мин 33 с** — вопреки наивной оценке «~13 ч» в ROADMAP.md, `--processes 6` меняет порядок величины), все 22 подкаталога (`<form>`/`<input>`/`<select>`/`<textarea>`/`<button>`/`<fieldset>`/`<label>`/`constraints`/`customizable-combobox`/`form-submission-0`/`form-submission-target` и др.), суммарно **371/442 harness OK, 1783/4539 сабтестов**. Три новых бага, каждый root-caused отдельной пробой `--dump-layout` вне WPT-прогона: [BUG-574](../bugs/BUG-574-OPEN.md) — крупнейшая находка: `Node.prototype.contains()` отсутствует целиком на любом виде узла (не сломанный стаб — метода вообще нет), проявляется как `elementDocument.contains is not a function` внутри общего хелпера `resources/testdriver.js`'s `getInViewCenterPoint`, используемого `test_driver.click()`/`Actions` (75 сабтестов/36 файлов в одной этой категории); поскольку хелпер общий для всего раннера, а не специфичен для forms, вероятно объясняет необъяснённый `Unhandled rejection`-шум в уже закрытых срезах (задним числом не переаудировано). [BUG-575](../bugs/BUG-575-OPEN.md) — `Element.prototype.localName` отсутствует целиком (только `tagName`/`nodeName` реализованы); вскрыто `form-submission-target/resources/reltester.js`, где `submitter.localName !== "form"` всегда истинно и уводит на неверную ветку (14 сабтестов/2 файла). [BUG-576](../bugs/BUG-576-OPEN.md) — `HTMLOptionsCollection.prototype.add()` (`select.options.add()`) отсутствует при работающем зеркальном `HTMLSelectElement.prototype.add()` (`select.add()`, 4 сабтеста/2 файла). Малый попутный гэп без отдельного BUG-NNN (по образцу `CustomElementRegistry` в `custom-elements`): `window.NodeList` глобальный конструктор не выставлен (`instanceof NodeList` → `ReferenceError`, 9 сабтестов/2 файла). Переподтверждены: BUG-351 (`insertAdjacentHTML`, 91 упоминание), BUG-412 (`getElementsByName`), BUG-416 (`getElementsByTagName`), BUG-446 (сетевой ESM-граф), BUG-568 (`document.write`), предел `test_driver.Actions is not a constructor` (53 упоминания). **Срез `html/browsers`** (2026-08-04, `WPT-VENDOR-html-browsers`) — прогнан целиком по 15 под-`--root`ам (все 8 подкаталогов первого уровня, `browsing-the-web` дополнительно разложен на свои 8), все с `--processes=4`; серийная оценка (files−testdriver)×25с предсказывала 5+ ч, реальный параллельный прогон занял ~50 мин суммарно. Итог: **173/586 harness OK, 69/950 сабтестов**. Гипотеза строки ROADMAP.md подтвердилась частично: `<iframe>` без browsing context (BUG-381/383) и BUG-359 (`window.open` не резолвит относительный URL) действительно доминируют — `sandboxing` 19/19 unexpected целиком на этом, большая часть `browsing-the-web`/`windows`/`the-window-object` тоже. Шесть новых багов нашлись независимо от этого предела: [BUG-585](../bugs/BUG-585-OPEN.md) — `Origin` WebIDL глобал (`Origin.from()`) отсутствует целиком, 181 сабтест в самодостаточном `origin/api/` (единственная зависимость, `resources/serializations.js`, вендорена и грузится штатно); попутно 25 из них также упираются в отсутствующий `SVGAnimatedString.baseVal` на `<a href>`/`<a xlink:href>`. [BUG-586](../bugs/BUG-586-OPEN.md) — `document.domain` не реализован: геттер `undefined` вместо строки, сеттер не валидирует и не бросает `SecurityError` там, где спека требует (повторная установка того же значения, установка на `createHTMLDocument()`/`createDocument()`). [BUG-587](../bugs/BUG-587-OPEN.md) — WindowProxy `[[DefineOwnProperty]]` не защищает unforgeable-свойства `window`/`document`/`location`/`top`: и совместимые, и несовместимые переопределения молча проходят вместо required-семантики (8/8 сабтестов, самодостаточный файл без iframe). [BUG-588](../bugs/BUG-588-OPEN.md) — `window.frameElement` отсутствует целиком: `undefined` вместо `null` даже на топ-уровне без единого iframe — независимо от предела browsing context. [BUG-589](../bugs/BUG-589-OPEN.md) — `window` не полноценный WebIDL exotic object: нет `Symbol.toStringTag` (`[object Object]` вместо `[object Window]`/`[object WindowProperties]`, тот же класс, что BUG-366/BUG-369, но на самом глобале), и indexed `[[DefineOwnProperty]]`/`[[Set]]` не отклоняет несуществующие числовые индексы в strict mode (`window[2**32-2]=1` должен бросать `TypeError` независимо от наличия iframe). [BUG-590](../bugs/BUG-590-OPEN.md) — `document.createEvent` отсутствует целиком; `dispatchEvent(new CustomEvent("beforeunload"))` не вызывает `onbeforeunload`/слушатель. Переподтверждены (без новых номеров): [BUG-376](../bugs/BUG-376-FIXED.md) (`location.protocol=` и другие компоненты не бросают/не навигируют — 47 хитов в `history`), известный класс невендоренных `/common/`-хелперов (`get_host_info`/`RemoteContext`/`token`/`addIframe`/`openWindow`), известный TLS-гэп `UnknownIssuer` на `.https.`-тестах (`offline`, см. `tests/wpt/certs/README.md`). Один воркер `--processes=4` уронился на гонке `--bidi-port`-токена при старте (`history`-прогон) — харнесс-флейк оркестратора, не баг движка; отчёт всё равно посчитан по успевшим завершиться процессам. |
| `html-aam` | ⬜ | ✅ |  | [BUG-599](../bugs/BUG-599-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/html-aam/`, 19 файлов: `META.yml`, `WEB_FEATURES.yml`, 16 тестовых файлов, `LICENSE-WPT.md`), скоуп ⬜ (кандидат, HTML Accessibility API… [Подробности](wpt-vendor-notes/html-aam.md). |
| `html-longdesc` | ⬜ | ✅ |  | [BUG-612](../bugs/BUG-612-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/html-longdesc/`, 27 файлов: `META.yml`, `README.html`, `LICENSE-WPT.md`, 21 тестовый файл (все с суффиксом `-manual` — ручной AT-протокол,… [Подробности](wpt-vendor-notes/html-longdesc.md). |
| `html-media-capture` | 🚫 | ✅ |  | [BUG-613](../bugs/BUG-613-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/html-media-capture/`, 16 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md`, 12 тестовых файлов с суффиксом `-manual`,… [Подробности](wpt-vendor-notes/html-media-capture.md). |
| `html-ruby-extensions` | ⬜ | ✅ |  | [BUG-614](../bugs/BUG-614-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/html-ruby-extensions/`, 195 файлов: `README.md`, `LICENSE-WPT.md`, 84 тестовых `html-ruby-NNN.html`, `reference/` с mismatch-эталонами),… [Подробности](wpt-vendor-notes/html-ruby-extensions.md). |
| `https-upgrades` | ⬜ | ✅ |  | [BUG-359](../bugs/BUG-359-FIXED.md) (переподтверждён) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/https-upgrades/`, 8 файлов: `README`, `resources/pass.html`+`pass-with-referrer.html`, 6 `tentative/*.sub.html` тестов, `LICENSE-WPT.md`).… [Подробности](wpt-vendor-notes/https-upgrades.md). |
| `idle-detection` | ⬜ | ✅ |  | [BUG-615](../bugs/BUG-615-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/idle-detection/`, 20 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md`, `basics.tentative.https.window.js`,… [Подробности](wpt-vendor-notes/idle-detection.md). |
| `imagebitmap-renderingcontext` | ⬜ | ✅ |  | [BUG-616](../bugs/BUG-616-OPEN.md), [BUG-617](../bugs/BUG-617-OPEN.md) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/imagebitmap-renderingcontext/`, 16 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `html-ruby-extensions`,… [Подробности](wpt-vendor-notes/imagebitmap-renderingcontext.md). |
| `images` | ⬜ | ✅ |  |  | Уже вендорена целиком раньше — как разделяемая фикстур-директория для `WPT-RUN-3` slice 22 (`tests/wpt/images/`, 84 файла + `LICENSE-WPT.md`), задача `WPT-VENDOR-images` из бэклога переоткрыта… [Подробности](wpt-vendor-notes/images.md). |
| `import-maps` | ⬜ | ✅ |  | [BUG-446](../bugs/BUG-446-OPEN.md), [BUG-485](../bugs/BUG-485-FIXED.md)/[BUG-565](../bugs/BUG-565-FIXED.md), [BUG-572](../bugs/BUG-572-OPEN.md) (все переподтверждены) | Вендорена целиком 2026-08-04 (коммит `35be3b44`, `tests/wpt/import-maps/`, 94 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `idle-detection`, 57 отобранных тестовых… [Подробности](wpt-vendor-notes/import-maps.md). |
| `inert` | ⬜ | — |  |  |  |
| `input-device-capabilities` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/input-device-capabilities/`, 3 файла: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `idle-detection`, единственный тест… [Подробности](wpt-vendor-notes/input-device-capabilities.md). |
| `input-events` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/input-events/`, 29 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `inert`, 21 тестовый файл + 1 общий… [Подробности](wpt-vendor-notes/input-events.md). |
| `installedapp` | 🚫 | ✅ |  | [BUG-624](../bugs/BUG-624-OPEN.md) (переподтверждён [BUG-480](../bugs/BUG-480-OPEN.md)) | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/installedapp/`, 5 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `idle-detection`,… [Подробности](wpt-vendor-notes/installedapp.md). |
| `intersection-observer` | ⬜ | ✅ |  | [BUG-628](../bugs/BUG-628-OPEN.md), [BUG-626](../bugs/BUG-626-OPEN.md), [BUG-627](../bugs/BUG-627-OPEN.md) (переподтверждены [BUG-368](../bugs/BUG-368-OPEN.md), [BUG-384](../bugs/BUG-384-FIXED.md), [BUG-346](../bugs/BUG-346-OPEN.md), [BUG-482](../bugs/BUG-482-OPEN.md)) | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/intersection-observer/`, 116 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `inert`, тестовые файлы… [Подробности](wpt-vendor-notes/intersection-observer.md). |
| `intervention-reporting` | ⬜ | — |  |  |  |
| `is-input-pending` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/is-input-pending/`, 12 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `input-events`,… [Подробности](wpt-vendor-notes/is-input-pending.md). |
| `jpegxl` | ⬜ | ✅ |  | [BUG-630](../bugs/BUG-630-OPEN.md) (переподтверждены [BUG-485](../bugs/BUG-485-FIXED.md)/[BUG-565](../bugs/BUG-565-FIXED.md), [BUG-380](../bugs/BUG-380-FIXED.md)) | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/jpegxl/`, 85 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `encoding`, 62 тестовых файла — 33… [Подробности](wpt-vendor-notes/jpegxl.md). |
| `js` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/js/`, 32 файла: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `is-input-pending`, `behaviours/` — 3 файла про приватные… [Подробности](wpt-vendor-notes/js.md). |
| `js-self-profiling` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/js-self-profiling/`, 42 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `js`, 20 тестовых файлов верхнего… [Подробности](wpt-vendor-notes/js-self-profiling.md). |
| `keyboard-lock` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/keyboard-lock/`, 7 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `is-input-pending`,… [Подробности](wpt-vendor-notes/keyboard-lock.md). |
| `keyboard-map` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/keyboard-map/`, 13 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `keyboard-lock`,… [Подробности](wpt-vendor-notes/keyboard-map.md). |
| `largest-contentful-paint` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/largest-contentful-paint/`, 78 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `keyboard-map`, 68 тестовых… [Подробности](wpt-vendor-notes/largest-contentful-paint.md). |
| `layout-instability` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/layout-instability/`, 87 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `element-timing`, 82 тестовых… [Подробности](wpt-vendor-notes/layout-instability.md). |
| `loading` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/loading/`, 119 файлов: `LICENSE-WPT.md` скопирован из соседней `js`, `early-hints/` — 47 `.h2.`-тестов HTTP/2 103 Early Hints (39… [Подробности](wpt-vendor-notes/loading.md). |
| `long-animation-frame` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/long-animation-frame/`, 57 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `loading`, 40 тестовых файлов… [Подробности](wpt-vendor-notes/long-animation-frame.md). |
| `longtask-timing` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/longtask-timing/`, 30 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `long-animation-frame`, 21 тестовый… [Подробности](wpt-vendor-notes/longtask-timing.md). |
| `magnetometer` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/magnetometer/`, 16 файлов: `META.yml`, `WEB_FEATURES.yml`, `idlharness.https.window.js`, 9 тестовых файлов верхнего уровня (7 `.https.`),… [Подробности](wpt-vendor-notes/magnetometer.md). |
| `managed` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/managed/`, 4 файла: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `magnetometer`, 2 тестовых файла верхнего уровня (оба `.https.`),… [Подробности](wpt-vendor-notes/managed.md). |
| `mathml` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mathml/`, 891 файл: `META.yml`, `README.md`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `managed`, `crashtests/` (252 id,… [Подробности](wpt-vendor-notes/mathml.md). |
| `measure-memory` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/measure-memory/`, 32 файла: `META.yml`, `README.md`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mathml`, 16 тестовых… [Подробности](wpt-vendor-notes/measure-memory.md). |
| `media` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/media/`, 39 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `measure-memory`, 37 общих медиа-фикстур —… [Подробности](wpt-vendor-notes/media.md). |
| `media-capabilities` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/media-capabilities/`, 10 файлов), включена несмотря на скоуп 🚫 по тому же постоянному решению пользователя, что… [Подробности](wpt-vendor-notes/media-capabilities.md). |
| `media-playback-quality` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/media-playback-quality/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `media`,… [Подробности](wpt-vendor-notes/media-playback-quality.md). |
| `media-source` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/media-source/`, 158 файлов, 3.0 МБ: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `media-playback-quality`, ~70… [Подробности](wpt-vendor-notes/media-source.md). |
| `mediacapture-extensions` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-extensions/`, 6 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `media-source`, 5 тестовых файлов верхнего уровня,… [Подробности](wpt-vendor-notes/mediacapture-extensions.md). |
| `mediacapture-fromelement` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-fromelement/`, 11 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-extensions`, 9… [Подробности](wpt-vendor-notes/mediacapture-fromelement.md). |
| `mediacapture-handle` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-handle/`, 2 файла: `LICENSE-WPT.md` скопирован из соседней `mediacapture-fromelement`,… [Подробности](wpt-vendor-notes/mediacapture-handle.md). |
| `mediacapture-image` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-image/`, 24 файла: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-handle`, 22 тестовых файла +… [Подробности](wpt-vendor-notes/mediacapture-image.md). |
| `mediacapture-insertable-streams` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-insertable-streams/`, 13 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-image`,… [Подробности](wpt-vendor-notes/mediacapture-insertable-streams.md). |
| `mediacapture-record` | 🚫 | ✅ |  | BUG-634 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-record/`, 27 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-fromelement`, 15… [Подробности](wpt-vendor-notes/mediacapture-record.md). |
| `mediacapture-region` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-region/`, 3 файла: `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-image`, единственный тест… [Подробности](wpt-vendor-notes/mediacapture-region.md). |
| `mediacapture-streams` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediacapture-streams/`, 73 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md`, 39 корневых тестовых файлов включая… [Подробности](wpt-vendor-notes/mediacapture-streams.md). |
| `mediasession` | 🚫 | ✅ |  | BUG-636 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mediasession/`, 10 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `mediacapture-streams`, `README.md`,… [Подробности](wpt-vendor-notes/mediasession.md). |
| `merchant-validation` | 🚫 | ✅ |  | BUG-637 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/merchant-validation/`, 5 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `mediasession`, 4 корневых тестовых файла:… [Подробности](wpt-vendor-notes/merchant-validation.md). |
| `mimesniff` | ⬜ | ✅ |  | BUG-638 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mimesniff/`, 34 файла: `META.yml`, `README.md`, `LICENSE-WPT.md` скопирован из соседней `mediasession`, `media/`, `mime-types/`,… [Подробности](wpt-vendor-notes/mimesniff.md). |
| `mixed-content` | ⬜ | 🟡 |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mixed-content/`, 533 файла: `META.yml`, `README.md`, `LICENSE-WPT.md`, `WEB_FEATURES.yml`, `gen/` — 388 сгенерированных тестовых HTML через… [Подробности](wpt-vendor-notes/mixed-content.md). |
| `mst-content-hint` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/mst-content-hint/`, 5 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `mixed-content`, 3 корневых тестовых файла:… [Подробности](wpt-vendor-notes/mst-content-hint.md). |
| `nav-tracking-mitigations` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/nav-tracking-mitigations/`, 9 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `element-timing`, `resources/` — 5 файлов, 2… [Подробности](wpt-vendor-notes/nav-tracking-mitigations.md). |
| `navigation-api` | ⬜ | ✅ |  | BUG-639 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/navigation-api/`, 493 файла, `LICENSE-WPT.md` скопирован из соседней `mimesniff`). API реально реализован (`window.navigation`), не… [Подробности](wpt-vendor-notes/navigation-api.md). |
| `navigation-timing` | ⬜ | ✅ |  | BUG-640 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/navigation-timing/`, 82 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `navigation-api`, `nav2-*`/`test-*`… [Подробности](wpt-vendor-notes/navigation-timing.md). |
| `netinfo` | ⬜ | ✅ |  | BUG-641 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/netinfo/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `navigation-timing`, `idlharness.any.js`,… [Подробности](wpt-vendor-notes/netinfo.md). |
| `network-error-logging` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/network-error-logging/`, 30 файлов: `META.yml`, `README.md`, `LICENSE-WPT.md` скопирован из соседней `netinfo`, 12 корневых тестовых… [Подробности](wpt-vendor-notes/network-error-logging.md). |
| `notifications` | ⬜ | ✅ |  | [BUG-642](../bugs/BUG-642-OPEN.md) | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/notifications/`, 46 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md`, 41 корневой тестовый/скриптовый файл, `resources/`).… [Подробности](wpt-vendor-notes/notifications.md). |
| `orientation-event` | 🚫 | ✅ |  | BUG-643 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/orientation-event/`, 41 файл: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `notifications`,… [Подробности](wpt-vendor-notes/orientation-event.md). |
| `orientation-sensor` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/orientation-sensor/`, 22 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `orientation-event`,… [Подробности](wpt-vendor-notes/orientation-sensor.md). |
| `page-lifecycle` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/page-lifecycle/`, 10 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `orientation-sensor`,… [Подробности](wpt-vendor-notes/page-lifecycle.md). |
| `page-visibility` | ⬜ | — |  |  |  |
| `paint-timing` | ⬜ | ✅ |  | BUG-645 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/paint-timing/`, 65 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `largest-contentful-paint`, `fcp-only/`,… [Подробности](wpt-vendor-notes/paint-timing.md). |
| `parakeet` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/parakeet/`, 3 файла: `META.yml`, `createAdRequest.tentative.https.sub.window.js`, `finalizeAd.tentative.https.sub.window.js`,… [Подробности](wpt-vendor-notes/parakeet.md). |
| `payment-method-basic-card` | 🚫 | ✅ |  | BUG-646 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/payment-method-basic-card/`, 7 файлов: `META.yml`, `apply_the_modifiers.html`, `billing-address-is-null-manual.https.html`,… [Подробности](wpt-vendor-notes/payment-method-basic-card.md). |
| `payment-method-id` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/payment-method-id/`, 1 файл: `META.yml`, `payment-request-ctor-pmi-handling.https.sub.html`; `LICENSE-WPT.md` скопирован из соседней… [Подробности](wpt-vendor-notes/payment-method-id.md). |
| `payment-request` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/payment-request/`, 76 файлов: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `payment-method-id`,… [Подробности](wpt-vendor-notes/payment-request.md). |
| `performance-timeline` | ⬜ | ✅ |  | BUG-648 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/performance-timeline/`, 70 файлов; `LICENSE-WPT.md` скопирован из соседней `navigation-timing`). `run_report.py --all --root… [Подробности](wpt-vendor-notes/performance-timeline.md). |
| `periodic-background-sync` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (`tests/wpt/periodic-background-sync/`, 2 тестовых файла + `service_workers/sw.js`), Service Worker расширение — фоновая ОС-интеграция. Прогон `run_report.py --all --root… [Подробности](wpt-vendor-notes/periodic-background-sync.md). |
| `permissions` | ⬜ | ✅ |  | BUG-649 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions/`, 18 файлов: `META.yml`, `WEB_FEATURES.yml`, `all-permissions.html`, `crashtests/` — 2 файла, `edge-cases.https.html`,… [Подробности](wpt-vendor-notes/permissions.md). |
| `permissions-policy` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions-policy/`, 79 файлов: `META.yml`, `README.md`, `WEB_FEATURES.yml`, ~55 корневых… [Подробности](wpt-vendor-notes/permissions-policy.md). |
| `permissions-request` | ⬜ | ✅ |  | BUG-650,BUG-651 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions-request/`, 3 файла: `META.yml`, `idlharness.any.js`, `LICENSE-WPT.md` скопирован из соседней `permissions-policy`).… [Подробности](wpt-vendor-notes/permissions-request.md). |
| `permissions-revoke` | ⬜ | ✅ |  | BUG-652 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions-revoke/`, 3 файла: `META.yml`, `idlharness.any.js`, `LICENSE-WPT.md` скопирован из соседней `permissions-request`).… [Подробности](wpt-vendor-notes/permissions-revoke.md). |
| `picture-in-picture` | 🚫 | ✅ |  | BUG-653 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/picture-in-picture/`, 24 файла: `META.yml`, `WEB_FEATURES.yml`, 17 тестовых файлов, `resources/` — 4 хелпера, `LICENSE-WPT.md` скопирован… [Подробности](wpt-vendor-notes/picture-in-picture.md). |
| `png` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/png/`, 11 файлов: `META.yml`, `WEB_FEATURES.yml`, `cICP-wins.html`/`cICP-wins-ref.html`, `cicp-chunk.html`, `exif-chunk.html`,… [Подробности](wpt-vendor-notes/png.md). |
| `pointerevents` | ⬜ | — |  |  |  |
| `pointerlock` | ⬜ | ✅ |  | BUG-655 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/pointerlock/`, 30 файлов: `META.yml`, `WEB_FEATURES.yml`, 27 тестовых файлов (7 `-manual.html` не запускаются `run_report.py`-фильтром),… [Подробности](wpt-vendor-notes/pointerlock.md). |
| `preload` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/preload/`, 126 файлов: `META.yml`, `WEB_FEATURES.yml`, ~65 корневых… [Подробности](wpt-vendor-notes/preload.md). |
| `presentation-api` | 🚫 | — |  |  | медиа/casting API |
| `print` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/print/`, 2 файла: `WEB_FEATURES.yml` + `crashtests/reload-crash.html`; `LICENSE-WPT.md` скопирован из соседней `preload`). Категория… [Подробности](wpt-vendor-notes/print.md). |
| `private-aggregation` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/private-aggregation/`, 17 файлов: 3 `private-aggregation-permissions-policy-*.https.sub.html` + 2 `.headers`, 6… [Подробности](wpt-vendor-notes/private-aggregation.md). |
| `private-click-measurement` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/private-click-measurement/`, 2 файла: `WEB_FEATURES.yml` + `idlharness.window.js`; `LICENSE-WPT.md` скопирован из соседней… [Подробности](wpt-vendor-notes/private-click-measurement.md). |
| `proximity` | 🚫 | — |  |  | датчик устройства |
| `push-api` | 🚫 | — |  |  | Push-уведомления — нужен пуш-сервис |
| `quirks` | ⬜ | ✅ |  | BUG-658 | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/quirks/`, 72 файла: `META.yml`, `hashless-hex-color/`, `unitless-length/`, `crashtests/`, `historical/`, `support/`, ~20 корневых тестов;… [Подробности](wpt-vendor-notes/quirks.md). |
| `referrer-policy` | ⬜ | — |  |  |  |
| `remote-playback` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/remote-playback/`, 22 файла; `LICENSE-WPT.md` скопирован из соседней `push-api`), включена несмотря на скоуп 🚫 (медиа-конвейер) по… [Подробности](wpt-vendor-notes/remote-playback.md). |
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
| `scroll-animations` | ⬜ | ✅ |  | BUG-670 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/scroll-animations/`, 283 файла: `animation-trigger/`, `css/`, `crashtests/`, `scroll-timelines/`, `view-timelines/`). Внекатегорийные… [Подробности](wpt-vendor-notes/scroll-animations.md). |
| `scroll-performance-timing` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/scroll-performance-timing/tentative/supported-entry-types.window.js`, 1 файл). Прогон `run_report.py --all --root scroll-performance-timing… [Подробности](wpt-vendor-notes/scroll-performance-timing.md). |
| `scroll-to-text-fragment` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/scroll-to-text-fragment/`, 62 файла). Прогон `run_report.py --all --root scroll-to-text-fragment --recursive` (~1:39, 39 отобранных id, 25… [Подробности](wpt-vendor-notes/scroll-to-text-fragment.md). |
| `secure-contexts` | ⬜ | ✅ |  | BUG-591 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/secure-contexts/`, 21 файл: `basic-dedicated-worker(.https).html`, `basic-popup-and-iframe-tests(.https).html(.js)`,… [Подробности](wpt-vendor-notes/secure-contexts.md). |
| `secure-payment-confirmation` | 🚫 | ✅ |  | BUG-646 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/secure-payment-confirmation/`, 28 файлов: `META.yml`, `WEB_FEATURES.yml`, 20 `.https.html` тестов (`authentication-*`, `constructor*`,… [Подробности](wpt-vendor-notes/secure-payment-confirmation.md). |
| `selection` | ⬜ | ✅ |  | BUG-671 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/selection/`, 100 файлов: `anonymous/`, `bidi/`, `caret/`, `contenteditable/`, `shadow-dom/`, `textcontrols/`). Прогон `run_report.py --all… [Подробности](wpt-vendor-notes/selection.md). |
| `serial` | 🚫 | ✅ |  | [BUG-672](../bugs/BUG-672-OPEN.md) | аппаратный API (Serial). Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/serial/`, 18 файлов). `run_report.py --all --root serial --recursive` — ~3:51, 10 selected ids (5… [Подробности](wpt-vendor-notes/serial.md). |
| `server-timing` | ⬜ | — |  |  |  |
| `service-workers` | ⬜ | ✅ |  | [BUG-674](../bugs/BUG-674-OPEN.md), [BUG-675](../bugs/BUG-675-OPEN.md) | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/service-workers/`, 794 файла: `cache-storage/`, `service-worker/`). `run_report.py --all --root service-workers --recursive --processes=4`… [Подробности](wpt-vendor-notes/service-workers.md). |
| `shadow-dom` | ⬜ | ✅ |  | BUG-676 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/shadow-dom/`, 393 файла: `crashtests/`, `declarative/`, `focus/`, `focus-navigation/`, `leaktests/`, `reference/`, `reference-target/`,… [Подробности](wpt-vendor-notes/shadow-dom.md). |
| `shape-detection` | ⬜ | ✅ |  | [BUG-677](../bugs/BUG-677-OPEN.md) | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/shape-detection/`, 26 файлов). `run_report.py --all --root shape-detection --recursive --processes=4` — 2:16, 23 отобранных id, все… [Подробности](wpt-vendor-notes/shape-detection.md). |
| `shared-storage` | 🚫 | — |  |  | ad-tech (Privacy Sandbox) |
| `shared-storage-selecturl-limit` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/shared-storage-selecturl-limit/`, 10 файлов: 7 корневых `.tentative.https.sub.html`, `resources/`). `run_report.py --all --root… [Подробности](wpt-vendor-notes/shared-storage-selecturl-limit.md). |
| `signed-exchange` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/signed-exchange/`, 129 файлов: `reporting/`, `resources/`, `service-workers/`, `subresource/`). `run_report.py --all --root signed-exchange… [Подробности](wpt-vendor-notes/signed-exchange.md). |
| `soft-navigation-heuristics` | ⬜ | ✅ |  | [BUG-678](../bugs/BUG-678-OPEN.md) | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/soft-navigation-heuristics/`, 104 файла: `detection/`, `dom/`, `history/`, `icp/`, `lcp/`, `resources/`, `smoke/`). `run_report.py --all… [Подробности](wpt-vendor-notes/soft-navigation-heuristics.md). |
| `speculation-rules` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/speculation-rules/`, 408 файлов: `activation-header/`, `prefetch/`, `prerender/`, `prerender-until-script/`, `resources/`,… [Подробности](wpt-vendor-notes/speculation-rules.md). |
| `speech-api` | 🚫 | — |  |  | нет речевого движка |
| `storage` | ⬜ | — |  |  |  |
| `storage-access-api` | ⬜ | — |  |  |  |
| `streams` | ⬜ | ✅ |  | BUG-684 | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/streams/`, 122 файла: `META.yml`, `WEB_FEATURES.yml`, `LICENSE-WPT.md` скопирован из соседней `js`, `idlharness.any.js`, `piping/`,… [Подробности](wpt-vendor-notes/streams.md). |
| `subapps` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/subapps/`, 9 файлов: `resources/subapps-helpers.js`, 7 корневых `.tentative.` тестов, `idlharness.tentative.https.window.js`).… [Подробности](wpt-vendor-notes/subapps.md). |
| `subresource-integrity` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/subresource-integrity/`, 44 файла: `integrity-policy/`, `signatures/`(включая `tentative/`), `unencoded-digest/tentative/`, `tools/`; плюс… [Подробности](wpt-vendor-notes/subresource-integrity.md). |
| `svg` | ⬜ | ✅ |  | [BUG-685](../bugs/BUG-685-OPEN.md) | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/svg/`, 2149 файлов + один недостающий довендоренный shared-хелпер `/resources/SVGAnimationTestCase-testharness.js`, 182 ссылки). Невендоренные… [Подробности](wpt-vendor-notes/svg.md). |
| `svg-aam` | ⬜ | ✅ |  | [BUG-686](../bugs/BUG-686-OPEN.md) | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/svg-aam/`, 8 файлов: `META.yml`, `name/` — 3 `comp_*.html`, `role/` — 4 `roles*.html`/`role-img.tentative.html`). Внекатегорийный хелпер… [Подробности](wpt-vendor-notes/svg-aam.md). |
| `timing-entrytypes-registry` | ⬜ | ✅ |  | [BUG-687](../bugs/BUG-687-OPEN.md) | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/timing-entrytypes-registry/`, 4 файла: `META.yml`, `registry.any.js`, `registry.window.js`, `resources/utils.js`). Прогон `run_report.py --all… [Подробности](wpt-vendor-notes/timing-entrytypes-registry.md). |
| `top-level-storage-access-api` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/top-level-storage-access-api/`, 3 файла: `META.yml`, `requestStorageAccessFor-insecure.sub.window.js`,… [Подробности](wpt-vendor-notes/top-level-storage-access-api.md). |
| `touch-events` | ⬜ | ✅ |  | [BUG-688](../bugs/BUG-688-OPEN.md) | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/touch-events/`, 17 файлов: `META.yml`/`WEB_FEATURES.yml`, `historical.html`, `idlharness.window.js`, `multi-touch-*`,… [Подробности](wpt-vendor-notes/touch-events.md). |
| `trust-tokens` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-06 (пин `35be3b44`, `tests/wpt/trust-tokens/`, 5 файлов: `META.yml`, `trust-token-parameter-validation(-xhr).tentative.https.html`,… [Подробности](wpt-vendor-notes/trust-tokens.md). |
| `trusted-types` | ⬜ | ✅ |  | [BUG-689](../bugs/BUG-689-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/trusted-types/`, 339 файлов: `META.yml`, root-level конструктор/атрибутные/CSP-тесты, `resources/`, `support/`). Прогон `run_report.py --all… [Подробности](wpt-vendor-notes/trusted-types.md). |
| `ua-client-hints` | ⬜ | ✅ |  | [BUG-690](../bugs/BUG-690-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/ua-client-hints/`, 4 файла: `META.yml`, `WEB_FEATURES.yml`, `idlharness.https.any.js`, `useragentdata.https.any.js`). Прогон `run_report.py… [Подробности](wpt-vendor-notes/ua-client-hints.md). |
| `uievents` | ⬜ | ✅ |  | [BUG-691](../bugs/BUG-691-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/uievents/`, 146 файлов: `META.yml`/`WEB_FEATURES.yml`, `click/`, `constructors/`, `hierarchy/`, `interface/`, `keyboard/`,… [Подробности](wpt-vendor-notes/uievents.md). |
| `upgrade-insecure-requests` | ⬜ | ✅ |  | [BUG-692](../bugs/BUG-692-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/upgrade-insecure-requests/`, 254 файла: `META.yml`/`README.md`/`WEB_FEATURES.yml`, `gen/` (196 сгенерированных `.https.html`, зависят от… [Подробности](wpt-vendor-notes/upgrade-insecure-requests.md). |
| `url` | ⬜ | ✅ |  | [BUG-693](../bugs/BUG-693-OPEN.md), [BUG-694](../bugs/BUG-694-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/url/`, 50 файлов + внекатегорийный `/common/subset-tests-by-key.js`, довендорен в `tests/wpt/common/`). Прогон `run_report.py --all --root url… [Подробности](wpt-vendor-notes/url.md). |
| `urlpattern` | ⬜ | ✅ |  | [BUG-695](../bugs/BUG-695-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/urlpattern/`, 12 файлов: `META.yml`/`WEB_FEATURES.yml`, `resources/` — 4 JSON/JS датасета, 8 корневых тестовых файлов). Самопитающаяся… [Подробности](wpt-vendor-notes/urlpattern.md). |
| `user-timing` | ⬜ | ✅ |  | [BUG-696](../bugs/BUG-696-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/user-timing/`, 44 файла + внекатегорийный `/common/performance-timeline-utils.js`, довендорен в `tests/wpt/common/`). Самопитающаяся категория… [Подробности](wpt-vendor-notes/user-timing.md). |
| `vibration` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/vibration/`, 16 файлов, 11 `-manual`), включена несмотря на скоуп 🚫 (аппаратный API вибро). Прогон `run_report.py --all --root vibration --recursive`: 4… [Подробности](wpt-vendor-notes/vibration.md). |
| `video-rvfc` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/video-rvfc/`, 8 файлов), включена несмотря на скоуп 🚫 (медиа-конвейер). Прогон `run_report.py --all --root video-rvfc --recursive`: 8 отобранных id, **2/8… [Подробности](wpt-vendor-notes/video-rvfc.md). |
| `viewport` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/viewport/`, 3 файла: `META.yml`, `WEB_FEATURES.yml`, `viewport-segments.html` — апстримная категория состоит из ровно одного теста).… [Подробности](wpt-vendor-notes/viewport.md). |
| `viewport-segments` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/viewport-segments/`, 3 файла: `viewport-segments-change-event.https.html`, `viewport-segments-env-variables.https.html`,… [Подробности](wpt-vendor-notes/viewport-segments.md). |
| `virtual-keyboard` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/virtual-keyboard/`, 6 файлов, 2 `-manual`), включена несмотря на скоуп 🚫 (мобильная ОС-интеграция). Прогон `run_report.py --all --root virtual-keyboard… [Подробности](wpt-vendor-notes/virtual-keyboard.md). |
| `visual-viewport` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/visual-viewport/`, 32 файла: `META.yml`/`WEB_FEATURES.yml`, 10 `-manual` — исключены отбором, `viewport_support.js`, 19 тестовых файлов).… [Подробности](wpt-vendor-notes/visual-viewport.md). |
| `wai-aria` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/wai-aria/`, 274 файла: `role/` — 20 файлов ролей, `checked`/`pressed` — 2 файла состояния, `aria-actions/` — 4 `.tentative.html` (черновая ARIA Actions… [Подробности](wpt-vendor-notes/wai-aria.md). |
| `wasm` | ⬜ | ✅ |  | [BUG-700](../bugs/BUG-700-OPEN.md), [BUG-699](../bugs/BUG-699-OPEN.md) | Вендорена целиком 2026-08-09 (пинованный коммит `35be3b44`, `git sparse-checkout add wasm`, `tests/wpt/wasm/`, 830 файлов — `core/` 530 (включая проповедальные подкаталоги… [Подробности](wpt-vendor-notes/wasm.md). |
| `web-animations` | ⬜ | ✅ |  | [BUG-704](../bugs/BUG-704-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-animations/`, 236 файлов: `animation-model/`, `animation-trigger/`, `crashtests/`, `interfaces/`, `resources/`, `responsive/`,… [Подробности](wpt-vendor-notes/web-animations.md). |
| `web-based-payment-handler` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/web-based-payment-handler/`, 33 файла, 6 `-manual`), включена несмотря на скоуп 🚫 (Payment Handler API — service-worker-based регистрация платёжного… [Подробности](wpt-vendor-notes/web-based-payment-handler.md). |
| `web-bundle` | ⬜ | ✅ |  | [BUG-705](../bugs/BUG-705-OPEN.md) | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-bundle/`, 116 файлов, `LICENSE-WPT.md` скопирован из `web-based-payment-handler`; в крейтах нет вообще никакой реализации — `grep… [Подробности](wpt-vendor-notes/web-bundle.md). |
| `web-extensions` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-extensions/`, 13 upstream-файлов: `META.yml` + 6 `browser.<api>.extension.js` тест-файлов —… [Подробности](wpt-vendor-notes/web-extensions.md). |
| `web-install` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-install/`, 4 тестовых файла + `resources/navigator-install-iframe-helper.html` + `WEB_FEATURES.yml`), включена несмотря на скоуп 🚫… [Подробности](wpt-vendor-notes/web-install.md). |
| `web-locks` | ⬜ | ✅ |  |  | Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-locks/`, 47 файлов: `bfcache/`, `crashtests/`, `resources/`, `LICENSE-WPT.md`, `META.yml`, `README.md`, `WEB_FEATURES.yml`). Ноль… [Подробности](wpt-vendor-notes/web-locks.md). |
| `web-nfc` | 🚫 | ✅ |  |  | аппаратный API (NFC). Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-nfc/`, 20 файлов: `META.yml`, `WEB_FEATURES.yml`, `README.md`, `resources/`, 11 не-`-manual` корневых тестов, 4… [Подробности](wpt-vendor-notes/web-nfc.md). |
| `web-otp` | 🚫 | ✅ |  |  | WebOTP (SMS). Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-otp/`, `META.yml`+`WEB_FEATURES.yml`+`idlharness.https.window.js`, `LICENSE-WPT.md` скопирован из `web-locks`), включена… [Подробности](wpt-vendor-notes/web-otp.md). |
| `web-share` | ⬜ | ✅ |  |  | Web Share API, Phase 1 стаб (`navigator.share`/`.canShare` всегда rejected/`false`, `crates/js/src/dom.rs:13350-13354`). Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-share/`, 27… [Подробности](wpt-vendor-notes/web-share.md). |
| `webaudio` | ⬜ | ✅ |  | BUG-707, BUG-708 | Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webaudio/`, 415 файлов). Прогон `run_report.py --all --root webaudio --recursive --processes=4` (~7 мин): 321 отобранный id — **205/321… [Подробности](wpt-vendor-notes/webaudio.md). |
| `webauthn` | 🚫 | ✅ |  | BUG-709 | WebAuthn — отдельная крипто/платформенная интеграция. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webauthn/`, 60 файлов, `LICENSE-WPT.md` скопирован из `webaudio`). 53/58 тестовых… [Подробности](wpt-vendor-notes/webauthn.md). |
| `webcodecs` | 🚫 | ✅ |  |  | Вендорена целиком 2026-08-09 (`tests/wpt/webcodecs/`, 134 файла), нет аппаратного/софт кодек-конвейера. `run_report.py --all --root webcodecs --recursive --processes=4` (~13 мин, 126 отобранных id):… [Подробности](wpt-vendor-notes/webcodecs.md). |
| `webdriver` | 🚫 | ✅ |  |  | тестовая инфраструктура самого WPT/WebDriver, не веб-фича сайта. Вендорена целиком 2026-08-09 (`tests/wpt/webdriver/`, 942 файла, `LICENSE-WPT.md` скопирован из `webauthn`): 889 `.py` протокольных… [Подробности](wpt-vendor-notes/webdriver.md). |
| `webgl` | ⬜ | ✅ |  | [BUG-711](../bugs/BUG-711-OPEN.md) | Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webgl/`, 12 файлов). `run_report.py --all --root webgl --recursive` (~38с, 8 отобранных id): 7/8 harness OK, 7/15 сабтестов. Контекст —… [Подробности](wpt-vendor-notes/webgl.md). |
| `webgpu` | 🚫 | ✅ |  | [BUG-712](../bugs/BUG-712-OPEN.md) | Скоуп 🚫 не по причине пустого движка — `crates/js/src/webgpu.rs`… [Подробности](wpt-vendor-notes/webgpu.md). |
| `webhid` | 🚫 | ✅ |  | [BUG-713](../bugs/BUG-713-OPEN.md) | аппаратный API (HID), но `crates/js/src/webhid.rs` реально реализует Phase 0 заглушку. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webhid/`, 11 файлов). `run_report.py --all --root webhid --recursive` (~1:35, 5 отобранных id, все `.https.`): 0/5 harness OK, все TIMEOUT на… [Подробности](wpt-vendor-notes/webhid.md). |
| `webidl` | ⬜ | ✅ |  | BUG-714, BUG-715 | Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webidl/`, 51 файл, `LICENSE-WPT.md` скопирован из `webhid`). `run_report.py --all --root webidl --recursive` (~1:43, 45 отобранных id, ноль `.https.`/testdriver/variant): **37/45 harness OK, 134/324 сабтестов** — одна из самых чистых по сигналу категорий бэклога. [Подробности](wpt-vendor-notes/webidl.md). |
| `webmcp` | 🚫 | ✅ |  |  | Экспериментальное предложение (`navigator.modelContext`), в движке не реализовано вовсе. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webmcp/`, 61 файл, `LICENSE-WPT.md` скопирован из `webidl`). `run_report.py --all --root webmcp --recursive --processes=4` (~6:10, 50 отобранных id, 49 `.https.`): **1/50 harness OK, 1/1 сабтестов** — только `non-secure.html`, остальные TIMEOUT на TLS-гэпе BUG-657. Живая проба подтвердила API отсутствует целиком (не частичная утечка, как у BUG-712/713) — новый номер не заводился. [Подробности](wpt-vendor-notes/webmcp.md). |
| `webmessaging` | ⬜ | ✅ |  | BUG-717, BUG-718 | Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webmessaging/`, 171 файл, `LICENSE-WPT.md` скопирован из `webmcp`; довендорены 7 внекатегорийных зависимостей, ранее 404). `run_report.py --all --root webmessaging --recursive` (~11:30, 136 отобранных id): **77/136 harness OK, 82/206 сабтестов**. Основная масса — TLS-гэп BUG-657 и отсутствие browsing context у iframe/window.open (BUG-480/359). Найдены [BUG-717](bugs/BUG-717-OPEN.md) (`window.postMessage` без клонирования и без валидации targetOrigin) и [BUG-718](bugs/BUG-718-OPEN.md) (`BroadcastChannel.postMessage` через `JSON.stringify` вместо structuredClone). [Подробности](wpt-vendor-notes/webmessaging.md). |
| `webmidi` | 🚫 | ✅ |  | BUG-719, BUG-720 | Скоуп 🚫 не по причине пустого движка — `crates/js/src/web_midi.rs` реально реализует Phase 0 заглушку. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webmidi/`, 1 файл). `run_report.py --all --root webmidi --recursive` (~1:30, 1 отобранный id): 0/1 harness OK, единственный TIMEOUT — уже задокументированный гэп невендоренных WebIDLParser.js/idlharness.js. Найдены [BUG-719](../bugs/BUG-719-OPEN.md) (guard-less конструкторы) и [BUG-720](../bugs/BUG-720-OPEN.md) (нет MIDIInputMap/MIDIOutputMap). [Подробности](wpt-vendor-notes/webmidi.md). |
| `webnn` | 🚫 | ✅ |  |  | Скоуп 🚫 подтверждён — нет ML-инференс рантайма, никакого `navigator.ml`/`MLContext`/`MLGraph` нет ни в `crates/`, ни живьём (в отличие от `webmidi`/`webgpu`/`webhid`, у которых был Phase 0 стаб). Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webnn/`, 196 файлов: `conformance_tests/` 111 + `validation_tests/` 64, все `.https.any.js` с фан-аутом `?cpu`/`?gpu`/`?npu`, ≈520 инстансов). Полный прогон не доведён до конца — выборка 62/62 id (`--processes=4`) дала нулевой разброс на TLS-гэпе `UnknownIssuer` (BUG-657), экстраполяция полного прогона ≈2.5ч ради переподтверждения того же гэпа признана нерентабельной (методология `mixed-content`). Новый BUG-NNN не заводился. [Подробности](wpt-vendor-notes/webnn.md). |
| `webrtc` | 🚫 | ✅ |  | BUG-721 | Скоуп 🚫 (ROADMAP-заметка «нет конвейера» была неточна — `crates/js/src/webrtc_stub.rs` реально реализует mDNS-only заглушку приватности §9D.5, тот же класс дрейфа, что у `webmidi`). Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc/`, 259 файлов, 224 id по глобу). `run_report.py --all --root webrtc --recursive --processes=4` (~17:42, 258 инстансов после variant-фан-аута): **102/258 harness OK, 86/1126 сабтестов**. 112 исходов — уже задокументированный TLS-гэп BUG-657. Доминирующая находка — [BUG-721](../bugs/BUG-721-OPEN.md) (`setConfiguration`/`getConfiguration` отсутствуют, конструктор не валидирует `RTCConfiguration`), 129/1040 непройденных сабтестов. [Подробности](wpt-vendor-notes/webrtc.md). |
| `webrtc-encoded-transform` | 🚫 | ✅ |  |  | Скоуп 🚫 подтверждён точно (в отличие от родительской `webrtc`) — `RTCRtpScriptTransform`/`SFrameEncrypterStream`/`SFrameDecrypterStream`/`RTCRtpSFrameEncrypter` отсутствуют в движке целиком, `webrtc_stub.rs` не реализует ни один из них. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-encoded-transform/`, 62 файла). `run_report.py --all --root webrtc-encoded-transform --recursive --processes=4` (~8:11, 38 id): **7/38 harness OK, 0/21 сабтестов**. 27 TIMEOUT (в основном уже задокументированный TLS-гэп BUG-657) + 4 ERROR (сессионное переиспользование результатов, BUG-380). Все 21 исполнившихся сабтестов падают на `ReferenceError: <API> is not defined` — новых багов не заведено. [Подробности](wpt-vendor-notes/webrtc-encoded-transform.md). |
| `webrtc-extensions` | 🚫 | ✅ |  |  | Скоуп 🚫 «нет конвейера». Вендорена + прогнана целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-extensions/`, 13 файлов, 10 id). `run_report.py --all --root webrtc-extensions --recursive` (~52с): **6/10 harness OK, 2/51 сабтестов**. Все падения — уже задокументированные дефекты: BUG-721 (getConfiguration/setConfiguration), no-op заглушки addTransceiver/getSenders/getReceivers (документированный скоуп webrtc_stub.rs), Phase 1 «нет захвата видео», BUG-380. Новых багов не заведено. [Подробности](wpt-vendor-notes/webrtc-extensions.md). |
| `webrtc-ice` | 🚫 | ✅ |  |  | Скоуп 🚫 подтверждён точно (в отличие от родительской `webrtc`) — `RTCIceTransport` (весь тестируемый API категории) отсутствует в `webrtc_stub.rs` целиком, ICE там существует только как внутренние поля `RTCPeerConnection`. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-ice/`, 3 файла). `run_report.py --all --root webrtc-ice --recursive` (~45с, 1 id): 0/1 harness OK, TIMEOUT на TLS-гэпе BUG-657. Новых багов не заведено. [Подробности](wpt-vendor-notes/webrtc-ice.md). |
| `webrtc-identity` | 🚫 | ✅ |  |  | Скоуп 🚫 подтверждён точно перед вендорингом — `RTCIdentity`/`IdentityProvider`/`IdentityAssertion`/`peerIdentity` отсутствуют в `webrtc_stub.rs` целиком. Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-identity/`, 5 файлов, 4 id). `run_report.py --all --root webrtc-identity --recursive` (~57с): **1/4 harness OK, 0/1 сабтестов**. 3 `.https.`-файла — TLS-гэп BUG-657 (один попутно словил BUG-380-паттерн). Исполнившийся тест падает на отсутствии валидации `peerIdentity` в конструкторе — тот же корень, что BUG-721. Новых багов не заведено. [Подробности](wpt-vendor-notes/webrtc-identity.md). |
| `webrtc-priority` | 🚫 | ✅ |  | BUG-726 | Скоуп 🚫 (заметка «нет конвейера» была неточна — тот же дрейф, что у `webrtc`). Вендорена + прогнана целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-priority/`, 2 файла, 2 id). `run_report.py --all --root webrtc-priority --recursive` (~27с): **2/2 harness OK, 0/9 сабтестов**. Найден [BUG-726](../bugs/BUG-726-OPEN.md) (`createDataChannel()` отбрасывает `options`, `.priority` не отражается). Остальные падения — уже задокументированный BUG-721-смежный пробел (`addTransceiver()` no-op). [Подробности](wpt-vendor-notes/webrtc-priority.md). |
| `webrtc-stats` | 🚫 | ✅ |  | BUG-727 | Скоуп 🚫 (подтверждено по существу). Вендорена + прогнана целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-stats/`, 8 файлов, 8 id). `run_report.py --all --root webrtc-stats --recursive`: **0/8 harness OK, 0/23 сабтестов** — стопроцентный отказ. Найден [BUG-727](../bugs/BUG-727-OPEN.md): стаб никогда не диспатчит `ontrack`/`ondatachannel`/`on(ice)connectionstatechange`, оба пира полностью изолированы. [Подробности](wpt-vendor-notes/webrtc-stats.md). |
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
| `/dom/nodes/Element-children.html` | OK | 2/2 |  | [BUG-322](../bugs/BUG-322-FIXED.md), [BUG-323](../bugs/BUG-323-FIXED.md), [BUG-328](../bugs/BUG-328-FIXED.md) | Оба сабтеста PASS с 2026-08-05 (BUG-328); полный пересчёт таблицы — следующий `run_report.py --all` |
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
