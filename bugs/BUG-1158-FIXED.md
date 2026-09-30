# BUG-1158 — yahoo: скрипт, вставленный через `appendChild`, падает `SyntaxError: Unexpected token ':'` (в Chrome ошибки нет)

**Статус:** FIXED 2026-09-30 (P3)
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-1121](BUG-1121-FIXED.md))
**Область:** не локализовано — путь вставки классического скрипта
(`crates/js/src/shim/web_api_shim_mid.js`: `_lumen_resource_after_insert` →
`_lumen_script_prepare` → `_lumen_script_execute_classic`, `(0, eval)(text)`).

## Симптом

`https://www.yahoo.com/`, видимое окно, `--maximized`, без блокировщика. После
[BUG-1121](BUG-1121-FIXED.md) (ошибки `Rapid initialization error … reading 'indexOf'` больше
нет) в stderr единственная ошибка страницы:

```
[JS error] Uncaught SyntaxError: Unexpected token ':'
    at eval (<anonymous>)
    at _lumen_script_execute_classic (…:12210:20)
    at _lumen_script_prepare (…:12428:9)
    at _lumen_resource_try_prepare (…:13053:9)
    at _lumen_resource_after_insert (…:13080:9)
    at _lumen_append_child (…)
    at _ctor.appendChild (…:7719:21)
    at <anonymous>:1:5504
```

Chrome 153 на той же странице этой ошибки не даёт (ни `Runtime.exceptionThrown`, ни в консоли).

## Что известно

- Вызов — из кода без URL (`<anonymous>:1:5504`: встроенный скрипт или `eval`). Какой именно,
  не установлено: в HTML yahoo `appendChild` в первой строке встроенных скриптов нашёлся только
  внутри экранированных строк Next.js-данных (webpack-бутстрап CMP, `__uspapi`, вставляет
  `<iframe>`, не `<script>`), и его позиция с 5504 не сопоставлена.
- Все встроенные JS-скрипты в итоговом DOM компилируются (`new Function(s.text)` без ошибки);
  значит, падает скрипт, который вставили и затем убрали, или внешний (`src`), который в итоговом
  DOM уже не лежит.
- `SyntaxError: Unexpected token ':'` — типичный результат исполнения JSON как JS. Синтетический
  тест (`application/json`, `application/ld+json`, `speculationrules`, `importmap`, `text/x-template`,
  тип с пробелом в начале) не воспроизводит: ни один не исполняется, как и в Chrome.

## Что сделать

1. Локализовать: перехватить `Node.prototype.appendChild`/`insertBefore` до скриптов страницы
   (`--mcp-live-port` + инъекция при старте) и записать `type`, `src`, первые 200 символов текста
   каждого вставляемого `<script>`; сравнить с Chrome (`Page.addScriptToEvaluateOnNewDocument`).
2. По результату — правка выбора «исполнять/не исполнять» (тип, `nomodule`, `noModule` у
   классического) или получения тела (`src`, ответ не JS).

Критерий: на yahoo нет `Unexpected token ':'`.

## Второй сайт (2026-09-25, P6): bbc

bbc.com — `Uncaught SyntaxError: Unexpected token ':'` 3 из 3 прогонов (видимое окно,
`LUMEN_NO_ADBLOCK=1`), тот же путь: `_lumen_append_child` → `_lumen_resource_after_insert` →
`_lumen_resource_try_prepare` → `_lumen_script_prepare` → `_lumen_script_execute_classic` → `eval`.
Chrome ошибки не даёт. Страница Next.js, так что второй кандидат для локализации по п. 1 выше —
с меньшим числом сторонних скриптов, чем yahoo. Найдено по ходу закрытия [BUG-493](BUG-493-FIXED.md).

## Исправление (P3, 2026-09-30)

Корень — не вставка скрипта, а `innerHTML` на самом `<script>`: Next.js/React кладёт тело через
`dangerouslySetInnerHTML`, а сырой HTML yahoo несёт `a<e` (JSON-экранирование `<`), которое
после декодирования даёт `a<e`. `parse_fragment_with_context` стартовал токенизатор в data state
независимо от контекста, `<e` открывал тег, тело обрезалось на `for(var e=…;a` →
`SyntaxError: Unexpected token ':'`. Найдено временным логом тела в `_lumen_script_execute_classic`
(4327 символов, обрыв перед `<`). Правка: для HTML-контекста script/style/iframe/noembed/noframes/xmp
токенизатор стартует в RAWTEXT, для title/textarea — в RCDATA (HTML LS §13.4 шаг 4)
(`tokenizer.rs::fragment_text_only_state`, `tree_builder.rs::run_pull_in_state`). Тесты:
`html-parser/tests/fragment_parsing.rs`, `dom/tests/v8_bug1158_raw_text_inner_html.rs`. Живая проверка:
на yahoo ошибок `[JS error]` нет. bbc из песочницы не открылся (TLS через прокси, `CaUsedAsEndEntity`) —
второй сайт из записи не перепроверен, но он тоже Next.js с тем же путём.
