# BUG-896 — CSS-модули (`import sheet from "./x.css" with {type: "css"}`) не поддерживаются: атрибут отклоняется после загрузки файла, при том что JSON-модули работают

**Статус:** FIXED 2026-09-22 (P6, [GAP-CSSMOD](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `GAP-CSSMOD` в [ROADMAP.md](../ROADMAP.md). Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было.
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
([BUG-897](BUG-897-FIXED.md)), — то есть CSS-модуль не сможет вернуть `default`
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
обратный обычному — сначала [BUG-897](BUG-897-FIXED.md)/[BUG-471](BUG-471-FIXED.md)
(объект таблицы), потом ветка `"css"` в `ModuleType::from_attr`.

## Исправлено

[BUG-897](BUG-897-FIXED.md)/CSSOM-5 закрылась ещё 2026-09-06 (конструируемый
`CSSStyleSheet`, `.replaceSync()`), сняв блокер. Дефис `ModuleType::from_attr`
в `import_attributes.rs` относится к отдельному, уже не используемому
QuickJS-препроцессору (см. его doc-комментарий — «V8 does not use this
preprocessor»); настоящий путь — `crates/js/src/v8_esm.rs`'s `DeclaredType`/
`declared_type()`/`module_text()`/`cache_key()`, которые читают атрибут
`type` прямо из V8's `FixedArray` для каждого импорта.

* `DeclaredType` получил вариант `Css` (был `Unsupported("css")`).
* `module_text()` для `Css` синтезирует ровно то, что делает HTML LS «create
  a CSS module script»: `const s = new CSSStyleSheet(); s.replaceSync(<CSS-
  текст как JS-строковый литерал>); export default s;` — тот же
  `new CSSStyleSheet()`/`.replaceSync()`, которым пользуется страничный
  скрипт (CSSOM-5). В отличие от JSON-модуля парсинг CSS никогда не
  проваливает импорт целиком: невалидные правила молча отбрасываются
  (CSS2 §4.2), как и везде в движке.
* `cache_key()` даёт CSS-импорту тот же специфик+суффикс-тип принцип, что и
  JSON (`specifier\0css`) — module map не путает JS/JSON/CSS-импорт одного
  и того же URL.

Новый тест `v8_esm::tests::v8_css_module_import_returns_constructed_stylesheet`
(`crates/js/src/v8_esm.rs`) — единственный тест модуля на рантайме с
установленным DOM/шимом (`CSSStyleSheet` существует только после него, в
отличие от голого `rt()`, которым пользуются остальные тесты файла).
Существующий `v8_unsupported_attribute_type_fails_to_load` переведён на
заведомо неподдерживаемый тип (`wasm`), раз `css` больше не такой.

`cargo test -p lumen-js --features v8-backend --lib v8_esm` 27/27 (было 25),
`cargo test -p lumen-js --features v8-backend` 4128/4129 passed (единственный
красный — предсуществующий флак `frame_bridge::tests::
inaccessible_bridge_mutation_does_not_mark_dirty`, воспроизводится и на
`main`, проходит в изоляции, не связан), `cargo clippy -p lumen-js --features
v8-backend --all-targets -- -D warnings` чист.

**Не в скоупе этого среза:** MIME-валидация `Content-Type: text/css` при
загрузке — WPT-снимок содержит вариант `content-type-checking`, но у
JSON-импорта той же проверки тоже нет (тот же класс отложенного долга, не
регрессия). Теневой каскад (`shadowRoot.adoptedStyleSheets` не подключён к
пейнту) — известное ограничение CSSOM-5, не этой задачи.
