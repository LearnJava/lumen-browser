# BUG-467: CSS Font Loading API (FontFace/FontFaceSet/FontFaceSetLoadEvent) practically unimplemented

**Статус:** OPEN (ДОРАБОТКА → FONTLOAD)
**Тип:** нереализованная функциональность, не дефект — ведётся как задача `FONTLOAD` в [ROADMAP.md](../ROADMAP.md) (решение 2026-08-28); P3 как баг не берёт
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6770` — `_lumen_get_fonts()`, CSS Fonts
Module Level 4 §11 / CSS Font Loading Module)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`;
расширен срезом 17 — массовый прогон `css/css-font-loading`

## Симптом

Оба файла, гейтящие раскладку через `document.fonts.ready.then(...)`
(`positioning/inline-static-position-001.html`,
`linebox/vertical-align-top-bottom-001.html`), TIMEOUT: гарнес не завершается
вовсе, ни один тест не регистрируется.

`_lumen_get_fonts()` (dom.rs:6770) строит объект `fontSet` с `length`/`item`/
`entries`/`forEach`/`Symbol.iterator`, но нигде не определяет свойство
`ready`. `document.fonts.ready` — `undefined`, `.then(...)` на нём бросает
синхронный `TypeError` внутри верхнеуровневого `<script>`, до того как
`testharness.js` успевает зарегистрировать хоть один `test()` — отсюда именно
TIMEOUT (гарнес молчит), а не чистый FAIL.

## Влияние вне WPT

`document.fonts.ready` — стандартный способ дождаться, что все `@font-face`
подключились, прежде чем измерять раскладку/рисовать canvas-текст (частый
паттерн в веб-шрифтовых библиотеках и в самих WPT-тестах CSS Fonts/Text).
Пересекается с уже задокументированным в `docs/wpt-status.md` (строка
`fonts`) ограничением, что `url()`-источники `@font-face` подгружаются
асинхронно фоновым потоком — `fonts.ready` мог бы стать штатным способом
дождаться этого события из живого (не headless-однослотового) JS, но сейчас
такого способа нет вовсе.

## .ini

`tests/wpt/metadata/css/CSS2/{positioning/inline-static-position-001,linebox/vertical-align-top-bottom-001}.html.ini`
— `expected: TIMEOUT` на уровне теста (сабтестов нет — гарнес не долетает до
`test()`).

## Расширение (срез 17, `css/css-font-loading`): вся категория, а не только `.ready`

Массовый прогон `css/css-font-loading` (21 testharness id, 17/21 harness OK,
0/79 сабтестов) показал, что `.ready` — лишь один симптом целиком
отсутствующей CSS Font Loading API. Ни один сабтест не прошёл; читая
`crates/js/src/dom.rs:6771` (`_lumen_get_fonts()`) и грепая `crates/js/src/
dom.rs`/`v8_runtime.rs` на `"FontFace"`/`"FontFaceSet"`/`"FontFaceSetLoadEvent"`
(ноль совпадений вне комментариев), подтверждено: движок не предоставляет ни
одного из трёх глобальных конструкторов, а `document.fonts` — statically-built
plain object лишь с `length`/`item`/`entries`/`forEach`/`Symbol.iterator`, не
настоящий `FontFaceSet`. Раздельные фасеты, все воспроизведены чтением кода +
логом прогона (`.tmp/wpt-css-font-loading.log`):

1. **`FontFace` не определён вовсе** — `new FontFace(family, source)` бросает
   `ReferenceError`, а не строит объект. Доминирующая причина: 12 из 21
   файлов синхронно падают на этом до регистрации ассерта (`empty-family-load`,
   `font-face-reject`, `fontface-descriptor-updates-2` (3 сабтеста),
   `fontface-font-variation-settings-persisted-js-api`, `fontface-fonts-
   loading`, `fontface-invalid-arraybuffer`, `fontface-invalid-family.
   tentative` (9 сабтестов), `fontface-load-in-modal-dialog`, `fontface-
   override-descriptor-getter-setter.sub` (24 сабтеста — все дескрипторы
   `ascentOverride`/`descentOverride`/`lineGapOverride`),
   `fontfacesetloadevent-constructor` (второй сабтест)).
2. **`FontFaceSet` не определён вовсе** — по спеке `FontFaceSet` не должен
   конструироваться (`new FontFaceSet()` обязан бросить `TypeError` — «illegal
   constructor»), но раз идентификатора нет глобально, движок бросает
   `ReferenceError` вместо ожидаемого спекой `TypeError`
   (`historical.html`: `assert_throws_js` требует `TypeError`, получает
   `ReferenceError` — фиксирует, что даже «этого не должно существовать»
   API не смоделировано корректно).
