# BUG-1334 — У `<p>`, `<ul>`, `<ol>`, `<dl>`, `<blockquote>`, `<pre>`, `<figure>`, `<menu>` нет полей и отступов UA-таблицы

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/style/ua.rs` — есть `apply_ua_heading_style` (:386), `apply_ua_hr_style` (:260), `apply_ua_body_margin` (:360), для остальных блочных элементов нет ничего)

## Симптом

`getComputedStyle()` на странице без author-CSS (`<!DOCTYPE html>`, стандартный режим):

| элемент | `margin-top` / `margin-bottom` | `margin-left` | `padding-left` | ожидается (HTML Rendering §15.3) |
|---|---|---|---|---|
| `<p>` | `0px` / `0px` | `0px` | `0px` | `1em` / `1em` |
| `<ul>`, `<ol>` | `0px` / `0px` | `0px` | `0px` | `1em` / `1em`, `padding-inline-start: 40px` |
| `<dl>` | `0px` / `0px` | `0px` | `0px` | `1em` / `1em` |
| `<dd>` | `0px` | `0px` | `0px` | `margin-inline-start: 40px` |
| `<blockquote>`, `<figure>` | `0px` / `0px` | `0px` | `0px` | `1em` / `1em`, `40px` по бокам |
| `<pre>`, `<menu>` | `0px` / `0px` | `0px` | `0px` | `1em` / `1em` (`menu`: ещё `40px` слева) |
| `<h1>` | `21.44px` | | | верно (`0.67em`) |
| `<hr>` | `8px` | | | верно |

`--dump-layout` двух соседних `<p>` при `body { margin: 0 }`: прямоугольники `y = 0` и `y = 17.72` — зазора между абзацами нет. Все страницы, где автор не сбросил поля, рендерятся с абзацами впритык (и список без отступа; маркер `li` при этом рисуется).

## Как найдено

WPT-RUN-14 срез 11: почти каждый тест `css/CSS2` начинается с `<p>Test passes if …</p>`, под которым стоит проверяемая фигура. Механизм расхождения для каждого теста отдельно не разбирался (вероятно, там, где результат зависит от абсолютного положения фигуры — `background-attachment: fixed`, фон, позиционируемый по вьюпорту, — эталон рассчитан на 16 px поля абзаца). **Доказательство — A/B одним бинарём:** в каждую пару «тест + эталон» из 235 `thick`-расхождений вставлен `<style>p,ul,ol,dl,blockquote,pre,figure,menu{margin:1em 0}…` (временные копии файлов, не закоммичены): 37 `thick` → `identical`, 3 → `thin-only`, 195 остались `thick` по другим причинам; новых провалов нет. 40 id из 485 не зелёных (`background-attachment-applies-to-*`, `background-position-applies-to-*c|d|e`, `border-width-005…008`, `border-bottom-018`, `*-color-129`).

Дефект шире WPT: на реальной странице без reset поля абзаца, списка, цитаты и `<pre>` — главный вклад в вертикальный ритм. Графические тесты (`graphic_tests/`) все начинаются с `* { margin: 0 }`, поэтому дыру не видят — BUG-204 закрыл только `<body>`.

## Что делать

Добавить `apply_ua_block_margins` рядом с `apply_ua_heading_style` (`ua.rs:386`), вызвать из `cascade.rs` до author-каскада (как соседние UA-функции). Значения — HTML Rendering §15.3.x: `p, dl, pre, menu, ul, ol` — `margin-block: 1em`; `blockquote, figure` — `1em 40px`; `dd` — `margin-inline-start: 40px`; `ul, ol, menu` — `padding-inline-start: 40px`; вложенные `ul ul`/`ol ul` — без вертикального поля. Затронет пиксели почти всех страниц с `<p>`: полный `graphic_tests/run.py`, золотые файлы в том же коммите.

## Как проверить

Страница `<!DOCTYPE html><body style="margin:0"><p>a</p><p>b</p>` — `y` второго `<p>` в `--dump-layout` должен стать 33.72 (`17.72 + 16`). `css/CSS2/backgrounds/background-attachment-applies-to-001.xht`, `css/CSS2/borders/border-width-005.xht` — `reftest_pixdiff.py --viewport 800x600 --ahem` даёт `identical`.

## Срез 13 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` tables/positioning/floats/floats-clear/abspos…)

A/B с `p,ul,ol,dl,blockquote,pre,figure,menu{display:block;margin:1em 0}` на 647 thick/identical id срезов 13: 33 → `identical`/`thin-only`, из них 18 в кластере «У `<p>`… нет UA-полей» (`floats-clear/clear-clearance-calculation-001.xht`, `-002.xht`, `-004.xht`), ещё 15 — раскиданы по другим кластерам (первое совпавшее правило). Новых провалов нет.
