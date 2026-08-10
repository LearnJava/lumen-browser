# BUG-746: `document.styleSheets` отсутствует — CSSOM-объектов таблиц стилей нет вовсе

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`) + shell/layout (публикация разобранных таблиц в JS-рантайм)
**Найден:** P3 при закрытии [BUG-732](BUG-732-FIXED.md), 2026-08-10

## Симптом

`typeof document.styleSheets === 'undefined'` на любой странице (проба
`.tmp/b732_probe.py`, локальная страница со `<style>` в `<head>`). Скрипт,
который перебирает таблицы стилей — сбор критического CSS, темизация,
инструменты вроде styled-components, любая проверка «есть ли уже мой
`<style>`» — падает на первом же обращении, а не получает пустой список.

## Почему выделено из BUG-732

BUG-732 объединяла шесть точек по признаку «закрывается локальным добавлением
в шим на готовых движковых данных». Пять из шести (`Node.contains`,
`Node.compareDocumentPosition`, `Element.attributes`, `document.images`,
`getComputedStyle().getPropertyValue('--x')`) этому признаку отвечали и закрыты
2026-08-10. Шестая — нет, и сама заявка предписывала выделить её номером,
если так окажется.

Готовых данных в JS-рантайме нет ни одного байта: `grep -rn "styleSheets\|
CSSStyleSheet\|cssRules" --include=*.rs crates/` даёт единственное совпадение —
комментарий в шелле. Разобранный `Stylesheet` живёт в шелле (`lumen-css-parser`),
JS-рантайм о нём не знает, натива для чтения нет.

## Что нужно сделать

1. **Публикация.** Прокинуть разобранные таблицы стилей из шелла в JS-рантайм —
   тем же способом, каким публикуются computed styles: сериализация
   layout/css-parser-side, `update_*` на `V8JsRuntime`, натив на чтение. Точки
   вызова — те же четыре в `crates/shell/src/main.rs` плюс
   `crates/driver/src/session.rs` (см. `update_custom_properties`, BUG-732,
   как готовый образец плюмбинга).
2. **CSSOM-объекты в шиме.** `StyleSheetList`, `CSSStyleSheet`
   (`ownerNode`/`href`/`media`/`title`/`disabled`/`type`/`cssRules`),
   `CSSRuleList`, `CSSStyleRule` (`selectorText`/`style`/`cssText`).
   `insertRule`/`deleteRule` — отдельный вопрос: они требуют обратной связи
   в каскад, без неё их лучше не заводить вовсе, чем завести молча
   не работающими.
3. Учесть, что `cssRules` у кросс-оригинного листа по спецификации бросает
   `SecurityError` — в шелле уже есть комментарий об этом
   (`crates/shell/src/main.rs`, CORS-гейт), корректное поведение тут важнее
   пустого списка.

## Ловушка при проверке

Страница без единого `<script>` не поднимает JS-контекст (`eval` отвечает
`JS context not available`), и пустого `<script></script>` тоже недостаточно —
нужен непустой (`<script>window.x=1;</script>`). Живой `file://`-адрес нельзя
передавать первым CLI-аргументом ([BUG-651](BUG-651-OPEN.md)) — стартовать
с `about:blank` и звать `navigate`.
