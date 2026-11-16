# BUG-1819 — `object-fit` не хранит `[ contain | cover ] || scale-down` (CSS Images L4): computed-style теряет значение

**Статус:** OPEN
**Заведён:** 2026-11-16 (P3, срез 9 [BUG-1408](BUG-1408-OPEN.md))
**Область:** layout (`crates/engine/layout/src/style/values/flexgrid.rs::ObjectFit`, `style/apply/layout.rs:138`,
`selector_query.rs::computed_style_to_map`)

## Симптом

`el.style.objectFit = "cover scale-down"` (inline-CSSOM принимает и канонизирует значение с BUG-1408 срез 9),
но `getComputedStyle(el).objectFit` отдаёт `fill` — начальное значение. Ожидается `cover scale-down`
(CSS Images L4 §5.5). WPT `css/css-images/parsing/object-fit-computed.html`: 1 красный сабтест
(`'cover scale-down'`), остальные 5 зелёные.

## Причина

Каскадный парсер (`ObjectFit::parse`) знает только одиночные ключи
(`fill`/`contain`/`cover`/`none`/`scale-down`), а `ComputedStyle.object_fit` — enum без пары значений;
сериализатор computed-style (`computed_style_to_map`) тоже одиночный. Отсюда падение в `fill`.

## Что делать

Хранить пару: `cover`/`contain` + флаг `scale-down` (иначе `scale_down: bool` рядом с покрывающим краем) —
затрагивает `ComputedStyle`, `ObjectFit::parse`, `computed_style_to_map` и потребителей `object-fit`
в раскладке/покраске заменяемого содержимого. Проверка: `object-fit-computed.html`, вход `cover scale-down`.
