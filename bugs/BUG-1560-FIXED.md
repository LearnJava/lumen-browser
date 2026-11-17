# BUG-1560 — Плавная прокрутка не реализована: `scroll-behavior: smooth` и `behavior: "smooth"` прокручивают мгновенно

**Статус:** FIXED 2026-11-17 (срез 1 — SMOOTH-SCROLL, 2026-10-22; срез 2 — SMOOTH-SCROLL-2)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** shell/js (`el.scrollTo({behavior})`, `scrollIntoView`, якорные переходы; `crates/shell/`)

## Симптом

Реклассифицировано в `ROADMAP.md` (ДОРАБОТКА → SMOOTH-SCROLL): плавной анимации прокрутки нет вообще. `el.scrollTo({top:50, behavior:"smooth"})` сразу даёт `scrollTop` 50, `window.scrollTo({behavior:"smooth"})` — то же; `scroll-behavior` разобран и хранится, но на прокрутку не влияет. Тесты ждут промежуточных значений и события `scrollend`: 24 id (171 из 236 сабтестов), 4 из них — `TIMEOUT` (ждут конца анимации).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `el.scrollTo({top:50,behavior:"smooth"}); el.scrollTop` | 50 (сразу) | меньше 50 до конца анимации |
| `el.style.scrollBehavior="smooth"; el.scrollTop=70; el.scrollTop` | 70 (сразу) | меньше 70 |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{scroll-behavior-*,smooth-*,scrollIntoView-smooth,visual-scrollIntoView-*,scrollLeftTop,background-change-during-smooth-scroll,…}.html` (24 id).

## Что делать

Задача SMOOTH-SCROLL: анимация прокрутки скроллера (easing по кадрам), прерывание пользователем, `scrollend`, `scroll-behavior` каскадом, `behavior: smooth` в `scroll*()`/`scrollIntoView()`, якорные переходы.

## Как проверить

`css/cssom-view/scroll-behavior-smooth.html`, `scrollIntoView-smooth.html`.

## Срез 1 (SMOOTH-SCROLL, 2026-10-22)

Сделано: плавная прокрутка элемента и вьюпорта шагами из шима по rAF (ease-out-cubic, 200 мс), один `scrollend` в конце,
`scroll-behavior` (не наследуется) для `behavior: auto`, `scrollTop`/`scrollLeft`-сеттеры, `document.scrollingElement` как вьюпорт,
`window.scrollX`, диапазон прокрутки страницы по раскладке. `css/cssom-view` PASS 768 → 913; `scroll-behavior-smooth` 3/3,
`scrollIntoView-smooth` 4/4, `smooth-scroll-nonstop` 2/2. Подробности — строка `SMOOTH-SCROLL` в `ROADMAP.md` и `subsystems/js.md`.

Остаток — задача `SMOOTH-SCROLL-2` (под-документы, цепочка вложенных скроллеров в `scrollIntoView`, scroll-snap на шагах,
промисы с `{interrupted}`, навигация по истории).

## Срез 2 (SMOOTH-SCROLL-2, 2026-11-17)

Сделано: `scrollIntoView` обходит ВСЮ цепочку скроллеров до вьюпорта за один вызов (якорь — margin box цели,
после каждого контейнера смещается в точку, где цель окажется в следующем контейнере; `scroll-padding` контейнера
сужает scrollport) — `scrollIntoView-multiple-nested` TIMEOUT → 4/4, `scroll-behavior-scrollintoview-nested` 2/2,
`scrollIntoView-scrollPadding` 0/3 → 3/3; финальный шаг анимации страницы несёт флаг snap и через канал page-scroll
приземляется в точку прилипания (`apply_page_y_snap`), мгновенно — `scrollend` шима не обгоняет приземление.
Плавная прокрутка под-документа больше не отключена: rAF фреймов идёт с FRAME-1, добавлены дренаж x-запроса
фрейма и пуш применённой позиции в рантайм ребёнка (`window.scrollX` фрейма настоящий); якорный переход `#frag`
двигает страницу и по x (голый `scrollTo` остаётся точным, без snap). Регрессий в `css/cssom-view` нет:
`scrollIntoView-smooth` 4/4, `smooth-scroll-nonstop` 2/2, `scrollIntoView-scrollMargin` 3/3.

Остаток по пунктам постановки, вынесенный отдельно: (1) `scroll-behavior-subframe-*` (2/40, 2/28) упирается не в
прокрутку, а в фасад iframe — у `contentDocument`/`contentWindow` нет ни `scrollingElement`, ни `scrollLeft/Top`,
ни `scrollTo`/`scrollX` (BUG-1743, кластер «фасад по значению»); (2) `scrollIntoView` через границу фрейма
(`scrollIntoView-iframes` 1/4) и дренаж элементных scroll-очередей рантаймов фреймов — тот же фасад; (3) sticky
(BUG-1468), writing-mode/rtl, `position: fixed`, `visualViewport`; (4) промисы с `{interrupted}` (BUG-479) и
навигация по истории (`scroll-behavior-smooth-navigation` 1/3).
