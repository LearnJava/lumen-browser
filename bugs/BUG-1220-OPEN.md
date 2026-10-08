# BUG-1220 — `/calendar`: `CSSStyleSheet.insertRule` бросает `SyntaxError`, Next откатывается на жёсткую навигацию, страница пустая

**Статус:** OPEN
**Компонент:** js/css-parser (`CSSStyleSheet.insertRule` отвергает правило, которое страница считает валидным)
**Найден:** 2026-09-29, стенд bankruptcy-platform, сборка `main` 22a782d55

## Симптом

`/calendar` — пустой экран: `document.documentElement.scrollHeight = 0`, 36 элементов, текста нет. В консоли (19 раз за сессию):

```
Failed to fetch RSC payload for http://my-stand.local/calendar. Falling back to browser navigation.
SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': the supplied text is not a valid rule.
    at s.insertRule …  at Array.forEach  at h  at Map.forEach  at u  at 54434 …
```

Код страницы (обход `Map` таблиц стилей → `insertRule` по массиву правил) получает исключение от `insertRule`,
Next отбрасывает RSC-навигацию и делает жёсткую, которая падает так же.

На сборке 2026-09-24 та же страница падала иначе: `TypeError: Cannot read properties of null (reading 'cssRules')` —
`link.sheet === null` для `<link rel="stylesheet">` (одна из двух таблиц `_next/static/css/*.css` отдавала 0 правил).
Обе ошибки в одном участке кода страницы, обе — поверхность CSSOM.

## Что делать

1. Вытащить текст отвергнутого правила. Хук на `insertRule`, поставленный из `eval` после загрузки, теряется при
   жёсткой навигации (проверено); ставить его надо до загрузки чанка — например, патчем шима под `LUMEN_*`-флаг
   или собрать чанк календаря и прогнать его правила через `CSSStyleSheet.insertRule` в изолированном тесте.
2. Минимизировать правило до одной строки и определить, какой at-rule/селектор парсер отвергает.
3. Проверить, что `link.sheet` не `null` для загруженной таблицы (наблюдалось на сборке 09-24, на 09-29 не проверено отдельно).

## Дополнение: WPT-RUN-14 срез 18 (2026-10-07, `css/css-fonts`)

Проба `<style id=st></style>` + `st.sheet.insertRule(...)` (`--dump-layout`, V8): принимаются только `.a{color:red}` и
`@media screen{.a{color:red}}`. `SyntaxError` дают **`@font-face{…}`** (все формы: `src:url()`, `src:local()`,
`ascent-override`, `size-adjust`), **`@keyframes`**, **`@property`**, **`@layer foo{}`**, **`@supports (…){…}`**,
`@font-feature-values`, `@font-palette-values`. Причина — `Stylesheet::insert_rule` (`css-parser/src/parser.rs:647`)
вставляет только `TopLevelRuleKind::Style` и `Media` (`:672…676`), а `parsed != Stylesheet::default()` (`:667`)
отвергает всё, что парсер раскладывает по другим спискам (`@font-face`, `@keyframes`, …). Это, вероятно, и есть
отвергнутое правило `/calendar` (Next.js вставляет `@font-face` и `@keyframes` через `insertRule`). Затронуты
`css-fonts/parsing/font-face-{src-format,src-list,src-local,src-tech,size-adjust,metric-overrides}.html` — 6 id, 136 из 136
сабтестов (`Failed to execute 'insertRule' on 'CSSStyleSheet': the supplied text is not a valid rule.`).
