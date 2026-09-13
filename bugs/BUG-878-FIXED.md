# BUG-878 — `<script src>`, добавленный в shadow root, не загружается и не исполняется: запроса нет вовсе

**Статус:** FIXED 2026-09-13
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `currentscript`)
**Область:** сбор подресурсов идёт по light-DOM-дереву (`crates/engine/layout/src/box_tree.rs::collect_requests_inner`, ср. [BUG-848](BUG-848-FIXED.md), позже починено); вставка узла в `ShadowRoot` (`crates/js/src/dom.rs:1310-1366` — `_lumen_make_shadow_root`, метод `appendChild`) не поднимает подготовку скрипта («prepare a script», HTML LS §4.12.1)
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
([BUG-826](BUG-826-FIXED.md)), а страница ничего не узнаёт, потому что
события всё равно не приходят.

## Цена по WPT

`shadow-dom/Document-prototype-currentScript.html` — все четыре сабтеста
файла («must not be set … in an open shadow tree», closed, и две
проверки «был в теневом дереве и удалён»): каждый ждёт `onload` от скрипта,
вставленного в shadow root.

## Причина (найдена чтением кода, не там, где предполагала карточка)

Загрузка динамически созданного `<script src>` уже была общим кодом для
любой вставки — `_lumen_resource_pending`/`_lumen_resource_try_prepare`
(`crates/js/src/shim/web_api_shim_mid.js`, BUG-571), а не отдельным путём
для документа. Дело было не в отсутствии «prepare a script» для
`ShadowRoot.appendChild` (эта функция и так вызывает тот же
`_lumen_append_child`, разнесённый глобальной обёрткой на все точки
вставки), а в проверке connected-состояния: `_lumen_resource_is_connected`
поднималась вверх только по настоящему `Node::parent`. `ShadowRoot`
намеренно НЕ является DOM-ребёнком своего host'а (доккомент
`Document::attach_shadow`: «not a DOM child of host»), поэтому подъём от
вставленного `<script>` доходил до узла `ShadowRoot` и там читал `None`
навсегда — элемент так и оставался в `_lumen_resource_pending`, фетч не
стартовал никогда, независимо от того, был ли host реально в документе.

Тот же дефект был и у нативного моста `_lumen_get_shadow_root_host`
(`crates/js/src/v8_runtime/install/dom_core.rs`): найдя узел `ShadowRoot`,
он пытался прочитать `node.parent` у самого этого узла вместо обратного
поиска по карте `host -> root` — `lumen_dom::Document` уже хранит
это отображение и уже умеет искать в обратную сторону для другого случая
(`enclosing_shadow_host`, стилевой каскад), но эта готовая логика не была
переиспользована в мосте, которым пользуется JS.

## Фикс (2026-09-13, `p1-gap-loadev-bug878`)

- `crates/engine/dom/src/lib.rs`: добавлен `Document::shadow_host_of` —
  прямой обратный поиск `root -> host` по карте `shadow_roots` (симметрия
  уже существующего `shadow_root_of`).
- `crates/js/src/v8_runtime/install/dom_core.rs`: `_lumen_get_shadow_root_host`
  использует `shadow_host_of` вместо чтения несуществующего `node.parent`
  у `ShadowRoot`-узла.
- `crates/js/src/shim/web_api_shim_mid.js`: `_lumen_resource_is_connected`
  и `Node.isConnected` — при упоре в `null` от `_lumen_get_parent` пробуют
  мост `_lumen_get_shadow_root_host`, прежде чем признать узел
  отсоединённым. Оба места намеренно держат один и тот же приём — исходный
  комментарий уже называл их «тем же тестом», но код разошёлся.

**Побочный эффект той же правки:** `HTMLSlotElement.assignedNodes()`
(`web_api_shim_mid.js:7172`) вызывает тот же сломанный
`_lumen_get_shadow_root_host` для получения хоста слота и до этой правки
всегда возвращал `[]`, независимо от разметки. Отдельного бага под это не
заводилось — тот же вызов, тот же фикс.

**Не в этом срезе:** `document.currentScript` внутри теневого дерева
(должен быть `null` по спеке, HTML LS §4.12.1) не проверялся отдельно —
фикс закрывает «запроса нет вовсе», а не весь набор семантики
`currentScript`. BUG-798 (`<embed>`/`<object>` без резолва ресурса вовсе)
не затронут — не тот же путь кода.

### Проверка фикса

Два новых теста `crates/js/src/dom/tests/v8_webworker.rs`:
`dynamic_external_script_in_shadow_root_executes_and_fires_load` (тот же
сценарий чанк-лоадера, что и BUG-571, но `root.appendChild` вместо
`document.head.appendChild`) и
`element_in_connected_shadow_root_is_connected`. Оба падают без фикса
(`git stash` на изменённые файлы, тот же прогон) и проходят с ним;
`cargo test -p lumen-js --features v8-backend --lib` — 3610/3610.

Гейт: `cargo clippy -p lumen-dom --all-targets -- -D warnings` и
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чисты. `scripts/scoped-test.sh` — единственный красный
(`cpu_snapshots_match_references`, те же 7 файлов) подтверждён A/B через
`git stash` тем же на этой ветке без правки — предсуществующий дрейф, не
регрессия (правка не трогает paint/layout).
