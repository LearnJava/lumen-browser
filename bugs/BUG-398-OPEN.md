# BUG-398 — Graphics ARIA roles (`graphics-document`/`graphics-object`/`graphics-symbol`) не распознаются, откатываются к Generic

**Статус:** OPEN
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

## Связанные

* `AXRole::parse`/`implicit_role` — `crates/engine/a11y/src/roles.rs`.
* `resolve_role` — `crates/engine/a11y/src/lib.rs:233-248` (откат к implicit —
  сам механизм корректен, пробел только в списке известных ролей).
* Тот же класс пробела, что могли бы вскрыть другие ARIA-расширения вне
  core-набора (`dpub-aria` — уже вендорена, `graphics-aria` — следующая
  запланированная категория backlog-а, `WPT-VENDOR-graphics-aria`) — стоит
  проверить, не тот же ли пробел там.
