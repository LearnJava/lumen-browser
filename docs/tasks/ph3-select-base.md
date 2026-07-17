# Задача: Кастомизируемый `<select>` (appearance: base-select)

**Developer:** P1
**Ветка:** `p1-select-base`
**Размер:** M
**Крейты:** `lumen-css-parser`, `lumen-layout`, `lumen-shell`

## Goal

Реализовать HTML/CSS «Customizable Select»: значение `appearance: base-select` для `<select>`
включает стилизуемое дерево виджета (кнопка + выпадающий список) с псевдоэлементами
`::picker(select)`, `::checkmark`, `::picker-icon` и элементом `<selectedcontent>`, вместо
непрозрачного нативного контрола. Спек: CSS Basic UI L4 / open-ui.org «Customizable Select».

## Current state (сверено с кодом 2026-07-05)

Инфраструктура select есть, но `base-select` НЕ поддержан:

- **`appearance` парсится в 3 значения**, `base-select` отсутствует:
  `crates/engine/layout/src/style.rs:3543` — `enum Appearance { Auto, None, Compat }`.
  Парсинг `crates/engine/layout/src/style.rs:13428-13432`: `"auto"→Auto`, `"none"→None`,
  всё прочее (включая `base-select`, `menulist`) → `Compat`. То есть `base-select` сейчас
  молча схлопывается в `Compat` и ничего не меняет.
- **`<select>` рисуется как непрозрачный нативный виджет** через `FormControlKind::Select`:
  `crates/engine/layout/src/box_tree.rs:3814-3817` (тег `select`),
  `crates/engine/layout/src/box_tree.rs:3821-3824` (тег `selectlist`, Phase-0 заглушка).
  Комментарий на `box_tree.rs:3818-3820` прямо помечает `::picker(select)` / `base-select`
  как «P4 wires» — то есть не сделано.
- **Выпадающий список — императивный оверлей шелла**, а не CSS-дерево:
  `crates/shell/src/forms.rs:757` `collect_select_options`,
  `crates/shell/src/forms.rs:794` `build_select_dropdown` (хардкод цветов/размеров: фон
  `255,255,255`, подсветка `0,120,215`, константы `DROPDOWN_*`),
  `crates/shell/src/forms.rs:880` `hit_select_option`,
  `crates/shell/src/forms.rs:917` `apply_select_choice`.
  Оверлей строится в `crates/shell/src/main.rs:11209-11211`; хит-тест
  `crates/shell/src/main.rs:12252-12271`; состояние открытости — `select_dropdown_node`
  (`crates/shell/src/main.rs:4121`, `crates/shell/src/main.rs:5975`).
- **Псевдоэлементов виджета нет**: `enum PseudoElementKind`
  (`crates/engine/css-parser/src/parser.rs:345`) содержит Before/After/FirstLine/FirstLetter/
  Marker/Selection/Placeholder/Highlight/Slotted/Unknown. Парсер функциональных PE —
  `crates/engine/css-parser/src/parser.rs:3969`. Ни `::picker`, ни `::checkmark`,
  ни `::picker-icon` не распознаются (уходят в `Unknown` → неподдержаны,
  см. `pseudo_element_is_supported` `parser.rs:551`).
- **`<selectedcontent>`** как элемент нигде не обрабатывается (grep пуст).
- `strip_ua_appearance_box_styling` (`style.rs:9900`) уже умеет снимать UA-бордер/фон при
  `appearance: none` — можно переиспользовать паттерн pre-scan каскада (`style.rs:6592-6607`).

Итого: фича отсутствует, есть только нативный select и заглушечный `selectlist`.

## Entry points

- `crates/engine/layout/src/style.rs:3543` — `enum Appearance` (добавить вариант `BaseSelect`).
- `crates/engine/layout/src/style.rs:13428` — рукав парсинга `appearance`.
- `crates/engine/css-parser/src/parser.rs:345` — `enum PseudoElementKind`.
- `crates/engine/css-parser/src/parser.rs:3737` / `:3969` — распознавание PE (простые/функц.).
- `crates/engine/layout/src/box_tree.rs:3814` — построение бокса `<select>`.
- `crates/shell/src/forms.rs:794` — текущий императивный dropdown (источник геометрии/цветов).
- `crates/shell/src/main.rs:11209` — точка, где оверлей строится и показывается.

## Срезы (декомпозиция)

### Срез 1 — XS — значение `appearance: base-select`
Добавить вариант `Appearance::BaseSelect` в `style.rs:3543`; в рукаве парсинга
`style.rs:13428-13432` распознать `"base-select"` (и по желанию `"base"`). Reset-ветка
`style.rs:15711` без изменений. Юнит-тест на парсинг рядом с `appearance_basic` (`style.rs:28251`).

