# BUG-1845 — inline-обработчики: в цепочке областей нет `document` и формы-владельца — `onsubmit="login.disabled = true"` формы входа Keycloak падает с `login is not defined`

**Статус:** OPEN
**Заведён:** 2026-11-26 (ZCode, portal.nextera.ru — страница входа Keycloak, сравнением с Chrome 158.0.8127.37)
**Область:** js ([`crates/js/src/shim/web_api_shim_mid.js:998`](../crates/js/src/shim/web_api_shim_mid.js) — `_lumen_compile_inline_handler`)

## Симптом

Форма входа портала (`nextera.ru/auth-keycloak/...`):

```html
<form id="kc-form-login" onsubmit="login.disabled = true; return true;">
  <button name="login" id="kc-login" type="submit">Sign in</button>
```

Отправка формы в Lumen пишет в журнал `[JS error] Uncaught ReferenceError: login is not defined`
(`.tmp/lumen-live-20261126-w.log`): внутри inline-обработчика `login` — это кнопка `name="login"`,
имя которой обязано попадать в область через форму-владельца. Вход при этом проходит: исключение в
`onsubmit` не отменяет отправку. В Chrome ошибки нет.

## Проба (одинаковый код в обоих браузерах)

Форма с `<input name=probeEl>` и `<button name=probeBtn>`, обработчик
`onsubmit="window.__probe={form:typeof probeEl,btn:typeof probeBtn,doc:typeof body};return false"`,
вызван `dispatchEvent(new Event("submit", {cancelable:true}))`:

| | Chrome 158 | Lumen a0666c3564+патч |
|---|---|---|
| `typeof probeEl` (элемент формы) | `object` | `undefined` |
| `typeof probeBtn` (кнопка формы) | `object` | `undefined` |
| `typeof body` (через `document`) | `object` | `undefined` |

По HTML-спеке (event handler processing algorithm) обработчик компилируется с цепочкой областей
элемент → форма-владелец → `document` → глобальная: имена потомков формы и самого `document`
видны без квалификации. В Lumen компиляция — глобальная область + `this` (BUG-360 сделал «обработчики
вообще работают» через `new Function('event', body)`, без областей).

## Что делать

`_lumen_compile_inline_handler` строит цепочку `with(element) with(formOwner) with(document)` вокруг
тела обработчика (форма-владелец — ближайший предок `<form>`, по `HTMLFormElement` named properties;
`document` даёт имена `body`, отдельных элементов с `id`/`name`).

## Как проверить

Проба выше — `object` × 3; страница входа Keycloak портала — без `ReferenceError` в журнале.
