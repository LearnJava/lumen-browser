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

## S2 — документный «style change event» для переходов (остаток)

Два красных подтеста `css-transitions/Document-getAnimations` («CSS Transitions targetting
(pseudo-)elements should have correct order after sorting» и её `::marker`-вариант) ждут 4 и 5
переходов, получают 3 и 4: у `child` есть переход `left`, но его стиль страница не читала, а модель
переходов Lumen ленивая — снимок «до» берётся при первом `getComputedStyle()` элемента
(CSSANIM-MODEL-2-S3), поэтому переход ненаблюдённого элемента не заводится вовсе. Псевдоэлементы
здесь уже работают: у `parent` находятся собственный `left`, `::marker`, `::before` и `::after`
(проверено пробной страницей — 4 записи в правильном порядке).

- **Что нужно:** событие смены стиля на уровне документа — после перестройки стиля снять значения
  всех элементов (или хотя бы тех, чьи `transition-property` не `none`), сравнить с предыдущим
  снимком и завести переходы. Это меняет компромисс CSSANIM-MODEL-2-S3 (там снимок на элемент при
  первом чтении выбран именно из-за цены «все элементы × все свойства»), поэтому нужен замер: снимок
  документа на каждом `_lumen_style_epoch` против стоимости текущей ленивой схемы.
- **Не трогать:** уже зелёные подтесты, планировщики Rust.
- **Готово, когда:** оба подтеста PASS, `.ini` удалён; замер `getAnimations()`/`getComputedStyle()`
  до/после на странице с ~1 000 элементами не хуже 10 % (иначе — решение в `docs/engine-gaps.md`).

## Вне срезов

Три подтеста `css-animations/Element-getAnimations-dynamic-changes` (порядок списка при правке
`animation-name` на месте, сохранение `startTime` существующих объектов) — это про composite order
CSS-анимаций, а не про псевдоэлементы; остаются в `.ini` этого файла.
