# BUG-1045: `getRootNode()` не пересекает границу shadow-дерева — у `ShadowRoot` метода нет вовсе, а узел внутри теневого дерева получает Element-обёртку вместо корня

**Статус:** FIXED 2026-09-23 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_get_root_node`, `_LUMEN_WRAPPER_MEMBERS.getRootNode`, `ShadowRoot.prototype.getRootNode`) + нативный слой (`crates/js/src/v8_runtime/install/dom_core.rs::_lumen_get_shadow_root_mode`)
**Найден:** P6, 2026-09-09, при закрытии [BUG-599](BUG-599-FIXED.md) (дорожка E2E)

## Симптом

Проба на живом рантайме (`V8JsRuntime` + `install_dom`, элемент с
`attachShadow({mode:'open'})` и одним потомком внутри теневого дерева):

```
typeof sr.getRootNode                      → "undefined"
inner.getRootNode().constructor.name       → "Element"
inner.getRootNode().host                   → undefined
inner.getRootNode() === document           → false
```

То есть:

* `shadowRoot.getRootNode()` бросает `TypeError` — метода нет;
* у узла **внутри** теневого дерева обход поднимается до узла `ShadowRoot`, но
  отдаёт его через `_lumen_make_element` — обычную Element-обёртку без `host`,
  а не объект `ShadowRoot`.

## Причина

Механизм, а не недосмотр в одном месте:

1. `_LUMEN_WRAPPER_MEMBERS.getRootNode` (`web_api_shim_mid.js:7444`) идёт по
   `_lumen_get_parent` до узла без родителя и возвращает `document`, если
   дошёл до корня документа, иначе `_lumen_make_element(cur)`. Опознать, что
   `cur` — это shadow-корень, ему нечем.
2. `Document::attach_shadow` (`crates/engine/dom/src/lib.rs:689`) кладёт
   `ShadowRoot` через `alloc` **без родителя**: связь с хостом живёт только в
   карте `shadow_roots: host → sr`. Поэтому обход и останавливается на
   shadow-корне, а не выходит на документ.
3. Обратного отображения `sr → host` наружу нет. Единственный биндинг с таким
   названием, `_lumen_get_shadow_root_host` (`dom_core.rs:1126`), поднимается
   до узла `ShadowRoot` и возвращает `node.parent` — а он у shadow-корня по
   построению `None`. Отдельно проверить его поведение стоит: тот же аргумент
   говорит, что `assignedNodes` (`web_api_shim_mid.js:7026`) на этом биндинге
   тоже не находит хост, но эта половина пробой **не** подтверждена и здесь не
   утверждается.
4. `_lumen_make_shadow_root` (`:2537`) строит объект через
   `Object.create(ShadowRoot.prototype)`, а на самом прототипе (`:3197`,
   наследует `DocumentFragment.prototype`, у которого из членов только
   `constructor`) `getRootNode` не определён. Собственный `getRootNode` есть
   только у литерала обычного `DocumentFragment` (`:2646`).

Правильное поведение — DOM §4.4: `getRootNode()` возвращает shadow-inclusive
root, то есть сам `ShadowRoot`; `getRootNode({composed: true})` продолжает
подъём через `host` до документа. Сейчас не работает ни та, ни другая ветка, и
опция `composed` игнорируется обёрткой полностью.

## Масштаб

* Вендоренный `tools/wptrunner/wptrunner/testdriver-extra.js` строит селектор
  ровно этим переходом: `current = current.getRootNode().host` (:163) и
  `element.getRootNode() == element.ownerDocument` (:144). Для узла в теневом
  дереве `.host` сейчас `undefined` — цикл обрывается **молча**, и
  `test_driver_internal.*` получает усечённый селектор вместо ошибки. Это
  остаток той же поломки, что описана в [BUG-599](BUG-599-FIXED.md): там
  чинилась ветка «корень = документ», здесь остаётся ветка «корень = shadow».
* [BUG-676](BUG-676-FIXED.md) (`ShadowRoot` как настоящий класс) явно записал
  `sr.getRootNode` как сознательно отложенный «в BUG-574/BUG-599»; при закрытии
  BUG-599 этот пункт был бы потерян — этот баг его и подхватывает.
* Цены на реальных страницах пока нет: E2E-проба (Keycloak + Next.js App
  Router) упиралась в `document.getRootNode`, а не в shadow-ветку. Веб-компоненты
  корпусом не измерялись.

## Что нужно для починки

Не однострочник — нужен путь nid → `ShadowRoot`-объект:

1. нативный биндинг «этот nid — shadow-корень?» и «его host» (карта
   `shadow_roots` уже есть, наружу не выведена; заодно проверить п.3 выше);
2. `getRootNode` в обёртках: остановиться на shadow-корне и вернуть
   `_lumen_make_shadow_root(cur, mode, host)`, а при `composed: true` —
   продолжить подъём с хоста;
3. `ShadowRoot.prototype.getRootNode` с той же семантикой `composed`;
4. регресс-тесты рядом с `crates/js/src/dom/tests/v8_bug599_get_root_node.rs`.

## Исправлено (2026-09-23, P3)

Пункт 1 оказался уже наполовину сделан по ходу других багов и не требовал
нового биндинга «этот nid — shadow-корень»: `_lumen_is_shadow_root` (нативный,
`dom_core.rs`) появился раньше этой сессии и просто не был подключён к
`getRootNode`; `_lumen_get_shadow_root_host` уже честно возвращает host через
карту `Document::shadow_host_of` (починен в BUG-878, а не читает `node.parent`,
как описывала эта карточка изначально). Не хватало только режима корня —
`_lumen_get_shadow_root` (host → root) намеренно прячет `closed`-корни ради
инкапсуляции `Element.shadowRoot`, а для `getRootNode()` эта инкапсуляция не
применяется: скрипт, стоящий внутри `closed`-дерева, уже держит ссылку на узел
этого дерева, так что скрывать `mode` смысла нет. Добавлен отдельный
`_lumen_get_shadow_root_mode(nid)`, который берёт `mode` напрямую по nid
корня, без host-скрывающей логики.

Реализация:

* общий JS-хелпер `_lumen_get_root_node(nid, options)` (`web_api_shim_mid.js`,
  рядом с `_lumen_make_shadow_root`) — поднимается по `_lumen_get_parent`,
  на первом узле без родителя проверяет `_lumen_is_shadow_root`; если да —
  строит `ShadowRoot` через `_lumen_make_shadow_root(cur, mode, host)` (или
  продолжает подъём с `host`, если передан `composed: true`); иначе
  возвращает `document` или обёртку топ-узла отсоединённого поддерева, как
  раньше;
* `_LUMEN_WRAPPER_MEMBERS.getRootNode` и `ShadowRoot.prototype.getRootNode`
  сведены к этому хелперу — у `ShadowRoot` метода не было вовсе, теперь есть;
* `_lumen_make_shadow_root` кеширует по nid, поэтому `inner.getRootNode() ===
  host.shadowRoot` (или `=== attachShadow()`'s return value) — тот же объект,
  не новая обёртка.

Регресс-тесты — 4 новых в `crates/js/src/dom/tests/v8_bug599_get_root_node.rs`
(узел внутри open-дерева, `composed: true` до документа, closed-корень с
честным `mode` при скрытом `element.shadowRoot`, `ShadowRoot` как свой
собственный корень). `cargo test -p lumen-js --features v8-backend --lib`:
4153/4153. `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` чист. Правка только в `crates/js` (JS-шим + один нативный
биндинг) — display list не затронут, полный `graphic_tests/run.py` не
требовался.

Не в скоупе: `assignedNodes` (`web_api_shim_mid.js`) — та же карточка
упоминала его как возможную вторую грань той же поломки, но это не
проверялось и не чинилось здесь.
