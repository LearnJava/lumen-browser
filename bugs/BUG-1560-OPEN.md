# BUG-1560 — Плавная прокрутка не реализована: `scroll-behavior: smooth` и `behavior: "smooth"` прокручивают мгновенно

**Статус:** OPEN (ДОРАБОТКА → SMOOTH-SCROLL)
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
