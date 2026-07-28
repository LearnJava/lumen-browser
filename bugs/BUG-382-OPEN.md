# BUG-382 — `getComputedStyle()` возвращает пустую строку для КАЖДОГО свойства, а `getBoundingClientRect()` — нулевой прямоугольник, примерно в трёх загрузках из четырёх: выкладка геометрии и стилей в JS-контекст гоняется со скриптом страницы

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs:10321-10337` и `main.rs:11067-11071` —
единственные две точки, где `collect_layout_rects` / `collect_computed_styles`
доезжают до JS через `update_layout_rects` / `update_computed_styles`; обе живут
внутри пути relayout), js (`crates/js/src/dom.rs:2498` `_lumen_get_computed_style`,
`dom.rs:12607` `window.getComputedStyle`)
**Найден:** P2, WPT-VENDOR-focus (2026-07-28), тест `focus/scroll-matches-focus.html`
+ живая проба через `--mcp-live-port` (`.tmp/probe-cs2.html`, `.tmp/probe-cs-live.py`)

## Симптом

Страница загружена, `wait document_ready` вернул успех, скрипт страницы уже
отработал (его глобал читается из того же eval-контекста) — и при этом:

```
__ready=1                     ← глобал, выставленный скриптом страницы: контекст тот самый
nid=7
rect={"x":0,"y":0,"width":0,"height":0,"top":0,"right":0,"bottom":0,"left":0}
native _lumen_get_computed_style(7,'display') = ""
getComputedStyle(el).display                  = ""
```

Элемент при этом — обычный `<div id="stat">` с `width:50px; color:red; z-index:3`
в `<style>` в `<head>`.

Тот же самый скрипт, тот же самый бинарь, соседний запуск:

```
rect={"x":8,"y":8,"width":50,"height":19.2,"top":8,"left":8,"right":58,"bottom":27.2}
native _lumen_get_computed_style(7,'display') = "block"
getComputedStyle(el).display                  = "block"
```

Четыре прогона подряд одного и того же скрипта дали `OK / EMPTY / EMPTY / EMPTY`.
Повторный eval через 6 секунд после первого **никогда** не восстанавливает
данные — если карта пуста на момент загрузки, она остаётся пустой.

В прогоне WPT это же поведение видно детерминированно, на `onload`:

```
FAIL :focus applies before scrolling into view
     - assert_equals: focusable style is correct expected "0" but got ""
```

(`focus/scroll-matches-focus.html`, первая же строка теста:
`assert_equals(getComputedStyle(focusable).zIndex, "0")`).

## Причина

Данные считаются правильно и полностью — виновата не сериализация:

* `computed_style_to_map` (`crates/engine/layout/src/selector_query.rs:590`)
  раскладывает `ComputedStyle` в 79 записей, включая `z-index`
  (`selector_query.rs:783`), `display`, `color`, `width`, `opacity`, `float`,
  `cursor`, `outline-*`;
* `collect_computed_styles` (`crates/engine/layout/src/lib.rs:1222`) обходит
  дерево боксов и делает `nid → map`;
* натив `_lumen_get_computed_style` (`crates/js/src/dom.rs:2498`) читает
  `computed_styles.lock().get(&nid).get(&prop)` и отдаёт `""`, когда записи нет.

Проблема в **доставке**. Обе точки, где карта попадает в JS-рантайм, лежат
внутри пути relayout и обусловлены `if self.js_present && let Some(lb_ref) =
self.layout_box.as_ref()` (`main.rs:10321`, `11067`). Никакой публикации по
завершении первичной раскладки страницы нет, и ни `getComputedStyle`, ни
`getBoundingClientRect` не запрашивают её сами — оба читают уже лежащий в
рантайме снимок. Успевает ли к моменту первого скрипта страницы произойти хоть
один relayout — вопрос гонки между потоком движка (ADR-016) и запуском JS;
отсюда «то работает, то нет» без единого изменения во входных данных.

Тот факт, что `""` приходит и от натива, и от `getPropertyValue`, и от
доступа через Proxy, отделяет этот баг от разбора имён свойств в шиме
(camelCase→kebab на `dom.rs:12622` корректен: в удачном прогоне `.display`,
`.zIndex` и `getPropertyValue('display')` возвращают верные значения).

## Влияние

Это, вероятно, самая тяжёлая находка категории — и она не про focus.

* `getComputedStyle()` — базовый инструмент любого фронтенда: измерение
  размеров, чтение CSS-переменных, определение темы, ожидание конца перехода.
  Пустая строка вместо значения не бросает исключение, а тихо ломает логику
  ниже по стеку (`parseFloat("") === NaN`).
* `getBoundingClientRect()` заявлен в `CAPABILITIES.md` как «real layout» и в
  удачном прогоне действительно точен — но нулевой прямоугольник в трёх
  загрузках из четырёх делает мёртвыми позиционирование поповеров/тултипов,
  виртуальные списки, drag&drop, ленивые загрузчики на ручном расчёте, весь
  IntersectionObserver-подобный код, написанный вручную.
* Недетерминированность делает баг особенно дорогим: страница «иногда
  работает», и симптом легко списать на что угодно другое.
* Для WPT это означает, что **любая** категория, чьи тесты меряют геометрию или
  стиль, даёт шум вместо сигнала — сюда попадает весь корпус `css/`, ради
  которого запланирован TEST-4 (reftest-executor).

## Как чинить

1. Публиковать снимок безусловно по завершении первичной раскладки страницы
   (там же, где шелл сообщает `document_ready` / применяет загруженную
   страницу), а не только из relayout-пути.
2. Проверить порядок относительно установки JS-контекста: публикация должна
   идти **после** того, как рантайм готов принять `update_computed_styles`,
   иначе гонка просто переедет.
3. Регрессионный гейт брать по счётчику, а не по wall-clock: тест, который
   после `document_ready` читает `getComputedStyle(body).display` и требует
   непустой строки, N раз подряд (баг воспроизводится ~3 раза из 4, поэтому
   одиночный зелёный прогон ничего не доказывает — см.
   `feedback_dont_describe_failure_mode_from_intuition`).
4. Вендоренный `focus/scroll-matches-focus.html` — готовый WPT-репродьюсер для
   половины про стили.

## Связанные

* [BUG-381](BUG-381-OPEN.md) — тот же тест `scroll-matches-focus.html` после
  первой строки упирается уже в отсутствие `focus()`/`activeElement`.
* `CAPABILITIES.md` — строка «getBoundingClientRect (real layout)» требует
  оговорки.
