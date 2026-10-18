# Заметка передачи: GRID-VWM-2 (ветка `p1-grid-vwm-2`, слот `p1-work`)

Кода задачи ещё нет — сессия ушла на разведку и на стенд проверки. Ниже то, что уже измерено.

## Стенд проверки WPT (работает, 20 с на 31 тест)

На `main` testharness по BiDi стоит в TIMEOUT: BUG-1597 / BUG-1630 (`loading` в модели вкладки хрома).
Обход **только локально, не коммитить**: в `crates/shell/src/chrome_ui.rs:410` заменить
`loading: Some(t.id) == active_id && self.nav_start.is_some(),` на `loading: false,`,
`cargo build -p lumen-shell`, скопировать `target/debug/lumen.exe` в `.tmp/lumen-wpt.exe`, затем вернуть файл
(`git checkout crates/shell/src/chrome_ui.rs`). Для правок движка: собрать **без** обхода нельзя гонять WPT,
поэтому держать обход применённым при замерах и снимать перед коммитом (`git diff --stat` перед `git add`).

Скрипты лежат в `.tmp/` рабочего дерева (не в git, при потере — пересоздать):
- `.tmp/wpt_run.sh <файл-со-списком-id> <out.json> [бинарь]` — `MSYS_NO_PATHCONV=1` (иначе `/css/...` превращается в путь Git),
  `tests/wpt/.venv/Scripts/python.exe tests/wpt/run_smoke.py --binary B --log-wptreport=OUT --no-manifest-update --processes 4 <ids>`;
  python venv — в основном чекауте `tests/wpt/.venv`. Флаг `--log-wptreport=` только со знаком `=`.
- `.tmp/ids_all.txt` — 31 id задачи (`/css/css-grid/...`); `.tmp/wpt_sum.py` — сводка; `.tmp/wpt_msg.py <json> <подстрока> <n>` — сообщения упавших сабтестов.

Базовая линия (бинарь с обходом, HEAD `df9b4b13e`): **118 / 687** сабтестов по 31 файлу
(зелёные: `grid-items-minimum-width-orthogonal-001` 46/46, `grid-item-alignment-with-orthogonal-flows-vertical-lr/rl` 4/4;
частично: `grid-align-baseline-vertical` 5/11, `grid-item-alignment-with-orthogonal-flows` 3/7, `grid-minimum-size-grid-items-022..025` 12–16 из 45–60).

Все 31 теста — testharness (`check-layout-th.js`), reftest нет. Тесты с `font: Ahem` + `document.fonts.ready.then(checkLayout)`
упираются в BUG-1306 (синхронная геометрия читает оценщик шрифта, `X` = 6,82 вместо 10): `grid-items-percentage-*-vertical-*`,
`grid-minimum-size-grid-items-02x` (часть), `grid-align-justify-stretch-with-orthogonal-flows`. Для проверки раскладки без BUG-1306
можно временно (локально) обернуть тело `window.checkLayout` в `tests/wpt/resources/check-layout-th.js:211` двумя вложенными
`requestAnimationFrame` — через кадр шрифт уже Ahem (верно по BUG-1306 §Симптом); файл вернуть `git checkout`.

## Найденные причины (проверено `--dump-layout`)

1. **Строки `minmax(...)` не растут по содержимому** — и в горизонтальной, и в вертикальной сетке.
   `grid_trampoline.rs::post_probe_item` (~строка 392) учитывает вклад item'а только для `Auto|MinContent|MaxContent|Fr`,
   `Minmax` пропущен; `grid.rs` (~589) задаёт строке `Minmax(min,_)` только `min.resolve_fixed`. Проба `.tmp/p1.html`:
   `grid-template-rows: minmax(auto,0px)` с item `height:60px` даёт строку 0; `minmax(10px,50px)` — 10 (по спеке при
   неопределённой высоте 50, §12.6 maximize tracks: свободное место бесконечно); `minmax(10px,auto)` с item 30 — 10 (надо 30).
   Это причина «expected 60 but got 0» во всех `grid-items-minimum-width-vertical-*` (4 файла × 44 сабтеста),
   `grid-items-minimum-height-orthogonal-001` (46), вероятно `grid-minimum-size-grid-items-024/025` (строки).
   План: в `post_probe_item` пускать `Minmax(min, _)` с содержимым в `min` (auto/min-content/max-content) и `Minmax(_, max)` с
   `max` auto/max-content; в `finish_probe_pass` — предел роста = `max(L, base)` для `max = Length`, при неопределённой
   высоте строка = предел, при определённой — раздача свободного места поровну с заморозкой на пределах. `minmax(_, fr)` не трогать
   (это GRID-FR-ROWS).
2. **Автоматический минимум столбцов не зажимается максимумом дорожки**: `grid-minimum-size-grid-items-022`:
   `grid-template-columns: minmax(auto, 0px)` с текстом `XXXXXXXXXX` даёт ширину 68,2 (ожидается 0), `minmax(auto, 25px)` — 68,2 (ожидается 25).
   Столбец `auto` с несокращаемым словом должен быть ≥ min-content (100 при Ahem), а у нас обрезается до ширины контейнера (50).
   Искать в `grid.rs` расчёт `col_widths` (до ~560) и `grid_auto_cols.rs`.
3. `grid-items-minimum-width-orthogonal-002`: ширина контейнера 1008 вместо 70 (shrink-to-fit grid с ортогональными items).
4. `grid-item-aspect-ratio-justify-self-001/002`: ширина item'а 0 вместо 16 (aspect-ratio + `justify-self: start|end` не даёт ширину).
5. `grid-align-baseline-vertical` (4,5 …), `grid-minimum-contribution-baseline-shim-vertical-*` (0/6) — не разбирались.
6. Пункт (4) задачи (`column-rule`/`row-rule` в вертикальной сетке): `grid_vertical.rs:492` — `b.subgrid_tracks = None`, `record_own_tracks` не вызывается.

## Порядок работы дальше

1. Пункт 1 выше (общий для горизонтали и вертикали, самый большой выигрыш ≈ 250 сабтестов) + тест в
   `crates/engine/layout/src/box_tree/tests/grid_vwm.rs`.
2. Пункты 2–4, потом (3) ортогональные items из задачи, (4) `column-rule`, (5) baseline, (6) `%`/auto-margin/aspect-ratio.
3. Перед коммитом: `cargo clippy -p lumen-layout --all-targets -- -D warnings`, `scripts/scoped-test.sh`, пиксели могут двигаться →
   `python graphic_tests/run.py --continue-on-fail` и голдены в том же коммите (docs/graphic-tests.md);
   документы: CAPABILITIES.md (строка «CSS Grid in a vertical writing-mode»), ROADMAP.md (строка GRID-VWM-2, остаток → новая задача),
   wpt-vendor-notes/css.md; убрать указатель из `STATUS-P1.md` строка `ROADMAP.md:1042`.
