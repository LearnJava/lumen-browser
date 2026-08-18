# Дорожка TEST — полнота тестирования движка

Бриф для ROADMAP.md-задач `TEST` / `TEST-1`…`TEST-8` (Фаза 3). Владелец тулинга — **P2**;
движковые фиксы по находкам — **P3** (краши/паники/UB) и **P1** (layout/paint).

## Зачем

Исследование индустриальной практики (2026-07-27): движки, написанные с нуля (Servo, Ladybird),
опираются на четыре столпа — WPT-конформанс с expectations-системой, reftests с детерминированными
шрифтами, фаззинг парсеров и DOM, специализированные конформанс-наборы (test262, Wasm spec tests,
WebGL conformance). У Lumen уже есть: скриншотные graphic_tests (магента-рамка, порог 0.5%),
golden-гейты на дампы (DEVX-3), testharness-путь WPT (P2-wpt, 277 категорий вендорится в
WPT-VENDOR), health-журнал с panic-hook (PERF-6), KNOWN_DEBTORS-храповик. Дорожка TEST закрывает
оставшиеся пробелы.

Источники: [web-platform-tests docs](https://web-platform-tests.org/) ·
[reftests](https://web-platform-tests.org/writing-tests/reftests.html) ·
[tc39/test262](https://github.com/tc39/test262) ·
[Ladybird Testing.md](https://github.com/LadybirdBrowser/ladybird/blob/master/Documentation/Testing.md) ·
[Fuzzing Ladybird (Domato)](https://awesomekling.substack.com/p/fuzzing-ladybird-with-tools-from) ·
[Servo Book: testing](https://book.servo.org/hacking/testing.html) ·
[Firefox reftest/metadata](https://firefox-source-docs.mozilla.org/web-platform/index.html)

Рекомендуемый порядок: **TEST-1 → TEST-2 → TEST-3 → TEST-4/TEST-5** → опциональные TEST-6…TEST-8.

---

## TEST-1: cargo-fuzz харнессы на парсеры (M)

Аналог Ladybird `Meta/Lagom/Fuzzers/FuzzCSSParser.cpp`, но на `cargo-fuzz`/libFuzzer.

- Каталог `fuzz/` в корне workspace (не член основного workspace — свой `Cargo.toml`,
  nightly-toolchain нужен только для прогона фаззера, сборку браузера не трогает).
- Таргеты (по одному на вход): `fuzz_css_parser` (lumen-css-parser), `fuzz_html_parser`,
  `fuzz_url` (Url::parse/resolve — BUG-346/347 показали хрупкость), `fuzz_font`
  (lumen-font, декодер TTF/WOFF2), `fuzz_image` (декодеры изображений).
- Seed-корпус из `graphic_tests/*.html`, `samples/`, вендоренных WPT-файлов.
- DoD: таргеты собираются и гоняются локально (`cargo +nightly fuzz run <target> -- -max_total_time=300`);
  README в `fuzz/` с командами; найденные краши минимизированы и заведены как BUG-NNN
  (группировка по первопричине, методология docs/wpt-status.md); краш-репры закоммичены в
  `fuzz/regressions/` и добавлены в корпус.
- Инвариант: цель — отсутствие паник/UB на произвольном входе, не корректность разбора.

## TEST-2: DOM/layout-фаззер страниц в стиле Domato (L)

Генеративный фаззинг целых страниц — то, что у Ladybird регулярно находит краши в DOM, layout,
декодерах и CSS-математике.

- `scripts/page_fuzz.py`: генератор случайных почти-валидных страниц (грамматика: теги/атрибуты
  из поддержанного подмножества, CSS-свойства из CSS-SPECS.md ✅-списка, небольшие JS-сниппеты
  DOM-мутаций) с фиксируемым seed — репро детерминировано.
- Прогон пачками через headless (`--screenshot`/`--dump-layout`, `--deterministic`) с таймаутом;
  сигналы: паника (panic-hook уже пишет в health.log, PERF-6), таймаут/hang, белый экран
  (`count_rendered_units==0` при `dom_nodes>=20` — эвристика PERF-6).
- Минимизатор: бисекция DOM-поддеревьев/CSS-правил до минимального репро.
- DoD: `--selftest` без браузера (генерация+минимизация на синтетике); прогон N=1000 страниц
  журналируется (`.tmp/page-fuzz/`); находки сгруппированы в BUG-NNN; seed-и упавших страниц
  закоммичены в регрессионный список и прогоняются как smoke.

### TEST-2: состояние (2026-08-18, сессия P2)

`scripts/page_fuzz.py` реализован и влит: генератор DOM/CSS/JS-грамматики (теги — по
`BoxKind`/UA-стилям движка, CSS-свойства — курированный список из `CSS-SPECS.md` ✅-инвентаря,
JS-сниппеты — DOM-мутации без сети), `run_lumen` (headless `--dump-layout` с таймаутом,
переиспользует паттерн `subprocess.run(..., timeout=...)` из `scripts/mem_perf.py`), классификатор
сигналов (`panic`/`crash` по коду возврата, `hang` по таймауту, `white` — реплика
`count_rendered_units`/`BROKEN_RENDER_DOM_MIN=20` из `crates/shell/src/health_log.rs`, т.к. этот
сигнал **не пишется в `health.log` для headless dump-режима** — `record_render_health` вызывается
только из окна с живым event-loop, см. код `crates/shell/src/main.rs`), ddmin-минимизатор
(субдерево-удаление + splice однодетных обёрток + CSS-правила), `--selftest` (детерминизм,
баланс тегов, сходимость минимизатора на синтетике, сигнальный smoke) — все проверки зелёные.

**Найденная готча, важная для интерпретации будущих находок**: наивная белоэкранная проверка
(≥20 боксов, 0 видимых юнитов) даёт ложные срабатывания на страницах, которые генератор в принципе
не наполнил видимым содержимым (малые `max-depth`/`max-children`, невезение RNG) — движок там
корректно ничего не рисует, потому что рисовать нечего. Добавлен гейт `expected_content_units()`:
считает текст/медиа, которые генератор реально положил в исходное DOM-дерево (до прогона через
движок), и «white»-находка засчитывается только если ожидаемый контент был, а отрисовалось 0
(иначе — категория `white_empty` в результатах, не заводится как баг). Без этого гейта первый же
прогон 300 страниц дал 2 ложных «находки» (`.tmp/page-fuzz/regressions/white_000{0,1}_*_min.html`,
после минимизации свелись к пустым `<th>`/`<tbody>` без единого текстового узла).

**Прогон на HEAD (2026-08-18, `dev-release`, seed 1/100/5000, всего 1430 страниц, max-depth
5–7, max-children 4–6, timeout 5с)**: 0 паник, 0 crash, 0 hang, 0 реальных white-screen — только
2 отфильтрованных `white_empty`. Харнесс рабочий и валидированный, но **на первом прогоне не нашёл
ничего**, заводить BUG-NNN не на что. Для следующей сессии, продолжающей TEST-2 (не обязательно
P2): грамматика сейчас нарочно консервативна (безопасные значения CSS + пара JS-мутаций) —
расширять в сторону более агрессивных мутаций (вложенные `calc()`, экстремальные grid/flex,
мутации через `MutationObserver`/`requestAnimationFrame`, XSS-подобные строки в текстовых узлах)
и/или поднять N на порядок, прежде чем считать подмножество исчерпанным. `--no-minimize` ускоряет
батч, если нужен только количественный прогон без сразу-минимизации.

## TEST-3: WPT expectations/metadata-система (M)

По образцу Firefox metadata: превращает 277 вендоренных категорий из разового аудита в
регрессионный набор.

- Формат per-test ожиданий (PASS/FAIL/TIMEOUT/…) рядом с вендоренными тестами или в одном
  json/ini на категорию; генерация из текущего прогона `tests/wpt/run_report.py`.
- Новый режим `run_report.py --check`: exit≠0 только на **отклонение от ожиданий** (регрессия
  ожидаемого PASS, новый TIMEOUT), а не на абсолютный счёт; неожиданные PASS печатаются как
  «сузить ожидания» (храповик, аналог KNOWN_DEBTORS).
- DoD: ожидания сгенерированы для уже вендоренных категорий; `--check` за ворота scoped-test
  (или отдельный гейт-скрипт); docs/wpt-status.md «Как обновить» дополнен.

### TEST-3: состояние (2026-08-18, сессия P2)

Реализовано и влито: `tests/wpt/expectations.py` — генератор+классификатор поверх **нативного**
формата `wptrunner` (`--metadata`/`.ini`, `tools/wptrunner/wptrunner/manifestexpected.py`), не
отдельный JSON/самодельный формат. Решение сознательное: `wptrunner` уже грузит `--metadata` на
каждом прогоне (`run_smoke.run`) и его собственный structured-логгер кладёт ключ `"expected"` в
JSON-отчёт **только когда результат разошёлся** с закоммиченным `.ini` (mozlog-конвенция,
`testrunner.py::test_ended`) — значит генератору достаточно писать `.ini`-оверрайды только для
отклонений от дефолта (`OK`/`PASS`), а `--check` вообще не парсит `.ini` сам, а просто читает
готовое поле `expected` из JSON. `.ini` собираются через реальный AST вендоренного
`wptmanifest` (`DataNode`/`KeyValueNode`/`ValueNode` + `serializer.serialize`) и проверены
круговым прогоном через настоящий загрузчик `wptrunner` (`static.compile` с тем же
`data_cls_getter`, что использует сам раннер) — сгенерированный файл неотличим от написанного
руками.

Два новых флага `run_report.py`:
- `--update-expected` — (пере)пишет `.ini` под `tests/wpt/metadata/<root>/` из текущего прогона;
  требует `--all --root <категория>` и **отказывается** работать по `dom/nodes` (это отдельный,
  ручной S5/S6-гейт `run_suite.py` — переписывать его автогенератором значит стереть
  вручную-подобранные комментарии и потенциально сузить обнаруживаемый им `curated_test_ids()`).
- `--check` — гейт: exit≠0 только если `expected` был "хорошим" (`OK`/`PASS`) и стал не таким,
  или новый статус — `TIMEOUT` (зависание всегда всплывает, даже на уже сломанном тесте);
  неожиданный PASS печатается как подсказка «сузить ожидания», не валит гейт.

Найденная и исправленная в процессе тонкость: у теста с `?query`-вариантами (WPT
`variant=`/несколько URL на один файл, например `websockets/extended-payload-length.html?wss` и
`...?wpt_flags=h2`) наивный путь `<METADATA_ROOT>/<test_id>.ini` ломается двояко — `?` невалиден
в имени файла на Windows, и разные варианты одного файла на самом деле должны делить один `.ini`
(секция — `manifestupdate.get_test_name(test_id)`, «basename пути + query + fragment», то самое
значение, которым `wptrunner` сам матчит `ExpectedManifest.get_test` в `testloader.py`). Функции
`metadata_ini_path`/`_test_node_for`/`build_expected_ini` группируют варианты по файлу и переиспользуют
`wptrunner.manifestupdate.get_test_name` напрямую вместо велосипеда. Симметричная ловушка в
`classify()`: список отбора (`all_vendored_test_ids`) содержит только «голые» id файлов без query,
а строки в `results` — уже раскрытые варианты, поэтому проверка «тест пропал» сравнивалась по
URL-пути (`urlsplit(...).path`), а не по точному id — иначе легитимно прошедший вариантный тест
ложно считался «MISSING».

Проверено сквозным прогоном (не просто unit-синтетикой): baseline сгенерирован реальным `dev-release`-
прогоном на 7 уже вендоренных категориях (`websockets`, `xml`, `x-frame-options`, `webrtc-svc`,
`url` — 1314 сабтестов, крупнейшая проверка на масштаб, `urlpattern`, `uievents`); следом
`--check` на каждой — 0 регрессий (exit 0); отрицательный сценарий (вручную удалён `.ini`-оверрайд
на заведомо падающий сабтест `websockets`) — `--check` корректно поймал регрессию (exit 1,
`REGRESSION: ... expected PASS regressed`), файл восстановлен.

**Не сделано в этой сессии**: `worklets` (20 `.https.html`-тестов) не уложился в отведённое время
прогона — похоже, не зависание, а кумулятивно медленные credential/CSP/import/referrer-тесты
воркетов (несколько сетевых загрузок с ожиданием на тест); не диагностировано глубже, генерация
для него отложена. Полный охват **289** `done`-категорий WPT-VENDOR (`grep -c "| done |"
ROADMAP.md` по `WPT-VENDOR`) — многочасовая работа, для следующей сессии: гонять
`--update-expected` пакетами по категориям (уже отработанный в этой сессии цикл), с таймаутом на
категорию и пропуском/журналированием зависших вместо блокировки всего пакета.

## TEST-4: WPT reftest-executor (L)

Сейчас интеграция wptrunner исполняет только testharness-тесты — reftests (основной способ
тестировать рендеринг в WPT, весь корпус `css/`) непрогоняемы.

- Поддержка `<link rel="match">` / `rel="mismatch">` (цепочки эталонов — по спеке WPT),
  `class="reftest-wait"`, `reftest-timeout`.
- Скриншоты через детерминированный CPU-рендер (`screenshot_cpu_rgba()`, уже используется
  snapshot-тестами) — пиксельное сравнение test vs reference, с fuzzy-annotation WPT
  (`<meta name="fuzzy">`) как допуском.
- Точки интеграции: `LumenTestharnessExecutor`-класс в `tests/wpt/browsers/lumen.py` +
  `run_report.py` (recursive-ветка).
- DoD: смоук на 3–5 простых css/-reftest-ах из апстрима (вендорить точечно); зелёный/красный
  результат совпадает с ожиданием; docs/wpt-status.md обновлён.
- Зависимость: TEST-5 (Ahem) желателен до массовых прогонов css/ — иначе текстовые reftests
  зашумлены метриками шрифта.

## TEST-5: шрифт Ahem (S)

Стандартный инструмент детерминированных reftest-ов: все глифы — квадраты em, известные метрики.

- Бандл `assets/fonts/Ahem.ttf` (свободная лицензия W3C), регистрация в lumen-font наравне с
  Inter-Regular.
- Загрузка как web-font из WPT-тестов (`/fonts/ahem.css` в вендоренном дереве) должна работать
  через существующий WOFF2/TTF-путь.
- DoD: unit-тест метрик (advance == em, ascent/descent по спеке Ahem); опц. один graphic-тест
  на Ahem-типографику; лицензия в assets/fonts/.

## TEST-6 (опц.): test262 smoke для V8-эмбеддинга (M)

Сам V8 конформен test262 — проверяем **нашу интеграцию**: создание realm, глобальные объекты,
microtask-порядок, `$262`-хост-хуки. Подмножество (language/ smoke + staging по фичам,
затронутым шимом), раннер поверх headless JS-исполнения. Не гейт — информационный отчёт.

## TEST-7 (опц.): Wasm spec tests (M)

Официальный набор spec-тестов WebAssembly на wasm-путь Lumen (P3-v8-s9). Раннер + отчёт,
группировка провалов в BUG-NNN.

## TEST-8 (опц.): WebGL conformance subset (L)

Khronos WebGL conformance suite (1.0, минимальное подмножество) на `webgl_canvas`.
Делать после стабилизации canvas/webgl-пути (см. BUG-348). Не гейт — отчёт.

---

## Что НЕ входит

- Acid2/Acid3 — уже покрыто категорией WPT-VENDOR-acid (done); современного смысла как гейт
  не имеет.
- Собственный in-tree дубль WPT-тестов (путь Ladybird из-за 5.5-часового CI) — у Lumen
  вендоринг по категориям уже решает выборочность.
- Перф-тесты — отдельная дорожка PERF.