3. **`FontFaceSetLoadEvent` не определён вовсе** (`fontfacesetloadevent-
   constructor.html`, первый сабтест).
4. **`document.fonts` — не `FontFaceSet`, а урезанный read-only снимок**:
   в `_lumen_get_fonts()` (dom.rs:6771) объект `fontSet` определяет только
   `length`/`item`/`entries`/`forEach`/`Symbol.iterator` — отсутствуют ВСЕ
   остальные члены интерфейса `FontFaceSet` (WHATWG-подобный `Set`):
   - **`size` отсутствует вовсе** (только `length`, спека и все evergreen-
     браузеры используют `size`, не `length`) — `fontfaceset-update-after-
     stylesheet-change.html`/`nonexistent-file-url.html` оба падают на первом
     же `assert_equals(document.fonts.size, 1, …)`, `undefined !== 1`,
     не добираясь до самой проверки динамического обновления при
     add/remove `<style>` или до поведения при недостижимом `file://`-источнике
     — те вопросы остаются непроверенными этим прогоном, но сам `size`
     подтверждён отсутствующим чтением кода.
   - `add`/`delete`/`clear`/`has`/`keys` отсутствуют —
     `fontfaceset-clear-css-connected`/`fontfaceset-delete-css-connected`
     («fonts.keys is not a function»), `fontfaceset-has` («fonts.keys is not
     a function or its return value is not iterable»).
   - `load`/`ready` (промисы) отсутствуют — `fontfaceset-load-css-connected`
     («fonts.load is not a function»), `fontfaceset-load-css-wide-keywords`
     (24 сабтеста, TIMEOUT — ошибка ловится, но что-то в цепочке промисов не
     резолвится/не таймаутит корректно), `fontfaceset-load-var` (4 сабтеста,
     TIMEOUT, тот же паттерн).
   - `addEventListener`/`onloading`/`onloadingdone`/`onloadingerror`
     отсутствуют — `fontface-loadingevent.html`: «fontSet.addEventListener is
     not a function».

`idlharness.https.html` — отдельный артефакт: `TEST_END: ERROR` с
`AssertionError: Got results from /css/css-font-loading/historical.html,
expected /css/css-font-loading/idlharness.https.html` — тот же класс, что
BUG-380 (browsing-context reuse не проверяет исход навигации между тестами),
не новый симптом этой категории.

Committed `.ini` для всех 21 непройденных файлов; повторный прогон из
worktree подтверждает 0 unexpected / 0 unexpected passes (79/79 expected).

Пересекается с [BUG-471](BUG-471-OPEN.md) (CSSOM `insertRule`/`sheet` не
подключены) — если `document.fonts` когда-нибудь станет реактивным на
добавление/удаление `@font-face`-правил через CSSOM, то потребуется тот же
фундамент, что и для BUG-471.

## Срез 26 (`css/css-ruby`, 2026-08-03)

`line-spacing.html` — `document.fonts.load("16px Ahem").then(...)` at top
level, before any `test()` registers: TIMEOUT (`document.fonts.load` is one
of the missing Maplike/loading members this bug already documents). `.ini`
under `tests/wpt/metadata/css/css-ruby/line-spacing.html.ini`,
`expected: TIMEOUT`.

## Срез 31 (`css/css-fonts`, 2026-08-03)

+2 files/100 subtests: global `FontFace` constructor is `undefined`
(`new FontFace(...)` throws `ReferenceError`) — the imperative half of the
Font Loading API, not just `document.fonts`'s read side. Several more
`css-fonts` files this slice TIMEOUT on `Cannot read properties of
undefined (reading 'then')` chained off a `document.fonts.*` call — same
family, not yet individually re-counted per file. `.ini` under
`tests/wpt/metadata/css/css-fonts/`.

## Срез 33 (`css/css-sizing`, 2026-08-03)

11 more files (`stretch/*`, `keyword-sizes-on-inline-block.html`, etc), all
the `document.fonts.ready.then(() => checkLayout(...))` idiom. `.ini` under
`tests/wpt/metadata/css/css-sizing/`, file-level `expected: TIMEOUT`.

## FONTLOAD-1 (P1, 2026-09-05, ветка `p1-fontload1-fontface-api`) — первый срез доработки

