# BUG-1727 — `attachShadow()` ничего не проверяет: нет `NotSupportedError`/`TypeError` для чужого хоста, повторного вызова, пустого `mode`, SVG-элемента

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js`, `attachShadow: function(init)`, ~стр. 9769)

## Симптом

Проба `lumen --dump-layout` (ни один вызов не бросил):

```
document.createElement('div').attachShadow({})                                 → не бросает (ожидание: TypeError — mode обязателен)
d.attachShadow({mode:'open'}); d.attachShadow({mode:'open'})                   → не бросает (ожидание: NotSupportedError)
document.createElement('a').attachShadow({mode:'open'})                        → не бросает (ожидание: NotSupportedError, `a` не в списке допустимых хостов)
document.createElement('input').attachShadow({mode:'open'})                    → не бросает
document.createElementNS(SVG, 'article').attachShadow({mode:'open'})          → не бросает (допустимы только HTML-элементы + valid custom element name)
document.createElement('div').attachShadow({mode:'open', slotAssignment:'exceptional'}) → не бросает (ожидание: TypeError, enum)
```

DOM LS §4.9 «attach a shadow root»: хост — HTML-элемент из списка (`article aside blockquote body div footer h1–h6 header main nav p section span`) или элемент с valid custom element name; повторный вызов на хосте с теневым корнем — `NotSupportedError` (кроме декларативного корня, см. [BUG-1228](BUG-1228-OPEN.md)).

## Как найдено

WPT-SHADOW-SCOUT (`docs/wpt-vendor-notes/shadow-dom.md`, кластер C): `attach-shadow-non-html-namespace.html` 38/304 (266 FAIL: «did not throw»), `Element-interface-attachShadow.html` 3/6, `Element-interface-attachShadow-custom-element.html`, `declarative-shadow-dom-attachment.html` (повторный `attachShadow`).

## Что делать

В `attachShadow` до создания корня: (1) `mode` — enum `open|closed`, иначе `TypeError`; `slotAssignment` — enum `named|manual`; (2) хост не из списка/не valid custom element name/не HTML-namespace → `NotSupportedError`; (3) уже есть корень (не созданный декларативным парсером) → `NotSupportedError`. Список допустимых имён вынести рядом с `attachShadow`, не копировать в тесты.

## Как проверить

Пробы выше бросают нужное; `run_corpus.py --prefixes shadow-dom/attach-shadow-non-html-namespace.html,shadow-dom/Element-interface-attachShadow.html` — ≥ 300/310 сабтестов. Осторожно: страницы реального мира вызывают `attachShadow` на `<a>`/`<button>`/`<input>`? — по спеке это ошибка и в Chrome, так что регрессий не ожидается; проверить 179-страничный A/B корпус по `--dump-display-list`.
