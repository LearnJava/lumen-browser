# BUG-764 — Роли DPUB-ARIA (`doc-*`, 41 штука) не распознаются, откатываются к Generic

**Статус:** FIXED 2026-09-18 (P3)
**Компонент:** a11y (`crates/engine/a11y/src/roles.rs::AXRole::parse`, `AXRole` enum;
потребитель — `crates/engine/a11y/src/lib.rs::resolve_role`)
**Найден:** P3, при закрытии [BUG-398](BUG-398-FIXED.md) 2026-08-11 — по прямому
указанию раздела «Связанные» той заявки («стоит проверить, не тот же ли пробел
в других ARIA-расширениях вне core-набора»)

## Симптом

Ни одна из 41 роли словаря DPUB-ARIA (`doc-abstract`, `doc-chapter`,
`doc-footnote`, `doc-pagebreak`, … — полный список в вендоренном тесте
`tests/wpt/dpub-aam/role/roles.html`, по одной ATTAcomm-фикстуре на роль в
`tests/wpt/dpub-aam/manual/`) не распознаётся `AXRole::parse` — грепом `doc-`
по `roles.rs` не встречается ни разу. Прямая проба на движке (временный Rust-тест
на `build_ax_tree`, после подтверждения удалён):

```rust
let tree = build_tree(r#"<div role="doc-chapter" aria-label="ch">
  <div role="doc-footnote" aria-label="fn"></div>
</div>"#);
// роли узлов: ["generic:ch", "generic:fn"] — вместо doc-chapter/doc-footnote
```

Accessible name (`aria-label`) считается верно, теряется ровно семантика роли —
тот же наблюдаемый профиль, что был у Graphics ARIA до BUG-398.

## Причина

Та же, что у [BUG-398](BUG-398-FIXED.md): `AXRole::parse` — ручной список веток
`eq_ignore_ascii_case(...)`, покрывающий WAI-ARIA 1.2 §5 (плюс три роли Graphics
ARIA, добавленные 2026-08-11). Модуль DPUB-ARIA (`https://www.w3.org/TR/dpub-aria/`,
Recommendation, словарь `doc-*` для digital-publishing/EPUB-семантики) в списке
не представлен вовсе; `parse()` → `None` → `resolve_role` откатывается к
`implicit_role(node)` → `Generic` для обычного `<div>`/`<section>`.

Пробел известен и зафиксирован при вендоринге (`docs/wpt-vendor-notes/dpub-aam.md`,
`dpub-aria.md`: «отдельно не реализовано в Lumen»), но отдельной строки в `BUGS.md`
до сих пор не имел — эта заявка переводит его из заметки в трекаемую задачу.

## Что нужно сделать

1. 41 вариант в `AXRole` + ветки `as_str()`/`parse()` — механически по списку из
   `tests/wpt/dpub-aam/role/roles.html` (там же помечена депрекированная
   `doc-biblioentry`; сверить с актуальной редакцией спеки, прежде чем её включать).
2. MSAA-маппинг в `platform/windows.rs::ax_role_to_msaa` — `match` исчерпывающий,
   компилятор сам потребует ветку на каждый вариант. Ожидаемые значения по
   платформам лежат в самих фикстурах `tests/wpt/dpub-aam/manual/doc-*-manual.html`
   (секции `ATK`/`AXAPI`/`IAccessible2`/`UIA`), брать оттуда, а не «по смыслу».
3. Проверить прозрачность для валидации контекста (`lib.rs::build_node`): роли,
   чей суперкласс — `group`/`section`-контейнер (`doc-part`, `doc-chapter`,
   `doc-endnotes`, …), должны попасть в список прозрачных ролей рядом с `Group`,
   иначе само распознавание роли уронит вложенные роли с обязательным родителем
   (`listitem`/`row`/`option`) в implicit — ровно тот побочный эффект, который
   пришлось учесть в BUG-398.
4. Объём (41 роль + платформенные ветки) заметно больше, чем у BUG-398 — стоит
   рассмотреть таблицу-константу вместо ручных веток `eq_ignore_ascii_case`.

## Фикс

Все 41 варианта DPUB-ARIA добавлены в `AXRole` (`roles.rs`) с `as_str()`/`parse()`,
именование `DocXxx` из `doc-xxx`. AT-маппинг извлечён не «по смыслу», а разбором
всех 39 фикстур `tests/wpt/dpub-aam/manual/doc-*-manual.html` (единый Python-скрипт,
парсинг `ATTAcomm`-JSON: поля `ATK.role`, `AXAPI.AXRole`/`AXSubrole`, `IAccessible2.role`,
`UIA.ControlType`) — сгруппировано по фактическим суперклассам:
`landmark`-регион (16 ролей), `landmark`-навигация (3: `doc-index`/`doc-pagelist`/`doc-toc`),
`section` (8), `note` (2: `doc-notice`/`doc-tip`), `footnote` (1), `link` (4:
`doc-backlink`/`doc-biblioref`/`doc-glossref`/`doc-noteref`), `listitem`-подобные
депрекированные (2: `doc-biblioentry`/`doc-endnote`), `img` (`doc-cover`),
`heading` (`doc-subtitle`), `separator` (`doc-pagebreak`).

