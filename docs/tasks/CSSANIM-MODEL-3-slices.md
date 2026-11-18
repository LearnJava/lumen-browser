# CSSANIM-MODEL-3 — срезы: псевдоэлементы в модели анимаций

Остаток CSSANIM-MODEL-2 ([`CSSANIM-MODEL-2-slices.md`](CSSANIM-MODEL-2-slices.md) §S6): `::marker` и
`::before`/`::after` в `getAnimations()`/`getComputedStyle()`, переходы на псевдоэлементах. Строка —
[`ROADMAP.md:1392`](../../ROADMAP.md).

## Разведка (срез 1, замер 2026-11-18)

Пять файлов, `python tests/wpt/run_smoke.py --binary target/dev-release/lumen.exe … --log-wptreport`:

| файл | было | стало |
|---|---|---|
| `css/css-animations/Element-getAnimations.tentative.html` | 22/22 PASS (`.ini` протух) | 22/22, `.ini` удалён |
| `css/css-animations/Document-getAnimations.tentative.html` | 17/19 (2 красных: `::marker`, замена эффекта) | 19/19, `.ini` удалён |
| `css/css-animations/Element-getAnimations-dynamic-changes.tentative.html` | 3/6 | 4/6 (`.ini` ужат до трёх) |
| `css/css-transitions/Element-getAnimations.tentative.html` | 7/7 (`.ini` протух) | 7/7, `.ini` удалён |
| `css/css-transitions/Document-getAnimations.tentative.html` | 4/7 | 5/7 (два псевдо-подтеста — §S2) |

Замеренный на «до» список красных подтестов (для протокола; формулировка остатка в ROADMAP была
размытой и в части `css-transitions/Element-getAnimations` неверной — псевдо-подтестов там нет):

1. `css-animations/Document` «pseudo element with replaced target does not affect animation ordering»
2. `css-animations/Document` «CSS Animations targetting (pseudo-)elements … (::marker)» — 5 ожидалось, 4
3. `css-transitions/Document` «sort by transition generation» — не про псевдоэлементы
4. `css-transitions/Document` «CSS Transitions targetting (pseudo-)elements …» — 4 ожидалось, 1
5. `css-transitions/Document` «… (::marker)» — 5 ожидалось, 1
6–8. `css-animations/Element-getAnimations-dynamic-changes` — три подтеста про порядок списка

## S1 — `::marker`, порядок и переходы на псевдоэлементах (сделано 2026-11-18, `p1-cssanim-model-3`)

- **Точка входа:** `crates/js/src/shim/web_api_shim_tail_b.js` — `_wa_pseudo_targets` (псевдоэлементы
  элемента), `_wa_doc_get_animations` (порядок документа), `_wa_motion_seed`/`_wa_motion_tr_sync`
  (снимок стиля и сверка), `_wa_gap_tr_*` (переход), `_css_prop` (свойство псевдоэлемента);
  `crates/js/src/v8_runtime/install/platform.rs` — `_lumen_get_computed_style_pseudo_entries`.
- **Сделано:**
  - `::marker` — третий псевдоэлемент модели, порядок `element, ::marker, ::before, ::after`
    (CSS Pseudo-Elements L4; WPT ждёт именно его). Псевдоэлемент без бокса (`::marker` не у
    `display: list-item`) отдаёт пустой `animation-name`/`transition-duration`, стоит одного
    короткого чтения на элемент.
  - `document.getAnimations()` сортирует CSS-анимацию по **владеющему** узлу (`a._nid`/`a._pseudo`),
    а не по `effect.target`: скрипт может заменить эффект, и список не должен переставляться
    (Web Animations §5.2.3).
  - `document.getAnimations()` начинает новое «style change event»: переходы, заведённые одним
    вызовом, делят поколение и сортируются по имени свойства, заведённые следующим — идут после.
  - Переходы на псевдоэлементах: `_wa_motion_seed_pseudos` снимает снимок `::marker`/`::before`/
    `::after` вместе со снимком элемента (снимок берётся только у псевдоэлемента с ненулевым
    `transition-duration` — `shortcut:` в коде); запись живёт под ключом `nid::before:property`
    (`_wa_tr_key`), элементный ключ не изменился; объект `CSSTransition` создаётся с
    `effect.pseudoElement`, оверлей читается и через `getComputedStyle(el, pseudo)`.
  - `_lumen_get_computed_style_pseudo_entries` получил тот же запасной каскад
    (`generated_pseudo_style`), что и по-свойственный натив: `::before`/`::after` с `content: ""`
    бокса не рисуют и в снимке слоя отсутствуют, но стиль у них есть.
  - Свойство, чьё вычисленное значение равно `color` (`border-top-color: currentcolor`), не заводит
    переход при смене `color` — меняется вычисленное значение, не указанное (CSS Color L4 §4.4).
    У `::marker` с `transition: all` и `color: red → green` было 9 переходов, стал 1.
