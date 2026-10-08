# BUG-1347 — `background-image: url(…) repeat` (лишний токен) принимается; недопустимое объявление должно отбрасываться

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** css-parser/layout (`crates/engine/layout/src/style/apply/` — `background-image` берёт первое значение и игнорирует хвост)

## Симптом

`#a { background-image: url(x.png) repeat }` — `getComputedStyle().backgroundImage` = `url("…")`, в display list есть `DrawBackgroundImage`; по CSS Values §2.2 объявление с хвостом после `<image>` недопустимо и отбрасывается — картинки быть не должно. `background-repeat` в тест не попадает.

## Как найдено

WPT-RUN-14 срез 11: `backgrounds/background-image-005.xht` (`p { background-image: url(support/swatch-red.png) repeat; }`, текст зелёный, критерий — «нет красного»). Тот же класс, что BUG-484 (инлайновый сеттер не валидирует).

## Что делать

В разборе `background-image` требовать конец списка слоёв после `<image>`/`none`.

## Как проверить

`css/CSS2/backgrounds/background-image-005.xht`.