Реализованы: глобальный конструктор `FontFace` (family/style/weight/stretch/
unicodeRange/featureSettings/variationSettings/display, `status`, `loaded`,
`.load()` — реально фетчит `url()`-источники через `fetch()`/`arrayBuffer()`
и валидирует байты через `_lumen_font_validate_bytes`, новый натив поверх
`lumen_font::{maybe_decode_font, Font::parse}`); `document.fonts` стал
настоящим `setlike<FontFace>` — стабильная идентичность объектов (был
`_lumen_get_fonts()`, пересобиравший plain-object на каждое обращение),
`size`/`has`/`add`/`delete`/`clear`/`keys`/`values`/`entries`/`forEach`/
`load(font, text)`/`ready`/`status`, события `loading`/`loadingdone`/
`loadingerror` (`FontFaceSetLoadEvent`); `new FontFaceSet(...)` теперь
бросает `TypeError` (`historical.html`). Замер до/после (`run_report.py --all
--root css/css-font-loading`, dev-release, Windows): 18/27 → 17/27 harness OK
(разница — уже заведённый [BUG-995](BUG-995-FIXED.md) на `fontface-loadingevent.html`,
которая раньше умирала ReferenceError'ом раньше своей клавиатурной части и
теперь доходит до нативного краша: не регрессия этого среза, воспроизведён
уже в бейзлайне ДО правки), 0/54 → 6/55 subtests PASS.

Два gap'а НЕ закрыты этим срезом, оба зафиксированы в заголовочном комментарии
секции (`crates/js/src/shim/web_api_shim_mid.js`, «FontFace and FontFaceSet»):

1. **CSS-connected `document.fonts` — одноразовый снимок.** `<style>`/CSSOM
   изменение после первого обращения к `document.fonts` не отражается —
   нужен тот же фундамент живого каскада, что и BUG-471/CSSOM-4. Не проходят
   по этой причине: `fontfaceset-update-after-stylesheet-change`,
   `fontfaceset-has` (вторая половина), `fontfaceset-load-css-connected`
   (что тут прошёл — совпадение: `.load()` на "WebFont" фейлится сам по себе).
2. **Нативный `status` CSS-декларированного `url()`-лица никогда не
   становится `Loaded`** (`crates/shell`'s `FontLoaded` не трогает
   `Document::fonts_mut()`) — `document.fonts.ready`/`.status` намеренно
   считают только script-driven загрузки (`FontFace.load()`/
   `document.fonts.load()`), не пассивную CSS. Иначе каждый существующий
   `document.fonts.ready.then(...)`-гейт в css-fonts/css-sizing/css-ruby
   (см. срезы 31/33 выше) стал бы вечно висящим промисом — регресс, не фикс.

Остаток: descriptor-значения (`unicodeRange`/`featureSettings`/…) принимаются
без разбора грамматики (просто `String(v)`) — `fontface-descriptor-updates*`,
`fontface-override-descriptors*`, `fontface-size-adjust-descriptor` (рефтесты,
не относятся к этому срезу, падают из-за неразобранных дескрипторов, влияющих
на реальный рендеринг, который скрипт-сконструированный `FontFace` в этом
срезе тоже не подключает к `lumen_font::FontRegistry`/рендереру — отдельный,
более крупный кусок FONTLOAD). Следующий срез — на выбор владельца FONTLOAD:
реактивность CSS-connected сета (закрывает больше id) или подключение
загруженного шрифта к рендерингу (закрывает FOUT/FOIT-класс тестов).

## FONTLOAD-2 (P1, 2026-09-05, ветка `p1-fontload2-css-font-loaded-status`) — второй срез: разведка + узкий безопасный фикс

Перед выбором среза — переисследование обоих gap'ов FONTLOAD-1 (агентом
code-explorer + ручная проверка), результат существенно уточняет их объём:

**Переоценка gap 1 (реактивность).** Формулировка «одноразовый снимок»
занижает масштаб: `crates/driver` (путь `run_report.py`/весь WPT-прогон)
**вообще никогда** не вызывает `Document::fonts_mut()` — grep
`fonts_mut\(\)|\.fonts\(\)` по `crates/driver/` даёт ноль совпадений.
`document.fonts.size` для КАЖДОГО WPT-файла `css/css-font-loading` равен `0`
с первой строки скрипта, а не устаревает после мутации — это баг №0,
отдельный от реактивности, и без него реактивность нечем тестировать штатным
набором. Хуже: `crates/driver/src/session.rs::run_pipeline` выполняет
скрипты страницы (стр. ~333–343) **до** парсинга `<style>`-блоков в
`Stylesheet` (стр. ~346–347, `BUG-429` намеренно оставил такой порядок —
скрипт мог создать DOM-узлы, которым нужен финальный layout). Заполнение
`document.fonts` до первого чтения скриптом требует либо перестановки этого
порядка (риск для BUG-429), либо отдельного раннего прохода только по
`@font-face`-правилам статичного `<head>` — тянет за собой архитектурное
решение, не одну функцию. `CSSOM-4`/`FlushHandles::maybe_flush`
(`crates/js/src/v8_runtime/style_flush.rs`) — верно применимый паттерн для
*дальнейшей* реактивности после первого заполнения (тот же `Arc<Stylesheet>`,
тот же охват `InProcessSession`), но сам не решает gap 0.

