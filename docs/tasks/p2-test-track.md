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

### TEST-1: состояние (2026-08-18, сессия P2)

Реализация закоммичена на неслитой ветке `p2-test-1` (branch = сигнал резервации
задачи, main не трогала): `fuzz/Cargo.toml` (не член основного workspace, пустая
`[workspace]`), 5 таргетов в `fuzz/fuzz_targets/`, курированный seed-корпус
(56 файлов) в `fuzz/corpus/<target>/` из `graphic_tests/`, `samples/`,
`assets/fonts/`, `crates/engine/image/tests/fixtures/`, `tests/wpt/`, `fuzz/README.md`.

Весь Rust-код **успешно компилируется и линкуется** до финальной стадии
(`cargo +nightly fuzz build --no-include-main-msvc` дошёл до линковки на всех
5 таргетах — сигнатуры входных точек verified: `lumen_css_parser::{parse,
parse_inline_style, parse_selector_list}`, `lumen_html_parser::parse`,
`lumen_core::url::Url::{parse, resolve}`, `lumen_font::{maybe_decode_font,
Font::parse}`, `lumen_image::decode`).

**Реально прогнать `cargo +nightly fuzz run` на dev-машине не удалось —
окружение, не дефект харнесса:**

1. **Нативный Windows-MSVC**: nightly `x86_64-pc-windows-msvc` от rustup не
   поставляет `librustc-nightly_rt.asan.a` (ASan-рантайм для линковки) —
   `cargo fuzz build --no-include-main-msvc` падает на линковке с "no such
   file or directory".
2. **Тот же таргет без ASan** (`-s none`, только SanitizerCoverage): линковщик
   MSVC (rust-lld в режиме `-flavor link`) не резолвит `__start___sancov_*`/
   `__stop___sancov_*` — секционные boundary-символы, которые SanitizerCoverage
   ожидает от линковщика. Это не баг конкретной версии тулчейна, а разница
   форматов объектных файлов: секционная агрегация, на которой держится этот
   трюк, — свойство ELF/Mach-O, PE/COFF (Windows) её не предоставляет тем же
   способом. Отсюда и `--no-include-main-msvc`-поддержка cargo-fuzz для
   Windows остаётся «basic»/экспериментальной даже в актуальном changelog.
