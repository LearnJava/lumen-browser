# BUG-1442 — Невалидные селекторы принимаются: `p,` (хвостовая запятая) и `p#` применяются, `.bar.` и `p:nth-child(+ 2n)` принимает `querySelector`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/selectors.rs` — `parse_simple_selector` `:1186`, `parse_ident`)

## Симптом

Стилевое правило `p, {background:red}` применяется (красный фон `<p>`), а должно отбрасываться целиком: пустой элемент списка делает селектор невалидным (Selectors 4 §3.1; `css3-modsel-154.xml`). То же для `p#` (идентификатор без имени). `document.querySelector('.bar.')`, `'p#'`, `'p:nth-child(+ 2n)'` не бросают `SyntaxError`. `..test`, `.foo..quux`, `.bar.`, `p:selection` и `p:nth-child(+ 2n)` в таблице стилей отбрасываются верно, `querySelector` принимает `.bar.` и `p:nth-child(+ 2n)` — расхождение между двумя путями разбора.

## Проба

`--dump-layout` страницы `<style>p{background:green;…}</style><style>⟨X⟩ {background:red;…}</style><p>x</p>` (красный фон — правило X применилось и перебило зелёный):

| X | фон `<p>` | ожидается |
|---|---|---|
| `p,` | **красный** | зелёный (правило отброшено) |
| `p#` | **красный** | зелёный |
| `..test`, `.foo..quux`, `.bar.`, `p:selection`, `p:nth-child(+ 2n)` | зелёный | зелёный |

`document.querySelector(<X>)`: `'.bar.'`, `'p#'`, `'p:nth-child(+ 2n)'` — **без исключения**; `'p,'`, `',p'`, `'p,,q'`, `'..test'`, `'a>>b'`, `'a>'`, `'>a'`, `'[a=]'`, `'1p'`, `'p:nth-child()'` бросают верно.

## Как найдено

WPT-RUN-14 срез 20: `selectors/old-tests/css3-modsel-154.xml` (`p, {…}`) — 1 id; остальные формы из таблицы не имеют своего теста в этом срезе.

## Что делать

В `parse_simple_selector`: `#` и `.` без идентификатора → `None`; хвостовая запятая в списке → весь список невалиден (кроме forgiving-контекстов `:is()`/`:where()`/`:has()`). Для `querySelector` использовать тот же разбор, что и для таблицы стилей, а не ослабленный.

## Как проверить

Таблица выше; `css/selectors/old-tests/css3-modsel-154.xml` (вне `.xml`-пути — тот же селектор в `.html`).
