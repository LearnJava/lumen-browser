# BUG-1441 — Селекторы, валидные по Selectors 4, отвергаются как `is not a valid selector`: `:is()`/`:where()` без аргументов, `:dir(lol)`, `:lang("…")`/`:lang(*-CH)`, незакрытый `)` в конце

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/selectors.rs` — `parse_pseudo` `:1367`, ветка `lang` `:1555`)

## Симптом

`querySelectorAll` и стилевое правило с селектором из таблицы дают `SyntaxError` / отбрасывают правило, хотя спецификация делает их валидными (forgiving-списки и `<string>`-форма `:lang()`). Цена: 20 reftest с `:lang(` (19 `selectors/selectors-4/lang-0NN` + `i18n/lang-pseudo-class-across-shadow-boundaries`; проба — на `lang-007`: `:lang("*-CH")` — зелёный фон, у нас нет; остальные не пробовались по одному) и сабтесты `is-where-basic` (1 из 15), `is-where-not` (1 из 18), `dir-selector-querySelector`, `missing-right-token`, `parse-is-where`, `parse-where`.

## Проба

`document.querySelectorAll(<селектор>)` на странице с `<div lang="fr-CH">`:

| селектор | у нас | по спецификации |
|---|---|---|
| `:is()`, `:where()`, `:not(:is())`, `:is(,div)` | **SyntaxError** | валидно (forgiving), 0 совпадений |
| `:dir(lol)` | **SyntaxError** | валидно, 0 совпадений (`dir-selector-querySelector.html` ждёт `null`; тот же тест проверяет `:dir( auto)` и `:dir(
auto	)` — пробелы вокруг идентификатора, здесь не пробовалось) |
| `:lang("fr")`, `:lang('fr')`, `:lang("fr","de")`, `:lang("")` | **SyntaxError** | валидно (Selectors 4 §8.2) |
| `:lang("*-CH")`, `:lang(*-CH)`, `:lang("*")` | **SyntaxError** | валидно (расширенное сопоставление) |
| `:lang(fr)`, `:lang(fr, de)`, `:lang(fr-CH)` (контроль) | работает | — |
| `span:not([class]` (конец текста без `)`) | **SyntaxError** | CSS Syntax §5.4.7: незакрытый блок закрывается в конце ввода — валидно |
| `CSS.supports("selector(&)")`, `selector(.a &)`, `selector(:is())` | `false` | `true` |
| `::backdrop`, `::spelling-error`, `::grammar-error`, `::search-text`, `::file-selector-button`, `::details-content`, `::column`, `::part(foo)`, `::part(--foo)` | **SyntaxError** | валидно, 0 совпадений (в таблице стилей разбираются; `::marker`, `::target-text`, `::highlight(x)`, `::placeholder`, `::slotted(b)` в `querySelectorAll` принимаются) |
| `<style>::part(foo){…}</style>` → `cssRules[0].selectorText` | `::part` (аргумент потерян) | `::part(foo)` |

В `parse_pseudo` ветка `lang` в комментарии прямо сказано: «строковые литералы … в Phase 0 не поддерживаем».

## Как найдено

WPT-RUN-14 срез 20: сообщения `:is() is not a valid selector`, `:dir(lol) is not a valid selector`, `span:not([class] is not a valid selector`, 20 reftest `lang-0NN`.

## Что делать

(1) Forgiving-разбор аргументов `:is()`/`:where()` (пустой и частично невалидный список — валидный селектор, который ничего не выбирает) — Selectors 4 §3.4. (2) `:dir()` — идентификатор любого значения, сопоставление только для `ltr`/`rtl`. (3) `:lang()` — `<ident>` и `<string>`, `*`-подстановка, сопоставление по RFC 4647 §3.3.2. (4) Закрывать незакрытые скобки в конце ввода для `querySelector` и `@supports selector()`; псевдоэлементы в `querySelectorAll` — валидны, ничего не выбирают; сериализация `::part(<ident>)` с аргументом. (5) `CSS.supports('selector(&)')`.

## Как проверить

Таблица выше; `css/selectors/selectors-4/lang-007.html`, `css/selectors/is-where-basic.html`, `css/selectors/missing-right-token.html`.
