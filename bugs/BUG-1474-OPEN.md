# BUG-1474 — `position-area`: одиночные `top`/`bottom`/`left`/`right` считаются углом, `self-*` и `span-<сторона> <сторона>` не распознаются, невалидные сочетания принимаются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** css-parser/layout (`crates/engine/layout/src/anchor.rs` — `InsetAreaKeyword`, разбор `position-area`; `crates/engine/layout/src/style/parse/`)

## Симптом

CSS Anchor Positioning 1 §3.1: одиночное осеспецифичное слово (`top`, `bottom`, `left`, `right`, `y-start`…) означает `<слово> span-all`, ось другого измерения занимает вся область; одиночное неоднозначное (`start`, `end`, `center`) дублируется на обе оси. У нас `position-area:top` даёт `0,0,100,50` (левый верхний угол) вместо `0,0,300,50`, `bottom`/`right` — правый нижний угол вместо полосы, `left` — левый верхний угол. Ключевые слова `self-x-start`, `self-y-end`, `span-bottom span-right`, `span-self-*` не разбираются — цель получает нулевой размер. Комбинации, запрещённые грамматикой (две оси одного направления: `left inline-start`, `top top`, `left left`, `top block-start`; неизвестные слова `foobar`, `start foobar`), принимаются и сериализуются как есть. 30 id по имени файла (`position-area-*`, `position-anchor-*`, `anchor-position-0*`): 17 reftest (`thick`), 13 testharness; единственный проверенный пробой механизм — грамматика и раскладка `position-area`, причину 6 reftest `position-anchor-*` проба не устанавливала; `position-area-parsing.html` — 477 из 2 125 сабтестов (306 «serialization should be canonical», ~170 «should not set the property value»), `position-area-basic.html` 10 из 47, `position-area-wm-dir.html` 25 из 44 (режимы записи/направление: `start start` при `rtl` и вертикальных).

## Проба

Проба (`--mcp`, контейнер 300×200, якорь `left:100;top:50;40×30`, `position-area` у абсолютной цели):

| значение | у нас `x,y,w,h` | ожидается |
|---|---|---|
| `top` | `0,0,100,50` | `0,0,300,50` |
| `bottom` | `140,80,160,120` | `0,80,300,120` |
| `left` | `0,0,100,50` | `0,0,100,200` |
| `right` | `140,80,160,120` | `140,0,160,200` |
| `span-left bottom` | `140,0,160,200` | `0,80,140,120` |
| `span-bottom span-right` | `0,0,0,0` | `100,50,200,150` |
| `self-x-start self-y-start` | `0,0,0,0` | `0,0,100,50` |
| `center`, `top center`, `span-all`, `start`, `end`, `x-start y-start` | верно | — |
| `style.positionArea = "left inline-start"` | принято | отклонено (`""`) |

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/position-area-basic.html`, `position-area-parsing.html`, `position-area-wm-dir.html`.

## Что делать

Заменить `InsetAreaKeyword` (~9 значений) разбором по грамматике: 12 категорий ключевых слов, проверка совместимости осей, одиночные слова, каноническая сериализация. Тот же корень, что пункт (4) задачи GAP-ANCHORCSSOM ([BUG-563](BUG-563-OPEN.md)); этот баг — про раскладку (`anchor.rs`), BUG-563 — про `element.style`.

## Как проверить

Таблица выше; `css/css-anchor-position/position-area-basic.html`, `position-area-parsing.html`.