**Переоценка gap 2 (`status` никогда не `Loaded`).** Одного нативного флага
недостаточно: `document.fonts` в JS — статичный снимок, `_lumen_wrap_css_font_face`
читает нативный `status` РОВНО ОДИН РАЗ, в момент первого касания
`document.fonts`. Если скрипт (типичный паттерн — `document.fonts.ready.then(...)`
до завершения фонового `url()`-фетча) уже построил снимок раньше, чем шрифт
догрузился, — правка одного нативного поля без пуша в JS ничего не меняет,
сколько раз потом ни читай `document.fonts`. Нужен реальный push
native→JS (`route_eval_js`), не просто мутация `Document::fonts`.

**Что сделано этим срезом** (только shell/live-путь — `crates/driver` не
трогается, WPT pass-rate `css/css-font-loading` этим срезом не двигается,
т.к. WPT туда не доходит по gap 0 выше):
- `FontFaceSet::mark_loaded` (`crates/engine/dom/src/font_faces.rs`) —
  предикат-based флип статуса в `Loaded`.
- `LoadEvent::FontLoaded`-обработчик (`crates/shell/src/app/user_event.rs`)
  теперь (а) флипает статус подходящего по family/weight/style
  CSS-connected `FontFace` нативно, (б) шлёт `_lumen_notify_css_font_loaded(family)`
  через `route_eval_js` (ADR-016-совместимый маршрут).
- `_lumen_notify_css_font_loaded` (`crates/js/src/shim/web_api_shim_mid.js`) —
  если `document.fonts` уже построен, находит непомеченный CSS-connected
  член с совпадающими дескрипторами и резолвит **только его собственные**
  `.status`/`.loaded` (`_resolveLoaded`), НЕ трогая общий `_pending`-счётчик
  сета — прямой вызов `_onFaceLoadEnd` без парного `_onFaceLoadStart` мог бы
  увести `_pending` в отрицательные значения и преждевременно зарезолвить
  `document.fonts.ready`, пока параллельный script-driven `.load()` другого
  лица ещё выполняется (тот самый регресс, которого боялся исходный
  комментарий FONTLOAD-1). `document.fonts.ready`/`.status` set-уровня
  остаются осознанно только script-driven — без изменений.

**Не закрыто, требует отдельного среза:**
- gap 0/1 выше (заполнение `document.fonts` в `crates/driver` + порядок
  скрипт/CSS-парсинг) — крупнее, чем «доработка», возможно стоит отдельного
  под-бага, а не безусловно следующего среза FONTLOAD.
- set-уровневые `ready`/`loading`/`loadingdone` для CSS-connected лиц (нужен
  сигнал о **начале** фонового фетча, не только о завершении, плюс отдельный
  «слот» на лицо в pending-счётчике вместо общего int).

## FONTLOAD-3 (P1, 2026-09-05, ветка `p1-fontload3-driver-font-population`) — коррекция «gap 0» и фикс для `InProcessSession`

**Уточнение атрибуции gap 0.** FONTLOAD-2 назвала целью `crates/driver/src/
session.rs::run_pipeline` (`InProcessSession`) как «путь run_report.py/весь
WPT-прогон». Это неточно: `tests/wpt/run_report.py`/`run_suite.py` гоняют
настоящий `wptrunner` поверх WebDriver BiDi против **отдельно запущенного
`lumen.exe`** (`lumen --bidi-port N`, `tests/wpt/README.md`) — то есть против
`crates/shell`, а не `crates/driver`. Проверено грепом: ни один файл в
`crates/driver/`/`tests/wpt/` не ссылается на другой — `InProcessSession`
вообще не участвует в WPT-прогоне через `run_report.py`. `crates/shell`
заполняет `document.fonts` с FONTLOAD-1/2 (`page_pipeline.rs:881-897`), так
что WPT pass rate `css/css-font-loading` этим срезом не двигается — он не
предполагался.

Настоящий адресат gap 0 (в узкой формулировке «`InProcessSession` никогда не
вызывает `fonts_mut()`») — `crates/driver`'s собственный Rust-тестовый набор
(`crates/driver/tests/`) и headless-эмбеддеры поверх `InProcessSession`
(`lumen --mcp`, `crates/shell/src/automation_server.rs::run_mcp_mode`) —
реальная, документированная поверхность (`lib.rs` «Уровни 2–3
тестирования»), но не WPT.