- **Готово, когда:** псевдо-подтесты `Element-getAnimations` и `Document-getAnimations` по
  анимациям зелёные; переходы на псевдоэлементах видны в `getAnimations()` — выполнено, замер выше.
- **Гейт:** `cargo clippy -p lumen-js --all-targets --features v8-backend,lumen-paint/backend-femtovg -- -D warnings`;
  `cargo test -p lumen-js --features v8-backend v8_cssanim_model v8_bug1293_motion_overlay v8_gap_rule_interp`;
  WPT по пяти файлам выше (`run_smoke.py`, `--log-wptreport`).
- **Тесты:** `v8_cssanim_model.rs` — `a_marker_sorts_between_the_element_and_the_other_pseudo_elements`,
  `a_pseudo_element_transition_has_its_own_key`, `a_property_that_follows_color_does_not_transition_with_it`.

## S2 — документный «style change event» для переходов (сделано 2026-11-18, `p1-cssanim-model-3-s2`)

Два красных подтеста `css-transitions/Document-getAnimations` («CSS Transitions targetting
(pseudo-)elements should have correct order after sorting» и её `::marker`-вариант) ждали 4 и 5
переходов, получали 3 и 4: у `child` есть переход `left`, но его стиль страница не читала, а модель
переходов Lumen ленивая — снимок «до» берётся при первом `getComputedStyle()` элемента
(CSSANIM-MODEL-2-S3), поэтому переход ненаблюдённого элемента не заводился вовсе. Псевдоэлементы
здесь уже работали: у `parent` находились собственный `left`, `::marker`, `::before` и `::after`.

- **Точка входа:** `crates/engine/layout/src/lib.rs` — `collect_transition_candidates`;
  `crates/engine/layout/src/style_map.rs` — `StyleMap::same_style`;
  `crates/js/src/v8_runtime/style_flush.rs` — `FlushHandles::snapshot_transition_stale`, вызов из
  обеих ветвей `maybe_flush` (scoped и полной) перед заменой `computed_styles`;
  `crates/js/src/v8_runtime/install/platform.rs` — `_lumen_style_change_events`;
  `crates/js/src/shim/web_api_shim_tail_b.js` — `_wa_motion_stale_seed` (из
  `_wa_doc_get_animations`) и флаг `flushed` у `_wa_motion_tr_sync`.
