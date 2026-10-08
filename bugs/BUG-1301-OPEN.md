# BUG-1301 — CSS-wide ключевые слова (`initial`/`inherit`/`unset`/`revert`) на `background-size|-repeat|-position|-origin|-clip|-attachment|-image` стирают все слои фона

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout (`crates/engine/layout/src/style/apply/css_wide.rs:1209-1217` — один арм на все `background-*` longhand'ы: `style.background_layers = Vec::new()` / копия родителя)


## Симптом

`<div style="width:170px;height:120px;background-image:url(g.png); background-size:initial">` — картинка **пропадает** (`DrawBackgroundImage` 0 раз; без объявления — 1). Так для каждого из `background-{size,repeat,position,clip,origin,attachment}` с любым из `initial|inherit|unset|revert`; `background-color:initial` и `background-size:auto` — работают. Арм `css_wide.rs:1209` написан как «все background-* не наследуются: initial = пустые слои» и очищает слои целиком, хотя должен сбросить только одно поле каждого слоя (и унаследовать только его, а не весь список).

## Как найдено

WPT-RUN-14 срез 6: `background-size-034.html` (`background-size: inherit` — картинка исчезает, виден только красный квадрат-«якорь») и `background-origin-008.html`; в корпусе больше нигде, но на реальных сайтах `background-size: inherit/initial` после `background:` — частая форма.

## Что делать

Раздельная обработка по полю: `initial` — значение по умолчанию для поля во всех слоях, `inherit` — поле из родителя при совпадении числа слоёв (§3.1 «repeat the list»), `unset` ≡ `initial` (не наследуется).

## Как проверить

`css/css-backgrounds/background-size-034.html`, `background-origin-008.html`; проба выше.

## Дополнение: WPT-RUN-14 срез 11 (2026-10-06, `css/CSS2`: backgrounds + borders)

`css/CSS2/backgrounds`: `background-repeat-005.xht`, `background-position-150.xht`, `-151.xht` — `background-repeat: inherit` / `background-position: inherit` у элемента с `background-image`: `DrawBackgroundImage` 0 раз (проба: `#d { background-image: url(…); background-repeat: inherit }` — картинки нет; тот же элемент без `inherit` — рисуется). Тот же арм `css_wide.rs:1209`.