3. **WSL (Ubuntu, штатный дистрибутив на этой машине)** — там нет `sudo`
   без пароля, поэтому `apt install build-essential` (C-тулчейн, нужен
   `libfuzzer-sys`'у для сборки самого рантайма libFuzzer) недоступен из
   сессии. `rustup`+nightly+`cargo-fuzz` внутри WSL установлены и рабочие —
   не хватает только компилятора C/C++.

**Для следующей сессии (любой, не обязательно P2):**
- Самый быстрый путь — пользователь один раз выполняет в WSL
  `sudo apt install -y build-essential clang`, дальше `fuzz/README.md`
  описывает весь остальной flow без дополнительных вопросов.
- Альтернатива без WSL: Linux-окружение с любым C-тулчейном (CI-контейнер,
  другая машина) — код уже готов, апробировать нужно только рантайм.
- Портативный MinGW-w64 на Windows пробовать не стоит без крайней
  необходимости: GNU ld тоже эмитит PE/COFF на этом таргете, то же
  ограничение с `__start_`/`__stop_`-секциями вероятно сохранится.

### TEST-1: состояние (2026-08-19, сессия P2) — прогон вынесен в CI

Выбрана вторая альтернатива из списка выше: `.github/workflows/fuzz.yml`,
джоб `fuzz` на `ubuntu-latest`. Локальный WSL-путь в этой сессии добить не
удалось и он остаётся необязательным: сначала `rustup` уронил установку
nightly на конфликте компонентов (`llvm-tools-preview` против уже стоящего
`llvm-tools`, файл `bin/llc`; rollback снёс `rustc` и следом упал
`cargo install cargo-fuzz`), а затем выяснилось, что диск `C:` заполнен на
100 %, из-за чего `ext4.vhdx` не может расти и корневая ФС WSL перешла в
read-only вплоть до `Input/output error` на собственных бинарниках. Оба
затыка — окружение dev-машины, к харнессам отношения не имеют.

**Локальный путь открылся 2026-08-19, тем же днём:** пользователь освободил
`C:` и доставил тулчейн в WSL. Проверено сквозным прогоном из Windows, без
интерактивной сессии в дистрибутиве — `wsl -- bash -lc 'cd /mnt/d/.../fuzz &&
cargo +nightly fuzz run fuzz_url -- -max_total_time=60'`: 651 579 итераций за
61 с (~10 700 exec/s), падений нет; тулчейн `rustc 1.100.0-nightly`,
`cargo-fuzz 0.13.2`, `clang 18.1.3`. Рецепт, замеры и ловушка с разрастанием
корпуса (одна минута прогона = 320 новых файлов в `corpus/`) — в
[`fuzz/README.md`](../../fuzz/README.md) §«Verified end-to-end». CI при этом
остаётся: он ловит регрессии без участия человека, локальный прогон нужен для
проверки фикса и минимизации входа.

Устройство джоба (детали и обоснования — в комментариях самого workflow):

- **nightly ставится только внутри джоба**; `rust-toolchain.toml` (пин 1.97.0)
  и сборка браузера не затронуты — `fuzz/` не член основного workspace.
  `llvm-tools` не запрашивается вовсе: он нужен лишь `cargo fuzz coverage`,
  а рядом с уже установленным компонентом валит установку тулчейна целиком
  (ровно грабли этой сессии).
- **Три триггера**: push, задевший `fuzz/**` (60 c на таргет — защита от
  протухания харнесса), еженедельный cron (300 c — собственно прогон),
  `workflow_dispatch` с входами `duration`/`targets` (ручной длинный прогон).
- **Один джоб, таргеты последовательно**, а не матрица из пяти: матрица
  пересобрала бы инструментированное дерево пять раз и заняла бы пять из
  20 слотов конкурентности (`docs/ci-offload.md` §4). Падение одного таргета
  не прерывает цикл — иначе один краш скрывает результаты остальных четырёх.
- **Регрессии из `fuzz/regressions/` проигрываются первыми** (одиночный
  replay, не фаззинг). Каталог пока пуст.
- **Артефакты**: краш-репро (`fuzz/artifacts/`, gitignored — в CI это
  единственная копия) заливаются при падении на 30 дней; выросший корпус —
  каждый прогон на 7 дней. Между прогонами корпус не сохраняется: поэтому
  свип еженедельный, а не ночной.

DoD брифа («таргеты собираются и гоняются») закрыт с поправкой на среду:
«гоняются» — в CI, а не локально.

**Первый же прогон дал находку — [BUG-787](../../bugs/BUG-787-FIXED.md)**
(исправлен P3 2026-08-20; репро лежит в `fuzz/regressions/`, таргет снят
с `KNOWN_FAILING` и снова блокирующий).
Прогон [32274370138](https://github.com/LearnJava/lumen-browser/actions/runs/32274370138)
(push-триггер, 60 c на таргет): 4 таргета чисто, `fuzz_image` — `ERROR:
libFuzzer: timeout after 29 seconds` на 111-байтном GIF89a 1×1. Скорость
работы фаззера для масштаба: ~9 500 exec/s, 85 тыс. итераций, `cov: 4310`
на `fuzz_css_parser`.

Находка перепроверена **вне фаззера и вне Linux**: тот же вход подвешивает
`lumen_image::decode` дольше 60 c в `dev-release` на Windows — то есть это
не артефакт ASan-сборки. Тремя пробами зависание локализовано до
`gif::Frames::decode_lzw_encoded_frame_into_buffer` (крейт `gif` 0.14.2);
детали и материал для фикса — в файле бага. Чинит P3: по брифу дорожки TEST
владелец тулинга — P2, движковые фиксы по находкам уходят P3/P1.

Репро в `fuzz/regressions/` до фикса намеренно **не** коммитился: replay-шаг CI
проигрывает этот каталог на каждом прогоне, и это повесило бы джоб
вместо отчёта о находке. Вместо этого `fuzz_image` был внесён в `KNOWN_FAILING`
воркфлоу — таргет продолжал гоняться и собирать репро, но его падение не
красило джоб (тот же размен, что `KNOWN_DEBTORS` в `graphic_tests/run.py`:
вечно красный джоб перестают читать). **С фиксом 2026-08-20 обе половины
размена отыграны назад:** репро лежит в `fuzz/regressions/fuzz_image-gif-lzw-hang`
и в `fuzz/corpus/fuzz_image/`, таргет из `KNOWN_FAILING` убран.

Обе половины проверены локально через WSL, прежде чем полагаться на CI:

```
cargo +nightly fuzz run fuzz_image regressions/fuzz_image-gif-lzw-hang
  → Executed … in 23 ms, exit 0        (весь вызов 1 мин 12 с: 60 с пересборка lumen-image)
cargo +nightly fuzz run fuzz_image -- -max_total_time=60
  → Done 16898 runs in 67 second(s), 0 крэшей, artifacts/ пуст
```

`fuzz/target/` в корневом чекауте прогрет с 2026-08-19 (там же гонялся
`fuzz_url`), и бинарь `fuzz_image` уже лежал собранным — поэтому проверка фикса
стоит минуту, а не холодную сборку инструментированного дерева. Готча из
`fuzz/README.md` подтвердилась на практике: 60-секундный прогон дописал **371**
файл в `corpus/fuzz_image/`, убрано `git clean -f fuzz/corpus/` — replay
(прогон одного файла) корпус не трогает, а фаззинг трогает.

**Второй прогон — вторая находка, [BUG-788](../../bugs/BUG-788-OPEN.md).**
Первый прогон на `main` (32277922403) 52 минуты простоял на `apt-get install
clang` и был отменён руками — apt на hosted-раннерах периодически залипает, а
шаг не давал ничего (образ уже несёт clang и g++); шаг убран, прогон
32283392133 отработал за 6,5 минут. В нём механизм `KNOWN_FAILING` показал
себя ровно как задуман: `fuzz_image` дал `::warning`, а джоб покраснел из-за
*новой* находки — `fuzz_css_parser` умер по OOM (2179 МБ при лимите 2048) на
входе в 12 КБ.

Находка так же перепроверена вне фаззера: на Windows, `dev-release`, со
счётчиком аллокаций в качестве глобального аллокатора — `parse` берёт 912 МиБ
за 2,55 с, тогда как `parse_inline_style`/`parse_selector_list` на том же
входе стоят микросекунды. ddmin по байтовым блокам ужал вход до **676 байт →
50 МиБ** (×74 000 к размеру входа), а тот же вход, повторённый дважды, не
разобрался за 300 с — то есть рост суперлинейный. Минимизированное репро (676
байт, base64) лежит в файле бага.

Практический вывод для дорожки: связка «CI гоняет фаззер — находку
перепроверяем локально обычной сборкой — минимизируем — заводим баг»
работает и не требует ни ASan, ни Linux на рабочей машине. Фаззер при этом
окупился дважды за два прогона.

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

### TEST-3: расширение охвата (2026-08-18, вторая сессия P2)

Продолжение по плану предыдущей сессии: `--update-expected --all --root <cat> --recursive` пакетами
по 4–8 категорий за раз (`timeout 65–130` на категорию, дальше — журналирование, а не блокировка).
Прогнано 43 категории из оставшихся 267 (`css`/`dom` уже покрыты ранее другим путём — WPT-RUN-3
ручные `.ini` и `dom/nodes` S5/S6-гейт, не пересчитывались): **19 получили полный baseline**
(`console`, `accessibility`, `acid`, `apng`, `audio-session`, `autoplay-policy-detection`, `avif`,
`captured-mouse-events`, `close-watcher`, `compat`, `compression`, `contacts`, `container-timing`,
`content-dpr`, `content-index`, `contenteditable`, `core-aam`, `cors`, `cpu-performance`),
**11 получили частичный baseline** (процесс убит по таймауту, но `.ini` уже частично записаны —
см. готчу ниже; `FileAPI`, `IndexedDB`, `WebCryptoAPI`, `accelerometer`, `background-fetch`,
`battery-status`, `beacon`, `bluetooth`, `connection-allowlist`, `content-security-policy`,
`custom-elements`), **13 не дали вообще ничего** за отведённый таймаут (`ai`, `ambient-light`,
`animation-worklet`, `attribution-reporting`, `audio-output`, `background-sync`, `badging`,
`browsing-topics`, `clear-site-data`, `client-hints`, `clipboard-apis`, `compute-pressure`,
`cookies`, `credential-management` — крупнее приведённого списка на 1, `cssom` дал `written=0`,
т.е. пустой файл без отклонений от `OK`/`PASS`), **4 упали сразу** (`annotation-model`,
`annotation-vocab` — «no tests selected», `annotation-protocol`, `appmanifest` — `CRITICAL Unable
to find any tests at the path(s)`, см. готчу ниже).

**Готча — «TIMEOUT» в этом журнале не означает пустой результат.** Лог `custom-elements`
(`.tmp/wpt-expected/custom-elements.log`, не закоммичен) показывает, что `wptrunner` сам штатно
доходит до `INFO Got 46 unexpected results...` и `report written to ...`, а внешний `timeout 130`
убивает процесс **после** этого — судя по всему, что-то в самом `run_report.py`/`expectations.py`
после записи отчёта не даёт интерпретатору штатно завершиться (не продиагностировано, что именно —
незавершённый поток/дескриптор). `.ini`-файлы `expectations.py` пишет по мере готовности, поэтому
у категорий с таким исходом уже есть частично годный (хоть и не гарантированно полный) baseline —
не перезаписывать их вслепую большим таймаутом, не проверив сначала, не полны ли они уже. Для
следующей сессии стоит поднять `--processes`/увеличить таймаут именно для крупных категорий
(`FileAPI` 115 файлов, `IndexedDB` 245, `WebCryptoAPI` 185 — см. их отдельные заметки в ROADMAP.md)
и, возможно, найти и исправить сам хвостовой хэнг, а не просто ждать его таймаутом.

**Готча — 4 категории проваливаются на этапе discovery, не выполнения.** `annotation-model`/
`annotation-vocab` дают «no tests selected» (0 отобранных id — вероятно, категория состоит только
из crashtest/manual/`ReadMe`-файлов, как ранее было с `accessibility`), `annotation-protocol`/
`appmanifest` падают с `CRITICAL Unable to find any tests at the path(s): /<cat>/files/index.html`
— тестовый id, который `run_report.py`'s дискавери сгенерировал, не резолвится в реальный файл
через `wptrunner`'s manifest (возможно, `index.html`/`files/` — не тест, а вспомогательная
страница, попавшая в перечень по ошибке обобщённого globbing). Не чинилось в этой сессии — не
блокирует остальной пакет, но стоит завести отдельную заметку/баг в тулинге, если таких категорий
наберётся больше при дальнейшем расширении охвата.

Осталось непокрытых **~247** категорий (267 минус 20 с любым результатом, включая частичные).
Список кандидатов и статус каждой попытки — `.tmp/wpt-expected/summary.tsv` в рабочем дереве
(не закоммичен, эфемерный; следующая сессия должна пересчитать remaining-список заново из
`grep "WPT-VENDOR" ROADMAP.md | grep "| done |"` минус `ls tests/wpt/metadata`).

### TEST-3: критический баг генератора — `.any.js`/`.window.js`/`.worker.js` писались под неверным именем (2026-08-18, та же сессия)

При попытке проверить свежесозданный `console`-baseline через `--check` сразу же (одним из
следующих шагов той же сессии) гейт вместо ожидаемого 0 показал **17 регрессий** — включая
подтесты, чей `.ini` буквально на диске уже содержал `expected: FAIL`, совпадающее с фактическим
результатом. Причина — `expectations.py::metadata_ini_path()` строила путь к `.ini` из **URL**
теста (`urlsplit(test_id).path`), а `wptrunner`'s собственный загрузчик метаданных
(`tools/wptrunner/wptrunner/expected.py::expected_path`, невендор-код, не патчится) строит его из
**исходного файла манифеста** (`ManifestItem.path`). Для обычного `.html`-теста эти два пути
совпадают, но для WPT-шаблонных типов — нет: `foo.any.js` разворачивается в `foo.any.html` /
`foo.any.worker.html` / ...; `foo.window.js` → `foo.window.html`; `foo.worker.js` →
`foo.worker.html` (`tools/manifest/sourcefile.py::global_variant_url`/`replace_end`). Раньше
`--update-expected` писал `.ini` под URL-именем (`console-is-a-namespace.any.html.ini`), а
`--check`, доверяя `wptrunner`, искал его под именем источника
(`console-is-a-namespace.any.js.ini`) — не находил, откатывался на дефолт `expected: PASS/OK`, и
любой ранее известный `FAIL` тут же читался как «регрессия». Эмпирически подтверждено: удаление
кэша `tests/wpt/metadata/.cache/` не помогло (не кэш — сразу отвергнутая гипотеза), а копирование
существующего `.ini` под именем `.any.js.ini` вручную немедленно убрало соответствующие ложные
регрессии из отчёта.

Опаснее всего то, что баг **не ограничен новыми категориями этой сессии** — `urlpattern`, одна из
7 категорий, которые предыдущая сессия явно проверяла `--check` и объявила «0 регрессий,
подтверждено сквозным прогоном», при повторном `--check` в этой сессии дала **407 регрессий** на
`urlpattern.any.html`/`urlpattern.https.any.html` (оба — `.any.js`-источники). Не разобрано до
конца, было ли это тем же баг-классом (файл `.ini` для этих тестов на момент прошлой проверки
попросту не существовал — значит на момент генерации все подтесты были чисто `PASS`, а баг
проявляется только когда появляется реальное расхождение) или независимой деградацией/нестабильным
тестом (регэкспы с astral-plane юникодом) — это отдельный вопрос для следующей сессии, не
закрывать втихую.

**Фикс (влит в этом коммите, `tests/wpt/expectations.py`)**: `metadata_ini_path()` теперь резолвит
`test_id` → путь источника через тот же самый вендоренный ридер манифеста
(`tools/manifest/manifest.load()` на `tests/wpt/metadata/MANIFEST.json`, тот файл, который
`wptrunner` использует по умолчанию — `wptcommandline.py`: «default is `${metadata_root}/MANIFEST.json`»)
вместо ручного разбора URL-суффиксов (который пришлось бы городить отдельно на каждый спецкейс:
`shadowrealm-in-*`, `print-reftest`, force-https-варианты и т.д.). Загрузка манифеста (~115k items,
25 МБ JSON) кэшируется один раз на запуск и стоит ~0.5 с — не бутылочное горлышко. Фолбэк на
старую URL-based эвристику остаётся для тестов, отсутствующих в манифесте.

**Регенерация в этой сессии**: из 30 категорий, записанных до фикса, 18 содержали хотя бы один
`.any./.window./.worker.`-тест и были удалены и перегенерированы заново; 11 успешно (`FileAPI`,
`audio-session`, `autoplay-policy-detection`, `captured-mouse-events`, `compat`, `compression`,
`console`, `contacts`, `content-index`, `cors`, `cpu-performance`), 7 не уложились даже в
увеличенный (200–350 с) таймаут повторно (`IndexedDB`, `WebCryptoAPI`, `background-fetch`,
`beacon`, `bluetooth`, `connection-allowlist`, `cookiestore`) и остались **без baseline** (чисто,
без файлов) — лучше отсутствие, чем баг под маской корректных данных. `--check` на `console`
после регенерации — 0 регрессий, exit 0 (перепроверено дважды). Итог сессии: **25 категорий с
доверенным baseline** (13 не задетых багом из этой сессии + 11 перегенерированных + `xml` из
прошлой сессии, тоже перегенерирована — 7 written), 7 категорий из этой сессии остались без
baseline из-за таймаута (см. выше).

Дополнительно проверены остальные 6 категорий прошлой сессии на риск того же бага (наличие
`.any.js`/`.window.js`/`.worker.js`-источников в вендоренном дереве, не только в уже записанном
`.ini`): `x-frame-options` и `webrtc-svc` — 0 таких файлов, багом не затронуты вообще; `uievents` —
1 такой файл, но он ни разу не породил отклонение (единственный закоммиченный `.ini`,
`ui_event_pseudo_target.html.ini`, — обычный `.html`), так что реального повреждённого артефакта
нет, перегенерация не требуется (пробовалась с таймаутом до 300 с — категория сама не укладывается,
никаких файлов не запишет, поэтому директория оставлена как была, не тронута); `url` (28 файлов) и
`websockets` (86 файлов) — ни одного `.any.`/`.window.`/`.worker.`-именованного `.ini` среди уже
закоммиченных, то есть на момент прошлой генерации все такие тесты были чисто `PASS` и баг не
успел создать неверный артефакт — риск остаётся **скрытым и будущим** (следующее реальное
расхождение в `.any.js`-тесте этих категорий запишется уже правильно фиксированным генератором, так
что регенерация не обязательна прямо сейчас, но полный `--check`-прогон этих двух крупных категорий
никогда не проводился с исправленным кодом — стоит сделать при следующем touch). `urlpattern` из
прошлой сессии требует отдельного разбора (см. выше — единственная из исходных 7 с реально
затронутым риском регрессии, а не просто отсутствием проверки).

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

### TEST-4: состояние (2026-08-19, сессия P2)

Реализовано и влито: `LumenRefTestExecutor`/`LumenIpcProtocol`
(`tools/wptrunner/wptrunner/executors/executorlumen.py`) + вайринг в
`tools/wptrunner/wptrunner/browsers/lumen.py` (`__wptrunner__["executor"]["reftest"]`).

**Решение по протоколу (не по брифу буквально)**: reftest-скриншоты идут через
`lumen --ipc-server` (детерминированный tiny-skia CPU-путь,
`crates/ipc/src/lib.rs`), а не через `browsingContext.captureScreenshot`
(BiDi), которым брифовый черновик неявно предполагал переиспользовать
`LumenTestharnessExecutor`. Причина: `captureScreenshot` рендерит через живой
wgpu-рендерер живого окна (`WinitSession::screenshot` →
`Renderer::new_headless`) — backend-зависим (Vulkan vs Dx12, см. готчу в
CLAUDE.md, срез 14 BUG-405), непригоден для пиксельного diff. `--ipc-server`
уже даёт нужный путь (`render_source_to_png`, тот же, что `--screenshot`) и
уже имел рабочий bincode-клиент на Python — `LumenIpcClient` в
`graphic_tests/run.py` (TAB-7). `LumenIpcProtocol` — независимый второй порт
того же протокола (не импорт: `graphic_tests/run.py` живёт под системным
Python, `tests/wpt/.venv` отдельно).

**Почему нужен свой `_run_server`**: `--ipc-port` в `crates/shell/src/main.rs`
документирован как чисто информационный — порт всегда выбирает ОС и печатает
его в stdout (`LUMEN_IPC_PORT=`), в отличие от `--bidi-port`, где порт
заранее выделяет сам `wptrunner`. Базовый `WebDriverBrowser._run_server`
поэтому не годится для IPC-режима (он поллит `self.port` — заранее
назначенный, для IPC несвязанный порт); `LumenBrowser._run_server`
override'ит его для `ipc_mode=True`, ожидая захвата порта/токена
`_IpcCapturingOutputHandler`'ом вместо `wait_for_service`.

**Сужение объёма (сознательное, зафиксировано в докстринге
`LumenRefTestExecutor`)**: `class="reftest-wait"` не поддержан. `NavigateTab`
у `--ipc-server` лишь сохраняет `PageSource` в слот вкладки — реальные
load+layout+растеризация происходят лениво на `Screenshot`
(`crates/shell/src/main.rs::run_ipc_server`), тем же однократным
неинтерактивным путём, что `--screenshot`/`--dump-layout` (скрипты
исполняются один раз при парсинге, но ничего не докручивает JS-события
после). У IPC-протокола и нет аналога `script.evaluate`, чтобы опрашивать
снятие класса. Смоук-фикстуры подобраны без reftest-wait — полноценная
поддержка (если понадобится для широких прогонов `css/`) требует либо нового
IPC-запроса, либо отдельного протокола поверх BiDi с CPU-скриншотом (не
существует), в этой сессии не делалось.

**Смоук (DoD)**: 4 уже вендоренных reftest-а из `css/` (категория `css/`
вендорена целиком ещё 2026-07-26, точечного вендоринга под эту задачу не
понадобилось) — `css/CSS2/normal-flow/block-in-inline-{align,baseline,
first-line}-001.html` и `css/css-backgrounds/background-334.html`, через
`python tests/wpt/run_smoke.py <id...>` (`LUMEN_PROFILE=dev-release`, Git
Bash: `MSYS2_ARG_CONV_EXCL="*"` — иначе MSYS переписывает ведущий `/css/...`
в `C:/Program Files/Git/css/...`). Результат: **3 PASS, 1 FAIL** — ровно то,
что и должно быть: три CSS2-рефтеста (без картинок) совпали с эталоном
пиксель-в-пиксель, а `background-334` (эталон — `.xht`) разошёлся на 4880
пикселей, что оказалось РЕАЛЬНОЙ находкой, не браком исполнителя (см. ниже).
Смоук подтверждает: executor корректно различает совпадение/несовпадение, не
просто всегда зелёный.

**Побочная находка — [BUG-786](../../bugs/BUG-786-OPEN.md)**: `<style>`
внутри `<![CDATA[ ... ]]>` в `.xht`/XHTML-документах теряет всё содержимое
целиком. `wptserve` отдаёт `.xht` как `application/xhtml+xml` (реальный
XML-тип), но у Lumen нет отдельного XML-пути для top-level документов —
всё уходит в HTML5-токенизатор, где `<style>` это RAWTEXT и CDATA-маркеры
остаются буквальным текстом, ломая всё правило целиком. Подтверждено
минимальным repro (`--dump-layout`), не предположением по скриншоту.
Системный класс — паттерн стандартен для старых CSS2.1-референсов WPT.
Заведено P2, чинится P1/P3.

**Не сделано в этой сессии**: `class=reftest-wait`, `<meta name="fuzzy">`
допуск (`RefTestImplementation` его уже поддерживает нативно — не
понадобился на этих 4 фикстурах, не проверялся), reftest-цепочки через
несколько `rel=match`/`rel=mismatch`. `docs/wpt-status.md` не переписан
целиком (277-строчная таблица, каждая строка уже фиксирует «reftest в X, не
исполняются раннером» — актуализация задним числом это отдельная
многочасовая работа, не входит в DoD смоука); следующая сессия, продолжающая
TEST-4 на широкий `css/`-прогон, должна это сделать по факту прогона, а не
заранее.

## TEST-5: шрифт Ahem (S)

Стандартный инструмент детерминированных reftest-ов: все глифы — квадраты em, известные метрики.

- Бандл `assets/fonts/Ahem.ttf` (свободная лицензия W3C), регистрация в lumen-font наравне с
  Inter-Regular.
- Загрузка как web-font из WPT-тестов (`/fonts/ahem.css` в вендоренном дереве) должна работать
  через существующий WOFF2/TTF-путь.
- DoD: unit-тест метрик (advance == em, ascent/descent по спеке Ahem); опц. один graphic-тест
  на Ahem-типографику; лицензия в assets/fonts/.

### TEST-5: состояние (2026-08-19, сессия P2)

Закрыто. `Ahem.ttf` уже был вендорен в `tests/wpt/fonts/Ahem.ttf` (обычный vendored WPT-ресурс,
не отдельная закачка) — скопирован байт-в-байт в `assets/fonts/Ahem.ttf` (sha256 совпадает).
Лицензия — `assets/fonts/LICENSE-Ahem.txt`, 3-Clause BSD (текст идентичен
`tests/wpt/fonts/LICENSE-WPT.md`, которым WPT лицензирует свои вспомогательные ресурсы).

**Регистрация не потребовала кода.** `SystemFontIndex::build_index`
(`crates/engine/font/src/system_fonts.rs`) рекурсивно сканирует директории из `default_font_dirs()`
+ явно заданные (тесты передают `assets/fonts/`), читает `name`/`OS/2` каждого `.ttf`/`.otf` и
индексирует по family из таблицы `name` — Inter/Golos Text/JetBrains Mono никогда не имели
специального кода на своё имя, и Ahem его тоже не получил: класть файл в `assets/fonts/` —
единственный нужный шаг. Тест `finds_bundled_inter` (`system_fonts.rs`) поднят с
`family_count() == 3` на `== 4`.

**Метрики подтверждены не «на глаз», а прочитаны напрямую через fontTools** (`python -c
"from fontTools.ttLib import TTFont; ..."`, см. историю сессии) и затем закреплены интеграционным
тестом `crates/engine/font/tests/cases/ahem_metrics.rs` (2 теста, зарегистрированы в
`tests/cases/mod.rs`) через наш собственный парсер (`Font::head/hhea/cmap/hmtx`), а не просто
скопированы из вывода fontTools: `units_per_em == 1000`, `hhea.ascent == 800` (0.8em),
`hhea.descent == -200` (0.2em ниже baseline, `ascent - descent == units_per_em`), и advance
(`hmtx.advance_width`) для `{A, X, a, z, 0, !, ., space}` — то есть букв, цифры, пунктуации и
пробела — везде ровно 1000 (1em). Это и есть контракт Ahem: сплошной квадрат ровно em×em для
любого печатаемого символа.

**Веб-шрифтовая загрузка не потребовала кода, проверено живым окном.** `@font-face { src:
url(...) }` на `.ttf` (в т.ч. на сам `tests/wpt/fonts/ahem.css`, который вендоренные WPT-тесты
используют как раз так) идёт через уже реализованный generic-путь загрузки веб-шрифтов
(`font-display: swap`, PH3-19 — см. CAPABILITIES.md): фоновый поток `fetch_image_bytes` тянет байты
относительно базового URL документа, `lumen_font::Font::parse` проверяет sfnt, движок шлёт
`LoadEvent::FontLoaded` и триггерит relayout (FOUT-swap) — TTF/WOFF2 не различаются на уровне этого
пути, Ahem ничем не отличается от любого другого `.ttf`.

Headless `--dump-layout` **не годится** для этой проверки: она однократна и завершается до того,
как фоновый fetch+`FontLoaded` успевает долететь (оба `<div>` в первой пробе измерились одинаково —
136.43px, т.е. оба на самом деле легли на дефолтный fallback, не на запрошенный веб-шрифт). Реальная
проверка — живое окно (`lumen.exe --mcp-live-port N about:blank` → MCP `navigate` на
`file://.../ahem-check/ahem.html` с `@font-face{font-family:'Ahem';src:url('Ahem.ttf')}` и
`<div style="font-family:Ahem;font-size:50px;display:inline-block">XXXX</div>` рядом с тем же
текстом в `font-family:'Inter'`), опрошенное через MCP-ресурс `resource://layout` до появления
свежей геометрии. stderr процесса подтверждает саму загрузку: `@font-face async загружен: «Ahem»
weight=400` → `FontLoaded: «Ahem» weight=400`; результирующая раскладка — `div` с Ahem получил
`border_box.width == 200.0` (ровно `4 символа × 50px` — то самое 1em-на-глиф, что закрепил
`ahem_metrics.rs`), а соседний `div` с Inter — `136.42578` (обычные, разные по ширине буквы). Не
закоммичено (скретч в `.tmp/ahem-check/`, gitignored) — воспроизводимо тем же рецептом при
следующем touch TEST-4/TEST-5.

**Графический тест не добавлен** — DoD помечает его опциональным, а его практическая ценность
раскрывается вместе с TEST-4 (reftest-executor): без него Ahem используется только косвенно,
через CPU-снапшоты обычных graphic_tests, где не нужна детерминированная типографика. Добавлять
эталон сейчас означало бы поддерживать PNG, который ничего не проверяет специфичного для Ahem
(тот же тест пройдёт и на bundled Inter). Возврат к этому пункту логичен при работе над TEST-4.

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