- **Сделано:**
  - Снимок «до» — это опубликованный `computed_styles` прошлой перестройки: он жив до того момента,
    когда ближайший flush его заменит. Поэтому Rust перед заменой копирует записи тех узлов, чей
    **style** называет переход — `StyleMap` это общий `Arc`, копия бесплатна. Кандидатов даёт
    `collect_transition_candidates`: обход `LayoutBox`-дерева, читающий только поле
    `ComputedStyle::transition_durations`; ни одна запись не сериализуется — обход опубликованной
    карты сериализовал бы стиль каждого узла (`StyleMap::get` ленив), т.е. десятки миллисекунд на
    тысяче элементов.
  - `_lumen_style_change_events()` сам делает `maybe_flush()` — это и есть «style change event»:
    перестройка стиля происходит здесь, — после чего сравнивает старые записи с новыми и отдаёт
    только **изменившиеся** свойства в *вычисленной* форме (`computed:`-ключи, `TRANSFORM_LIST_KEY`),
    той самой, с которой сравнивает `_wa_motion_read`; подставленный layout'ом used px поэтому не
    читается как изменение стиля. Узлы с тем же `Arc<ComputedStyle>` и той же used-геометрией
    (`StyleMap::same_style`) пропускаются, не разбирая свойств.
  - `_wa_motion_stale_seed` для каждого изменившегося свойства заводит запись `_wa_gap_tr` со старым
    значением и сразу зовёт `_wa_motion_read`, то есть тот же `_wa_gap_tr_value`, что и чтение стиля
    страницей. Узел при этом **не** попадает в `_wa_motion_by_nid` — иначе каждый следующий
    `document.getAnimations()` перечитывал бы у него ~300 свойств, тогда как страница этот элемент
    никогда не читала и сегодня не платит за него ничего. При первом чтении стиля элемента полный
    снимок `_wa_motion_seed` добавляет его туда сам и сохраняет заведённые записи.
  - Цена включается только после первого `document.getAnimations()` страницы
    (`transition_stale_needed`): страница, которая туда не ходит, не платит ничего.
- **Границы (осознанные):** документный вход — только `document.getAnimations()`;
  `element.getAnimations()` по-прежнему отвечает из того, что страница уже наблюдала. Псевдоэлементы
  элемента, чей стиль никто не читал, не находятся — записи элемента не несут снимка псевдоэлемента
  (его снимает только `_wa_motion_seed_pseudos`). Инлайновые элементы бокса не имеют и в кандидаты
  не попадают. Записано в `docs/engine-gaps.md`.
- **Готово, когда:** оба подтеста PASS, `.ini` удалён — выполнено: `css-transitions/Document-getAnimations`
  7/7 (`run_smoke.py`, 2026-11-18), `.ini` файла удалён; остальные четыре файла разведки не хуже
  (`css-animations/{Element,Document}-getAnimations` 22/22 и 19/19, `css-transitions/Element-getAnimations`
  7/7, `Element-getAnimations-dynamic-changes` 2/5 — те же три, что и в §«Вне срезов»).
- **Замер (2026-11-18, `--mcp` headless, `dev-release`, интерлив ветка/`main` ×3, медиана):** страница
  из 1 000 `<div>`, у каждого десятого `transition: all 1s; left: 0px`; страница читает 400 стилей,
  затем 60 раз меняет `left` и зовёт `document.getAnimations()`.
  - `getComputedStyle()`: **0.697 против 0.683 мс (+2.0 %)**.
  - `document.getAnimations()` при одинаковой работе ветвей (страница читает стиль изменённого
    элемента, у обеих по 4 живых перехода): **75.1 против 73.0 мс (+2.8 %)** — в пределах 10 %.
  - Без этого чтения `main` находит 4 перехода, ветка — 60 (ровно те, ради которых срез), и вызов
    стоит 74 против 38 мс. Платятся перечисление 60 переходов и сортировка `_wa_animations`
    (`compareDocumentPosition` на пару), а не сравнение снимков: сам `_lumen_style_change_events`
    — 2.9 мс после flush, и он же единственная новая работа в общем пути.
- **Гейт:** `cargo clippy -p lumen-js --all-targets --features v8-backend,lumen-paint/backend-femtovg -- -D warnings`;
  `cargo test -p lumen-js --features v8-backend v8_cssanim_model`;
  `cargo test -p lumen-layout transition_candidates`;
  WPT по пяти файлам выше (`run_smoke.py`) и категории `css/css-transitions`, `css/css-animations`
  (`run_corpus.py --categories …`).
- **Тесты:** `v8_cssanim_model.rs` — `a_transition_on_an_element_nobody_read_starts_at_a_document_style_change_event`;
  `deep_traversal_stress.rs` — `collect_transition_candidates_lists_only_the_boxes_that_ask_for_a_transition`.


## Вне срезов

Три подтеста `css-animations/Element-getAnimations-dynamic-changes` (порядок списка при правке
`animation-name` на месте, сохранение `startTime` существующих объектов) — это про composite order
CSS-анимаций, а не про псевдоэлементы; остаются в `.ini` этого файла.
