# BUG-1297 — CSSOM `background-*` / `border-*` / `box-shadow` / `border-radius`: принимает невалидное, не канонизирует, `getComputedStyle()` отдаёт `""` или не то

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** js + layout (`element.style` для background/border — `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand`; `crates/engine/layout/src/selector_query.rs::computed_style_to_map`)


## Симптом

Проба (`d.style[p] = v; d.style[p]`), всё должно быть `""` (значение отброшено):

| присваивание | получено |
|---|---|
| `backgroundSize = '-1px'` | `-1px` |
| `backgroundRepeat = 'repeat repeat-x'` | `repeat repeat-x` |
| `backgroundAttachment = 'auto'` | `auto` |
| `backgroundImage = 'red'` | `red` |
| `backgroundPosition = 'left right'` | `left right` |
| `backgroundClip = 'fill-box'` | `fill-box` |
| `boxShadow = 'auto'`, `borderRadius = 'auto'` | `auto` |
| `borderImageSlice = 'fill'`, `borderImageRepeat = 'space'` | принято (свойства парсером не разобраны) |

`getComputedStyle(div)` на элементе с `background-color:green`: `backgroundPosition` — `""` (должно `0% 0%`), `borderImageSource` — `""` (`none`), `borderRadius` — `""` (`0px`); у `background-image:none,none; background-repeat:space round` `backgroundRepeat` = `repeat, repeat` (теряет `space`/`round` и число слоёв). Сериализация: `no-repeat url(/favicon.ico)` вместо `url("/favicon.ico") no-repeat`, `fill 1 2% 3 4%` вместо `1 2% 3 4% fill`, `center left` вместо `left center`, `1px` вместо `1px auto`.

Механизм тот же, что [BUG-484](BUG-484-FIXED.md) (валидаторы по семействам свойств) и [BUG-472](BUG-472-OPEN.md)/[BUG-1278](BUG-1278-OPEN.md) (рукописная карта computed): для `background-*`, `border-*`, `box-shadow`, `border-radius` валидаторов и строк карты нет.

## Как найдено

WPT-RUN-14 срез 6: `css/css-backgrounds/parsing/*` и `inheritance.sub.html` — 45 файлов, 463 упавших сабтеста: `*-computed` 16, `*-invalid` 13, `*-valid` 12, прочее 4. Не входят `border-image-*` (они в BUG-492: свойства не разобраны вовсе).

## Что делать

Валидаторы и канонизаторы по грамматике свойства (как в CSSOM-2 для остальных семейств) и строки `background-position/-size/-repeat/-origin/-clip/-attachment`, `border-radius`, `border-*-color/-style`, `box-shadow` в `computed_style_to_map` с полным числом слоёв.

## Как проверить

`css/css-backgrounds/parsing/{background,border,box-shadow,border-radius}-*.html`, `inheritance.sub.html`.

## Повторное измерение: WPT-RUN-14 срез 23 (2026-10-08)

Тот же класс «`element.style` не валидирует и не канонизирует» — для `shape-outside`/`shape-margin`/`shape-image-threshold`, `clip-path: <basic-shape>` и свойств Text Decoration: BUG-1516 (74 id, 972 из 1 157 сабтестов).

## Срез 1 (P3, 2026-10-19)

Закрыто: валидаторы и канонизаторы по грамматике — новый модуль `crates/engine/layout/src/style/values/background_cssom.rs` (`canonical_specified_background`), в JS подключён через `_LUMEN_BACKGROUND_PROPERTIES` → `_lumen_css_canonical_background`: `background-{image,size,repeat,attachment,origin,clip,position,position-x,position-y}`, `box-shadow`, `border-radius` и четыре `border-*-radius`. В computed:

- `ComputedStyle::background_specified` хранит списки per-layer-свойств как заданы автором — их длина больше не равна числу слоёв (`background-repeat: repeat-x` при двух изображениях остаётся `repeat-x`);
- каскад отбрасывает весь список `background-{repeat,attachment,origin,clip}` при одном недопустимом элементе (раньше недопустимый элемент молча выпадал);
- в карте computed: `background-position`, `border-radius` (с эллиптическими углами), `border-*-radius` с двумя осями, `box-shadow` и `border-*-color` с `currentcolor` → используемый цвет, шортхенды `border-color`/`border-style` в кратчайшей форме;
- `expand_border_4`/`split_radius_pair` делят значение по пробелам верхнего уровня (`border-color: rgb(1, 2, 3) rgb(4, 5, 6)` и `calc(… + …)` раньше ломались и объявление выпадало); `border-style: hidden` разбирается; ширина рамки из `calc()` не бывает отрицательной;
- JS: `style.<шортхенд TRBL> = ''` снимает четыре лонгхенда, `hidden` допустим в `border-*-style`; `calc` в канонической записи — по порядку единиц, вычитание записывается ` - `.

WPT `css/css-backgrounds/parsing`: 370 → 629 из 916 сабтестов, регрессий нет. Регрессионные тесты: `v8_bug1297_background_cssom.rs`, `background_cssom::tests`, `selector_query::tests::computed_map_*`.

### Остаток

- шортхенды `background` / `border` / `border-image` (`background-valid` 40, `background-shorthand-serialization` 10, `border-shorthand` 31, `background-invalid`, `border-invalid`): разбор и сериализация шортхенда из лонгхендов;
- `background-image` computed: `radial-gradient()`/`conic-gradient()` не разбираются (45 сабтестов `background-image-computed`), отрицательные радиусы радиального градиента и `cross-fade()` процентами в `background-image-invalid`;
- `background-repeat: space/round` двумя значениями и `background-clip: border-area` — в движке нет значений;
- `background-position-x/-y: x-start|x-end` и `right -10px` в computed; `calc()` в `background-size` (`calc(10px + 0.5em)` → `auto`);
- `border-width` computed — четыре значения без свёртки (`2px 0px 0px 0px`), WPT ждёт именно её;
- `-webkit-border-radius` и `border-radius` как пара шортхендов (`Serialize … style is not iterable`).
