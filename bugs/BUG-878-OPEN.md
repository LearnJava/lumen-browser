# BUG-878 — `<script src>`, добавленный в shadow root, не загружается и не исполняется: запроса нет вовсе

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `currentscript`)
**Область:** сбор подресурсов идёт по light-DOM-дереву (`crates/engine/layout/src/box_tree.rs::collect_requests_inner`, ср. [BUG-848](BUG-848-OPEN.md)); вставка узла в `ShadowRoot` (`crates/js/src/dom.rs:1310-1366` — `_lumen_make_shadow_root`, метод `appendChild`) не поднимает подготовку скрипта («prepare a script», HTML LS §4.12.1)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var root = host.attachShadow({mode: 'open'});
var s = document.createElement('script');
s.src = 'x.js';
root.appendChild(s);   // ничего не происходит
```

Сервер не видит запроса за `x.js` вовсе, скрипт не исполняется, событий
`load`/`error` нет. Тот же файл, подключённый обычным `<script src>` в
документе, грузится и исполняется нормально — то есть дело именно в
вставке в теневое дерево.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant currentscript`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`). Страница подключает
`vcip-currentscript.js` обычным тегом и тот же файл — с меткой
`?in-shadow` — через `root.appendChild`:

```
cs-inline is-script=script
cs-external is=script src=vcip-currentscript.js root=document   ← обычный
cs-shadow-appended
cs-checked
[server saw: GET /vcip-currentscript.js]                        ← только один
```

`?in-shadow` в списке запросов сервера отсутствует. Сервер — единственный
свидетель: собственный лог браузера про запрос молчит либо врёт
([BUG-826](BUG-826-OPEN.md)), а страница ничего не узнаёт, потому что
события всё равно не приходят.

## Цена по WPT

`shadow-dom/Document-prototype-currentScript.html` — все четыре сабтеста
файла («must not be set … in an open shadow tree», closed, и две
проверки «был в теневом дереве и удалён»): каждый ждёт `onload` от скрипта,
вставленного в shadow root.

## Что дальше

«Prepare a script» в HTML LS §4.12.1 привязан к «becomes connected», а
теневое дерево — часть композированного дерева документа, поэтому вставка
в него обязана готовить скрипт ровно так же, как вставка в документ.
Смежная форма того же пробела — сбор подресурсов вообще (BUG-848: запрос
собирается только с `<img>` и только по light-DOM).
