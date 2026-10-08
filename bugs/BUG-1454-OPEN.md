# BUG-1454 — `@namespace` не реализован: префикс `ns|E` отвергается, `*|E` отбрасывает префикс, пространство имён элемента не сопоставляется — 100 id среза

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser/layout (`@namespace` разбирается и игнорируется: `crates/engine/css-parser/src/parser/selectors.rs:1189-1223`; `CSS-SPECS.md` — `@namespace` «parsed; no XML namespaces»)

## Симптом

Правило `@namespace` разбирается, но не хранится; селектор `ns|E` в `parse_simple_selector` отвергается безусловно (комментарий: «`@namespace` не поддержан, а необъявленный префикс по Namespaces §6.3 — невалидный селектор»), `*|E` отбрасывает префикс, `|E` превращается в тип `"|"`, не совпадающий ни с чем. Проба шла на `file://` (расширение `.xml` разобрано тем же HTML-парсером, `xmlns` на элементе не проверялся отдельно), поэтому строки «ожидается» — по спецификации, а «у нас» подтверждают только то, что селектор с префиксом не совпадает, а умолчание не ограничивает. Пространство имён элемента в модели есть (`QualName.namespace`, `Namespace::Other` для разобранных XML-документов — GAP-XMLDOC), но сопоставление селектора его не читает. 100 id среза содержат `@namespace` или лежат в `css-namespaces/`: 19 из 23 id `css-namespaces` (`prefix-00N.xml`, `syntax-0NN.xml`), 77 `selectors/old-tests/css3-modsel-*.xml`, `is-default-ns-001`, `is-default-ns-003`, `not-default-ns-003`, `attribute-selectors/attribute-case/semantics.html`.

## Проба

XML-документ (`.xml`, `<html xmlns="http://www.w3.org/1999/xhtml">` + `<test xmlns="y">`), `--dump-layout`, зелёный фон = правило применилось:

| правило | у нас | ожидается |
|---|---|---|
| `@namespace f "y"; f\|test{background:lime}` | не применено | применено |
| `@namespace f "z"; f\|test{…}` | не применено | не применено |
| `@namespace "y"; test{…}` | применено | применено |
| `@namespace "z"; test{…}` | **применено** | не применено |
| `@namespace "http://www.w3.org/1999/xhtml"; p{…}` | применено | применено |
| `@namespace "z"; p{…}` (элемент `p` в XHTML) | **применено** | не применено |
| `@namespace h "http://www.w3.org/1999/xhtml"; h\|p{…}` | не применено | применено |
| `*\|test{…}` | применено | применено |
| `\|test{…}` (элемент без пространства) | не применено | не применено |
| `@namespace a "http://a"; [a\|foo]{…}` на `<p a:foo>` | не применено | применено |

## Как найдено

WPT-RUN-14 срез 20: 19 из 23 `css-namespaces/prefix-00N.xml`/`syntax-0NN.xml` и 81 id `css/selectors` (77 `old-tests/css3-modsel-*.xml` + 4 отдельных).

## Что делать

Хранить объявления `@namespace` листа (префикс → URI, умолчание) и применять их при сопоставлении: `ns|E`, `|E`, `*|E`, `ns|*`, `[ns|attr]`, умолчание для голых типов. Разбор — Namespaces 3 §3, §6. Строка `@namespace` в `CSS-SPECS.md` переведена в 🟡 в этом же коммите.

## Как проверить

Таблица выше; `css/css-namespaces/prefix-001.xml`, `css/selectors/old-tests/css3-modsel-99.xml`.
