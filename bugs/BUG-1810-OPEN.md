# BUG-1810 — own `opacity`/`mask-image`/`blend`/`transform` на replaced-боксах (`<canvas>`/`<video>`/`<iframe>`/`<audio>`) молча игнорируются

**Статус:** OPEN
**Заведён:** 2026-11-14 (P3, попутно BUG-1798)
**Область:** layout/paint (`crates/engine/layout/src/stacking.rs::box_can_own_stacking_context`, обходы краски/хит-теста, читающие тот же предикат)

## Суть

`box_can_own_stacking_context` (`stacking.rs:290`) допускает владельцем слоя краски только
`Block | FlowRoot | Table | Image { .. } | FormControl { .. }`. Replaced-боксы `Video`/`Canvas`/
`Iframe`/`Audio` в списке нет: их собственные `opacity < 1`, `mask-image`, `mix-blend-mode`,
`transform`/`filter` (триггеры `creates_stacking_context`) никогда не попадают в stacking-дерево,
и краска применяется без эффекта. CSS-спека применяет opacity/transform к replaced-элементам так же,
как к обычным; `<img>` уже в списке — медиа-элементы просто забыты.

## Свидетельство

`css-opacity-on-div`/`css-mask-on-div` (`draw-element-image/`): эталонные страницы — `<canvas>` с
`background: green; opacity: 0.5` / `mask-image` — рисуются непрозрачными/без маски. Замер
2026-11-14, `run_smoke.py` + `--screenshot`: эталон маски рисуется как без маски (254/255 разницы по
каналам на всей площади), эталон opacity — как без прозрачности (127 = ровно половина). Тестовая
сторона (картинка элемента через `drawElementImage`, BUG-1798) тот же эффект на содержимом применяет
корректно — расхождение именно в краске replaced-бокса на странице.

## Что чинить

Добавить `BoxKind::Canvas`/`Video`/`Iframe`/`Audio` (и проверить `SvgRoot`) в
`box_can_own_stacking_context`, затем прогнать категорию replaced/stacking и графический гейт —
предикат ключует и краску, и hit-test, изменение поднимает существующие страницы (opacity/transform
на видео-плеерах, canvas-виджетах) — вероятны золотые сдвиги в графических тестах.
