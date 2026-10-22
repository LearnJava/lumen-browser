# Срез 100 css-gaps (p4-gap-rule-pixdiff-100): multicol `multicol-gap-decorations-007`

Ветка `p4-p4-gap-rule-pixdiff-100`, слот `p4-work` (`.claude/worktrees/p4-work`). Код ещё не менялся: сделан только разбор.

## Что должно получиться (Edge/Chrome, `007-ref.html`, вьюпорт 800×600)

Внешний multicol `columns:2; column-fill:auto; height:200; gap:20` (колонки 300 px), внутри вложенный
`columns:3; column-height:100; column-wrap:wrap; gap:20; border:2` с содержимым 60 px / спаннер 18 px / 750 px.
Сейчас вложенный контейнер остаётся монолитом в первой колонке внешнего (diff 94240 px).
Эталон по y (внутри внешней колонки 1, вложенный фрагмент 0, border-top 2):

- ряд 1 до спаннера: 2..22 (60 px сбалансированы → 20/20/20), спаннер 22..40;
- ряд после спаннера — **свежий ряд полной высоты**: 40..140 (не «остаток ряда» 62 px, как сейчас в `row_used`);
- `row-gap` 20 → следующий ряд 160..200, обрезан колонкой внешнего (cap = 200 − 160 = 40, три колонки по 40);
- колонка внешнего 2, фрагмент 1 (без border-top, слайс): ряд 0..100 (3×100), gap 20, последний ряд 120..130 (330−300=30 → 10/10/10, сбалансирован), border-bottom 2 → высота фрагмента 132.
- правила: column-rule в полосах (2..22, 40..140, 160..200 | 0..100, 120..130), row-rule по центру row-gap (145..155 и 105..115); у спаннера нет ни щели, ни правила.

Ожидание «свежий ряд» — только во вложенной (фрагментируемой) раскладке. Автономный путь (`row_used` в
`emit_sliced_fragments`) НЕ трогать: он даёт верные `css-multicol/column-height-006/013/017-020` и css-gaps 002/008-012.

## План (ничего из этого ещё не написано)

1. `multicol_nested.rs`: `NestedFlow.items` → `segs: Vec<NestedSeg>` (`Cols(Vec<(LayoutBox, mt, mb)>)` | `Span(LayoutBox, mt, mb)`),
   плюс `rows: Option<(col_h, row_gap)>`. Старый `cut_nested` (один сегмент без рядов) оставить, брать items из `segs[0]`.
   Новая `cut_nested_rows(nf, b, h, mt, cur_y, col_dx, viewport)`: идти по сегментам с курсором `(m, y)`
   (`m` — фрагмент = колонка внешнего, `y` — от верха контента фрагмента; `avail(m) = h − top(m) − (clone? db:0)`,
   `top(m) = (m==0? mt:0) + (m==0||clone? dt:0)`, `ct(m)` = страничный y верха контента = `cur_y + top(m)`):
   - спаннер: если `y>0` и не влезает — в следующий фрагмент; `shift_tree(clone, col_dx(m), ct(m)+y+smt − rect.y)`; `y += smt+h+smb`;
   - Cols: ряд за рядом. Перед рядом (не первым в сегменте) `y += row_gap`; если `avail(m)−y < 1` и `y>0` — следующий фрагмент (y=0, щель отбрасывается).
     `cap_r = min(col_h, avail(m)−y)`. Страницы — `Flow::new(items)` + `model.paginate_ex(&|g| caps[g/n], false)` по накопленным `caps`;
     ряд последний, если `pages.len() <= (r+1)*n`. Последний ряд: при `nf.balance` высота `model.balanced_tail(&|g| caps[g/n], r*n, (r+1)*n, cap_r)`,
     иначе `cap_r`; перегнать `paginate_ex` с итоговой высотой. Окно `Window{lo,hi,lead,tail,last: g+1==total, extent: высота ряда,
     col_top: ct(m)+y_ряда, dx: col_dx(m)+nf.content_x+j*(col_w+col_gap)}`, куски `cut(item, off, &win, 0)`, у `InlineRun` `rect.width = col_w`.
   - фрагменты как в старом `cut_nested`: `shell(b)`, `rect.x = b.rect.x + col_dx(m)`, `rect.y = cur_y + (m==0? mt:0)`,
     высота = `top_edge + content_h + bottom_edge` (не последний: `content_h = avail(m)`; последний: конец контента),
     `slice_edges(&mut frag, m>0 && !clone, !last && !clone)`; `extent = max(y + height − cur_y)`.
   Измеренные в Measure-фазе items стоят в x=0,y=0, спаннер — в `(content_x, cur_y)`; поэтому спаннеру сдвиг по x — `col_dx(m)`, колонкам — `col_dx(m)+content_x+j*pitch`.
