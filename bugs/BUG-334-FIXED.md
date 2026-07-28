# BUG-334: SVG `<use>` без `width`/`height` не наследует 100%-размер текущего viewport'а (иконки хрома искажены)

**Статус:** FIXED 2026-07-24
**Компонент:** layout (SVG `<use>`/`<symbol>`, `crates/engine/layout/src/box_tree.rs:1157-1185`)
**Найден:** P1, CC-1 (`docs/tasks/p1-css-chrome.md`), скрин-сверка `about:chrome-preview` с эталоном 2026-07-24
**Исправлено:** P1, CC-2 2026-07-24 — `svg_root_own_size()` резолвит CSS-размер внешнего `<svg>`
и передаётся как `own_svg_size` вниз по рекурсии; fallback-цепочка `vp_w`/`vp_h` теперь
использует его вместо `vb.width`/`vb.height`. Подтверждено `--dump-layout about:chrome-preview`:
все `SvgRoot` icon-инстансы дают `w=14.00 h=14.00` (было — path красился в 24-юнитных
координатах поверх 14px контейнера). Новый регресс-тест
`box_tree::tests::svg_use_symbol_no_explicit_size_scales_to_css_icon_size`.

## Симптом

Иконки тулбара (назад/вперёд/обновить и другие ~35 иконок спрайта `about:chrome-preview`)
рисуются искажёнными — вместо чистых 14×14 глифов видны деформированные фигуры (кроп
скриншота toolbar: стрелка "назад" превращается в нечто вроде `<`/`Y`, «обновить» — в
подобие `£`).

Разметка (типовой паттерн icon-спрайта, ~35 использований в `assets/chrome/chrome.html`):

```html
<svg style="display:none">
  <symbol id="i-back" viewBox="0 0 24 24"><polyline points="15 18 9 12 15 6"/></symbol>
  ...
</svg>
...
<button class="tb-btn"><svg class="icon"><use href="#i-back"/></svg></button>
```

```css
.icon{ width:14px; height:14px; stroke:currentColor; fill:none; stroke-width:1.8; ... }
```

Ни `<use>`, ни `<symbol>` не задают `width`/`height` атрибутами — размер иконки задаётся
CSS на **внешнем** `<svg class="icon">` (14×14, подтверждено дампом: `SvgRoot ... w=14.00
h=14.00`). Тем не менее путь рисуется 1:1 в координатах `viewBox` символа (24×24), без
масштабирования к 14×14 — итоговый `SvgShape` получает `rect` нулевого размера
(`w=0.00 h=0.00`), а сам path красится с исходными 24-юнитными координатами поверх 14px
контейнера.

## Причина (локализовано)

`box_tree.rs:1170-1180` (комментарий "BUG-246"):

```rust
// Viewport size: `<use>` width/height win; else the symbol's own
// width/height; else fall back to the viewBox dims (→ identity).
let vp_w = attr_dim(child_id, "width")
    .or_else(|| attr_dim(target_id, "width"))
    .unwrap_or(vb.width);
let vp_h = attr_dim(child_id, "height")
    .or_else(|| attr_dim(target_id, "height"))
    .unwrap_or(vb.height);
```

`attr_dim` читает только HTML/SVG-**атрибуты** `width`/`height` — не CSS-вычисленный размер.
Когда ни `<use>`, ни `<symbol>` не задают атрибуты, `vp_w`/`vp_h` откатываются к `vb.width`/
`vb.height` (собственный `viewBox` символа) → identity-трансформ (без масштабирования).

Это ветка "else" из фикса BUG-246 (2026-06-30), которая на момент фикса считалась identity
по спеке — но по SVG 2 §5.7/§7.10, когда `<use>` и целевой `<symbol>` не задают ширину/высоту,
**используемое значение — `100%`**, которое резолвится относительно **текущего SVG viewport**
(здесь — внешний `<svg class="icon">`, уже корректно засайженный CSS в 14×14), а не «единица
= один user-unit viewBox'а». Функция не имеет доступа к CSS-размеру внешнего `<svg>` в момент
вычисления (`vp_w`/`vp_h` смотрят только на атрибуты `child_id`/`target_id`, не на layout-размер
предка) — отсюда откат на «идентичность» вместо «100% предка».

Это ровно паттерн icon-спрайта, который CC-2 (`docs/tasks/p1-css-chrome.md`) закладывал как
основной путь (~35 иконок эталона) — без фикса CC-2 придётся либо форсить явные
`width`/`height` на каждом `<use>` в `gen_chrome_assets.py` (build-time патч атрибутов —
дешёвый обходной путь), либо чинить движок здесь.

## Воспроизведение

`cargo run -p lumen-shell -- --dump-layout about:chrome-preview` → искать `SvgShape ... rect=(...,
0.00, 0.00)` рядом с `SvgRoot ... w=14.00 h=14.00` (любая кнопка тулбара, например строка с
`path d="M 15 18 L 9 12 L 15 6"`).

## Что закрыло бы

`vp_w`/`vp_h` в `box_tree.rs:1170-1180` должны получать доступ к текущему (CSS-резолвленному)
размеру внешнего `<svg>`-viewport'а как финальный fallback (вместо `vb.width`/`vb.height`),
когда ни `<use>`, ни `<symbol>` не задают явные `width`/`height`. После фикса — визуальная
проверка `about:chrome-preview` (иконки тулбара/сайдбара должны быть чистыми глифами, не
искажёнными фигурами).