### Срез 2 — XS — псевдоэлементы виджета в css-parser
Добавить в `PseudoElementKind` (`parser.rs:345`) варианты `Picker(select)` / `Checkmark` /
`PickerIcon` (минимально — `::picker(select)`, остальные при наличии времени). Распознавание:
функциональный `::picker(...)` в `parse_functional_pseudo_element` (`parser.rs:3969`);
простые `::checkmark`/`::picker-icon` в `parser.rs:3737`. Обновить `pe_to_css_str`
(`parser.rs:676`) и `pseudo_element_is_supported` (`parser.rs:551`). Юнит-тесты на парсинг
селекторов.

### Срез 3 — S — дерево бокса base-select в layout
В `box_tree.rs:3814` при `style.appearance == BaseSelect` строить не непрозрачный
`FormControlKind::Select`, а стилизуемое дерево: кнопка-триггер (генерируемый контейнер) +
`<selectedcontent>` (клон текста выбранной опции) + опциональный `::picker-icon`. Опции
(`<option>`) становятся обычными layout-боксами внутри `::picker(select)`-контейнера
(изначально `display:none`, раскрывается при popover-открытии — Срез 4). Начать с button+
selectedcontent; picker-дерево отдельным подшагом.

### Срез 4 — S — интеграция с оверлеем/поповером шелла
Когда select в режиме `base-select`, показ списка должен рендерить CSS-дерево `::picker(select)`
(author-стилизуемое), а не хардкод `build_select_dropdown` (`forms.rs:794`). Минимальный путь:
в `main.rs:11209` ветвить по `appearance`: `Auto/Compat` → старый нативный оверлей,
`BaseSelect` → генерируемое дерево из display-list layout-а. Хит-тест опций
(`main.rs:12252`) переиспользовать через геометрию боксов, а не `hit_select_option`.

### Срез 5 — XS — доки/тесты
`CAPABILITIES.md` (forms), `CSS-SPECS.md` (appearance: base-select), `subsystems/layout.md`.
Graphic-тест (см. ниже).

## Tests

- Юнит (css-parser): `::picker(select)`, `::checkmark` парсятся в новые `PseudoElementKind`.
- Юнит (layout): `appearance: base-select` → `Appearance::BaseSelect`; бокс-дерево содержит
  button + selectedcontent (проверка структуры `LayoutBox`).
- Юнит (shell): при `BaseSelect` оверлей строится из layout-дерева, а не `build_select_dropdown`.
- Graphic-тест: новый файл в `graphic_tests/` (магента-рамка) — `<select>` c
  `appearance:base-select` + author CSS на `::picker(select)`/`option`; демо в
  `1000000-final.html`; строка в `run.py`; `COVERAGE.md`.

## Definition of done

- [x] `appearance: base-select` парсится в `Appearance::BaseSelect`. (срез 1)
- [x] `::picker(select)` (мин.), опц. `::checkmark`/`::picker-icon` — парсятся и поддержаны. (срез 2)
- [x] `<select appearance:base-select>` строит стилизуемое дерево (button + `<selectedcontent>`). (срез 3)
- [x] Author-CSS на `option` применяется к раскрытому списку (`build_base_select_dropdown`, срез 4). `::picker(select)` контейнер — только фон из `background-color` на `<select>`; полная стилизация контейнера + author-геометрия строк отложены (см. ниже).
- [x] Клик по опции меняет выбор — переиспользован `apply_select_choice`/`hit_select_option` (геометрия строк намеренно совпадает с нативным dropdown). (срез 4)
- [x] Нативный select (`Auto`) не задет — регрессий нет (регресс-гард в layout-тестах).
- [x] Юнит зелёные; доки обновлены (CSS-SPECS, subsystems/layout.md).

### Отложено (follow-up)
- **Graphic-тест.** base-select — новая фича без Edge-эталона в репо, а сам picker виден только по клику (не в статичном graphic-тесте). Закрытый триггер (срез 3) стилизуется author-CSS на `select`, но strict-порог 0.5% против Edge потребует KNOWN_DEBTORS-эталона. Покрытие обеспечено детерминированными юнит-тестами (структура бокса + display-list dropdown). Отдельная задача при появлении Edge-эталона.
- **Полная стилизация `::picker(select)`** (border/padding/радиусы контейнера) и **author-driven высота строк** — требуют вынести опции в box-tree как реальный top-layer поповер (не shell-оверлей с фиксированной геометрией). Отдельный срез.
