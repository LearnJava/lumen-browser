# BUG-890 — глобального конструктора `CustomElementRegistry` нет, а вместе с ним и всей области видимости реестров: `new CustomElementRegistry()`, `createElement(..., {customElements})`, `importNode(..., {customElements})`

**Статус:** OPEN (ДОРАБОТКА → [GAP-CEREG](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-CEREG` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `custom-registry`)
**Область:** js (`crates/js/src/dom.rs:6716` — `var customElements = {` — реестр существует как ОДИН объектный литерал, класса за ним нет; `grep -n "CustomElementRegistry" crates/` — ноль совпадений)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Сам реестр работает: `customElements.define()` регистрирует класс,
`connectedCallback` вызывается при вставке, `get`/`whenDefined` на месте. Но за
единственным экземпляром нет интерфейса — `typeof CustomElementRegistry` даёт
`undefined`, поэтому:

* `new CustomElementRegistry()` — `ReferenceError`;
* `document.createElement(tag, {customElements: reg})` и
  `document.importNode(node, {customElements: reg})` (форма из HTML LS
  [whatwg/html#10854](https://github.com/whatwg/html/issues/10854)) не с чем
  вызвать;
* `element.attachShadow({customElements: ...})` и
  `shadowrootcustomelementregistry` — тем более.

Падение синхронное и на первой строке файла, поэтому вердикт TIMEOUT: ни один
`test()` зарегистрироваться не успевает.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant custom-registry`
(2026-08-23, dev-release, Linux):

```
customElements = object        global-ctor = undefined
define = defined               ce-connected
upgrade-on-append = appended   get = function        whenDefined = function
new-registry        THREW CustomElementRegistry is not defined
createElement-opts  THREW CustomElementRegistry is not defined
importNode-opts     THREW CustomElementRegistry is not defined
```

## Цена по WPT

5 id снимка WPT-RUN-5 с текстом `CustomElementRegistry is not defined`, вся
папка `custom-elements/registries/`: `Document-importNode.html`,
`Document-createElement.html`, `Document-createElementNS.html`,
`scoped-registry-initialize.html`,
`scoped-registry-effective-global-registry.html` — последний до этого среза
числился за [BUG-480](BUG-480-OPEN.md) по маркеру исходника (`<iframe>` в
файле есть), хотя бросает он раньше, ещё до фрейма.

## Что дальше

Минимальный шаг, закрывающий текст ошибки, — вынести литерал в класс
`CustomElementRegistry` и опубликовать глобал, оставив `window.customElements`
его экземпляром (тесты `registries/*` пойдут дальше первой строки и станут
честными FAIL). Сама область видимости (реестр на теневое дерево) — отдельная
работа: `_lumen_ce_*`-натив знает один глобальный словарь определений.

**Срез 1 (2026-09-22, P6, `p6-gap-cereg`):** сделан ровно этот минимальный шаг
— `crates/js/src/shim/web_api_shim_mid.js` (`var customElements = {...}` →
`function CustomElementRegistry(registryStore, pendingStore) {...}` с методами
на прототипе; `window.customElements = new CustomElementRegistry(_lumen_ce_registry,
_lumen_ce_pending)` делит хранилище с натив-хуками апгрейда
(`_lumen_ce_maybe_connected`/`_maybe_disconnected`/`_maybe_attr_changed`,
которые продолжают читать глобальные `_lumen_ce_registry`/`_lumen_ce_pending`
напрямую), а `new CustomElementRegistry()` без аргументов получает свои
приватные `_registry`/`_pending` — изолированные, но НЕ привязанные ни к
дереву, ни к `_lumen_ce_upgrade_all` (апгрейд при вставке элемента срабатывает
только для реестра, чьи хранилища совпадают с глобальными — проверка
`this._registry === _lumen_ce_registry` в `define()`). `ReferenceError` на
`new-registry`/`createElement-opts`/`importNode-opts` снят; `createElement`/
`importNode`/`attachShadow` по-прежнему игнорируют опцию `customElements` —
привязка отдельного реестра к дереву (scoped registries, HTML LS §4.13.1)
остаётся открытой частью GAP-CEREG, статус не меняется. Тесты:
`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs` —
`custom_elements_registry_is_a_public_constructor`,
`custom_elements_registry_new_instance_is_isolated_from_global`; все 91
существующих `*_custom.rs`-теста и 9/9 `custom_elements_*`-тестов проходят
без изменений поведения. Верифицировано `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` (чисто) и `cargo test -p lumen-js
--features v8-backend` (2070+91 ok, регрессий нет); полный `scoped-test.sh`
не был дождан до конца из-за линковки нескольких v8-бинарников подряд на
машине с ограниченной памятью (правки не затрагивают Rust-API других
крейтов, поэтому реверс-зависимости не могут быть задеты).

**Срез 3 (2026-09-22, P6, `p6-gap-cereg-srez3`): GAP-CEREG ЗАКРЫТ.**
Оставшийся кусок — неявное наследование области видимости через HTML-парсер
(`innerHTML`/`insertAdjacentHTML` внутри scoped shadow root, без явной опции
`customElements`). Разрешение области у `_lumen_ce_registry_for_nid` уже было
динамическим обходом дерева вверх, так что проблема была не в разрешении, а в
том, что путь через строку разметки вообще не запускал upgrade reaction:
`_lumen_set_inner_html` (общая обёртка для `Element.innerHTML` и
`ShadowRoot.innerHTML`) и `insertAdjacentHTML` (через `before`/`prepend`/
`append`/`after`) не вызывали `_lumen_ce_maybe_connected` ни разу — даже для
элемента, зарегистрированного в ГЛОБАЛЬНОМ `customElements`. Добавлена
`_lumen_ce_upgrade_subtree(nid)` (`web_api_shim_mid.js`) — рекурсивный обход
всего вставленного поддерева, а не только узла верхнего уровня (разметка
`innerHTML` может завести кастомный элемент на любой глубине за один вызов);
подключена в `_lumen_set_inner_html` (по прямым детям `nid`, сам `nid` не
трогаем — он не был (пере)подключён) и в `insertAdjacentHTML` (по каждому
распарсенному узлу верхнего уровня). Поскольку резолвинг области остаётся
тем же самым обходом дерева, scoped-реестр shadow root подхватывается
автоматически, без отдельного кода на этот случай. Тесты:
`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs` —
`custom_element_upgraded_via_inner_html`,
`custom_element_upgraded_via_insert_adjacent_html`,
`custom_element_scoped_registry_inherited_via_inner_html_in_shadow_root`,
`custom_element_upgraded_via_inner_html_at_any_depth`. Верифицировано
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
(чисто), `cargo test -p lumen-js --features v8-backend` (4128 ok, два
случайных флака — `frame_bridge::…`, `credentials::…` — воспроизведены и на
main без правок, изоляцией подтверждена независимость от изменения) и
`scripts/scoped-test.sh` (единственный красный тест —
`lumen-driver::cases::snapshot_cpu::cpu_snapshots_match_references`,
предсуществующий дрейф, воспроизведён и без правок).
