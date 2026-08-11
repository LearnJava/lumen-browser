# BUG-398 — Graphics ARIA roles (`graphics-document`/`graphics-object`/`graphics-symbol`) не распознаются, откатываются к Generic

**Статус:** FIXED 2026-08-11 (P3)
**Компонент:** a11y (`crates/engine/a11y/src/roles.rs::AXRole::parse`, `AXRole` enum;
потребитель — `crates/engine/a11y/src/lib.rs::resolve_role`)
**Найден:** P2, WPT-VENDOR-graphics-aam (2026-07-28), прогон не дал сигнала —
находка получена прямой пробой на движке (Rust-тест на `build_ax_tree`), не прогоном

## Симптом

Категория `graphics-aam` (`tests/wpt/graphics-aam/`, 6 файлов, все `*-manual.html`)
целиком состоит из ATTAcomm-тестов (ручная сверка с ассистивными технологиями,
тот же формат, что `core-aam`/`dpub-aam`) — `run_report.py --all --root
graphics-aam --recursive` даёт `no tests selected`, потому что `all_vendored_test_ids()`
по правилу исключает любой файл с `-manual` в имени, а таких файлов здесь 100%.
Прогон не даёт сигнала by design, не как признак проблемы.

По правилу «пробуй даже без сигнала» — прямая проба через Rust-тест на
`lumen_a11y::build_ax_tree`:

```rust
let tree = build_tree(r#"<div role="graphics-document" aria-label="house">
  <div role="graphics-object" aria-label="door"></div>
</div>"#);
```

Оба узла попадают в `AXRole::Generic` (подтверждено выводом
`collect_roles_dfs(&tree.root, AXRole::Generic)` → `["", "", "house", "door"]` —
последние две записи это `name` полей узлов с ролями `graphics-document`/
`graphics-object`, вычисленное accessible name `aria-label` сохраняется корректно,
но сама роль теряется).

## Причина

`AXRole::parse` (`roles.rs:266`) — это ручной список из ~65 веток
`eq_ignore_ascii_case(...)`, покрывающий WAI-ARIA 1.2 §5 (landmark/widget/document
structure/window roles). В нём нет ни одной из трёх ролей модуля Graphics ARIA
(`https://www.w3.org/TR/graphics-aria/`, отдельное от core-ARIA расширение для
доступных SVG/графического контента): `graphics-document`, `graphics-object`,
`graphics-symbol`. `parse()` возвращает `None` для незнакомого токена →
`resolve_role` (`lib.rs:233-248`) проходит весь список токенов `role`-атрибута
без совпадения → откатывается к `implicit_role(node)`, которая для обычного
`<div>`/`<svg>`-обёртки без специальной семантики тега даёт `AXRole::Generic`.

Формально WAI-ARIA откат «неизвестная роль → следующий валидный токен →
implicit role» реализован правильно (тот же путь, что для любого опечатанного
`role="..."`), но три эти роли не опечатка — они валидные, стандартизованные
и ожидаются категорией WPT целиком: любая страница с доступной SVG-инфографикой,
использующая `role="graphics-document"`/`graphics-object`/`graphics-symbol`
(рекомендованный W3C паттерн для accessible диаграмм, карт, иконок), теряет
семантику роли целиком — AT видит только `aria-label` без роли, вместо
`ROLE_DOCUMENT_FRAME`/аналога (см. фикстуры категории, `AXAPI`/`ATK`/`UIA`
ожидания в каждом `*-manual.html`).

## Что нужно сделать

1. Добавить 3 варианта в `AXRole` (`roles.rs`): `GraphicsDocument`,
   `GraphicsObject`, `GraphicsSymbol` — с `as_str()` → `"graphics-document"` /
   `"graphics-object"` / `"graphics-symbol"` и соответствующими ветками в `parse()`.
2. Спека определяет эти роли как имеющие суперклассы (`graphics-document` наследует
   от `document`/`img` в разных контекстах, `graphics-object` от `group`,
   `graphics-symbol` от `img`/`graphic`) — при добавлении сверить
   `is_role_valid_in_context` (`lib.rs`, соседняя функция) не требует ли контекстных
   ограничений для консистентности с остальным списком.
