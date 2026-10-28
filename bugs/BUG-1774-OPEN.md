# BUG-1774 — схлопнутый пробел между `inline-block` и следующим inline-элементом теряется

**Статус:** OPEN
**Заведён:** 2026-10-28 (P5, ревизия очередей: нарезка ANCHOR-SCROLL-3, срез 1 вынесен в баг)
**Область:** layout (`crates/engine/layout/src/box_tree/build.rs:1342-1356` — `build_box_inner`, ветка `is_inline_content(...)` при сборке `InlineBlockRow`)

## Симптом

`<div><span style="display:inline-block;width:100px;height:20px"></span> <b>foo</b></div>` —
`InlineRun` с `foo` стоит в x=100, а должен стоять правее на ширину пробела. С голым текстовым
узлом (` foo` без `<b>`) раскладка верная — x=120 у Ahem 20px (ширину пробела учитывает
`inline_run_lead_space`, `crates/engine/layout/src/layout_dispatch/dispatch_box.rs:~1418`).
Замер — бинарь `dev-release` слота `p1-work`, 2026-10-28.

Из-за этого не проходят WPT `css/css-anchor-position/anchor-scroll-001.html` и
`anchor-scroll-update-001/002/003/004/007.html`: якорь в них — inline-block, за которым через
пробел идёт inline-элемент. В строке ROADMAP ANCHOR-SCROLL-3 причиной был назван BUG-1477, но он
уже закрыт; реальная причина — эта.

## Механизм

`had_ws` дописывает пробел только в `pending.last_mut()`. Когда предыдущим соседом был атомарный
inline (`pending` пуст, `row_items` не пуст), дописывать некуда, и пробел пропадает.

## Как чинить (предложение)

До вызова `collect_inline_segments` вычислить
`lead_space = had_ws && pending.is_empty() && pending_escapes.is_empty() && !row_items.is_empty()`.
После вызова, если в `pending` появился новый сегмент (`pending.len() > before`), его
`style.white_space` не сохраняет пробелы, а текст не начинается с `is_wrap_whitespace`, —
вставить `' '` в начало текста первого нового сегмента. Именно префикс-пробел, а не
`BoxKind::InlineSpace`: ширина `InlineSpace` берётся как `char_width(' ')` и у Ahem даёт 5 px
вместо 20, а префикс идёт тем же путём, что ` foo`, и меряется верно.

Не трогать: ветку атомарного inline (`build.rs:1376`, `InlineSpace` между двумя inline-block),
`segment_break.rs`, `inline_wrap`.

## Готово, когда

1. Новый unit-тест рядом с `crates/engine/layout/src/box_tree/tests/anchor_inline.rs`:
   для разметки из «Симптома» `InlineRun` стоит правее блока ровно на ширину пробела (то же
   значение, что у варианта ` foo` без `<b>`).
2. Шесть id WPT выше дают PASS (`tests/wpt/run_report.py --root css/css-anchor-position`
   или попиксельно через `tests/wpt/reftest_pixdiff.py` — ожидается `identical`).
3. Гейт: `cargo clippy -p lumen-layout --all-targets -- -D warnings`, `cargo test -p lumen-layout
   anchor_inline`; раскладка двигает пиксели — полный `python graphic_tests/run.py
   --continue-on-fail` и перегенерация эталонов в том же коммите.
