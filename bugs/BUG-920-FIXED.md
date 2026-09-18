# BUG-920 — `iframe.src` отдаёт атрибут дословно вместо разрешённого URL: собственное свойство из `iframe_element.rs` затеняет строку рефлексии на прототипе

**Статус:** FIXED 2026-09-19 (P3)
**Заведён:** 2026-08-25 (P1, попутно к [BUG-854](BUG-854-FIXED.md))
**Область:** `crates/js/src/iframe_element.rs` (`patchIframeElement` →
`reflectAttr('src', 'src')`, собственное свойство на элементе) поверх
`crates/js/src/dom.rs` (`_lumen_install_reflection(HTMLIFrameElement.prototype,
[['src', 'src', 'url'], …])`)
**Владелец:** P1/P3

## Симптом

Страница по адресу `http://127.0.0.1:PORT/p.html`:

```html
<iframe src="/child.html"></iframe>
<frame  src="/child.html">
<script>
  document.querySelector('iframe').src;  // "/child.html"        ← должно быть абсолютным
  document.querySelector('frame').src;   // "http://127.0.0.1:PORT/child.html"
</script>
```

HTML LS объявляет `HTMLIFrameElement.src` как `[ReflectURL] USVString`, то есть
геттер обязан вернуть URL, разрешённый относительно базы документа. Тот же
`[ReflectURL]` стоит у `HTMLFrameElement.src`, и там Lumen отвечает верно —
разница не в спеке, а в том, какой из двух кодов победил.

## Механизм

Строка рефлексии на `HTMLIFrameElement.prototype` (`dom.rs`) заведена с типом
`'url'` и разрешает адрес через `_lumen_reflect_url` — она корректна. Но
`iframe_element.rs::patchIframeElement` ставит на **сам элемент** собственное
свойство `src`, читающее `getAttribute('src')` как есть; собственное свойство
всегда выигрывает у прототипного, поэтому корректный аксессор недостижим.

Это форма [BUG-796](BUG-796-FIXED.md), где такое же затенение (`content` из
общей таблицы врапперов) отбирало `meta.content` у каждого элемента, только
здесь затеняющее свойство ставится не таблицей врапперов, а отдельным шимом.
Тем же путём затеняются `name`, `srcdoc`, `width`, `height`, `sandbox`,
`allow`, `referrerPolicy`, `loading` — из них `referrerPolicy` теряет ещё и
enum-нормализацию (прототипная строка приводит значение к известному ключу,
собственное свойство отдаёт мусор дословно).

## Как проверить фикс

`iframe.src` на странице выше должен стать абсолютным, а
`iframe.referrerPolicy = 'BOGUS'` — читаться как `''`. Контроль: `<frame>` на
той же странице уже отвечает правильно, поэтому расхождение двух тегов в одном
прогоне — самая дешёвая проверка.

Осторожно: просто удалить `reflectAttr(...)` из `patchIframeElement` мало —
`contentDocument`/`contentWindow` и `getSVGDocument` там же, и они нужны;
проверять надо весь набор членов, а не только `src`.

## Фикс (P3, 2026-09-19)

Из `patchIframeElement` (`iframe_element.rs`) убраны собственные свойства
`src`/`name`/`srcdoc`/`allow`/`referrerPolicy`/`loading` — на прототипе
(`_lumen_install_reflection(HTMLIFrameElement.prototype, …)`, `web_api_shim_tail_b.js`)
для них уже стоит корректная строка (`url`-резолв для `src`, `enum`-нормализация
для `referrerPolicy`), и она теперь достижима.

Уточнение к «Механизму»: `width`/`height`/`sandbox` затенения на самом деле НЕ
было — для них нет строки на `HTMLIFrameElement.prototype` вообще (только
собственное свойство из `iframe_element.rs`), поэтому они оставлены как есть.
`contentDocument`/`contentWindow`/`getSVGDocument` тоже не тронуты — их источник
не прототипная рефлексия, а бридж под-документов.

Регрессионный тест: `iframe_element::tests::does_not_shadow_prototype_reflected_properties`
проверяет, что `patchIframeElement` не ставит собственных дескрипторов ни для
одного из шести полей.