**Второй уточнённый момент — тайминг, не только присутствие.** Первая
попытка среза вставила заполнение `document.fonts` ПОСЛЕ выполнения скриптов
страницы (там же, где раньше `sheet`/layout-парсинг), рассудив, что порядок
не важен раз лейаут ещё не построен. Проба (`crates/driver/tests/cases/
zzz_probe_fontload3.rs`, временный тестовый файл, удалён) на странице с
синхронным `<script>window.__probe = document.fonts.size</script>` в
`<head>` показала `sync=0, post=0` — то есть даже ПОСЛЕ навигации
`document.fonts.size` читался нулём. Причина — JS-обёртка `document.fonts`
кэширует свой `FontFaceSet`-снимок на первое обращение
(`_lumen_wrapper_slot(this, '__fonts__', _lumen_make_font_face_set)`,
`crates/js/src/shim/web_api_shim_mid.js:8781`): если первое касание
происходит ДО нативного заполнения, снимок замораживается пустым на всю
жизнь страницы, сколько потом ни заполняй нативную сторону — тот же класс
проблемы, что FONTLOAD-2 уже описала для `status` отдельного лица (gap 2),
но здесь применительно к самому МНОЖЕСТВУ. Именно такой синхронный
top-level `document.fonts.ready.then(...)`-паттерн и есть исходный симптом
этого бага. Фикс: заполнение перенесено ПЕРЕД блок выполнения скриптов
(`session.rs::run_pipeline`), отдельным ранним CSS-парсом статичной
(до-скриптовой) разметки — существующий пост-скриптовый парс для лейаута не
трогается (у него остаётся прежнее поведение: скрипт-вставленный `<style>`
по-прежнему попадает в каскад лейаута, просто не отражается в
`document.fonts` — та же одноразовость снимка, что уже документирована как
CSSOM-реактивность, не новый регресс). Риска для BUG-429 нет: сбор
`@font-face`-правил — синтаксическая операция над уже распарсенным HTML, не
зависит от того, что скрипты построят в DOM, и не участвует в лейауте.

Реализовано (`crates/driver/src/font_faces.rs`, новый модуль — не общий с
`crates/shell/src/subresources.rs::rule_to_font_face`: `lumen-dom` и
`lumen-css-parser` — соседние листовые крейты без связи друг с другом, общий
дом для конвертера пришлось бы заводить новым крейтом ради одной ~15-строчной
функции; `session.rs` и так уже за потолком 2000 строк и расти ему нельзя).
Каждая запись стартует `FontFaceStatus::Unloaded` (конструктор по умолчанию)
даже для `local()`-источников — в отличие от shell, `InProcessSession` не
держит `FontRegistry`/`SystemFontIndex`, а `Unloaded` — это и есть
спек-корректное начальное состояние CSS-connected лица, которое ничего ещё
не заставило грузиться. `scripts/file-size-baseline.tsv` обновлён (`session.
rs` 2950→2981, +31 — храповик SPLIT, `docs/lint-policy.md` §5.1).

Три новых юнит-теста (`crates/driver/tests/cases/
fontload3_document_fonts_population.rs`): синхронный top-level скрипт видит
корректный `document.fonts.size` (тест самого фикса тайминга, не просто
финального состояния), `family`/`status` после навигации, пустая страница
даёт пустой сет. `cargo test -p lumen-driver` 196/196 (было 193, без
регрессий), `cargo clippy -p lumen-driver --all-targets -- -D warnings`
чист. Обратно-зависимая проверка (`lumen-ai`/`lumen-bidi-server`/
`lumen-canvas`/`lumen-chrome`/`lumen-image`/`lumen-js`/`lumen-knowledge`/
`lumen-layout`/`lumen-mcp`/`lumen-paint`/`lumen-shell`/`lumen-storage`) —
без регрессий; `lumen-network` из замыкания пропущен намеренно — известный
сломанный гейт [BUG-805](BUG-805-OPEN.md) (зависает на UDP-тесте), не
связан с этим срезом.

**Не входит в срез** (остаётся тем, чем было для gap 0/1 в FONTLOAD-2, плюс
уточнение): реальный WPT-путь (`crates/shell`, `run_report.py`) — эта
диагностика ещё не проверена на предмет того же самого «снимок замёрз до
заполнения» тайминга для CSS-connected сета (FONTLOAD-1/2 заполняют
`document.fonts` в `page_pipeline.rs` тоже ПОСЛЕ `run_scripts_with_dom`,
т.е. потенциально несут ТОТ ЖЕ класс дефекта, что этот срез только что
исправил для `InProcessSession`) — это отдельный, ещё не измеренный кусок
работы над `crates/shell`, не покрытый этим срезом.

## FONTLOAD-4 (P1, 2026-09-05, ветка `p1-fontload4-shell-font-population`) — тот же тайминг-риск в `crates/shell`, реальный WPT-путь