`doc-pagefooter`/`doc-pageheader` фикстур не имеют (только в `role/roles.html`,
подтверждено: `grep -rn "pagefooter|pageheader" tests/wpt/dpub-aam/` не находит
их ни в одном `manual/*.html`) — AAM для них пока не описан отдельно. Взяты как
generic sectioning-контейнер (тот же MSAA/transparency-класс, что и `section`-роли);
если апстрим фикстуры появятся, значения нужно будет свериться заново.

MSAA-маппинг (`platform/windows.rs::ax_role_to_msaa`) — по тем же группам:
container-роли (32 штуки, все `landmark`/`section`/`note`/`footnote`/pagefooter/pageheader)
→ `ROLE_SYSTEM_GROUPING` (тот же упрощённый маппинг, что уже используют
WAI-ARIA landmark-роли — своих `ROLE_SYSTEM_*` под `IA2_ROLE_LANDMARK`/`SECTION`/
`NOTE`/`FOOTNOTE` в Windows MSAA нет), `link`-роли → `ROLE_SYSTEM_LINK`,
`doc-biblioentry`/`doc-endnote` → `ROLE_SYSTEM_LISTITEM`, `doc-cover` →
`ROLE_SYSTEM_GRAPHIC`, `doc-subtitle` → `ROLE_SYSTEM_COLUMNHEADER` (тот же
условный «ближайший вариант», что уже применяется к `AXRole::Heading`),
`doc-pagebreak` → `ROLE_SYSTEM_SEPARATOR`.

Прозрачность (`lib.rs::build_node`, п.3 заявки): 32 container-роли (все, кроме
4 `link`, 2 `listitem`-подобных, `doc-cover`, `doc-subtitle`, `doc-pagebreak`)
добавлены в список прозрачных ролей рядом с `Group`/`GraphicsObject` — без этого
`<div role="doc-bibliography"><div role="listitem">` ронял бы вложенный `listitem`
в `Generic` (у `<div>` implicit-роль не зависит от контекста, а
`is_role_valid_in_context` требовал бы прямого родителя `List`).

Контекстные ограничения (`is_role_valid_in_context`) для новых ролей не добавлялись —
спека не даёт для них WPT-фикстуры с проверкой родителя, а существующая таблица
ограничивает только те роли, для которых такая проверка уже была нужна раньше
(симметрично с тем, как это сделано для Graphics ARIA в BUG-398).

**Тесты** (`cargo test -p lumen-a11y` — 143 интеграционных + 26 юнит, зелёные):
4 новых в `tests/cases/ax_tree.rs` — `doc-chapter`/`doc-footnote` на реальной
разметке из симптома заявки, регистронезависимость токена, round-trip
`as_str()` → `parse()` по всем 41 вариантам (с проверкой, что их ровно 41),
прозрачность `doc-bibliography` для вложенного `listitem` (тот же
дифференциальный паттерн, что у аналогичного теста для `graphics-object`
в BUG-398).

**Гейт:** `cargo clippy -p lumen-a11y --all-targets -- -D warnings` — чисто.
`scripts/scoped-test.sh` — единственный красный тест,
`cases::snapshot_cpu::cpu_snapshots_match_references` (7 файлов:
`55-text-rendering`/`57-canvas-2d`/`32-list-markers`/`34-forms`/
`45-multiple-backgrounds`/`51-scrollbar-rendering`/`1000000-final`) — известный
несвязанный дрейф CPU-эталонов на `main` (см. BUG-1008), a11y-роли на
растеризацию не влияют.

## Связанные

* [BUG-398](BUG-398-FIXED.md) — тот же дефект для трёх ролей Graphics ARIA,
  закрыт 2026-08-11; его правка — рабочий шаблон для этой.
* [BUG-686](BUG-686-OPEN.md) — соседний, но другой путь: implicit-роли SVG
  (`implicit_role`, namespace не проверяется), а не explicit `role=`.
* `docs/wpt-vendor-notes/dpub-aam.md` / `dpub-aria.md` — вендоринг категорий,
  инфраструктурная часть (хелпер `/wai-aria/scripts/aria-utils.js` не довендорен,
  `WPT-VENDOR-wai-aria`, ROADMAP.md:560), поэтому автоматический
  `dpub-aam/role/roles.html` сигнала не даёт — проверять пробой на движке.