3. Платформенные мапперы (`platform/windows.rs::ax_role_to_msaa`,
   `platform/macos.rs`) — добавить ветки по ожиданиям из вендоренных фикстур:
   `graphics-document` → `ROLE_SYSTEM_DOCUMENT` (MSAA) / `AXDocument` (subrole,
   macOS) / `Document` (UIA `ControlType`); `graphics-object` → `ROLE_SYSTEM_GROUPING`
   (аналогично `role="group"`); `graphics-symbol` → `ROLE_SYSTEM_GRAPHIC`.
   Точные значения — см. `graphics-*-manual.html` в `tests/wpt/graphics-aam/`
   (каждый файл несёт ожидания под `ATK`/`AXAPI`/`IAccessible2`/`UIA` явно).

## Как исправлено (P3, 2026-08-11)

Ветка `p3-bug-398`, один коммит в `crates/engine/a11y`.

1. **`roles.rs`** — три варианта `AXRole::GraphicsDocument`/`GraphicsObject`/
   `GraphicsSymbol`, ветки `as_str()` (`"graphics-document"`/`"graphics-object"`/
   `"graphics-symbol"`) и case-insensitive ветки `parse()`. Строка роли из
   `as_str()` — то же значение, которое драйвер/MCP отдаёт наружу
   (`session.rs::ax_node_to_a11y`, `winit_session.rs`), так что поверхность
   автоматизации получила роли тем же движением, без отдельной правки.
2. **`platform/windows.rs::ax_role_to_msaa`** — `GraphicsDocument` →
   `ROLE_SYSTEM_DOCUMENT` (0x000F), `GraphicsObject` → `ROLE_SYSTEM_GROUPING`
   (0x001C), `GraphicsSymbol` → `ROLE_SYSTEM_GRAPHIC` (0x0028). Значения взяты не
   «по смыслу», а из ожиданий вендоренных фикстур `tests/wpt/graphics-aam/*.html`
   (секции `IAccessible2`/`UIA` каждого файла). `match` там исчерпывающий, так что
   пропустить платформенный маппинг при добавлении варианта компилятор не даёт.
   `platform/macos.rs`/`linux.rs` — маппинга ролей не содержат вовсе (Phase 0
   заглушки), править нечего.
3. **`lib.rs::build_node`** — `GraphicsObject` добавлен в список ролей, прозрачных
   для валидации контекста (рядом с `Group`, чьим подклассом graphics-object и
   является по спеке). Это не косметика: **сам факт распознавания роли завёл бы
   регрессию** — узел, который раньше падал в прозрачный `Generic`, стал бы
   непрозрачным родителем, и вложенная роль с обязательным родителем
   (`listitem`/`row`/`option`/…) перестала бы проходить
   `is_role_valid_in_context` и откатывалась бы к implicit-роли. Контекстных
   ограничений сами три роли не требуют (п. 2 заявки) — ветка `_ => true`.

**Тесты** (`cargo test -p lumen-a11y` — 139 интеграционных + 26 юнит, зелёные):
5 интеграционных в `tests/cases/ax_tree.rs`, повторяющих разметку фикстур
graphics-aam (HTML- и SVG-вариант `graphics-document` с вложенным
`graphics-object`, `graphics-symbol` на `<g>`, регистронезависимость токена,
прозрачность `graphics-object` для контекста ребёнка) + 3 юнита на MSAA-маппинг.
Тест на прозрачность проверен дифференциально: со снятым `AXRole::GraphicsObject`
из списка прозрачных ролей он краснеет (первая его редакция — `<tr>` внутри
`<tbody role="graphics-object">` — была ложно-зелёной: implicit-роли вообще не
проходят валидацию контекста, поэтому различал бы её только explicit `role=`).

**Не входит:** implicit-роли SVG-элементов ([BUG-686](BUG-686-OPEN.md), открыт) —
это другая функция (`implicit_role`) и другой путь; данный фикс даёт ему
недостающие варианты enum, но сам маппинг тегов не трогает. Автоматический тест
категории `graphics-aria` (`graphics-roles.html`) по-прежнему SKIP — ему нужны
невендоренные `/wai-aria/scripts/aria-utils.js` и `test_driver.get_computed_role`
(`WPT-VENDOR-wai-aria`, ROADMAP.md:560), это инфраструктурный разрыв, не дефект
движка.

## Связанные

* `AXRole::parse`/`implicit_role` — `crates/engine/a11y/src/roles.rs`.
* `resolve_role` — `crates/engine/a11y/src/lib.rs:233-248` (откат к implicit —
  сам механизм корректен, пробел только в списке известных ролей).
* Тот же класс пробела, что могли бы вскрыть другие ARIA-расширения вне
  core-набора (`dpub-aria` — уже вендорена, `graphics-aria` — следующая
  запланированная категория backlog-а, `WPT-VENDOR-graphics-aria`) — стоит
  проверить, не тот же ли пробел там.
