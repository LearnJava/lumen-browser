# BUG-1147 — кросс-фреймовый Element-фасад не определяет `.style`/`classList`/`dataset`

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-24 (w3schools, разбор совместимости top100; передан P6 по решению пользователя).
**Область:** js (`crates/js/src/frame_bridge.rs::frameElem`)

## Симптом

Тот же фасад `frameElem`, что чинился в [BUG-970](BUG-970-FIXED.md) (`.attributes`), не определяет
`.style` (и `classList`, `dataset`): `iframe.contentDocument.documentElement.style` и `body.style` —
`undefined`. w3schools (FastCMP, `fast-cmp-en-tcfeuv2.js:1:174891`):
`Cannot set properties of undefined (setting 'cssText')` — диалог согласия на cookie не строится.
Репро `.tmp/compat/g6/site/iframedoc.html` (iframe без `src`): Lumen `typeof style === 'undefined'` →
`TypeError`; Chrome отдаёт `CSSStyleDeclaration`, `cssText` применяется, конструктор
`HTMLHtmlElement`.

## Масштаб

Тот же класс пробела, что закрыт для `.attributes` в BUG-970 — фасад покрывает широкую
поверхность (`nodeType`/`tagName`/`getAttribute`/`children`/`querySelector`/…), но не инлайновые
стили/классы/data-атрибуты чужого поддокумента.

## Исправление (2026-09-28, P6)

Своих реализаций в фасаде не заведено: `frameElem` отдаёт те же объекты главного шима —
`CSSStyleDeclaration` (`_lumen_make_style`), `DOMTokenList` (`_lumen_make_class_list`),
`DOMStringMap` (`_lumen_make_dataset`), — привязанные к под-документу новым необязательным
аргументом `fbid`. Экземпляр с `__fbid__` читает и пишет атрибуты `style`/`class`/`data-*`
через нативы бриджа `_lumen_f_attr`/`_lumen_f_set_attr`/`_lumen_f_remove_attr`/
`_lumen_f_attr_names`; без него — через локальные, как раньше. Развилка — одна четвёрка
`_lumen_backing_*` в `web_api_shim_mid.js`, поэтому разбор/сериализация `cssText`, раскрытие
сокращённых свойств и валидация значений у фасада те же, что у элемента своего документа.
Представления строятся при первом чтении и живут столько же, сколько фасад
(`el.style === el.style`); `set style(v)` — `[PutForwards=cssText]`, как в BUG-494.

`__fbid__` проверяется через `in`, а не чтением: Proxy стиля отвечает на чтение неизвестного
свойства `getPropertyValue`, то есть лишним разбором атрибута.

Тесты: `crates/js/tests/cases/bug1147_frame_facade_style.rs` — полный V8-шим + зарегистрированный
под-документ `<iframe>`: сценарий FastCMP (`body.style.cssText = …`), чтение разметочного
`style=""`, запись в атрибут ребёнка и обратно, `classList`/`dataset` живые над атрибутами.

Вне этого бага: вторая половина разбора w3schools — синхронно после `appendChild` динамического
`<iframe>` без `src` у Lumen `contentDocument === null`, а Chrome уже отдаёт начальный документ
`about:blank` — остаётся в [BUG-480](BUG-480-OPEN.md) §«Реальные сайты». Конструктор фасада
(`HTMLHtmlElement` в Chrome) — тоже не здесь: фасад остаётся простым объектом.
