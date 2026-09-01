# BUG-944 — CSS `scroll-initial-target` не реализовано: свойство не парсится и не каскадируется

**Статус:** OPEN
**Тип:** нереализованная функциональность — CSS-свойство отсутствует целиком, не в `CSS-SPECS.md` вообще (CSS Scroll Snap 2 — новый модуль).
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** css-parser, layout (`crates/engine/layout/src/style/values/scroll.rs` — соседние `scroll-snap-*` свойства заведены там же)
**Владелец:** P4.

## Симптом

`scroll-initial-target: nearest` не парсится ни в один известный тип
`ComputedStyle` — workspace-wide `grep -rn scroll-initial-target crates/`
даёт ноль совпадений. По CSS Scroll Snap Module Level 2 §4 свойство
помечает scroll-контейнер, который браузер обязан прокрутить в область
видимости при загрузке документа («scroll the initial target into view on
load»); этот шаг никогда не выполняется.

## Прямое измерение

Workspace-wide `grep -rn scroll-initial-target crates/` — ноль совпадений
(ни в `css-parser`, ни в `layout/src/style/values/scroll.rs`, где живут
`scroll-snap-type`/`scroll-snap-align`/`scroll-snap-stop`).

## Кого это держит

`css/css-scroll-snap/scroll-initial-target/scroll-initial-target-shadow-dom.
tentative.html` — тест ставит `scroll-initial-target` на элемент внутри
shadow DOM и ждёт, что после загрузки его scroll-контейнер уже прокручен;
без свойства первая же проверка `scrollTop` не совпадает.

## Направление починки

Через `/lumen-add-css-property`: поле в `ComputedStyle`
(`scroll_initial_target: bool`, единственное разрешённое значение —
булев дескриптор `nearest`/отсутствие), парсинг рядом с
`scroll-snap-stop`, и шаг после первичного layout документа — найти
элемент(ы) с `scroll-initial-target: nearest` (спека — не более одного на
scroll-контейнер, ближайший выигрывает) и прокрутить их в видимость до
первого paint. Второй модуль (CSS Scroll Snap 2) стоит добавить в
`CSS-SPECS.md` отдельной строкой P4 — сейчас его там нет вовсе.
