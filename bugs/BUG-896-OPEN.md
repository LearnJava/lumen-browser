# BUG-896 — CSS-модули (`import sheet from "./x.css" with {type: "css"}`) не поддерживаются: атрибут отклоняется после загрузки файла, при том что JSON-модули работают

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `module-types`)
**Область:** js (`crates/js/src/import_attributes.rs:44` — `ModuleType::from_attr` знает только `"json"`; `crates/js/src/v8_esm.rs:371`/`:403` — та же развилка и текст ошибки)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`import(url, {with: {type: "css"}})` отклоняется с
`module '<URL>': unsupported import attribute type 'css'`; статическая форма
(`<script type=module>` с `import sheet from "./x.css" with {type: "css"}`)
даёт то же самое строкой `module error:`. JSON-модуль на той же странице
разрешается и отдаёт разобранный объект, обычный `.mjs` — тоже.

Файл при этом СКАЧИВАЕТСЯ (сервер пробы видит `GET /vcsi-sheet.css`), то есть
отказ происходит после сети, на классификации типа. Это же объясняет, почему в
снимке рядом стоит вторая сигнатура — `module '<URL>': network error: HTTP N`:
тесты `css-module/charset-*.html` берут файл по редиректу/с иным заголовком.

Смежное ограничение, замеренное здесь же: даже при поддержке типа модулю
нужен объект `CSSStyleSheet`, которого в движке нет вовсе
([BUG-897](BUG-897-OPEN.md)), — то есть CSS-модуль не сможет вернуть `default`
до того, как появится конструируемая таблица стилей.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant module-types`
(2026-08-23, dev-release, Linux):

```
module-ran
dyn-js plain-module
dyn-css-err Error: module 'http://.../vcsi-sheet.css': unsupported import attribute type 'css'
dyn-json {"vcsi":42}
[engine] module error: JS runtime error: module '...': unsupported import attribute type 'css'
[server saw: GET /vcsi-data.json, GET /vcsi-mod.mjs, GET /vcsi-sheet.css, GET /vcsi-static-css.mjs]
```

## Цена по WPT

9 id снимка WPT-RUN-5 (механизм `module-type-unsupported`): шесть
`the-script-element/css-module/*` (`charset-bom`, `charset`, `charset-2`,
`relative-urls`, `import-css-module-basic`, `content-type-checking`) и три
`text-module/*` (`charset`, `charset-2`, `module`). Четыре из девяти до этого
среза числились за `resource-no-load-event` ([BUG-826](BUG-826-FIXED.md)), а
один — за [BUG-480](BUG-480-OPEN.md): в файлах есть и `<script>`, и `<iframe>`,
но движок печатает свою причину раньше любого ожидания.

## Что дальше

HTML LS «create a CSS module script»: тело разбирается как таблица стилей и
экспортируется как `CSSStyleSheet` по умолчанию. Порядок работ поэтому
обратный обычному — сначала [BUG-897](BUG-897-OPEN.md)/[BUG-471](BUG-471-OPEN.md)
(объект таблицы), потом ветка `"css"` в `ModuleType::from_attr`.
