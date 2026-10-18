# MULTICOL-FRAG-OOF — передача (ветка `p1-multicol-frag-oof`, слот p1-work)

## Сделано (WIP-коммит)

- `box_tree/multicol_flow.rs`: `scan_oof` (классификация abspos в items: `None`/`Inside`/`Unsupported` — fixed, sticky, anchor, abspos с CB вне item или под `overflow: clip`), `take_oof` (вынимает abspos из фрагментируемой цепочки item), `cut_oof` (режет abspos как блок по окнам колонок; `fragmentable(b, depth, oof)`).
- `box_tree/multicol_trampoline.rs`: `emit_sliced_fragments` вынимает abspos и режет их по окнам + пустым колонкам до нижнего края (`oof_windows`); индексы в `MulticolInit::oof_out` (не отдаются `lay_out_abs_children` как свои); `frag_cols`/`frag_segments` — геометрия колонок для abspos самого контейнера, `cut_own_abspos` в `finish_frame` после `finish_after_match` (учтён relative-сдвиг). Без `column-height` rows и без спаннеров.
- `box_tree/tests/multicol_oof.rs`: 5 тестов (проба задачи: `top: 150px` → 2-я колонка, y=50; срез по краю колонки; abspos в relpos-обёртке; статическая позиция за колонкой открывает следующую; fixed — атомарный путь). `cargo test -p lumen-layout -- --skip deep_` — 5167 зелёных до правки `clip`.
- Замер WPT (run_corpus, dev-release, 134 id: `css/css-break/out-of-flow-in-multicolumn-*` + `css/css-multicol/abspos*`): 12 -> 24 PASS (14 новых, 2 регресса: 099, 104). 104 (abspos под `overflow: clip`) исправлено последней правкой (`scan_oof` -> Unsupported), НЕ пересобрано/не перемерено.

## Дальше

1. `cargo build --profile dev-release --bin lumen`, перемерить 134 id: команда в `.tmp/wpt-oof/` — `tests/wpt/run_corpus.py --categories css --skip-manifest-update --prefixes "$(cat .tmp/wpt-oof/prefixes.txt)"` (python — `D:/RustProjects/lumen-browser/.tmp/venv-wpt/Scripts/python.exe`; сравнение `.tmp/wpt-oof/cmp.py`, каталоги `before`/`after`).
2. Регресс 099 (`columns:2` balance, `contain:size` + relpos 200px с abspos 200px): layout верный (куски 8,149.72 h50 и 116,49.72 h150 зелёные поверх красных), причина красного в рендере не найдена — смотреть `.tmp/oof/099.png` (скриншот снят, не просмотрен): порядок отрисовки / вырезанный abspos-кусок под красным relpos?
3. Разобрать верхние кластеры из 110 непрошедших (002 003 … 126, `abspos-after-spanner*`, `abspos-autopos-contained-by-viewport-*`): спаннеры (006, `abspos-after-spanner`: abspos после спаннера стоит на y=0), CB вне item (multicol position:relative с abspos в static-обёртке -> сейчас `Unsupported`, атомарный путь), column-height rows. Причину устанавливать пробой (`--dump-layout`), не по имени.
4. Гейт: `cargo clippy -p lumen-layout --all-targets -- -D warnings`, `scripts/scoped-test.sh`, графтесты (двигает пиксели: полный `python graphic_tests/run.py --continue-on-fail` + голдены в том же коммите), `dump_golden.py`.
5. Доки в том же коммите: `subsystems/layout.md` (строка Done), `CAPABILITIES.md`, `ROADMAP.md` (строка MULTICOL-FRAG-OOF: что закрыто/остаток), `docs/wpt-vendor-notes/css.md` §css-break, указатель `ROADMAP.md:1051` из `STATUS-P1.md` удалить в закрывающем коммите; удалить этот файл.
