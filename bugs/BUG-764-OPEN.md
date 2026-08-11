# BUG-764 — Роли DPUB-ARIA (`doc-*`, 41 штука) не распознаются, откатываются к Generic

**Статус:** OPEN
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

## Связанные

* [BUG-398](BUG-398-FIXED.md) — тот же дефект для трёх ролей Graphics ARIA,
  закрыт 2026-08-11; его правка — рабочий шаблон для этой.
* [BUG-686](BUG-686-OPEN.md) — соседний, но другой путь: implicit-роли SVG
  (`implicit_role`, namespace не проверяется), а не explicit `role=`.
* `docs/wpt-vendor-notes/dpub-aam.md` / `dpub-aria.md` — вендоринг категорий,
  инфраструктурная часть (хелпер `/wai-aria/scripts/aria-utils.js` не довендорен,
  `WPT-VENDOR-wai-aria`, ROADMAP.md:560), поэтому автоматический
  `dpub-aam/role/roles.html` сигнала не даёт — проверять пробой на движке.
