# BUG-1277 — авторские border/background/padding не отключают нативный вид контрола (appearance-disabling properties, CSS UI L4)

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3 — `css/css-ui`, крупнейший кластер среза)
**Область:** layout (`crates/engine/layout/src/style/ua.rs::strip_ua_appearance_box_styling` — срабатывает только при `appearance: none`), paint (`display_list/form_controls.rs`)

## Симптом

CSS Basic UI L4 §appearance-disabling-properties и HTML LS §15.5 «widgets»: если автор задаёт у виджета с `appearance: auto`
хотя бы одно из свойств `background-*`, `border-*` (включая логические и `border-image-*`, `border-*-radius`),
`padding-*`, виджет теряет нативный вид и рисуется как при `appearance: none`. В Lumen нативный вид остаётся.

Проба (`--screenshot`, 300×80): `compute-kind-widget-generated/kind-of-widget-fallback-button-border-top-color-001.html`
(скрипт переписывает в `el.style` то же значение `border-top-color`, что вернул `getComputedStyle`) — кнопка с серым
фоном и рамкой; эталон `compute-kind-widget-fallback-button-ref.html` (`button { appearance: none }`) — голый текст.
Тот же результат даёт статическое `style="border-top-color: red"`: `--dump-layout` показывает
`bg=#ffffffff bw=(1,1,1,1) bc=(#ff0000ff,#767676ff,…)` у `<textarea>`, а не голый бокс.

## Причина

`strip_ua_appearance_box_styling` (снимает UA-рамку, отступы и фон до авторского каскада) вызывается только когда
выигравший `appearance` — `none`. Ветки «`appearance: auto` + авторское appearance-disabling свойство → used `none`»
нет нигде. Это нереализованная часть `appearance`, а не регресс.

## Что делать

В `compute_style` до авторского каскада определить, задаёт ли автор (не UA) у виджета хоть одно свойство из списка
§appearance-disabling-properties, и тогда трактовать used `appearance` как `none` (снятие UA-рамки/фона/отступов и
отказ от нативной отрисовки в paint). Значения, совпадающие с UA, тоже считаются: тест пишет `getComputedStyle` обратно.
Исключения по спецификации: `<input type=checkbox|radio>`, `<select>` (dropdown) и ссылки в WPT проходят уже сейчас.

## Как проверить

WPT `css/css-ui/compute-kind-widget-generated/*` — 538 reftest упали (button, `input` button/submit/reset/text/search,
`textarea`, `select multiple`, `input type=color`, `meter`, `progress` и 10 `grouped-*`), 264 проходят (checkbox,
radio, select dropdown/menulist-button, link, range); плюс `compute-kind-widget-no-fallback-props-001.html`.
`reftest_pixdiff.py --viewport 800x600`: все 539 — `thick` (от 328 до 24 332 пикселей на пару).
