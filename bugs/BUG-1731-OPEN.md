# BUG-1731 — фокус в теневых деревьях: `document.activeElement` не ретаргетится, `:focus` не матчит хост, `delegatesFocus` не делегирует

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js/shell (`crates/js/src/shim/web_api_shim_mid.js` `get activeElement()` ~стр. 14011, `focus()`; псевдокласс `:focus`/`:focus-within` в селекторном движке)

## Симптом

Проба `lumen --dump-layout` (focus() в обычном документе, без Tab):

```
host.attachShadow({mode:'open'}).appendChild(input); input.focus();
document.activeElement.nodeName → "INPUT"  (ожидание: хост — DOM-ретаргетинг DocumentOrShadowRoot.activeElement); sr.activeElement → INPUT (верно)
host с delegatesFocus:true, внутри <input id=q>;  host.focus()
  sr.activeElement → null (ожидание: #q — фокус не делегирован), host.matches(':focus') === false
inner.focus(); host.matches(':focus') → false   (ожидание true: хост shadow-including-предок фокусного элемента матчит :focus; :focus-within тоже)
```

## Как найдено

WPT-SHADOW-SCOUT (кластер H): `focus/focus-method-delegatesFocus.html` 2/15, `focus-pseudo-matches-on-shadow-host.html` 8/20, `focus-selector-delegatesFocus.html` 6/12, `DocumentOrShadowRoot-activeElement.html` 2/6, `focus-autofocus.html` 1/5, `ShadowRoot-interface.html` 10/12, reftest'ы `focus-pseudo-on-shadow-host-1/2` (FAIL), `focus-within-shadow.html`; 15 id, потолок ≈ 8 баллов.

## Что делать

(1) `activeElement` у документа и корня — ретаргетинг через `host` до того дерева, в котором лежит получатель; (2) `focus()` на хосте с `delegatesFocus` — «focus delegate» (первый фокусируемый в порядке дерева, с учётом `autofocus`); (3) `:focus`/`:focus-within` — по shadow-including-предкам фокусного элемента, `:focus` на хосте с `delegatesFocus` — только если фокус внутри.

## Как проверить

Пробы выше; `run_corpus.py --prefixes shadow-dom/focus,shadow-dom/ShadowRoot-interface.html` без Tab-тестов ([AUTOMATION-KEY-DEFAULT](../ROADMAP.md) нужен для `focus-tabindex-order-*`).