Проверила ровно то, что FONTLOAD-3 оставила непроверенным. Пробный тест
(`parse_and_layout` на странице `<style>@font-face{...local('Arial')}</style>`
+ синхронный top-level `<script>document.documentElement.setAttribute(
'data-sync', String(document.fonts.size))</script>`) до фикса дал `sync=0` —
подтверждает, что дефект существует и в `crates/shell`, не только в
`InProcessSession`: `page_pipeline.rs` заполнял `document.fonts` строго
ПОСЛЕ `run_scripts_with_dom` (код, унаследованный с FONTLOAD-1/2, строка
~881), а JS-обёртка `document.fonts` кэширует свой `FontFaceSet`-снимок на
первое касание (`_lumen_wrapper_slot`, `web_api_shim_mid.js`) — ровно тот же
механизм, что FONTLOAD-3 уже описала для `InProcessSession`.

Фикс: блок заполнения `document.fonts` перенесён в `page_pipeline.rs` с
конца функции (после скриптов, после возможной пересборки каскада) на место
сразу после первой (pre-script) сборки `cascade` — до `run_scripts_with_dom`.
В отличие от `crates/driver` (там пришлось заводить отдельный ранний парсинг
`@font-face`, `crates/driver/src/font_faces.rs`), `crates/shell` уже строит
pre-script `cascade` для другой цели (BUG-443 — чтобы parse-time скрипт видел
реальный computed style/geometry), так что переиспользован тот же `cascade.
sheet.font_faces`/`cascade.font_registry` без нового прохода по CSS. После
фикса проба даёт `sync=1`; оставлена постоянным тестом
`crates/shell/src/tests/page_pipeline.rs::
fontload4_document_fonts_sync_read_sees_font_face_rules`.

**Измерение на реальном WPT-пути** (в отличие от FONTLOAD-3, `crates/shell`
— это именно то, что гоняет `run_report.py`/`wptrunner` через BiDi, так что
здесь pass rate обязан был сдвинуться, и сдвинулся): `run_report.py --all
--root css/css-font-loading` (dev-release, Windows) — 17/27 harness OK без
изменений, сабтесты **6/55 → 8/55**. Новые UNEXPECTED-PASS:
`fontfaceset-clear-css-connected`, `fontfaceset-delete-css-connected`, обе
половины `fontfacesetloadevent-constructor`, `historical.html`,
`nonexistent-file-url.html`.

**`.ini`-baseline этой категории намеренно не тронут.** `--update-expected`
немедленно вскрывает не связанный с шрифтами долг: `css/css-font-loading` —
одна из «21 категории, которой после фикса выборки по манифесту (WPT-RUN-7
срез 4) требуется перегенерация baseline» (`ROADMAP.md`, строка WPT-RUN-7).
Манифест теперь корректно находит новые id (`fontface-descriptor-updates.
html`, `fontface-from-arraybuffer.html`, `fontface-override-descriptors.
html`, `fontface-size-adjust-descriptor.html`, `fontfaceset-clear-css-
connected-2.html`, `fontfaceset-delete-css-connected-2.html`) — настоящие
reftest-файлы с `-ref.html`-компаньоном, которые этот раннер не умеет
исполнять как testharness и которые `--check` рапортует как `REGRESSION:
expected None, got MISSING (crash before test_start)`. Смешивать этот
несвязанный сигнал с фиксом тайминга в одном коммите неверно — перегенерация
baseline для всей категории остаётся владельцу WPT-RUN-7.

Заодно исправлен дрейф в шапке-комментарии `crates/js/src/shim/
web_api_shim_mid.js` («FontFace and FontFaceSet»): она всё ещё утверждала,
что «on the driver/WPT path `document.fonts` is never populated at all» —
устарело уже после FONTLOAD-3 (которая как раз это исправила для
`InProcessSession`), просто не была обновлена тогда.

`cargo test -p lumen-shell --bin lumen` 1723/1723 (было 1722, без
регрессий, +1 новый тест), `cargo clippy -p lumen-shell --all-targets --
-D warnings` чист.

**Не входит в срез:** реактивность CSS-connected сета на CSSOM-изменения
(gap 1 из FONTLOAD-1/2, оба пути — `crates/shell` и `crates/driver`) — тот
же больший фундамент BUG-471/CSSOM-4, что и раньше; перегенерация `.ini`
baseline `css/css-font-loading` под WPT-RUN-7 срез 4 (см. выше).

## FONTLOAD-5 (P1, 2026-09-05, ветка `p1-fontload5-css-connected-loading-signal`) — CSS-connected лицо в общем pending-счётчике сета

