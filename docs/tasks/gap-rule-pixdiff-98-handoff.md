# Срез 98 css-gaps (p4-gap-rule-pixdiff-98) — заметка передачи

Ветка `p4-gap-rule-pixdiff-98`, слот `p4-work`, база — `main` на срезе 97 (`5ec7b2f62`). Кода среза нет: сессия ушла на разведку и проект. Базовый прогон (`dev-release`, 800×600): identical 303, thick 8, thin-only 2, no-match-ref 92.

## Остаток css-gaps (px различий)

- `flex/fragmentation/008` — 2500; `multicol-gap-decorations-006` — 90 (thin); `-007` — 94240; `multicol-…-writing-mode` 11400 и `…-multi-value-writing-mode` 21700; `grid-gap-decorations-024/025/027/visited-alpha-clamp` (`:visited`, не чинится); `subgrid/fragmentation/002` — 30 (thin, АА).

## Выбранная цель: 006 (и далее 007) — вложенный multicol внутри внешнего с `column-fill: auto`

Причина: `fragmentable()` в `multicol_flow.rs` отсекает `column_count.is_some()`, поэтому вложенный multicol — монолит, внешний его не режет (в рендере Lumen и тест, и ref — один 145-px блок в 1-й колонке). У 006 различаются только правила: Lumen рисует одну полосу x=108, y 18..142; Chrome — две: 80 px в колонке 1 внешнего и 45 px (`x=318`) в колонке 2. Ref (`-ref.html`) тоже рендерится нашей раскладкой, поэтому тест пройдёт, если фрагменты вложенного контейнера дадут высоту контента 80 и 45.

Ожидание Chrome для 006 (сверено по геометрии ref): внешний — 3 колонки по 200 px, высота 100; вложенный `columns:2; padding:10; box-decoration-break: clone; gap:10`, ребёнок 250 px. Фрагмент 1 (внешняя колонка 1): высота столбца 100−10−10 = 80, обе внутренние колонки по 80 (160 px потока), коробка 100 px. Остаток 90 px идёт во фрагмент 2 (внешняя колонка 2), там внутренний `balance` даёт 45+45, коробка 45+20 = 65 px. Правила — по `emit_column_rules` из высоты коробки (`content_h = rect.height − border − padding`), так что отдельной правки painter'а не нужно.

## Проект (не начат)

1. `MulticolInit` получает `keep_flow: bool` и `Frame` — `nested: Option<Box<NestedFlow>>`. В `run()` при `Descend` из фазы Measure: `child_init.keep_flow = parent.sliceable && parent.container_h.is_some() && !parent.balance && parent.col_rows.is_none() && сегмент из одного item`.
2. Внутренний кадр в `emit_sliced_fragments` (при `keep_flow`, один сегмент без spanner/rows/oof) клонирует измеренные items `(LayoutBox, mt, mb)` и параметры (`n`, `col_w`, `col_gap`, `balance`, `content_x`, отступы коробки) в `NestedFlow`. Дальше обычная нарезка — это запасной путь. При всплытии кадра (`run`, ветка `Some(mut parent)`) `nested` кладётся в `parent.init.nested[work idx]`.
3. Внешний `emit_sliced_fragments`: если сегмент — один item с `NestedFlow`, внешний `!balance`, `container_h = Some(H)`, без rows — новая ветка. Окна потока: `Flow::new(items)`, `model.paginate_ex(&h_of, false)` с `h_of(m)` = высота внутренней колонки фрагмента `m / n` (H минус декорация сверху/снизу; при `clone` обе всегда, при `slice` верх у первого, низ у последнего, `slice_edges`). Последний фрагмент при внутреннем `balance` — бисекция наименьшей высоты (`balanced`-подобная, с префиксом уже зафиксированных окон). Для каждого фрагмента: `shell(inner)` с `rect = (x+dx_внешней_колонки, cur_y(+mt у первого), width, высота)`; дети — `multicol_flow::cut(item, off, &Window{ lo, hi, lead, tail, last, extent: h, col_top: frag_y + декорация_сверху, dx: dx_внешней_колонки + content_x_внутр + j·(col_w+gap) }, 0)`. Не-последний фрагмент занимает всю колонку внешнего (H), последний — по содержимому. Заполнить `frag_cols`/`frag_segments`, `consumed`, `cur_y`.
4. Ограничения, которые остаются (записать в CSS-SPECS строку 113): сиблинги вложенного в том же сегменте, внешний `column-fill: balance`, вложенный со spanner/rows (это `007`) — как раньше, монолит.
5. Тесты в `box_tree/tests/multicol_nested_break.rs` (есть образец `fragments_follow_box_decoration_break_slice`); `box-decoration-break` в style не разобран (CSS-SPECS строка 597), `clone`-поведение 006 задаётся самим тестом: придётся хотя бы читать это свойство (см. `box-decoration-break` — ⬜) либо считать `clone` для вложенных multicol. Решить при реализации (разбор свойства — отдельный малый шаг в css-parser/ComputedStyle).

## Прочее по остатку

- `flex/fragmentation/008`: Edge (`msedge --headless --screenshot`) кладёт item 3/4 во 2-ю колонку на `y=50..80`, при этом фрагмент 2 имеет высоту 84 (контент до 82, нижняя рамка 82..84), а 1-й — 90. Правило не выводится из одного примера; не брать без ещё одного замера.
- `writing-mode` multicol (2 теста) — нужен вертикальный multicol layout.

## Как проверять

- Сборка: `cargo build --profile dev-release --bin lumen` (холодно ≈ 6 мин; `CARGO_INCREMENTAL=0`, слот `p4-work`).
- Прогон: `.tmp/gaps_pixdiff.py "" out.json` (копия из корня `.tmp/`, использует `tests/wpt/reftest_pixdiff.py`; на Git Bash — `MSYS_NO_PATHCONV=1` для путей, начинающихся с `/css/…`), снимки одного теста — `.tmp/shot.py <path-без-.html> <tag>`, различия по блокам — `.tmp/diffbox.py a.png b.png 800 600`.
- Гейт перед закрывающим коммитом: `cargo clippy -p lumen-layout --all-targets -- -D warnings`, `multicol_*` тесты (`scripts/scoped-test.sh`), `graphic_tests/run.py`, дамп золотых.
