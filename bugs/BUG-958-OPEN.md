# BUG-958 — `CSS.supports(conditionText)` (one-argument, no parens) всегда `false`, даже для реально поддержанного свойства

**Статус:** OPEN
**Тип:** дефект реализованного кода — `parse_supports_atom` (css-parser) требует ведущую `(` и не реализует запасной путь спецификации (парсинг строки как «голого» `<declaration>`).
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 35, живая проба через `--mcp-live-port`)
**Область:** css-parser (`crates/engine/css-parser/src/parser.rs::parse_supports_atom`)
**Владелец:** P3.

## Симптом

`CSS.supports("writing-mode: horizontal-tb")` — однoаргументная форма без
оборачивающих скобок — отвечает `false`, хотя движок это свойство
действительно поддерживает. Живая проба на том же документе:

```js
CSS.supports("writing-mode: horizontal-tb")    // false  ← баг
CSS.supports("(writing-mode: horizontal-tb)")  // true   ← корректно
CSS.supports("writing-mode", "horizontal-tb")  // true   ← корректно (двухаргументная форма)
```

Только «голая» (без скобок) однoаргументная форма затронута; двухаргументная
и скобочная формы работают верно.

## Причина

`parse_supports_condition` → `parse_supports_expr` → `parse_supports_term` →
`parse_supports_atom` (`crates/engine/css-parser/src/parser.rs:2257`).
`parse_supports_atom` разбирает `font-tech()`/`font-format()`/`selector()`, а
затем — только если следующий байт `(` — заходит в общую ветку
condition/declaration. Если строка не начинается с `(`, функция долетает до
последней строки и возвращает `SupportsCondition::Unknown`
(`parser.rs:2340`), у которого `.evaluate()` (`parser.rs:1346`) жёстко
`false`.

Спецификация (CSS Conditional Rules L3, `CSS.supports(conditionText)`)
требует запасной путь: если `conditionText` не парсится как полноценный
`<supports-condition>`, попытаться распарсить её как «голый» `<declaration>`
(`prop: value` без скобок) и, если это удалось, вызвать двухаргументную
форму с получившейся парой. Lumen этот запасной путь не реализует вовсе —
`Unknown` не пытается интерпретировать строку как декларацию.

## Прямое измерение

Живая проба (dev-release, `main` = `6e7b28889`, `--mcp-live-port`,
собственный http-сервер на `127.0.0.1:8935`): страница выполняет три формы
вызова `CSS.supports` на идентичном свойстве/значении и складывает результат
в `window.__r`; `eval` через MCP вернул
`{"condNoParen":false,"condParen":true,"propForm":true}` — расхождение
воспроизводится напрямую, не косвенно по логам.

## Кого это держит

Классифицирует три из 47 unclassified id среза 34 (после — 44):
`css/css-writing-modes/forms/textarea-rows-cols-sizing.html`,
`select-multiple-scrolling.optional.html`,
`select-size-scrolling-and-sizing.optional.html`. Все пять файлов в
`css/css-writing-modes/forms/` гейтят каждый `test()`/`promise_test()`
именно этой идиомой:

```js
for (const writingMode of ["horizontal-tb", "vertical-lr", ...]) {
    if (!CSS.supports(`writing-mode: ${writingMode}`))
        continue;
    ...
    test(t => { ... }, `... ${writingMode} ...`);
}
```

Поскольку вызов всегда `false`, цикл ни разу не доходит до `test()` —
регистрируется ноль сабтестов. `testharness.js` в этом случае не завершает
прогон сам: `test_end` в снапшоте несёт только `"status": "TIMEOUT"` без
единого `test_status` — ни одного сабтеста не появилось, что подтверждает
и снапшот, и живая проба. Два оставшихся файла кластера
(`select-multiple-keyboard-selection.optional.html`,
`text-input-block-size.optional.html`) — тот же `TIMEOUT` в снапшоте, но
у них TIMEOUT уже объяснён другим механизмом (testdriver-action и т. п.),
поэтому маркер под них не заводился.

Шире: любой WPT-тест, использующий идиому "голого" `CSS.supports("prop:
value")" без скобок для feature-detection, получает `false` независимо от
реальной поддержки — по корпусу таких файлов 7
(`grep -rlE "CSS\.supports\(\s*\`?[a-zA-Z-]+\s*:\s" tests/wpt --include="*.html"`),
не только в `css-writing-modes`.

## Направление починки

В `parse_supports_atom`/`parse_supports_condition` добавить запасной путь
по спецификации: если ведущий символ — не `(` и разбор как полного
`<supports-condition>` не удался, попытаться распарсить всю строку как
`<ident> ":" <value>` и, если это получилось, вернуть
`SupportsCondition::Decl { property, value }` вместо `Unknown`.
