# BUG-1017 — `document.defaultView` не существовал у главного документа

**Статус:** FIXED 2026-09-06
**Крейт:** js (`crates/js/src/shim/web_api_shim_mid.js` — объект `document` и
`_lumen_build_detached_document`)
**Найден:** P1, 2026-09-06, живым прогоном google.com после [BUG-1015](BUG-1015-FIXED.md)

---

## Симптом

В каждом живом прогоне `google.com` скрипты страницы падали:

```
[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'devicePixelRatio')
    at new vwa (<anonymous>:363:80)
    …
    at _._ModuleManager_initialize (<anonymous>:1033:90)
script error: JS runtime error: Cannot read properties of undefined (reading 'devicePixelRatio')
```

Ошибка обрывала `_ModuleManager_initialize` — то есть заметная часть страницы просто
не инициализировалась.

## Механизм

`devicePixelRatio` у нас есть (`web_api_shim_tail_mc.js` ставит `globalThis.devicePixelRatio = 1`).
Undefined был объект, у которого его читали. Проба перебором кандидатов на сборке до фикса:

```
window.devicePixelRatio=number:1
document.defaultView=undefined                ← вот он
document.defaultView.dpr=THROW:TypeError:Cannot read properties of undefined (reading 'devicePixelRatio')
div.ownerDocument.defaultView=undefined
window.top=object          window.top.dpr=number:1
window.parent=object       window.parent.dpr=number:1
window.self.dpr=number:1
```

`Document.defaultView` (HTML §3.1.5) — WindowProxy браузингового контекста документа —
у живого документа не был определён вовсе. Идиома, через которую его читают, —
`node.ownerDocument.defaultView.<что-нибудь>`; у google это `devicePixelRatio`.

**Почему пробел прожил так долго:** у под-документов `<iframe>` `defaultView` есть с самого
начала — его определяет фасад `contentDocument` в `crates/js/src/frame_bridge.rs:1897`,
и там же есть тест `d1.defaultView === w1`. Отсутствовал он ровно у верхнеуровневого
`document`, которого этот фасад не касается.

## Фикс

1. Геттер `defaultView` на объекте `document`: возвращает `window` (в шиме `window`
   и есть глобальный объект, BUG-280).
2. `_lumen_build_detached_document` (`new Document()`, `DOMImplementation.createDocument`/
   `createHTMLDocument`) — `defaultView` возвращает `null`: у такого документа нет
   браузингового контекста (HTML §3.1.5). Прописано именно там, а не унаследовано,
   потому что `proto` — это `Document.prototype`, общий с живым документом.

## Проверка

- `document_default_view_is_the_window`, `owner_document_default_view_reaches_window_properties`
  (та самая идиома через элемент, до `devicePixelRatio`), `detached_document_default_view_is_null`
  — `crates/js/src/dom/tests/v8_core/mod.rs`.
- Та же проба на пересобранной сборке: `document.defaultView=object`,
  `document.defaultView.dpr=number:1`, `div.ownerDocument.defaultView=object`,
  `createHTMLDocument().defaultView=null`.
- Живой `google.com --maximized`: **0 ошибок JS** против TypeError в каждом прогоне до фикса.
- `cargo test -p lumen-js --features v8-backend` — 3520/3521, единственный провал
  предсуществующий [BUG-997](BUG-997-OPEN.md).
- `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings` — чисто.

## Попутные находки — ИСПРАВЛЕНО 2026-09-06, две из трёх были артефактом метода

Первая редакция этого раздела называла три соседние дыры. **Две из них не существуют:**
они измерены headless-прогоном (`--trace-nav`), а headless-одноходовка не создаёт
браузинговые контексты фреймов вовсе — это уже описанное свойство пути
([docs/automation.md](../docs/automation.md) §Headless), а не поведение движка.
Перемер в живом окне:

```
bare:  cw=object cd=object location.href=about:blank URL=about:blank
blank: cw=object cd=object location.href=about:blank URL=about:blank
real:  cw=object cd=object location.href=http://…/child.html
window.length=3  frames.length=3
```

- ~~`iframe.contentWindow`/`contentDocument` — `null` у `<iframe>` без `src`~~ — неверно,
  в живом окне оба объекты.
- ~~`window.frames[0]` — `undefined`~~ — неверно, `window.length === 3`.
- **`about:blank` действительно обрабатывался неправильно**, но не так, как здесь было
  написано: контекст создавался, а вот `<iframe src="about:blank">` получал
  синтетическую страницу «Не удалось загрузить фрейм» вместо пустого документа.
  Заведено и исправлено отдельно — [BUG-1018](BUG-1018-FIXED.md).

**Урок метода:** headless-прогон — не источник утверждений о фреймах, окнах и всём,
что живёт на событийном цикле. Проверять в живом окне прежде, чем записывать находку.
