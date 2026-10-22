# BUG-1684 — инкрементальная перераскладка теряет каскадный стиль бокс-less элементов (BUG-1191)

**Статус:** OPEN
**Заведён:** 2026-10-22 (Chat, найдено при починке тестов `v8_gap_uashadowslot`, ветка `chat-fix-details-summary`)
**Область:** layout/js (публикация записей стиля элементов без бокса — BUG-1191 — на инкрементальном пути раскладки, `counters: None`)

## Симптом

BUG-1191 отдаёт `getComputedStyle()` элементу без бокса (как Chrome), в том числе содержимому
закрытого `<details>` (`content-visibility: hidden` у слота). При первичной раскладке это работает:
у `<p>` внутри закрытого `<details>` 361 свойство. После `d.setAttribute('open','')` и
`d.removeAttribute('open')` то же содержимое отдаёт `getComputedStyle(p).length === 0` — записи
стиля бокс-less элементов не публикуются инкрементальной перераскладкой.

Проба (JS в `crates/js/src/dom/tests/v8_gap_uashadowslot.rs`, `runtime(&[("details", &[])])`):

```
closed 361/…/1/false … reclosed 0///false
```

## Побочный эффект

Тест `dom::tests::v8_gap_uashadowslot::slotted_child_inherits_from_ua_slot` ожидает `e3:-` и проходит
только благодаря этой дыре; Chrome дал бы `block/hidden`. После починки ожидание теста нужно
исправить на поведение Chrome.

## Готово, когда

- После цикла open → close у содержимого `<details>` `getComputedStyle(el).length > 0` и значения
  совпадают с первичной раскладкой (тест на цикл open/close в `v8_gap_uashadowslot.rs`).
- `slotted_child_inherits_from_ua_slot` ожидает поведение Chrome и проходит.
- `cargo test -p lumen-js --features v8-backend --lib -- uashadowslot details` и тесты затронутого
  крейта layout зелёные.

## Частичный фикс (2026-10-22, P3, BUG-1480)

Публикация записей стиля элементов, потерявших бокс при инкрементальном флаше, возвращена
(`publish_boxless_computed_style` в `style_flush.rs`): цикл open → close у `<details>` отдаёт
`getComputedStyle(p).length > 0` и `display: block` (тест в `closed_details_renders_summary_only_and_open_reveals_content`).
Не сделано: ожидание `e3` в `slotted_child_inherits_from_ua_slot` по-прежнему `-` (проверить, почему оно не
изменилось, и привести к поведению Chrome) — только оно остаётся от «Готово, когда». Запись бокс-less элемента,
у которого меняется стиль, пока он остаётся бокс-less, обновляется только полным сбором.