Закрыла gap 2, оставленный FONTLOAD-1/2 как принципиально не тронутый:
`document.fonts.ready`/`.status` (агрегат СЕТА) считали только
script-driven загрузки (`FontFace.load()`/`document.fonts.load()`),
никогда — фоновый CSS `url()`-фетч. Синхронный top-level
`document.fonts.ready.then(...)` без единого явного `.load()` — тот самый
паттерн, ради которого весь трек и existует — резолвился немедленно, даже
если CSS-connected лицо ещё качается в фоне.

Фикс в два слоя:

1. **Native (`crates/shell/src/page_pipeline.rs`):** лицо с `url()`-источником,
   который реально ставится в очередь фонового фетча (`pending_web_fonts`),
   теперь помечается `FontFaceStatus::Loading` уже в момент заполнения
   `document.fonts` — ДО любых скриптов страницы (тот же порядок, что
   FONTLOAD-4 установила). Правило с неразрешённым `local()` и без `url()`
   вовсе остаётся `Unloaded` — иначе `ready` завис бы навечно (ничего
   никогда его не загрузит).
2. **Шим (`crates/js/src/shim/web_api_shim_mid.js`):** `_lumen_make_font_face_set`
   при первом касании `document.fonts` видит нативный `Loading` и сразу
   сеет счётчик сета (`_lumen_font_face_load_start`) — плюс регистрирует
   владельца в `_lumen_font_face_owners` для CSS-connected лиц (раньше это
   делал только `FontFaceSet.add()`, так что у них не было владельца
   вовсе). Флаг ожидания (`_cssFetchPending`) заведён ОТДЕЛЬНО от
   `_status`/`_loadedPromise` — см. регрессию ниже, почему это важно.
   `_lumen_notify_css_font_loaded` (уже существовавшая с FONTLOAD-2) теперь
   парно закрывает счётчик через `_cssFetchPending`, независимо от того,
   успел ли конкурентный `.load()` скрипта уже сам зарезолвить `_status`.

