# BUG-951 — `<label>.focus()` не форвардит фокус на связанный контрол и не фокусирует саму метку

**Статус:** OPEN
**Тип:** дефект реализованного кода — `HTMLElement.prototype.focus` уже существует и работает для всех прочих тегов; у `<label>` не хватает одной специальной ветки (HTML LS §6.6.3 «the focusing steps», шаг про forwarding), а не целой подсистемы.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 32, живая проба `verify_slice32_gaps.py --variant label-focus-forward`)
**Область:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — `_lumen_is_focusable` не знает тега `LABEL`; `HTMLElement.prototype.focus` не резолвит «связанный контрол» метки)
**Владелец:** P3.

## Симптом

`label.focus()` для `<label>` без собственного `tabindex` — полный no-op:
не фокусируется ни сама метка, ни связанный с ней контрол (`for="id"` или
первый labelable-потомок). `document.activeElement` после вызова остаётся
пустым, событие `focus` не долетает никуда.

Причина видна по коду: `_lumen_is_focusable(nid)` (`web_api_shim_tail_b.js:723`)
перечисляет спецслучаи по тегу (`INPUT`, `A`/`AREA`, `AUDIO`/`VIDEO`,
`BODY`/`HTML`) и иначе смотрит в `_LUMEN_FOCUSABLE_TAGS` — `LABEL` не входит
ни туда, ни туда. `HTMLElement.prototype.focus` (строка 822) при
`!_lumen_is_focusable(nid)` просто возвращает — форвардинга на связанный
контрол нет вовсе ни в этой функции, ни где-либо ещё (грепом по `label` в
`crates/js/src/shim/*.js` — только `_LUMEN_LABELABLE_TAGS`, обслуживающая
клик-активацию, не фокус).

## Прямое измерение

Живая проба (`--variant label-focus-forward`, dev-release):
`<label id=label-a for=input-a>` и `<label id=label-b><input id=input-b>…</label>`,
оба без `tabindex`. `label-a.focus()` → `activeElement-after-label-a-focus =`
(пусто), ни `input-a-focused`, ни `label-a-focused-BAD` не напечатаны.
`label-b.focus()` → тот же результат. Событие `focus` не долетает ни до
метки, ни до контрола — не «форвардинг сломан», а фокус не устанавливается
вообще никуда.

## Кого это держит

`html/semantics/forms/the-label-element/forward-focus-to-associated-element.html`
— 4 из 6 сабтестов зависят от форвардинга (`label-a`/`label-b`/`label-e`
пустой `tabindex`/`label-f` скрытая с `tabindex`); 2 сабтеста (`label-c`
явный `tabindex`, `label-d` отрицательный) ожидают фокус на самой метке и,
скорее всего, уже проходят — `tabindex` делает `_lumen_is_focusable` истинным
через общую ветку (строка 737), не проверялось этой пробой отдельно.

## Направление починки

В `_lumen_is_focusable`/`HTMLElement.prototype.focus` добавить: если тег —
`LABEL` и у метки нет собственного `tabindex`, резолвить «связанный контрол»
(`for`-атрибут → `getElementById`, иначе первый потомок из
`_LUMEN_LABELABLE_TAGS`) и вызвать `.focus()` на нём вместо метки. Метка с
явным `tabindex` (включая пустую строку и отрицательный) фокусируется как
обычно — общая ветка уже это покрывает, специальный случай нужен только
когда `tabindex` отсутствует.
