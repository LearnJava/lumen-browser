# BUG-1852 — Скролл страницы на habr: scrollY уходит, вьюпорт становится пустым

**Статус:** OPEN
**Найден:** 2026-11-26, PERF-сессия (проверка BUG-1573 на живом habr)

## Симптом (со слов пользователя + замеры)

Во время загрузки habr «бегунок уходит вниз, изображение мигает, ничего не
прокручивается». После загрузки: `window.scrollTo(0, 1200)` → `window.scrollY`
стабильно 1200, но вьюпорт после перерисовки — **сплошной `#f0f0f0`** (PNG
1920×845, 1 уникальный цвет, 8 КБ против ~94–208 КБ у нормального кадра).
Контент на новой позиции прокрутки не рисуется вообще.

## Замеры (`.tmp/scroll_shot_probe.py`, `.tmp/scroll_probe*.py`, dev-release)

| Метрика | habr | локальный стенд |
|---|---|---|
| `documentElement.scrollHeight` | **845** (=clientHeight) | 4000 (корректно) |
| `body.scrollHeight` | 47993 | 4000 |
| `body` computed `overflow-y` | `scroll` | `scroll` (стенд 2) |
| `documentElement` `overflow-y` | `visible` | — |
| `scrollY` после `scrollTo(0,1200)` | 1200 (держится) | 1200 |
| Скриншот после скролла | **пустой #f0f0f0** | контент сместился (OK) |
| `getBoundingClientRect` элемента | не меняется (top=271) | — |

Локальные стенды `.tmp/scroll_simple.html` и `.tmp/scroll_simple2.html`
(в т.ч. с `body{overflow-y:scroll}`) скроллятся корректно — дефект
**не** в общем механизме propagation (CSS Overflow 3 §3.5 реализован:
`collect_page_scroll_containers`, `page_scrollable_extent`,
`ViewportRole::Propagated`), а в чём-то специфичном для дерева habr.

## Гипотезы (по убыванию)

1. **`html` имеет `overflow-x` не-`visible`** (типичный сброс
   `overflow-x:hidden` от горизонтального скролла) → по §3.5 propagation
   `body{overflow-y:scroll}` НЕ срабатывает (наша проверка требует
   `visible` по ОБЕИМ осям: `container_anchor.rs:25`,
   `lib.rs:557-561`) → `body` остаётся самостоятельным скроллером
   (виден его скроллбар — «бегунок»), а вьюпортный скролл уходит в
   область, где body-контейнер отрисовывает только свои 845px → пусто.
   Проверить computed `overflow-x` у `documentElement` на habr.
   Открытый вопрос семантики: Chrome propagated ли по-осно или целиком?
2. **`content-visibility: auto` / `contain-intrinsic-size`** на секциях
   habr: пропущенные поддеревья не перерисовываются при скролле →
   «мигание». Проверить computed стили контейнеров контента.
3. Скролл-экстент `content_height(html)` не учитывает переполнение body
   (845 вместо 47993) → `max_scroll` неверен → paint-clamp/пустой кадр.

## Критерий закрытия

На habr `documentElement.scrollHeight` ≈ контенту (~48k), скриншот после
`scrollTo(0,1200)` показывает контент (не однотонный фон), `scrollY`
совпадает с видимой позицией. Регресс-тест: html `overflow-x:hidden` +
body `overflow-y:scroll` + контент 5000px — скролл не должен давать
пустой вьюпорт.

## Методика

- `perf_audit.LiveBrowser` + MCP `eval`/`resources/read` (`resource://screenshot` → PNG bytes → sha1 + попиксельный анализ zlib).
- Пиксельный анализ: shot@0 = 55 цветов, shot@1200 = **1 цвет** `#f0f0f0`.
- **Ретест 2026-11-26** на dev-release после merge `bug-1848-scroll`
  (BUG-1850-FIXED, portal scroll + `display_list_metrics`): хэш
  `27a3b23cc88e`/8322 байта — **воспроизводится**, фикс portal-кейса
  этот дефект не закрыл (другой механизм: там wheel+range, тут
  scrollTo+пустой кадр).