**Найдена и исправлена собственная регрессия среза до коммита.** Первая
версия фикса завела статус `'loading'` прямо в `face._status` (не
отдельным флагом) — и `FontFace.prototype.load()` короткое замыкание
`if (status === 'loading' || 'loaded') return this._loadedPromise` начало
возвращать промис, который резолвит ТОЛЬКО нативный фоновый фетч. Тот
фетч — fire-and-forget и молча ничего не шлёт при неудаче
(`page_load.rs`'а тред просто `return`ится без события). Живой замер
(`run_report.py --all --root css/css-font-loading`, A/B до/после) поймал
это: `fontfaceset-load-css-connected.html` (снимает `<style>`, зовёт
`fonts.load("...WebFont")`, ждёт `result.length === 0`) до среза давал
FAIL (0 unexpected, `expected: FAIL` в `.ini` — раньше `.load()` делал
СВОЙ независимый фетч и резолвился/реджектился быстро), после первой
версии фикса — TIMEOUT (harness 17/27 → 16/27, единственная реальная
регрессия, остальные 8/55 сабтестов не сдвинулись вовсе). Решение —
развести флаг ожидания сета (`_cssFetchPending`) и `_status`, чтобы
`.load()` продолжал делать собственный независимый фетч даже пока нативный
фоновый фетч того же лица ещё не завершился. Финальный замер:
17/27 harness OK / 8/55 сабтестов — байт-в-байт то же самое, что baseline
до среза (никакого нового `UNEXPECTED-PASS`/`FAIL` в этой категории —
ожидаемо: ни один WPT-файл здесь не гейтится ЧИСТО на `.ready` без
`.load()`, ценность фикса ловится только целевыми Rust/JS-юнит-тестами
ниже, не этой WPT-категорией).

Тесты: `crates/shell/src/tests/page_pipeline.rs::
fontload5_url_sourced_face_is_populated_as_loading` +
`fontload5_unresolvable_local_only_face_stays_unloaded` (native-статус на
заполнении); `crates/js/src/dom/tests/v8_fontface_shadow_custom.rs::
css_connected_loading_face_counts_as_pending_on_first_touch` +
`css_connected_load_completing_pairs_off_pending_and_resolves_ready`
(сет-уровневый `ready`/`.status` реально ждёт и резолвится). `cargo test -p
lumen-shell --bin lumen --features v8` 1725/1725 (было 1723, +2), `cargo
test -p lumen-js --features v8-backend` 3464/3465 (1 давно известный
непричастный флейк, BUG-997 — падает и на чистом `main`), `cargo clippy`
для обоих крейтов чист.

**Не входит в срез:** реактивность CSS-connected сета на CSSOM-изменения
(gap 1, прежний фундамент BUG-471/CSSOM-4); descriptor grammar (`unicodeRange`
и т.п. остаются `String(v)`) и подключение script-constructed `FontFace` к
`lumen_font::FontRegistry`/рендерингу (FOUT/FOIT) — по итогам ре-скоупинга
агентом-исследователем оба оценены как отдельный, более крупный кусок
FONTLOAD, требующий нового js↔shell каллбэк-шва через границу слоёв, не
один срез.

## FONTLOAD-6 (P1, 2026-09-05, ветка `p1-fontload6-scripted-fontface-registry`) — script-constructed `FontFace` → рендеринг

Закрыла второй пункт, оставленный FONTLOAD-5 нетронутым: `new FontFace(family,
source)` + `.load()`/`document.fonts.add()` валидировали байты (FONTLOAD-1) и
резолвили промис/`.status`, но никогда не трогали
`lumen_font::FontRegistry` — текст, стилизованный семейством script-
сконструированного лица, продолжал рисоваться фолбэком даже после того, как
`.status` становился `'loaded'`.

Фикс в три слоя, тот же шов native↔shell↔JS, что FONTLOAD-2/5 уже
использовали для похожей задачи (JS/engine-поток не может напрямую мутировать
UI-thread-owned реестр/рендерер, ADR-016; BUG-976 — почему нет):

1. **Шим (`crates/js/src/shim/web_api_shim_mid.js`):** `_lumen_font_face_try_one_source`
   теперь прокидывает валидированные байты источника через цепочку
   колбэков в `FontFace.prototype.load`, которая сохраняет их в
   `face._loadedBytes`. Новая `_lumen_maybe_register_scripted_font_face(face)`
   вызывается из двух мест — конца `.load()`'s success-ветки и
   `FontFaceSet.prototype.add` — и регистрирует лицо ровно один раз
   (`face._registeredForRender`), как только оно одновременно (а) провалидировано
   (`_loadedBytes` есть) и (б) состоит хотя бы в одном `FontFaceSet`
   (`_lumen_font_face_owners`). Оба порядка вызова (`.load().then(() =>
   fonts.add(face))` и `fonts.add(face); face.load()`) сходятся в одну и ту же
   функцию. CSS-connected лица исключены явной проверкой `_cssConnected` —
   у них уже есть собственный путь регистрации через `LoadEvent::FontLoaded`,
   дублировать его не нужно (и лишний повод не путать два источника одного и
   того же шрифта в реестре).
2. **Натив (`crates/js/src/v8_runtime/install/dom_core.rs`,
   `_lumen_register_scripted_font_face`):** декодирует (WOFF/WOFF2/как есть)
   и валидирует байты через уже существующие `lumen_font::{maybe_decode_font,
   Font::parse}`, парсит `weight`/`style` дескрипторы (новый
   `parse_scripted_font_weight` — минимальный keyword/first-number разбор,
   зеркало `crates/shell/src/subresources.rs::parse_font_weight`, продублировано
   потому что `crates/js` лежит ниже `crates/shell` в слоях крейтов) и кладёт
   `(family, weight, style, decoded_bytes)` в новую очередь
   `pending_scripted_font_faces` (`V8JsRuntime`, тот же паттерн, что
   `pending_history_url_updates`/canvas-обновления — `Arc<Mutex<Vec<_>>>`,
   drain раз в кадр).
3. **Shell (`crates/shell/src/app/about_to_wait.rs`):** дренирует очередь
   каждый `about_to_wait`, регистрирует каждую запись через
   `page_font_registry.register_from_bytes`, обновляет `FontProvider` рендерера
   и форсирует `relayout_chrome` + `request_redraw` — тот же путь, которым уже
   идёт `LoadEvent::FontLoaded` для CSS-connected лиц.

Намеренно узко: дескрипторы `unicodeRange`/остальные не разбираются по
грамматике (тот же пре-существующий пробел FONTLOAD-1); лицо, удалённое из
всех `FontFaceSet` после регистрации, остаётся в `FontRegistry` (снять запись
нечем — у `FontRegistry` нет операции unregister, то же ограничение уже есть
у CSS-connected лиц).

Тесты (`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs`):
`register_scripted_font_face_queues_valid_bytes`/`_rejects_garbage_bytes`
(натив напрямую, реальные байты Ahem через `atob`, не синтетическая строка —
иначе проверялась бы только ветка отказа), `script_constructed_font_face_
registers_on_add_then_load`/`_registers_on_load_then_add` (оба порядка
вызова), `css_connected_face_is_excluded_from_scripted_registration` (явная
проверка guard'а, не только «байтов и так не было»). `cargo test -p lumen-js
--features v8-backend -- v8_fontface_shadow_custom` 45/45 (4 новых), `cargo
clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` и
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings` оба
чисты.

**Не входит в срез:** реактивность CSS-connected сета (gap 1, BUG-471/CSSOM-4);
descriptor grammar; unregister-путь для лица, покинувшего все `FontFaceSet`.
