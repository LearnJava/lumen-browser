# BUG-1591 — css-ruby: блок и `inline-block` внутри `<rt>` схлопываются в 0×0 и сдвигают `<ruby>`, блоки внутри `<ruby>` не инлайнизируются, `<rbc>` — `HTMLUnknownElement`, `CSS.supports` для `ruby-*` всегда `false`

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** layout/js (`crates/engine/layout/src/box_tree/build.rs` `build_ruby_box`/`build_ruby_group_box`; `ruby-*` в `CSS.supports`; интерфейс `<rbc>`)

## Симптом

Кластер из 76 id. Пробой подтверждены две причины (блочное содержимое `<rt>` и инлайнизация); остальные 58 id (`ruby-autohide-*`, `ruby-tab-in-base-*`, `ruby-whitespace-*`, `ruby-line-break*`, `ruby-bidi-*`, `ruby-justification`, `*-float`, `pseudo-first-line/letter`, `ruby-intrinsic-isize-*`) названы по именам файлов и не изолированы. Выравнивание `ruby-align` на текстовых аннотациях работает (`start`/`center`/`space-between`/`space-around` дают разные `x` у базы и аннотации), поэтому `ruby-align-001*` (аннотация — блок шириной 160 px) падает не из-за самого свойства, а из-за первой причины ниже.

## Проба

`--dump-layout` + `console.log(getBoundingClientRect())`, шрифт Ahem через `LUMEN_CPU_SYSTEM_FONTS`:

| вызов | у нас | ожидается |
|---|---|---|
| `<ruby><rb>X</rb><rt><div style="width:50px;height:5px"></div></rt></ruby>`: `div`, `rt`, `ruby` | `0,0 0×0`; `0,321 800×0`; `393,321 14×20` | `div` 50×5; `rt` по ширине аннотации; `ruby` у левого края |
| то же с `<span style="display:inline-block;width:50px;height:5px">` вместо `div` | `0×0`; `800×0`; `393,341 14×20` | `span` 50×5 |
| `<ruby style="ruby-align:center"><rb>X X X</rb><rt>YYYYYYYY</rt></ruby>` (контроль, текстовая аннотация) | база сдвинута (`x=28`), аннотация `x=0` | то же |
| `getComputedStyle(div.i).display` у `<div style="display:block">` внутри `<ruby>` | `block` | инлайнизированное значение |
| `document.createElement("rbc").constructor.name`; `rb`, `rt`, `rtc` | `HTMLUnknownElement`; `HTMLElement` ×3 | `HTMLElement` ×4 |
| `CSS.supports("ruby-align","start")`, `("ruby-position","over")`, `("ruby-merge","auto")` | `false` ×3 (при том, что `style.rubyPosition = "over"` принимается как есть) | `true` ×3 |
| `ruby-merge: collapse` через `style` | `""` | `collapse` |
| `getComputedStyle(rt).fontSize` у `<rt>` в `<ruby>` с 16 px | `16px` | `8px` (BUG-1520) |
| `display: ruby-text`, `display: ruby-base` у `<span>`: `getComputedStyle().display` | `inline` / `inline` | `ruby-text` / `ruby-base` (BUG-1476) |

## Как найдено

WPT-RUN-14 срез 27: `css/css-ruby/ruby-align-001*`, `ruby-inlinize-blocks-*`, `ruby-box-generation-*`, `ruby-box-model-001`, `rb-display-001`, `rt-display-001`, `block-ruby-*`.

## Что делать

В `build_ruby_group_box`/`lay_out_ruby` разобрать блочное и `inline-block` содержимое `<rt>` (CSS Ruby 1 §«Anonymous ruby boxes»: блоки инлайнизируются); завести `rbc` в интерфейсы HTML; добавить `ruby-*` в `CSS.supports`. После правки перепрогнать кластер: часть из 58 неизолированных id закроется тем же.

## Как проверить

`css/css-ruby/ruby-align-001.html`, `ruby-inlinize-blocks-001.html`, `ruby-box-generation-001.html`, `rb-display-001.html`.