2. `multicol_trampoline.rs`: `keep_nested_flow` (≈стр. 714) принимает контейнер с `col_rows` (условия: `container_h` нет, `position: static`;
   без рядов — как раньше один сегмент без спаннера), и **дописывает** сегмент в `nested_out`, а не перезаписывает; `post_span_item` (≈стр. 2032)
   при `keep_flow` дописывает `Span(c.clone(), mt, mb)` (mt/mb — style.margin_* против `content_width`; создаёт поток, если спаннер первый).
   Поток годен, только если `segs.len()` = число непустых колонок-сегментов + число спаннеров — проверять там, где `nested_out.take()` (≈стр. 337),
   иначе выбросить (атомарный/сеточный/вложенный сегмент не записывается). `emit_nested_fragments` (≈стр. 751): при `nf.rows.is_some()` звать `cut_nested_rows`.
   `wants_nested_flow` проверяет ряды ВНЕШНЕГО (`init.col_rows.is_none()`) — не менять.
3. Paint `crates/engine/paint/src/multicol_gap_decorations.rs::read_bands` (ветка со спаннером): полоса после спаннера — **свежий ряд**, если
   высота её первого фрагмента не умещается в остаток `row_start + col_h − top` (в 007: bottom 140 > 2+100); тогда `row_start = top`, `preceded=false`... но
   спаннер всё равно не даёт щели до неё (`gap_after` предыдущей полосы = false, когда между ними спаннер; row-gap — между 140 и 160 внутри одного ряда-цепочки).
   Нижняя граница последней полосы: `min(row_start + col_h, низ контента бокса)` — только если у контейнера `height: auto` (в `emit_column_rules`
   `content_h` уже считается из rect; для автономных рядов это no-op). Это даёт 160..200 (фрагмент 0, обрезан внешним) и 120..130 (сбалансированный хвост фрагмента 1).
4. Тесты в `crates/engine/layout/src/box_tree/` рядом с `multicol_nested_break.rs` (`nested_multicol_*`): три колонки по 20 до спаннера, ряд 40..140,
   ряд-обрезок 160..200, второй фрагмент 100/100/100 + 10/10/10. Плюс paint-тест на полосы фрагмента.
5. Документы в том же коммите: `CSS-SPECS.md` (ячейка CSS Gap Decorations L1 — дописать «Срез 100»; строка очень длинная — править через
   `Edit`/python с `newline=''`), `CAPABILITIES.md` при необходимости, `STATUS-P4.MD` — удалить указатель задачи, когда задача закрыта.
   Не сделано останется: `writing-mode` (multicol `…-writing-mode`, `…-multi-value-writing-mode`).

## Как проверять

- Бинарь собран в слоте: `target/dev-release/lumen.exe` (сборка 22 мин, `CARGO_INCREMENTAL=0 cargo build --profile dev-release --bin lumen`; запускать в фоне, лог `.tmp/build100.log`).
- Снимок: `target/dev-release/lumen.exe --viewport 800x600 --screenshot .tmp/m007.png file:///<abs>/tests/wpt/css/css-gaps/multicol/multicol-gap-decorations-007.html`
  (эталон `…-007-ref.html`); профиль цветов по столбцу: `python .tmp/col.py <png> <x> <y0> <y1>` (в эталоне при x=20: 2-21 розовый, 22-39 серый, 40-139 розовый, 140-144 cyan, 145-154 зелёный, 155-159 cyan, 160-199 розовый).
