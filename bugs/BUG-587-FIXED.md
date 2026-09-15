# BUG-587: WindowProxy `[[DefineOwnProperty]]` does not enforce unforgeable own properties

**Статус:** FIXED 2026-09-15 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`, `web_api_shim_tail_b.js`)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL [[DefineOwnProperty]] success: "window" - assert_true: [[Get]]: unchanged expected true got false
FAIL [[DefineOwnProperty]] failure: "window" - assert_false: [[Value]], [[Enumerable]]: true expected false got true
```

Same pair of failures for `"window"`, `"document"`, `"location"`, `"top"` — 8
failures total, from
`html/browsers/the-windowproxy-exotic-object/windowproxy-define-own-property-unforgeable-same-origin.html`.

## Причина

Per the WindowProxy `[[DefineOwnProperty]]` algorithm (HTML LS
`#windowproxy-defineownproperty`), same-origin own accessor properties like
`window`, `document`, `location`, `top` are unforgeable: a "compatible"
redefinition must leave the underlying `[[Get]]` behavior unchanged, and an
"incompatible" one (e.g. flipping `enumerable`/turning it into a data
property) must be rejected outright rather than silently applied. Lumen's
WindowProxy accepts both kinds of redefinition as if the property were an
ordinary configurable own property — no unforgeable-property check exists on
the define-own-property path.

## Масштаб

Narrow, self-contained test file (8/8 assertions failing, no iframe or
cross-origin dependency — same-origin only). Adjacent files in the same
directory (`windowproxy-prototype-setting-cross-origin*.sub.html`,
`windowproxy-prevent-extensions.html`) fail separately on the already-known
"`<iframe>` without browsing context" limitation, not this defect.

## Исправление

Уточнение при разборе: `location` уже был правильно оформлен — единственный
accessor из четырёх, определённый через `Object.defineProperty(globalThis,
'location', {get, set, enumerable: true, configurable: false})`
(`web_api_shim_mid_b.js`), поэтому весь тест для него уже проходил.
`window`/`document`/`top`, наоборот, были обычными configurable/writable DATA-
свойствами: `document` появлялся через `var document = {...}`, `window` через
`window = globalThis` (плюс промежуточный `var window = {...}`), `top` через
плоское присваивание `window.top = window`. Стандартный алгоритм
`[[DefineOwnProperty]]` для DATA-свойства не запрещает смену типа на accessor
(если объект ещё configurable) и не запрещает смену значения (если тип не
менялся) — отсюда обе половины симптома: несовместимое переопределение
(`{get}`, преобразующее в accessor) сначала неожиданно проходило, а после
того как redefine с `configurable: false` реально фиксировал свойство,
попытка вернуть его в DATA-форму (`{value, enumerable: true}`) тоже проходила,
хотя должна была быть отклонена.

Правка переводит `window`/`document`/`top` в такие же non-configurable
accessor-свойства, как `location`, — дальше HTML-специфичного кода не нужно
вообще: сам движок JS (`OrdinaryDefineOwnProperty`) уже реализует ровно
проверку совместимости, которую требует алгоритм WindowProxy
`[[DefineOwnProperty]]#windowproxy-defineownproperty`, если свойство —
non-configurable accessor. `document`: геттер над захваченным по замыканию
объектом-синглтоном (`web_api_shim_mid.js`, сразу после
`Object.setPrototypeOf(document, Document.prototype)`). `window`/`top`:
геттеры, возвращающие `globalThis` (`web_api_shim_tail_b.js`, в блоке
`window = globalThis`) — **обязательно `globalThis`, не бэрное `window`**:
глобальная `var`-переменная резолвится через `[[Get]]` на самом глобальном
объекте, так что геттер, читающий `window`, вызвал бы сам себя и уронил стек
при первом же обращении (поймано тестом на живом прогоне до коммита).
`self`/`frames`/`parent` не тронуты: `frames`/`parent` в спеке
`[Replaceable]`, обычное configurable-свойство для них корректно; `self` вне
скоупа этого файла (не покрыт этим тестом).

Новый тест
`v8_runtime::tests::dom_suspend_focus::window_document_location_top_are_unforgeable_own_properties`
воспроизводит оба сценария теста (`success`/`failure`) для всех четырёх
ключей. `cargo test -p lumen-js --lib --features v8-backend` 3676/3676,
`cargo clippy --workspace --all-targets -- -D warnings` чист. Правка только в
`.js`-шимах и тестовом файле, Rust-логика не тронута.
