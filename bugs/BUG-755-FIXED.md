# BUG-755 — Forced Colors Mode нельзя включить в автоматизированном прогоне: вся WPT-категория `forced-colors-mode` непроверяема

**Статус:** FIXED 2026-09-14 (P3)
**Компонент:** shell (`crates/shell/src/window_mode.rs` — `a11y_store` init),
CLI (`crates/shell/src/cli_args.rs::extract_forced_colors`), tooling
(`tools/wptrunner/wptrunner/browsers/lumen.py`)
**Найден:** P3, BUG-388 (2026-08-10), прогон
`run_report.py --all --root forced-colors-mode --recursive`

## Симптом

Forced Colors Mode (CSS Color Adjust L1 §3) включается **единственным** способом
— тумблером в a11y-панели живого окна. Хранилище настройки создаётся как
`A11yPrefs::open_in_memory()`, то есть каждый процесс стартует с
`DEFAULT_FORCED_COLORS = false` и ничего не помнит между запусками. Ни
CLI-флага, ни переменной окружения, ни BiDi/MCP-команды, ни pref-файла у режима
нет.

Следствие: в любом headless/автоматическом прогоне `forced_colors_active()`
всегда `false`, и вся вендоренная категория `forced-colors-mode` (14 тестов,
38 сабтестов) проверяет не то, что называет.

## Почему это не «просто не покрыто тестом»

Тесты категории при выключенном режиме **не падают, а зеленеют вхолостую** —
типичный ложно-зелёный:

* `forced-colors-mode-41.html` — 9/9 PASS на
  `assert_not_equals(value, "rgb(0, 128, 0)")`: любое значение, кроме авторского
  зелёного, считается успехом, включая пустую строку;
* `forced-colors-mode-27.html` — 1/1 PASS на
  `assert_equals(html_color, div_color)`: `"" === ""` тоже равенство.

То есть отчёт `forced-colors-mode` сейчас читается как «основной набор цветовых
свойств форсируется правильно», хотя прогон вообще не входил в режим. Именно на
этом основании в [BUG-388](BUG-388-FIXED.md) был сделан вывод «27 и 41 PASS ⇒
forced colors реализован по существу» — вывод верный по коду (юнит-тесты
`forced_colors_*` в `style.rs` его подтверждают), но полученный не оттуда,
откуда казалось.

## Причина

`A11yPrefs::open_in_memory()` (`crates/shell/src/main.rs:1177`) — единственная
точка создания хранилища; персистентного пути нет вовсе (ср.
`A11yPrefs::open(path)`, `crates/storage/src/a11y_prefs.rs:128` — публичный API
есть, вызовов ноль). Значение доезжает до движка через
`lumen_layout::set_forced_colors(self.a11y_store.forced_colors())` в трёх местах
(`main.rs:8795`, `10086`, `10589`), так что достаточно повлиять на источник.

## Как чинить

Любой из вариантов (первый — минимальный):

1. CLI-флаг `--forced-colors` (и/или `LUMEN_FORCED_COLORS=1`), выставляющий
   `a11y_store.set_forced_colors(true)` до первой раскладки. Дальше
   `browsers/lumen.py::browser_kwargs` добавляет флаг, когда `run_info_data`
   помечен forced-colors, — и категория становится проверяемой.
2. BiDi/MCP-команда на переключение a11y-настроек — заодно закрывает
   `prefers-reduced-motion`/`cursor-size`, у которых та же проблема.
3. Персистентный `A11yPrefs::open(browser_data_dir()/a11y.db)` — но это меняет
   поведение продукта, а не только автоматизации.

Проверка: `run_report.py --all --root forced-colors-mode --recursive` с
включённым режимом; ожидание — 27/41 остаются PASS **по существу**, а не
вхолостую.

## Связанные

* [BUG-388](BUG-388-FIXED.md) — форсирование `scrollbar-color`/
  `font-variant-emoji`; движковая часть закрыта, но её WPT-подтверждение
  упирается в этот баг и в BUG-443/555.
* [BUG-443](BUG-443-FIXED.md) / [BUG-555](BUG-555-DUPLICATE.md) — второй барьер той же
  категории: `getComputedStyle()` из инлайнового `<script>` во время разбора
  возвращает `""` для ВСЕХ свойств, а тесты `forced-colors-mode` читают стиль
  именно оттуда. Пока не закрыты оба, `forced-colors-mode-54/60` красные
  независимо от движка.

## Исправление (2026-09-14, P3)

Реализован вариант 1 из «Как чинить» — минимальный CLI-флаг:

* `cli_args::extract_forced_colors` — новый экстрактор по образцу
  `extract_click_log`: флаг `--forced-colors` ИЛИ переменная окружения
  `LUMEN_FORCED_COLORS=1`. Подключён в цепочку `extract_*` перед диспетчером
  `CliMode` и прокинут только в `run_window_mode` (единственный конструктор
  живого `Lumen`-состояния, где вообще существует `a11y_store`).
* `window_mode.rs` — `a11y_store` теперь строится через промежуточную
  привязку: если флаг взведён, сразу после `open_in_memory()` вызывается
  `set_forced_colors(true)`, до первого прохода layout. `set_forced_colors()`
  (`relayout.rs:137`) читает `self.a11y_store.forced_colors()` заново на
  КАЖДЫЙ relayout-проход, а не один раз при старте, так что установка до
  первого прохода действует немедленно, без дополнительной синхронизации.
* `tools/wptrunner/wptrunner/browsers/lumen.py` — `browser_kwargs` читает
  `LUMEN_FORCED_COLORS` из окружения **самого wptrunner-процесса** (не
  дочернего: `LumenBrowser.__init__` строит для ребёнка отдельный узкий
  `env`-словарь, куда внешняя переменная не протекла бы) и передаёт в
  `LumenBrowser` булев `forced_colors`; `make_command()` добавляет
  `--forced-colors` только для `--bidi-port` (testharness). `--ipc-server`
  (reftest) сознательно не тронут: та ветка рендерит через
  `render_source_to_png`, у которого `a11y_store` нет вовсе — флаг был бы
  молча проигнорирован, и добавление его в команду создавало бы ложное
  впечатление, что forced-colors reftests уже работают.

Проверка: `LUMEN_FORCED_COLORS=1 python run_report.py --all --root
forced-colors-mode --recursive` — команда теперь существует и доходит до
движка; сам прогон в этом срезе не запускался (WPT-стенд — отдельный трек,
см. `project_e2e_track_external_stand` в памяти проекта). Живое
подтверждение «27/41 PASS по существу» и второй барьер
(`forced-colors-mode-54/60`, зависит от уже закрытых BUG-443/555) остаются
открытым остатком для следующего WPT-прогона категории.

`cargo clippy -p lumen-shell --all-targets -- -D warnings` чист;
`cargo test -p lumen-shell forced_colors` — 1 новый тест зелёный
(`cli_args::forced_colors_tests::flag_present_is_extracted_and_removed`),
плюс уже существующий `panels::a11y_panel::tests::hit_forced_colors_toggle`
не затронут.
