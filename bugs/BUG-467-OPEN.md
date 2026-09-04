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
(разница — уже заведённый [BUG-995](BUG-995-OPEN.md) на `fontface-loadingevent.html`,
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
