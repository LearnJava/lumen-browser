# BUG-892 — `document.forms`/`scripts`/`links` отсутствуют (`document.images` — есть): коллекции документа сделаны по одной, а не таблицей

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `collections`)
**Область:** js (`crates/js/src/dom.rs:6125` — единственный геттер `get images()` в литерале `document`; `forms`/`scripts`/`links`/`embeds`/`plugins`/`anchors` не объявлены)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`document.images` — живая `HTMLCollection` (заведена [BUG-732](BUG-732-FIXED.md)
как шестая из шести точек), и она работает: `.length` растёт при вставке
элемента. Соседние коллекции того же раздела HTML LS §3.1.5 не заведены вовсе:
`document.forms`, `document.scripts`, `document.links` — `undefined`, поэтому
`document.forms.length` бросает `Cannot read properties of undefined (reading
'length')`, а `document.forms.namedItem(...)` — `(reading 'namedItem')`.

Классы на месте (`HTMLCollection`, `HTMLFormControlsCollection`,
`HTMLOptionsCollection` — глобалы есть), `form.elements` работает, то есть
не хватает ровно объявления геттеров.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant collections`
(2026-08-23, dev-release, Linux):

```
globals = HTMLCollection,NodeList,HTMLFormControlsCollection,HTMLOptionsCollection,NamedNodeMap
doc-images = 1                 live-update = 1->2
doc-forms   THREW Cannot read properties of undefined (reading 'length')
doc-scripts THREW Cannot read properties of undefined (reading 'length')
doc-links = undefined          namedItem THREW ... (reading 'namedItem')
getElementsByTagName = 1       children = 6        form-elements = 1
```

## Цена по WPT

3 id снимка WPT-RUN-5 (механизм `document-collections-missing`):
`shadow-dom/leaktests/html-collection.html`,
`html/semantics/forms/the-button-element/button-events.html` и
`html/semantics/forms/the-form-element/form-autocomplete.html` — два последних
начинаются с `document.forms.fm1.onsubmit = ...`, то есть падают на первой же
строке, не зарегистрировав ни одного `test()`. Форма шире кластера:
`document.forms` и `document.scripts` — обиходные точки любой страницы, не
только тестовой.

## Что дальше

Одна строка на коллекцию по образцу `get images()` — тот же
`_lumen_make_nid_collection` с другим селектором (`form`, `script`,
`a[href], area[href]`), плюс `namedItem`, который `HTMLCollection` уже умеет.


## Реальные сайты (2026-09-24, разбор совместимости без блокировщика)

Четыре сайта из top100 падают именно здесь:

- **imdb, espn, amazon** — сайт отдаёт 202 с челленджем AWS WAF; `challenge.js` `_0x4f1b75` перебирает
  `document.scripts[i]` → `[unhandled-rejection] TypeError: Cannot read properties of undefined
  (reading 'length')`. После четырёх попыток — «Max challenge attempts exceeded»: imdb 14 узлов против
  1939, espn 13 против 6253. С подставленным `document.scripts` (геттер на
  `getElementsByTagName('script')`) живой Lumen проходит `AwsWafIntegration.getToken()` без ошибок.
  Репро — `.tmp/compat/g1/doc_scripts.html`, полный челлендж — `.tmp/compat/g1/awswaf.html` и
  `awswaf.html?shim=1` в worktree аудита.
- **discord** — Webflow-чанк `for(var a=document.links,l=0;l<a.length;…)` → `reading 'length'`,
  1051 узел против 1210.

Передан P6 по решению пользователя; по числу затронутых сайтов — первый в очереди.
