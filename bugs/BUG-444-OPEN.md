# BUG-444 — checkedness не имеет хранилища, отдельного от content-атрибута `checked`: `el.checked = …` затирает значение по умолчанию, `defaultChecked`/`form.reset()` восстанавливают его только по снимку

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — own-свойство `checked` в
`_lumen_build_element`), shell (`crates/shell/src/forms.rs` —
`collect_form_entries`/`check_validity_form` и покраска чекбокса читают
атрибут `checked` из документа)
**Найден:** P1 при починке [BUG-383](BUG-383-FIXED.md), 2026-07-29

## Симптом

```js
var c = document.querySelector('input[type=checkbox][checked]');
c.checked = false;          // снимает галочку
c.defaultChecked            // → должно остаться true
form.reset();               // → должно вернуть галочку
```

До обхода, добавленного в BUG-383, обе строки давали `false`: сеттер `checked`
писал прямо в content-атрибут `checked`, а `defaultChecked` и `reset()` читали
тот же самый атрибут — то есть значение по умолчанию физически исчезало при
первой же записи текущего состояния.

## Причина

HTML LS §4.10.5.5 различает две величины: **checkedness** (текущее состояние,
меняется пользователем и скриптом) и content-атрибут `checked` (значение по
умолчанию, `defaultChecked`). Их связывает «dirty checkedness flag»: после
первого изменения текущего состояния атрибут перестаёт на него влиять.

В Lumen хранилище одно — сам атрибут. Так сделано не по недосмотру: по атрибуту
шелл красит чекбокс, `collect_form_entries` собирает форму, а
`check_validity_form` считает валидность, и все три читают документ напрямую из
Rust. Отдельное JS-хранилище (как `_input_values` для `value`) эти три пути не
увидят.

Это тот же дефект модели, что и [BUG-441](BUG-441-FIXED.md) — там он про
`value`, здесь про `checked`; чинить их логично одной правкой.

## Обход, который уже стоит

BUG-383 добавил `_lumen_default_checked` — снимок значения по умолчанию,
снимаемый **при первой записи** `el.checked` из скрипта. На нём работают
`defaultChecked` и `form.reset()`. Обход не покрывает два случая:

* пользователь щёлкнул чекбокс мышью — шелл меняет атрибут из Rust, снимок не
  снимается, значение по умолчанию теряется;
* документ переразбирается/узел пересоздаётся — снимок живёт по nid и чистится
  `_lumen_gc_collect`.

## Как чинить

Механизм уже заведён: [BUG-441](BUG-441-FIXED.md) (исправлен 2026-08-04) добавил
в `Document` хранилище `dirty_values` для `value` — `HashMap<NodeId, String>`,
где наличие записи и есть dirty value flag, с доступом через
`control_value`/`set_control_value`/`clear_control_value`. Его читают и вёрстка,
и `collect_dom_form_fields`, и `element_validity`, и JS-шим через нативы
`_lumen_{get,set,clear}_dirty_value`.

Для checkedness нужен такой же сосед — `dirty_checkedness: HashMap<NodeId, bool>`
рядом с `dirty_values` — плюс перевод всех читателей `checked`-атрибута
(покраска чекбокса в шелле, `collect_fields_in`, `:checked` в `style.rs`,
геттер/сеттер `checked` в шиме) на него. Content-атрибут остаётся значением
по умолчанию, как того требует спека, и `_lumen_default_checked`-обход из
BUG-383 после этого снимается.

## Связанные

* [BUG-441](BUG-441-FIXED.md) — то же самое для `value`.
* [BUG-383](BUG-383-FIXED.md) — правка, которая вскрыла дефект и поставила обход.
