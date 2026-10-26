# BUG-1509 — Дерево псевдоэлементов View Transition не видно из DOM: нет UA-стиля `::view-transition*`, нет CSS-анимаций на них, `getAnimations()` пуст

**Статус:** OPEN (ДОРАБОТКА → VT-API-2)
**Тип:** нереализованная функциональность (UA-таблица стилей и анимации дерева псевдоэлементов), не дефект — задача `VT-API-2` в ROADMAP.md.
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout/js (`crates/engine/layout/src/style/pseudo.rs` — `compute_view_transition_pseudo_style`; `crates/js/src/view_transitions.rs`; снимок и кросс-фейд собирает шелл)

## Срез 1

**Срез 1 влит 2026-10-23 (P3):** UA-таблица стилей `::view-transition*` (`compute_view_transition_tree_style` в `layout/style/pseudo.rs`: цепочка корень → group → image-pair → old/new, авторские правила поверх UA при любой специфичности, `animation-*: inherit` читает родителя) и ответ `getComputedStyle(html, '::view-transition…')` из `view_transitions_shim.js` (`_lumen_vt_pseudo_style`, натив `_lumen_vt_tree_style`): дерево существует от вызова колбэка до конца перехода, `old`/`new` — только для имён своего состояния, group после активации копирует `mix-blend-mode`/`text-orientation`/`color-scheme`/…; вне перехода, у не-корня, `::view-transition-group` без аргумента и с `*` — пусто. `getAnimations({subtree:true})` и `document.getAnimations()` листят CSS-анимации псевдодерева (авторские и UA `-ua-view-transition-fade-in/out`, `-group-anim-<name>` между рамками старого и нового состояния). **Остаток (VT-API-2):** значения анимаций в `getComputedStyle` псевдоэлемента (наложение работает только по nid), `width`/`height`/`transform` группы в вычисленном стиле, `Element.animate(..., {pseudoElement: '::view-transition-…'})`, `finished` по концу кросс-фейда, `pagereveal`/`pageswap`. 

## Симптом

`document.startViewTransition(cb)` выполняется (`ready`, `finished` разрешаются), шелл рисует кросс-фейд, но страница не видит ничего: во время перехода `getComputedStyle(document.documentElement, '::view-transition').position` — `static` (ожидается `absolute`; UA-таблица стилей `::view-transition`, `-group`, `-image-pair`, `-old`, `-new` с `position:fixed/absolute`, `animation-name: -ua-view-transition-group-anim-<name>` и т.д. отсутствует), `document.getAnimations()` и `documentElement.getAnimations({subtree:true})` — 0 (под `--mcp` кадры не тикают — `requestAnimationFrame` не срабатывает, — поэтому этот счёт сам по себе не доказывает отсутствия анимаций; доказательство — `position:static` выше; без CSS-анимаций на псевдоэлементах тесты `pseudo-element-animations`, `group-animation-for-root-transition`, `dynamic-stylesheet-animations*`, `nested/group-children-animations` не видят ни `-ua-` анимаций, ни их таймингов). Рисование: `--screenshot` страницы `3d-transform-incoming.html` (цель убирается в update-callback, эталон — синий квадрат на розовом фоне) даёт белый фон, зелёный квадрат на месте цели и красный квадрат 10×10 у левого края (`--screenshot` не прокручивает кадры, так что это не доказательство; в IPC-пути `wptrunner` не проверялось), а 160 из 177 reftest `css-view-transitions` ещё и `reftest-wait` (WPT-RUN-15) — первым их блокирует исполнитель. `only-child-*` (6 id) читают цвет псевдоэлемента и получают `rgb(0, 0, 0)` вместо цвета картинки. `mix-blend-mode-only-on-transition` — `isolate` против `auto`.

## Проба

Проба (`--mcp`, страница с `view-transition-name:a`, после `await vt.ready`):

| проверка | у нас | ожидается |
|---|---|---|
| `gcs(html,'::view-transition').position` | `static` | `absolute` |
| `gcs(html,'::view-transition-group(a)').position` | `static` | `absolute` |
| `document.getAnimations().length` | `0` | `≥ 3` (группа, old, new) |
| `documentElement.getAnimations({subtree:true}).length` | `0` | `≥ 3` |
| `vt.ready`, `vt.finished` | разрешаются | разрешаются |

## Как найдено

WPT-RUN-14 срез 22: `css-view-transitions/{pseudo-get-computed-style*,computed-style-no-active-transition,pseudo-element-animations*,group-animation-for-root-transition,dynamic-stylesheet-animations*,only-child-*,mix-blend-mode-only-on-transition,finished-promise-defers-cleanup,paused-animation-at-end,hit-test-*,nested/group-children-animations}` — 43 id (21 OK с падениями, 20 reftest `thick`, 1 TIMEOUT, 1 ERROR); 25 из них — `reftest-wait` (WPT-RUN-15).

## Что делать

UA-таблица стилей псевдоэлементов перехода и создание CSS-анимаций (`-ua-view-transition-*`) на дереве псевдоэлементов при активном переходе; тесты на их основе увидят тайминги. Рисование кросс-фейда остаётся в шелле.

## Как проверить

Таблица выше; `css/css-view-transitions/pseudo-get-computed-style.html`, `pseudo-element-animations.html`.