- Пиксельное сравнение: `python .tmp/gaps_pixdiff.py multicol/multicol-gap-decorations-007` (весь `css-gaps` — 7 мин, базовый прогон `.tmp/gaps_base99.log`:
  304 identical, 8 thick; после среза не должно стать хуже, ожидается 305).
- Гейт перед закрывающим коммитом: `cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings` + таргетные тесты
  (`scripts/scoped-test.sh`), графические тесты — полный `python graphic_tests/run.py --continue-on-fail` (рисование меняется) либо, если захват экрана недоступен, `python graphic_tests/dump_golden.py` и пометка об этом в сообщении коммита.
- Машина медленная: `find`/широкий `grep` по репозиторию зависают — искать через `Grep` с узким `path`.

## Состояние на конец сессии 2 (код написан, `cargo check` чист, 007 ещё 94240 px)

Сделано (всё в ветке, WIP-коммит):
- `multicol_nested.rs`: `NestedSeg { Cols, Span }`, `NestedFlow.segs/rows`, новая `cut_nested_rows` (курсор `(m, y)`, свежий ряд после спаннера,
  обрезок ряда по концу фрагмента, балансировка последнего ряда), `ComputedStyle::multicol_fragment` ставится на фрагменты.
- `multicol_trampoline.rs`: `nested_flow_eligible/nested_flow_mut/nested_flow_complete`, `keep_nested_flow` дописывает сегменты,
  `post_span_item` дописывает `Span`, `emit_nested_fragments` зовёт `cut_nested_rows` при `nf.rows.is_some()`.
- paint: `MulticolGeom::fragment_bottom` (из `multicol_fragment`), `read_bands` — свежий ряд после спаннера (`bottom <= row_start + col_h`),
  `gap_before` (нет row-gap между полосой и свежим рядом вплотную к спаннеру), `clip()` нижней границы последней полосы.
- Бинарь `target/dev-release/lumen.exe` собран с этим кодом (5 мин, `CARGO_INCREMENTAL=0 cargo build --profile dev-release --bin lumen`);
  базовый бинарь слота p4-work скопирован в `.tmp/lumen-base.exe` (срез 99).

Результат: `python .tmp/gaps_pixdiff.py multicol/multicol-gap-decorations-007` → thick 94240 (как до правок): вложенный поток НЕ режется.
Дальше (отладка, в таком порядке):
1. Почему `emit_nested_fragments` не срабатывает: добавить временный `eprintln!` в `keep_nested_flow`/`nested_flow_complete`/`emit_nested_fragments`
   (достигается ли `keep_nested_flow` для обоих Cols-сегментов? `nested_flow_complete` считает `segs.len()` против числа сегментов с items + спаннеров;
   `wants_nested_flow` требует у внешнего `seg.sliceable && item_idxs.len()==1`; `is_cuttable_multicol(b)` требует, чтобы дети внутри бокса —
   у вложенного с рядами/спаннером монолитная раскладка может выпускать детей за rect; `forced_rows_wanted(frame)` может обходить `emit_sliced_fragments`
   для внутреннего; `scan_oof`).
2. Тест слоя layout рядом с `multicol_nested_break.rs` (три колонки по 20 до спаннера, ряд 40..140, обрезок 160..200, второй фрагмент 100/100/100 + 10/10/10).
3. После 007: полный `python .tmp/gaps_pixdiff.py` (7 мин; базово 304 identical, 8 thick, `.tmp/lumen-base.exe` для сравнения — поменять `r.BIN`),
   `cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings`, `scripts/scoped-test.sh`, `graphic_tests/dump_golden.py`,
   документы (`CSS-SPECS.md` ячейка css-gaps «Срез 100» через python с `newline=''`), удалить указатель из `STATUS-P4.MD`.
